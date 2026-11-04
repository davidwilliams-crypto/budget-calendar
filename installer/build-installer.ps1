<#
.SYNOPSIS
Builds the Budget Calendar setup program from an archived local release.
.DESCRIPTION
Refuses unless VERSION equals the version in budget-gui/Cargo.toml and local-releases/vVERSION holds an
archive whose executable and icon still match its release.json. The setup program therefore packages
the archived build and its matching icon. Run scripts/archive-release.ps1 first. Relative paths are resolved against the
repository root, not the current directory.
.PARAMETER Version
Version to package. Default: the version in budget-gui/Cargo.toml. When given, it must equal that version.
.PARAMETER Makensis
Path to makensis.exe. Default: makensis on PATH, then the standard NSIS install folders.
.PARAMETER OutputDirectory
Folder for the setup program, inside this repository. Default: target/.
.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File installer/build-installer.ps1
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Makensis,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')

function Get-CargoVersion {
    $manifest = Join-Path $repository 'budget-gui/Cargo.toml'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw 'GUI Cargo manifest is missing.' }
    $packageText = [IO.File]::ReadAllText($manifest)
    $packageSection = [regex]::Match($packageText, '(?ms)^\[package\]\s*\r?\n(?<content>.*?)(?=^\[|\z)')
    $versionMatch = [regex]::Match($packageSection.Groups['content'].Value, '(?m)^version\s*=\s*"(?<version>[^"]+)"\s*$')
    if (-not $versionMatch.Success) { throw 'budget-gui/Cargo.toml has no package version.' }
    return $versionMatch.Groups['version'].Value
}

function Find-Makensis {
    if (-not [string]::IsNullOrWhiteSpace($Makensis)) {
        if (-not (Test-Path -LiteralPath $Makensis -PathType Leaf)) { throw "makensis was not found at $Makensis." }
        return (Resolve-Path -LiteralPath $Makensis).ProviderPath
    }
    $command = Get-Command makensis -CommandType Application -ErrorAction SilentlyContinue
    if ($null -ne $command) { return $command.Source }
    foreach ($folder in @([Environment]::GetEnvironmentVariable('ProgramFiles(x86)'), $env:ProgramFiles)) {
        if ([string]::IsNullOrWhiteSpace($folder)) { continue }
        $candidate = Join-Path $folder 'NSIS\makensis.exe'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    throw 'makensis was not found. Install NSIS or pass -Makensis <path to makensis.exe>.'
}

$cargoVersion = Get-CargoVersion
if ([string]::IsNullOrWhiteSpace($Version)) { $Version = $cargoVersion }
if ($Version -cnotmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {
    throw 'VERSION must be a semantic version such as 0.6.0.'
}
if ($Version -cne $cargoVersion) {
    throw "VERSION $Version does not match budget-gui/Cargo.toml ($cargoVersion). The installer packages the GUI version it is built for; archive and build that version, or pass -Version $cargoVersion."
}

$release = Join-Path $repository "local-releases/v$Version"
$releaseManifest = Join-Path $release 'release.json'
if (-not (Test-Path -LiteralPath $releaseManifest -PathType Leaf)) {
    throw "No archived release for $Version. Archive the build with scripts/archive-release.ps1 first; the installer packages the archived executable."
}
$saved = [IO.File]::ReadAllText($releaseManifest) | ConvertFrom-Json
if ($saved.version -cne $Version) { throw "Archived release manifest is for version $($saved.version), not $Version." }
$payload = Join-Path $release 'budget-gui.exe'
if (-not (Test-Path -LiteralPath $payload -PathType Leaf)) { throw "Archived executable is missing: $payload" }
$actual = (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -cne $saved.sha256) {
    throw "Archived executable differs from its release.json; it will not be packaged. Restore the archive or archive a new version."
}
$iconPayload = Join-Path $release 'budget-calendar.ico'
if (-not (Test-Path -LiteralPath $iconPayload -PathType Leaf)) { throw "Archived icon is missing: $iconPayload" }
if ($null -eq $saved.PSObject.Properties['files'] -or $null -eq $saved.files -or
    $null -eq $saved.files.PSObject.Properties['budget-calendar.ico'] -or
    $saved.files.'budget-calendar.ico' -isnot [string] -or
    $saved.files.'budget-calendar.ico' -cnotmatch '^[a-f0-9]{64}$') {
    throw 'Archived release.json is missing a valid SHA256 checksum for budget-calendar.ico.'
}
$iconHash = (Get-FileHash -LiteralPath $iconPayload -Algorithm SHA256).Hash.ToLowerInvariant()
if ($iconHash -cne $saved.files.'budget-calendar.ico') {
    throw 'Archived icon differs from its release.json; it will not be packaged. Restore the archive or archive a new version.'
}

# Windows file properties need a purely numeric a.b.c.d version; any pre-release or build suffix is dropped.
$numeric = [regex]::Match($Version, '^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)')
$fileVersion = '{0}.{1}.{2}.0' -f $numeric.Groups['major'].Value, $numeric.Groups['minor'].Value, $numeric.Groups['patch'].Value

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = Join-Path $repository 'target' }
elseif (-not [IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $repository $OutputDirectory }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\', '/')
if (-not $OutputDirectory.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "OutputDirectory must stay inside this repository: $OutputDirectory"
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$setup = Join-Path $OutputDirectory "BudgetCalendar-${Version}-setup.exe"

$compiler = Find-Makensis
# Run from installer/ so the script's relative paths resolve the same way NSIS expects.
Push-Location -LiteralPath (Join-Path $repository 'installer')
try {
    & $compiler "/DVERSION=$Version" "/DFILEVERSION=$fileVersion" "/DPAYLOAD=$payload" "/DICON_PAYLOAD=$iconPayload" "/DOUTPUT_DIR=$OutputDirectory" 'budget.nsi'
    if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE." }
} finally { Pop-Location }
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) { throw "makensis reported success but $setup was not written." }
Write-Output "Built the setup program for Budget Calendar ${Version}: $setup"
