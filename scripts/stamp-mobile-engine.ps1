#Requires -Version 5.1
<#
.SYNOPSIS
Records which Rust sources built the shipped phone calculation engine, and checks it is still current.
.DESCRIPTION
WebAssembly bytes can vary by machine, so freshness is judged by a source hash instead. scripts/build-mobile.ps1
writes that hash into budget_engine.wasm as a custom section, which browsers ignore, so it travels with the
engine in every package and source archive. -Check fails when the sources have changed since, or when the
engine was not produced by build-mobile.ps1. Only a fresh build can be stamped; an engine that already carries
a stamp is refused, so a stale engine cannot be blessed by stamping it again.
Line endings and the desktop version line in Cargo.lock do not affect the hash. Prints the source hash.
#>
[CmdletBinding()]
param([string]$WasmPath, [string]$BuiltFrom, [switch]$Check, [switch]$SourceHashOnly)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$sectionName = 'budget-source-sha256'
$rebuild = 'Run scripts/build-mobile.ps1.'
function Get-SourceHash {
    $relative = [Collections.Generic.List[string]]::new()
    foreach ($name in @('Cargo.toml', 'Cargo.lock', 'budget-core/Cargo.toml', 'budget-engine/Cargo.toml')) {
        if (-not (Test-Path -LiteralPath (Join-Path $root $name) -PathType Leaf)) { throw "Missing engine source: $name" }
        $relative.Add($name)
    }
    foreach ($directory in @('budget-core/src', 'budget-engine/src')) {
        $full = Join-Path $root $directory
        if (-not (Test-Path -LiteralPath $full -PathType Container)) { throw "Missing engine source: $directory" }
        foreach ($item in Get-ChildItem -LiteralPath $full -Recurse -File -Force) {
            $relative.Add($directory + '/' + $item.FullName.Substring($full.Length).TrimStart('\', '/').Replace('\', '/'))
        }
    }
    $names = $relative.ToArray()
    [Array]::Sort($names, [StringComparer]::Ordinal)
    $latin1 = [Text.Encoding]::GetEncoding(28591)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $inventory = [Text.StringBuilder]::new()
        foreach ($name in $names) {
            $content = $latin1.GetString([IO.File]::ReadAllBytes((Join-Path $root $name))).Replace("`r`n", "`n")
            if ($name -ceq 'Cargo.lock') { $content = [regex]::Replace($content, '(?m)^(name = "budget-gui"\nversion = ")[^"]*"', '$1"') }
            [void]$inventory.Append($name).Append("`n").Append([BitConverter]::ToString($sha.ComputeHash($latin1.GetBytes($content)))).Append("`n")
        }
        return [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($inventory.ToString()))).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
}
function Read-Leb([byte[]]$Bytes, [ref]$Position) {
    [long]$value = 0; $shift = 0
    while ($true) {
        if ($Position.Value -ge $Bytes.Length -or $shift -gt 28) { throw "budget_engine.wasm is not a WebAssembly engine. $rebuild" }
        $byte = $Bytes[$Position.Value]; $Position.Value++
        $value = $value -bor (([long]($byte -band 0x7f)) -shl $shift)
        if (($byte -band 0x80) -eq 0) { return $value }
        $shift += 7
    }
}
function Get-Stamp([byte[]]$Bytes) {
    if ($Bytes.Length -lt 8 -or [BitConverter]::ToString($Bytes[0..7]) -cne '00-61-73-6D-01-00-00-00') { throw "budget_engine.wasm is not a WebAssembly engine. $rebuild" }
    $position = 8; $found = $null
    while ($position -lt $Bytes.Length) {
        $id = $Bytes[$position]; $position++
        $size = Read-Leb $Bytes ([ref]$position)
        $end = $position + $size
        if ($end -gt $Bytes.Length) { throw "budget_engine.wasm is not a WebAssembly engine. $rebuild" }
        if ($id -eq 0) {
            $cursor = $position
            $length = Read-Leb $Bytes ([ref]$cursor)
            if ($cursor + $length -le $end -and [Text.Encoding]::ASCII.GetString($Bytes, $cursor, $length) -ceq $sectionName) {
                $cursor += $length
                $found = [Text.Encoding]::ASCII.GetString($Bytes, $cursor, $end - $cursor)
            }
        }
        $position = $end
    }
    return $found
}
$current = Get-SourceHash
if ($SourceHashOnly) { Write-Output $current; return }
if ([string]::IsNullOrWhiteSpace($WasmPath)) { $WasmPath = Join-Path $root 'budget-mobile/public/budget_engine.wasm' }
if (-not (Test-Path -LiteralPath $WasmPath -PathType Leaf)) { throw "budget_engine.wasm is missing. $rebuild" }
$bytes = [IO.File]::ReadAllBytes($WasmPath)
$stamp = Get-Stamp $bytes
if ($Check) {
    if ($null -eq $stamp) { throw "budget_engine.wasm has no source record, so it did not come from scripts/build-mobile.ps1. $rebuild" }
    if ($stamp -cne $current) { throw "budget_engine.wasm does not match the current Rust sources. $rebuild" }
    Write-Output $current
    return
}
if ($null -ne $stamp) { throw 'budget_engine.wasm is already stamped. Only a fresh build can be stamped; run scripts/build-mobile.ps1.' }
if ($BuiltFrom -and $BuiltFrom -cne $current) { throw "Rust sources changed during the build, so this engine is not stamped. $rebuild" }
$name = [Text.Encoding]::ASCII.GetBytes($sectionName)
$value = [Text.Encoding]::ASCII.GetBytes($current)
# Both lengths are under 128 here, so each is a single LEB128 byte.
$body = 1 + $name.Length + $value.Length
$stream = [IO.MemoryStream]::new()
try {
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.WriteByte(0); $stream.WriteByte([byte]$body); $stream.WriteByte([byte]$name.Length)
    $stream.Write($name, 0, $name.Length)
    $stream.Write($value, 0, $value.Length)
    [IO.File]::WriteAllBytes($WasmPath, $stream.ToArray())
} finally { $stream.Dispose() }
Write-Output $current
