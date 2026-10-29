#Requires -Version 5.1
<#
.SYNOPSIS
Builds an immutable, credential-free portable Node/Docker deployment ZIP; does not deploy it.
.DESCRIPTION
Run scripts/build-mobile.ps1 first. The GUI Cargo.toml supplies the application version.
Default output is target/mobile-release-vVERSION/. Relative output paths resolve against this repository.
#>
[CmdletBinding()]
param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')
function Assert-SafePath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    if (-not $full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Package paths must stay inside this repository: $full"
    }
    $cursor = $full
    while ($cursor.Length -ge $root.Length) {
        if (Test-Path -LiteralPath $cursor) {
            if (((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Package paths may not traverse links: $cursor" }
        }
        if ($cursor -eq $root) { break }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $full
}
function Get-Sha256([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
$gui = [IO.File]::ReadAllText((Assert-SafePath (Join-Path $root 'budget-gui/Cargo.toml')))
if ($gui -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)"\s*$') { throw 'GUI application version is missing.' }
$version = $Matches[1]
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = "target/mobile-release-v$version" }
$output = Assert-SafePath ([IO.Path]::Combine($root, $OutputDirectory))
$target = Assert-SafePath (Join-Path $root 'target')
if (-not $output.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Mobile package output must be a subdirectory of target/.' }
$files = [Collections.Generic.SortedDictionary[string, string]]::new([StringComparer]::Ordinal)
# Only these documented files and direct runtime assets are deployable. Never recurse into data,
# caches or node_modules. In particular JSON is not an allowed static extension.
foreach ($name in @('package.json', 'README.md', 'ENGINE-CONTRACT.md', 'SERVER-CONTRACT.md', 'server/DEPLOYMENT.md', 'Dockerfile', '.dockerignore')) {
    $path = Assert-SafePath (Join-Path $root "budget-mobile/$name")
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing package input: $name" }
    $files.Add($name, $path)
}
foreach ($rule in @(
    @{ Directory = 'public'; Extensions = @('.mjs', '.js', '.html', '.css', '.webmanifest', '.png', '.wasm') },
    @{ Directory = 'server'; Extensions = @('.mjs') }
)) {
    $directory = Assert-SafePath (Join-Path $root ('budget-mobile/' + $rule.Directory))
    foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
        if (-not $item.PSIsContainer -and $rule.Extensions -contains $item.Extension.ToLowerInvariant()) {
            $path = Assert-SafePath $item.FullName
            $files.Add(($rule.Directory + '/' + $item.Name), $path)
        }
    }
}
foreach ($required in @('public/index.html', 'public/app.mjs', 'public/manifest.webmanifest', 'public/sw.js', 'public/budget_engine.wasm', 'server/start.mjs', 'server/server.mjs')) {
    if (-not $files.ContainsKey($required)) { throw "Missing required runtime input: $required" }
}
$wasm = [IO.File]::ReadAllBytes($files['public/budget_engine.wasm'])
if ($wasm.Length -lt 8 -or [BitConverter]::ToString($wasm[0..7]) -cne '00-61-73-6D-01-00-00-00') { throw 'WebAssembly engine is missing or invalid; run scripts/build-mobile.ps1.' }
$hashes = [ordered]@{}
foreach ($entry in $files.GetEnumerator()) { $hashes[$entry.Key] = Get-Sha256 $entry.Value }
$identity = [ordered]@{ app = 'Budget Calendar'; version = $version; node = '>=24.19.0 <25'; files = $hashes }
$identityJson = $identity | ConvertTo-Json -Depth 6
$zipName = "budget-mobile-v$version.zip"
$manifestPath = Join-Path $output 'release.json'
$zipPath = Join-Path $output $zipName
if (Test-Path -LiteralPath $output) {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or -not (Test-Path -LiteralPath $zipPath -PathType Leaf)) { throw "Version collision: incomplete package already exists at $output" }
    $existing = [IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json
    if (($existing.identity | ConvertTo-Json -Depth 6) -cne $identityJson -or $existing.sha256 -cne (Get-Sha256 $zipPath)) {
        throw "Version collision at $output. Increment the GUI version and build again; existing packages are never overwritten."
    }
    Write-Output "Identical mobile package already exists: $output"
    return
}
[void][IO.Directory]::CreateDirectory($target)
$staging = Assert-SafePath (Join-Path $target ('mobile-package-staging-' + [Guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($staging)
try {
    $stagedZip = Join-Path $staging $zipName
    $zip = [IO.Compression.ZipFile]::Open($stagedZip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($entry in $files.GetEnumerator()) {
            # Verify the bytes copied, rather than hashing and then rereading a possibly changed file.
            $bytes = [IO.File]::ReadAllBytes($entry.Value)
            $sha = [Security.Cryptography.SHA256]::Create()
            try { $actual = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
            if ($actual -cne $hashes[$entry.Key]) { throw "Package source changed while reading: $($entry.Key)" }
            $member = $zip.CreateEntry($entry.Key, [IO.Compression.CompressionLevel]::Optimal)
            $member.LastWriteTime = [DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
            $stream = $member.Open()
            try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
        }
        $member = $zip.CreateEntry('app-version.json')
        $member.LastWriteTime = [DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
        $stream = $member.Open()
        try { $bytes = [Text.UTF8Encoding]::new($false).GetBytes($identityJson); $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
    } finally { $zip.Dispose() }
    $release = [ordered]@{ identity = $identity; archive = $zipName; sha256 = Get-Sha256 $stagedZip; deployed = $false }
    [IO.File]::WriteAllText((Join-Path $staging 'release.json'), ($release | ConvertTo-Json -Depth 7), [Text.UTF8Encoding]::new($false))
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($output))
    [IO.Directory]::Move($staging, $output)
    Write-Output "Mobile deployment package created: $output"
} finally {
    # Only the checked, generated staging directory is ever removed.
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath (Assert-SafePath $staging) -Recurse -Force }
}
