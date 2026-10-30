#Requires -Version 5.1
<#
.SYNOPSIS
Writes the service worker cache name from a hash of the deployable phone app files.
.DESCRIPTION
A phone only notices a new build when sw.js itself changes, so this must run after the last edit
to budget-mobile/public. scripts/build-mobile.ps1 runs it; scripts/package-mobile.ps1 uses -Check
and refuses a stale stamp. Line endings do not affect the hash, so a Git checkout cannot stale it.
Prints the cache name.
#>
[CmdletBinding()]
param([string]$PublicDirectory, [switch]$Check)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([string]::IsNullOrWhiteSpace($PublicDirectory)) { $PublicDirectory = Join-Path $PSScriptRoot '../budget-mobile/public' }
$public = [IO.Path]::GetFullPath($PublicDirectory)
$workerPath = Join-Path $public 'sw.js'
$runtime = @('.mjs', '.js', '.html', '.css', '.webmanifest', '.png', '.wasm')
$text = @('.mjs', '.js', '.html', '.css', '.webmanifest')
$cacheLine = "(?m)^const CACHE='budget-household-[^']*';"
$latin1 = [Text.Encoding]::GetEncoding(28591)
$worker = $latin1.GetString([IO.File]::ReadAllBytes($workerPath))
if (([regex]::Matches($worker, $cacheLine)).Count -ne 1) { throw 'sw.js must contain exactly one CACHE line.' }
if ($worker -notmatch "(?m)^const SHELL=\[(.*)\];") { throw 'sw.js must contain a SHELL list.' }
$shell = @([regex]::Matches($Matches[1], "'\./([^']*)'") | ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ })
$names = @(Get-ChildItem -LiteralPath $public -Force | Where-Object { -not $_.PSIsContainer -and $runtime -contains $_.Extension.ToLowerInvariant() } | ForEach-Object Name)
[Array]::Sort($names, [StringComparer]::Ordinal)
foreach ($name in $names) { if ($name -cne 'sw.js' -and $shell -cnotcontains $name) { throw "$name is not in the sw.js SHELL list, so it would be missing offline." } }
foreach ($name in $shell) { if ($names -cnotcontains $name) { throw "The sw.js SHELL list names a missing file: $name" } }
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $inventory = [Text.StringBuilder]::new()
    foreach ($name in $names) {
        $bytes = [IO.File]::ReadAllBytes((Join-Path $public $name))
        if ($text -contains [IO.Path]::GetExtension($name).ToLowerInvariant()) {
            $content = $latin1.GetString($bytes).Replace("`r`n", "`n")
            if ($name -ceq 'sw.js') { $content = [regex]::Replace($content, $cacheLine, "const CACHE='';") }
            $bytes = $latin1.GetBytes($content)
        }
        [void]$inventory.Append($name).Append("`n").Append([BitConverter]::ToString($sha.ComputeHash($bytes))).Append("`n")
    }
    $digest = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($inventory.ToString()))).Replace('-', '').ToLowerInvariant()
} finally { $sha.Dispose() }
$name = 'budget-household-' + $digest.Substring(0, 16)
$stamped = [regex]::Replace($worker, $cacheLine, "const CACHE='$name';")
if ($stamped -cne $worker) {
    if ($Check) { throw 'sw.js does not match the current phone app files. Run scripts/stamp-mobile-worker.ps1 after the last change.' }
    [IO.File]::WriteAllBytes($workerPath, $latin1.GetBytes($stamped))
}
Write-Output $name
