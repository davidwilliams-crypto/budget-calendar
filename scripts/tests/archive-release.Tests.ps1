# Run directly with PowerShell; no Pester or other dependencies are required.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$scriptUnderTest = Join-Path $repository 'scripts/archive-release.ps1'
if (-not (Test-Path -LiteralPath $scriptUnderTest -PathType Leaf)) {
    throw 'Release archive script is missing.'
}
$testRoot = Join-Path $repository ('target/release-archive-tests-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    throw 'The Windows .NET Framework C# compiler is required to make test executables.'
}
Add-Type -TypeDefinition @'
using System;
using System.IO;
public static class BudgetArchiveTestHash {
    public static string FileHash(string path) {
        ulong value = 14695981039346656037UL;
        foreach (byte b in File.ReadAllBytes(path)) { value = unchecked((value ^ b) * 1099511628211UL); }
        return value.ToString("x16");
    }
}
'@

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Write-Text([string]$Path, [string]$Text) {
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

function New-Fixture([string]$Name) {
    $root = Join-Path $testRoot $Name
    Write-Text (Join-Path $root 'scripts/archive-release.ps1') ([IO.File]::ReadAllText($scriptUnderTest))
    Write-Text (Join-Path $root 'Cargo.toml') '[workspace]'
    Write-Text (Join-Path $root 'Cargo.lock') '# fixture lock'
    Write-Text (Join-Path $root 'rust-toolchain.toml') '[toolchain]'
    Write-Text (Join-Path $root '.gitignore') '/target'
    Write-Text (Join-Path $root '.gitattributes') '* text=auto'
    Write-Text (Join-Path $root 'AGENTS.md') 'Fixture workflow'
    Write-Text (Join-Path $root 'SPEC.md') 'Fixture specification'
    Write-Text (Join-Path $root 'CHANGELOG.md') "# Changelog`n`n## [0.3.0] - 2026-10-27`n`n- First tracked release.`n"
    Write-Text (Join-Path $root 'budget-gui/Cargo.toml') "[package]`nname = `"budget-gui`"`nversion = `"0.3.0`"`n"
    Write-Text (Join-Path $root 'budget-gui/build.rs') 'fn main() {}'
    Write-Text (Join-Path $root 'budget-gui/src/main.rs') 'fn main() {}'
    Write-Text (Join-Path $root 'budget-gui/assets/budget-calendar.ico') 'fixture icon'
    Write-Text (Join-Path $root 'budget-core/Cargo.toml') '[package]'
    Write-Text (Join-Path $root 'budget-core/src/lib.rs') 'pub fn fixture() {}'
    Write-Text (Join-Path $root 'budget-core/tests/example.rs') '#[test] fn fixture() {}'
    Write-Text (Join-Path $root 'vendor/webbrowser/Cargo.toml') '[package]'
    Write-Text (Join-Path $root 'vendor/webbrowser/src/lib.rs') 'pub fn fixture() {}'
    Write-Text (Join-Path $root 'docs/ux.md') 'Fixture UX'
    # These all contain user/private or generated data and must never enter source.zip.
    foreach ($relative in @('Dave.bgt', 'budget-core/src/private.bgt', 'budget-gui/assets/private.bgt',
        'budget-gui/src/.env', 'budget-gui/src/secrets.json', 'reference/source.rs', 'analysis/source.rs',
        'target/source.rs', '.git/config', 'local-releases/old/source.rs',
        'budget-gui/src/target/generated.rs', 'budget-core/tests/docs/generated.rs',
        'scripts/.git/private.ps1', 'vendor/webbrowser/src/local-releases/old.rs',
        'budget-core/src/analysis/private.rs', 'budget-gui/src/reference/private.rs',
        'budget-core/src/TARGET/generated.rs')) {
        Write-Text (Join-Path $root $relative) 'DO NOT ARCHIVE PRIVATE DATA'
    }
    New-Executable $root 'budget app.exe' '0.3.0' 'fixture-build-a' 'first'
    return $root
}

function New-BuildManifest([string]$Root, [string]$BuildId) {
    $inputs = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.gitignore', '.gitattributes',
        'AGENTS.md', 'SPEC.md', 'CHANGELOG.md', 'budget-gui/Cargo.toml', 'budget-gui/build.rs',
        'budget-gui/src/main.rs', 'budget-gui/assets/budget-calendar.ico', 'budget-core/Cargo.toml',
        'budget-core/src/lib.rs', 'budget-core/tests/example.rs', 'vendor/webbrowser/Cargo.toml',
        'vendor/webbrowser/src/lib.rs', 'docs/ux.md', 'scripts/archive-release.ps1') | Sort-Object
    $entries = @($inputs | ForEach-Object {
        @{ path = $_; fnv1a = [BudgetArchiveTestHash]::FileHash((Join-Path $Root $_)) }
    })
    $manifest = @{ build_id = $BuildId; target = 'x86_64-pc-windows-msvc'; profile = 'release'; compiler = 'fixture'; inputs = $entries }
    Write-Text (Join-Path $Root ('target/release/build/budget-gui-fixture/out/build-inputs-' + $BuildId + '.json')) ($manifest | ConvertTo-Json -Depth 5)
}

function New-Executable([string]$Root, [string]$Name, [string]$Version, [string]$BuildId, [string]$Variant) {
    $source = Join-Path $Root ('target/' + $Name + '.cs')
    $exe = Join-Path $Root ('target/' + $Name)
    $json = '{"app":"Budget Calendar","version":"' + $Version + '","build_id":"' + $BuildId + '","target":"x86_64-pc-windows-msvc","profile":"release"}'
    $escaped = $json.Replace('"', '""')
    Write-Text $source @"
using System;
class Fixture {
    static int Main(string[] args) {
        if (args.Length != 1 || args[0] != "--version-json") return 9;
        Console.WriteLine(@"$escaped");
        // $Variant changes the executable bytes while keeping its metadata contract.
        return 0;
    }
}
"@
    & $compiler '/nologo' '/target:exe' ('/out:' + $exe) $source | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Fixture executable compilation failed: $Name" }
    # C# comments do not affect output; a non-PE overlay exercises changed binary bytes.
    if ($Variant -ne 'first') {
        $stream = [IO.File]::Open($exe, [IO.FileMode]::Append)
        try { $bytes = [Text.Encoding]::UTF8.GetBytes($Variant); $stream.Write($bytes, 0, $bytes.Length) }
        finally { $stream.Dispose() }
    }
    New-BuildManifest $Root $BuildId
}

function Invoke-Archive([string]$Root, [string]$Executable = 'budget app.exe') {
    & (Join-Path $Root 'scripts/archive-release.ps1') -ExecutablePath (Join-Path $Root ('target/' + $Executable)) | Out-Null
}

function Assert-ArchiveFails([string]$Root, [string]$Expected, [string]$Executable = 'budget app.exe') {
    $caught = $null
    try { Invoke-Archive $Root $Executable } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught) "Expected archive failure containing '$Expected'."
    Assert-True ($caught -match $Expected) "Expected '$Expected', received: $caught"
}

function Get-ArchiveSnapshot([string]$Root) {
    $dir = Join-Path $Root 'local-releases/v0.3.0'
    return @(Get-ChildItem -LiteralPath $dir -File -Recurse | Sort-Object FullName | ForEach-Object {
        $_.Name + ':' + (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash + ':' + $_.LastWriteTimeUtc.Ticks
    }) -join "`n"
}

function Assert-NoPublishedVersion([string]$Root) {
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $Root 'local-releases/v0.3.0'))) 'Failure published version 0.3.0.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $Root 'local-releases/v0.2.0'))) 'Failure published version 0.2.0.'
    Assert-True (-not (Get-ChildItem -LiteralPath (Join-Path $Root 'local-releases') -Force | Where-Object Name -Match 'staging')) 'Failure leaked staging files.'
}

$passed = 0
function Test-Case([string]$Name, [scriptblock]$Test) {
    & $Test
    $script:passed++
    Write-Host "PASS $Name"
}

Test-Case 'archives binary identity and only application source; repeated archive changes no files' {
    $root = New-Fixture 'complete archive with spaces'
    Invoke-Archive $root
    $release = Join-Path $root 'local-releases/v0.3.0'
    foreach ($file in @('budget-gui.exe', 'budget-calendar.ico', 'CHANGELOG.md', 'source.zip', 'release.json', 'build-inputs.json')) {
        Assert-True (Test-Path -LiteralPath (Join-Path $release $file) -PathType Leaf) "Missing archived $file"
    }
    $manifest = Get-Content -LiteralPath (Join-Path $release 'release.json') -Raw | ConvertFrom-Json
    Assert-True ($manifest.version -eq '0.3.0' -and $manifest.build_id -eq 'fixture-build-a') 'Manifest must preserve executable identity.'
    Assert-True ($manifest.target -eq 'x86_64-pc-windows-msvc' -and $manifest.profile -eq 'release') 'Manifest must preserve target/profile.'
    $exeHash = (Get-FileHash -LiteralPath (Join-Path $root 'target/budget app.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-True ($manifest.sha256 -eq $exeHash) 'Manifest hash must describe the preserved executable.'
    Assert-True ($manifest.release_date -eq '2026-10-27') 'Manifest must preserve the dated changelog entry.'
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead((Join-Path $release 'source.zip'))
    try {
        $names = @($zip.Entries | ForEach-Object FullName)
        foreach ($required in @('Cargo.toml', 'Cargo.lock', 'budget-gui/Cargo.toml', 'budget-gui/build.rs',
            'budget-gui/src/main.rs', 'budget-gui/assets/budget-calendar.ico', 'budget-core/src/lib.rs',
            'budget-core/tests/example.rs', 'vendor/webbrowser/src/lib.rs', 'AGENTS.md', 'CHANGELOG.md',
            'scripts/archive-release.ps1', 'docs/ux.md')) {
            Assert-True ($names -contains $required) "Source archive is missing $required"
        }
        Assert-True (-not ($names | Where-Object { $_ -match '(?i)(^|/)(target|reference|analysis|\.git|local-releases)/|\.bgt$|\.env$|secrets\.json$' })) 'Source archive contains private or generated files.'
    } finally { $zip.Dispose() }
    $before = Get-ArchiveSnapshot $root
    Invoke-Archive $root
    Assert-True ((Get-ArchiveSnapshot $root) -ceq $before) 'Idempotent archive must not rewrite files or timestamps.'
    Assert-True (-not (Get-ChildItem -LiteralPath (Join-Path $root 'local-releases') -Force | Where-Object Name -Match 'staging')) 'Staging directory leaked.'
}

Test-Case 'rejects a changed binary under the same version without altering the existing archive' {
    $root = New-Fixture 'binary collision'
    Invoke-Archive $root
    $before = Get-ArchiveSnapshot $root
    New-Executable $root 'changed.exe' '0.3.0' 'fixture-build-a' 'changed bytes'
    Assert-ArchiveFails $root 'already|different|collision' 'changed.exe'
    Assert-True ((Get-ArchiveSnapshot $root) -ceq $before) 'Collision altered the archived release.'
}

Test-Case 'rejects a different build identity under the same version' {
    $root = New-Fixture 'build collision'
    Invoke-Archive $root
    $before = Get-ArchiveSnapshot $root
    New-Executable $root 'changed.exe' '0.3.0' 'fixture-build-b' 'changed build'
    Assert-ArchiveFails $root 'already|different|collision' 'changed.exe'
    Assert-True ((Get-ArchiveSnapshot $root) -ceq $before) 'Build collision altered the archived release.'
}

Test-Case 'rejects executable version that differs from the GUI manifest before publishing' {
    $root = New-Fixture 'version mismatch'
    New-Executable $root 'old.exe' '0.2.0' 'fixture-build-old' 'old'
    Assert-ArchiveFails $root 'version' 'old.exe'
    Assert-NoPublishedVersion $root
}

Test-Case 'requires a dated changelog section for the executable version' {
    $root = New-Fixture 'missing changelog entry'
    Write-Text (Join-Path $root 'CHANGELOG.md') "# Changelog`n`n## [0.2.0] - 2026-10-26`n"
    Assert-ArchiveFails $root 'changelog|dated'
    Assert-NoPublishedVersion $root
}

Test-Case 'requires the executable and icon before publishing' {
    $root = New-Fixture 'missing files'
    Assert-ArchiveFails $root 'executable|missing|exist' 'missing.exe'
    Remove-Item -LiteralPath (Join-Path $root 'budget-gui/assets/budget-calendar.ico')
    Assert-ArchiveFails $root 'icon|missing|exist'
    Assert-NoPublishedVersion $root
}

Test-Case 'refuses a missing build input manifest' {
    $root = New-Fixture 'missing build manifest'
    Remove-Item -LiteralPath (Join-Path $root 'target/release/build/budget-gui-fixture/out/build-inputs-fixture-build-a.json')
    Assert-ArchiveFails $root 'manifest|fresh|build'
    Assert-NoPublishedVersion $root
}

Test-Case 'refuses source changed since the executable was built' {
    $root = New-Fixture 'stale source'
    Write-Text (Join-Path $root 'budget-gui/src/main.rs') 'fn newer_source() {}'
    Assert-ArchiveFails $root 'source|input|changed'
    Assert-NoPublishedVersion $root
}

Test-Case 'refuses traversal, private paths and duplicated paths in build input manifests' {
    foreach ($badPath in @('../private.rs', 'budget-core/src/private.bgt', 'target/source.rs', 'budget-gui/src/main.rs')) {
        $root = New-Fixture ('invalid manifest ' + [Guid]::NewGuid().ToString('N'))
        $manifestPath = Join-Path $root 'target/release/build/budget-gui-fixture/out/build-inputs-fixture-build-a.json'
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        $manifest.inputs += [pscustomobject]@{ path = $badPath; fnv1a = '0000000000000000' }
        Write-Text $manifestPath ($manifest | ConvertTo-Json -Depth 5)
        Assert-ArchiveFails $root 'path|allowlist|duplicate|inventory'
        Assert-NoPublishedVersion $root
    }
}

Test-Case 'refuses a corrupted archived executable even when release metadata is unchanged' {
    $root = New-Fixture 'corrupted archive'
    Invoke-Archive $root
    Write-Text (Join-Path $root 'local-releases/v0.3.0/budget-gui.exe') 'corrupted binary'
    $before = Get-ArchiveSnapshot $root
    Assert-ArchiveFails $root 'differ|overwrit|hash'
    Assert-True ((Get-ArchiveSnapshot $root) -ceq $before) 'Corrupt archive must remain untouched for inspection.'
}

Test-Case 'refuses output outside its repository' {
    $root = New-Fixture 'path boundary'
    $caught = $null
    try {
        & (Join-Path $root 'scripts/archive-release.ps1') -ExecutablePath (Join-Path $root 'target/budget app.exe') -OutputDirectory (Join-Path $testRoot 'outside-repository') | Out-Null
    } catch { $caught = $_.Exception.Message }
    Assert-True ($caught -match 'inside|within|repository') 'Archive must reject an output directory outside the repository.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $testRoot 'outside-repository'))) 'Boundary rejection wrote outside the repository.'
}

Test-Case 'installer never clobbers or deletes user files and requires an explicit version' {
    $nsi = Get-Content -LiteralPath (Join-Path $repository 'installer/budget.nsi') -Raw
    Assert-True ($nsi -notmatch 'lib\.rs') 'Installer must not ship or delete a stray lib.rs.'
    Assert-True ($nsi -notmatch '(?m)^\s*Delete\s+"\$INSTDIR\\sample\.bgt"') 'Uninstaller must not delete a user-editable sample.bgt.'
    Assert-True ($nsi -match 'IfFileExists\s+"\$INSTDIR\\sample\.bgt"') 'Installer must not unconditionally overwrite sample.bgt.'
    Assert-True ($nsi -notmatch '!define VERSION "') 'Installer must not carry a stale default version.'
    Assert-True ($nsi -match '!error') 'Installer must fail when VERSION is not supplied.'
}

Write-Host "$passed release archive tests passed. Fixtures retained at $testRoot"
