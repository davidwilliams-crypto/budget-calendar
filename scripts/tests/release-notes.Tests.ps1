# Run directly with PowerShell; no Pester or other dependencies are required.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$scriptUnderTest = Join-Path $repository 'scripts/release-notes.ps1'
if (-not (Test-Path -LiteralPath $scriptUnderTest -PathType Leaf)) { throw 'Release notes script is missing.' }
$testRoot = Join-Path $repository ('target/release-notes-tests-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Write-Text([string]$Path, [string]$Text) {
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

# Runs the script in its own PowerShell, as the workflow does. Returns the notes, or $null if it failed.
function Invoke-Notes([string]$Root, [string]$Version, [string]$Signing) {
    $ErrorActionPreference = 'Continue'  # an expected failure writes to stderr
    $out = Join-Path $testRoot ('notes-' + [Guid]::NewGuid().ToString('N') + '.md')
    & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptUnderTest -Version $Version -Signing $Signing -OutputPath $out @(if ($Root) { '-Root', $Root }) 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { return $null }
    return [IO.File]::ReadAllText($out)
}

$guide = "## Install`r`n`r`n1. Download BudgetCalendar-{VERSION}-setup.exe.`r`n2. Open it.`r`n" +
    "<!-- signed: kept when signed -->`r`n   - CERT STEP`r`n" +
    "<!-- unsigned: kept when unsigned -->`r`n   - PLAIN STEP for {VERSION}`r`n" +
    "<!-- end -->`r`n3. Finish.`r`n"
$changelog = "# Changelog`r`n`r`n## [1.2.3] - 2026-11-04`r`n`r`n- New thing.`r`n`r`n## [1.2.2] - 2026-11-01`r`n`r`n- Old thing.`r`n"

try {
    $fixture = Join-Path $testRoot 'fixture'
    Write-Text (Join-Path $fixture 'docs/INSTALL.md') $guide
    Write-Text (Join-Path $fixture 'CHANGELOG.md') $changelog

    $unsigned = Invoke-Notes $fixture '1.2.3' 'unsigned'
    Assert-True ($null -ne $unsigned) 'Unsigned notes were not written.'
    Assert-True ($unsigned.Contains('PLAIN STEP for 1.2.3') -and -not $unsigned.Contains('CERT STEP')) 'Unsigned notes do not keep only the unsigned step.'
    Assert-True ($unsigned.Contains("2. Open it.`n   - PLAIN STEP for 1.2.3`n3. Finish.")) 'Unsigned step is not in place inside the numbered list.'
    Assert-True (-not $unsigned.Contains('<!--') -and -not $unsigned.Contains("`r") -and -not $unsigned.Contains('{VERSION}')) 'Notes keep markers, CR or {VERSION}.'
    Assert-True ($unsigned.Contains("## What's new in 1.2.3`n`n- New thing.") -and -not $unsigned.Contains('Old thing')) 'Notes do not hold exactly this version''s changelog entry.'

    $signed = Invoke-Notes $fixture '1.2.3' 'signed'
    Assert-True ($null -ne $signed -and $signed.Contains("2. Open it.`n   - CERT STEP`n3. Finish.") -and -not $signed.Contains('PLAIN STEP')) 'Signed notes do not keep only the signed step.'

    Assert-True ($null -eq (Invoke-Notes $fixture '9.9.9' 'signed')) 'A version with no changelog entry did not fail.'

    $broken = Join-Path $testRoot 'broken'
    Write-Text (Join-Path $broken 'CHANGELOG.md') $changelog
    Write-Text (Join-Path $broken 'docs/INSTALL.md') ($guide -replace '<!-- end -->\r\n', '')
    Assert-True ($null -eq (Invoke-Notes $broken '1.2.3' 'signed')) 'A guide without its end marker did not fail.'
    Write-Text (Join-Path $broken 'docs/INSTALL.md') "2. Open it.`r`n   - Only one version.`r`n"
    Assert-True ($null -eq (Invoke-Notes $broken '1.2.3' 'unsigned')) 'A guide without signed/unsigned steps did not fail.'

    # The real guide and changelog, read with the default -Root as the release workflow does:
    # each build's notes must carry the matching Unknown publisher advice.
    $real = $null
    $version = [regex]::Match([IO.File]::ReadAllText((Join-Path $repository 'CHANGELOG.md')), '(?m)^## \[(?<v>[^\]]+)\]').Groups['v'].Value
    $realUnsigned = Invoke-Notes $real $version 'unsigned'
    $realSigned = Invoke-Notes $real $version 'signed'
    Assert-True ($null -ne $realUnsigned -and $null -ne $realSigned) 'Notes could not be built from docs/INSTALL.md and CHANGELOG.md.'
    Assert-True ($realUnsigned.Contains('Run anyway') -and $realUnsigned.Contains("BudgetCalendar-$version-setup.exe") -and -not $realUnsigned.Contains('Delete it')) 'Unsigned notes from docs/INSTALL.md tell people to delete the unsigned file.'
    Assert-True ($realSigned.Contains('Delete it') -and -not $realSigned.Contains('not yet code-signed')) 'Signed notes from docs/INSTALL.md lack the delete-if-unknown-publisher step.'

    Write-Output 'release-notes tests passed.'
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}
