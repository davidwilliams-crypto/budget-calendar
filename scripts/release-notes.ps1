<#
.SYNOPSIS
Writes the GitHub release notes for one version: the install guide, then that version's changelog entry.

.DESCRIPTION
docs/INSTALL.md holds two versions of the "open the file" step, between HTML comment markers:
  <!-- signed: ... -->    for a code-signed build
  <!-- unsigned: ... -->  for a build made before code signing is set up
  <!-- end -->
Only the part matching -Signing is kept, and the markers are removed. {VERSION} becomes the version.
Fails if the guide's markers are missing or out of order, or if CHANGELOG.md has no entry for the version.

.PARAMETER Version
The version being released, such as 0.11.1.

.PARAMETER Signing
'signed' or 'unsigned': whether the setup program on the release is code-signed.

.PARAMETER OutputPath
Where to write the notes (UTF-8, LF line endings).

.PARAMETER Root
The repository folder holding docs/INSTALL.md and CHANGELOG.md. Defaults to this repository.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][ValidateSet('signed', 'unsigned')][string]$Signing,
    [Parameter(Mandatory)][string]$OutputPath,
    [string]$Root
)

$ErrorActionPreference = 'Stop'
# $PSScriptRoot is empty in parameter defaults under Windows PowerShell 5.1, so the default is set here.
if ([string]::IsNullOrWhiteSpace($Root)) { $Root = Join-Path $PSScriptRoot '..' }
$Root = [IO.Path]::GetFullPath($Root)

function Read-Lf([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Path is missing." }
    return [IO.File]::ReadAllText($Path).Replace("`r`n", "`n")
}

$guidePath = Join-Path $Root 'docs/INSTALL.md'
$guide = Read-Lf $guidePath
$pattern = '(?ms)^<!-- signed:[^\n]*-->\n(?<signed>.*?)^<!-- unsigned:[^\n]*-->\n(?<unsigned>.*?)^<!-- end -->\n?'
$parts = [regex]::Matches($guide, $pattern)
if ($parts.Count -ne 1) {
    throw "$guidePath must have exactly one '<!-- signed: -->', '<!-- unsigned: -->', '<!-- end -->' block, in that order."
}
$part = $parts[0]
$guide = $guide.Substring(0, $part.Index) + $part.Groups[$Signing].Value + $guide.Substring($part.Index + $part.Length)
if ($guide -match '<!--') { throw "$guidePath has a marker outside its signed/unsigned block." }
$guide = $guide.Replace('{VERSION}', $Version).Trim()

$changelogPath = Join-Path $Root 'CHANGELOG.md'
$changelog = Read-Lf $changelogPath
$entry = [regex]::Match($changelog, '(?ms)^## \[' + [regex]::Escape($Version) + '\][^\n]*\n(?<body>.*?)(?=^## \[|\z)')
if (-not $entry.Success -or [string]::IsNullOrWhiteSpace($entry.Groups['body'].Value)) {
    throw "$changelogPath has no entry for $Version."
}

$notes = @($guide, '', "## What's new in $Version", '', $entry.Groups['body'].Value.Trim(), '')
[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($OutputPath)))
[IO.File]::WriteAllText($OutputPath, ($notes -join "`n"), [Text.UTF8Encoding]::new($false))
