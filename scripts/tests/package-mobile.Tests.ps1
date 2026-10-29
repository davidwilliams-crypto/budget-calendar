#Requires -Version 5.1
# Generated fixtures only; no user budget or live deployment is accessed.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $root ('target/mobile-package-tests-' + [Guid]::NewGuid().ToString('N'))
function Write-Fixture([string]$Name, [string]$Text) {
    $path = Join-Path $fixture $Name
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
    [IO.File]::WriteAllText($path, $Text, [Text.UTF8Encoding]::new($false))
}
function Assert-True([bool]$Value, [string]$Message) { if (-not $Value) { throw $Message } }
function Assert-Fails([scriptblock]$Action, [string]$Pattern) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match $Pattern) "Expected '$Pattern', got '$caught'"
}
Write-Fixture 'scripts/package-mobile.ps1' ([IO.File]::ReadAllText((Join-Path $root 'scripts/package-mobile.ps1')))
Write-Fixture 'budget-gui/Cargo.toml' "[package]`nname = `"budget-gui`"`nversion = `"0.8.0`"`n"
$runtime = @('package.json', 'README.md', 'ENGINE-CONTRACT.md', 'SERVER-CONTRACT.md', 'server/DEPLOYMENT.md', 'Dockerfile', '.dockerignore',
    'public/index.html', 'public/app.mjs', 'public/sw.js', 'public/manifest.webmanifest', 'public/styles.css',
    'public/icon-192.png', 'public/icon-512.png', 'server/start.mjs', 'server/server.mjs')
foreach ($name in $runtime) { Write-Fixture "budget-mobile/$name" "generated fixture $name" }
[IO.File]::WriteAllBytes((Join-Path $fixture 'budget-mobile/public/budget_engine.wasm'), [byte[]](0,97,115,109,1,0,0,0))
foreach ($name in @('public/household.bgt', 'public/cached-budget.json', 'public/database.sqlite', 'public/.env',
    'public/data/private.mjs', 'public/private/recovery.mjs', 'public/node_modules/library.mjs',
    'server/data/private.mjs', 'server/.env', 'server/credentials.json', 'data/budget.sqlite', 'node_modules/package.json')) {
    Write-Fixture "budget-mobile/$name" 'GENERATED PRIVATE TEST DATA - EXCLUDE'
}
$script = Join-Path $fixture 'scripts/package-mobile.ps1'
& $script | Out-Null
$output = Join-Path $fixture 'target/mobile-release-v0.8.0'
$archive = Join-Path $output 'budget-mobile-v0.8.0.zip'
$manifest = [IO.File]::ReadAllText((Join-Path $output 'release.json')) | ConvertFrom-Json
Assert-True ($manifest.identity.version -ceq '0.8.0' -and $manifest.deployed -eq $false) 'Wrong application version or deployment status.'
Assert-True ($manifest.sha256 -ceq (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()) 'ZIP checksum mismatch.'
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $names = @($zip.Entries | ForEach-Object FullName)
    foreach ($name in ($runtime + @('public/budget_engine.wasm', 'app-version.json'))) { Assert-True ($names -contains $name) "Missing package file: $name" }
    Assert-True ($names.Count -eq $runtime.Count + 2) 'Package contains unexpected private or generated files.'
    foreach ($entry in $zip.Entries) {
        if ($entry.FullName -eq 'app-version.json') { continue }
        $sha = [Security.Cryptography.SHA256]::Create()
        $stream = $entry.Open()
        try { $digest = ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() } finally { $stream.Dispose(); $sha.Dispose() }
        Assert-True ($manifest.identity.files.($entry.FullName) -ceq $digest) "Bad per-file checksum: $($entry.FullName)"
    }
} finally { $zip.Dispose() }
$before = (Get-Item -LiteralPath $archive).LastWriteTimeUtc.Ticks
& $script | Out-Null
Assert-True ((Get-Item -LiteralPath $archive).LastWriteTimeUtc.Ticks -eq $before) 'Repeated package rewrote the archive.'
Write-Fixture 'budget-mobile/public/app.mjs' 'changed source'
Assert-Fails { & $script } 'Version collision'
Assert-True ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $manifest.sha256) 'Collision changed existing archive.'
Assert-Fails { & $script -OutputDirectory '../outside' } 'inside this repository'
Assert-Fails { & $script -OutputDirectory 'budget-mobile/output' } 'subdirectory of target'
Write-Fixture 'budget-mobile/public/budget_engine.wasm' 'invalid wasm'
Assert-Fails { & $script -OutputDirectory 'target/invalid-wasm' } 'WebAssembly engine'
Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture 'target/invalid-wasm'))) 'Invalid WASM published a package.'
$docker = [IO.File]::ReadAllText((Join-Path $root 'budget-mobile/Dockerfile'))
$ignore = [IO.File]::ReadAllText((Join-Path $root 'budget-mobile/.dockerignore'))
Assert-True ($docker -notmatch '(?m)^COPY\s+\.\s' -and $docker -notmatch '(?m)^RUN.*npm (install|ci)' ) 'Docker must copy explicit runtime files with no external install.'
Assert-True ($docker -match 'BUDGET_DATA_DIR=/data' -and $docker -match 'USER node') 'Docker must use a private persistent path and non-root user.'
Assert-True ($ignore -match '(?m)^\*\*\s*$' -and $ignore -notmatch '!public/\*\.json') 'Docker context must deny all by default and exclude private JSON.'
Write-Host "8 mobile package checks passed. Generated fixtures retained at $fixture"

