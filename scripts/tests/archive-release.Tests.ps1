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
    // Same identity rule as budget-gui/build.rs: text (no NUL byte) is hashed with CRLF as LF.
    public static string FileHash(string path) {
        byte[] bytes = File.ReadAllBytes(path);
        bool text = Array.IndexOf(bytes, (byte)0) < 0;
        ulong value = 14695981039346656037UL;
        for (int i = 0; i < bytes.Length; i++) {
            if (text && bytes[i] == 13 && i + 1 < bytes.Length && bytes[i + 1] == 10) continue;
            value = unchecked((value ^ bytes[i]) * 1099511628211UL);
        }
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

function Write-FixtureIcon([string]$Path) {
    # A generated 16px, 32-bit DIB icon is sufficient for a real NSIS compile. All test assets live
    # under target/: neither the shipped artwork nor any user budget supplies fixture data.
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
    $stream = [IO.MemoryStream]::new()
    $writer = [IO.BinaryWriter]::new($stream)
    try {
        foreach ($value in @(0, 1, 1)) { $writer.Write([uint16]$value) }
        $writer.Write([byte[]](16, 16, 0, 0))
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]1128); $writer.Write([uint32]22)
        $writer.Write([uint32]40); $writer.Write([int]16); $writer.Write([int]32)
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        foreach ($value in @(0, 1024, 0, 0, 0, 0)) { $writer.Write([uint32]$value) }
        for ($pixel = 0; $pixel -lt 256; $pixel++) { $writer.Write([byte[]](255, 120, 40, 255)) }
        $writer.Write([byte[]]::new(64))
        $writer.Flush()
        [IO.File]::WriteAllBytes($Path, $stream.ToArray())
    } finally { $writer.Dispose(); $stream.Dispose() }
}

function New-Fixture([string]$Name) {
    $root = Join-Path $testRoot $Name
    Write-Text (Join-Path $root 'scripts/archive-release.ps1') ([IO.File]::ReadAllText($scriptUnderTest))
    Write-Text (Join-Path $root 'Cargo.toml') '[workspace]'
    Write-Text (Join-Path $root 'Cargo.lock') '# fixture lock'
    Write-Text (Join-Path $root 'rust-toolchain.toml') '[toolchain]'
    Write-Text (Join-Path $root '.cargo/config.toml') '# fixture cargo config'
    Write-Text (Join-Path $root '.gitignore') "/target`n/local-releases`n"
    Write-Text (Join-Path $root '.gitattributes') '* text=auto'
    Write-Text (Join-Path $root 'AGENTS.md') 'Fixture workflow'
    Write-Text (Join-Path $root 'SPEC.md') 'Fixture specification'
    Write-Text (Join-Path $root 'CHANGELOG.md') "# Changelog`n`n## [0.3.0] - 2026-10-27`n`n- First tracked release.`n"
    Write-Text (Join-Path $root 'budget-gui/Cargo.toml') "[package]`nname = `"budget-gui`"`nversion = `"0.3.0`"`n"
    Write-Text (Join-Path $root 'budget-gui/build.rs') 'fn main() {}'
    Write-Text (Join-Path $root 'budget-gui/src/main.rs') 'fn main() {}'
    Write-FixtureIcon (Join-Path $root 'budget-gui/assets/budget-calendar.ico')
    Write-Text (Join-Path $root 'budget-core/Cargo.toml') '[package]'
    Write-Text (Join-Path $root 'budget-core/src/lib.rs') 'pub fn fixture() {}'
    Write-Text (Join-Path $root 'budget-core/tests/example.rs') '#[test] fn fixture() {}'
    Write-Text (Join-Path $root 'vendor/webbrowser/Cargo.toml') '[package]'
    Write-Text (Join-Path $root 'vendor/webbrowser/src/lib.rs') 'pub fn fixture() {}'
    Write-Text (Join-Path $root 'docs/ux.md') 'Fixture UX'
    Write-Text (Join-Path $root 'installer/budget.nsi') "; fixture installer`n"
    Write-Text (Join-Path $root 'installer/build-installer.ps1') "# fixture installer wrapper`n"
    foreach ($relative in @('README.md', 'budget-engine/Cargo.toml', 'budget-mobile/package.json', 'budget-mobile/README.md', 'budget-mobile/ENGINE-CONTRACT.md', 'budget-mobile/SERVER-CONTRACT.md', 'budget-mobile/server/DEPLOYMENT.md', 'budget-mobile/Dockerfile', 'budget-mobile/.dockerignore', 'budget-engine/src/lib.rs', 'budget-engine/tests/parity.mjs', 'budget-mobile/public/app.mjs', 'budget-mobile/public/sw.js', 'budget-mobile/public/index.html', 'budget-mobile/public/styles.css', 'budget-mobile/public/manifest.webmanifest', 'budget-mobile/public/icon-192.png', 'budget-mobile/public/budget_engine.wasm', 'budget-mobile/server/server.mjs', 'budget-mobile/tests/server.test.mjs')) {
        Write-Text (Join-Path $root $relative) "fixture source: $relative"
    }
    # These all contain user/private or generated data and must never enter source.zip.
    foreach ($relative in @('Dave.bgt', 'budget-core/src/private.bgt', 'budget-gui/assets/private.bgt',
        'budget-gui/src/.env', 'budget-gui/src/secrets.json', 'reference/source.rs', 'analysis/source.rs',
        'target/source.rs', '.git/config', 'local-releases/old/source.rs',
        'budget-gui/src/target/generated.rs', 'budget-core/tests/docs/generated.rs',
        'scripts/.git/private.ps1', 'vendor/webbrowser/src/local-releases/old.rs',
        'budget-core/src/analysis/private.rs', 'budget-gui/src/reference/private.rs',
        'budget-core/src/TARGET/generated.rs',
        'budget-engine/src/data/private.rs', 'budget-engine/tests/node_modules/private.mjs',
        'budget-mobile/public/cached-budget.json', 'budget-mobile/public/private.sqlite', 'budget-mobile/public/.env',
        'budget-mobile/public/household.bgt', 'budget-mobile/public/data/private.mjs',
        'budget-mobile/public/credentials/private.js', 'budget-mobile/public/secrets/private.mjs',
        'budget-mobile/server/node_modules/private.mjs', 'budget-mobile/server/private.sqlite',
        'budget-mobile/tests/target/generated.mjs', 'budget-mobile/data/private.mjs', 'budget-mobile/.env')) {
        Write-Text (Join-Path $root $relative) 'DO NOT ARCHIVE PRIVATE DATA'
    }
    New-Executable $root 'budget app.exe' '0.3.0' 'fixture-build-a' 'first'
    return $root
}

function New-BuildManifest([string]$Root, [string]$BuildId) {
    $inputs = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml', '.gitignore', '.gitattributes',
        'AGENTS.md', 'SPEC.md', 'CHANGELOG.md', 'budget-gui/Cargo.toml', 'budget-gui/build.rs',
        'budget-gui/src/main.rs', 'budget-gui/assets/budget-calendar.ico', 'budget-core/Cargo.toml',
        'budget-core/src/lib.rs', 'budget-core/tests/example.rs', 'vendor/webbrowser/Cargo.toml',
        'vendor/webbrowser/src/lib.rs', 'docs/ux.md', 'scripts/archive-release.ps1',
        'installer/budget.nsi', 'installer/build-installer.ps1',
        'README.md', 'budget-engine/Cargo.toml', 'budget-mobile/package.json', 'budget-mobile/README.md', 'budget-mobile/ENGINE-CONTRACT.md', 'budget-mobile/SERVER-CONTRACT.md', 'budget-mobile/server/DEPLOYMENT.md', 'budget-mobile/Dockerfile', 'budget-mobile/.dockerignore', 'budget-engine/src/lib.rs', 'budget-engine/tests/parity.mjs', 'budget-mobile/public/app.mjs', 'budget-mobile/public/sw.js', 'budget-mobile/public/index.html', 'budget-mobile/public/styles.css', 'budget-mobile/public/manifest.webmanifest', 'budget-mobile/public/icon-192.png', 'budget-mobile/public/budget_engine.wasm', 'budget-mobile/server/server.mjs', 'budget-mobile/tests/server.test.mjs') | Sort-Object
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

function Invoke-TestGit([string]$Root, [string[]]$Arguments) {
    # Git writes progress and warnings to stderr; only the exit code decides success here.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& git -C $Root -c user.name=Fixture -c user.email=fixture@example.invalid @Arguments 2>$null)
        $exitCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
    if ($exitCode -ne 0) { throw "git $($Arguments -join ' ') failed in fixture with exit code $exitCode." }
    # The leading comma keeps a one-line result an array, so callers can index it as [0].
    return ,$output
}

function Remove-PlantedGitFolders([string]$Root) {
    # The private-data fixtures plant fake .git folders to prove they are never archived. A real git
    # repository cannot contain them, so remove them before initialising.
    Get-ChildItem -LiteralPath $Root -Recurse -Force -Directory -Filter '.git' | Remove-Item -Recurse -Force
}

function Initialize-TestRepository([string]$Root) {
    Remove-PlantedGitFolders $Root
    [void](Invoke-TestGit $Root @('init', '-q', '-b', 'main'))
    [void](Invoke-TestGit $Root @('add', '-A'))
    [void](Invoke-TestGit $Root @('commit', '-q', '-m', 'fixture source'))
}

function Convert-TextInputsToCrlf([string]$Root) {
    # Mirrors a core.autocrlf=true Windows checkout: text inputs gain CRLF; binary inputs keep their bytes.
    $manifest = Get-Content -LiteralPath (Join-Path $Root 'target/release/build/budget-gui-fixture/out/build-inputs-fixture-build-a.json') -Raw | ConvertFrom-Json
    foreach ($entry in $manifest.inputs) {
        $path = Join-Path $Root $entry.path
        $bytes = [IO.File]::ReadAllBytes($path)
        if ([Array]::IndexOf($bytes, [byte]0) -ge 0) { continue }
        $text = [Text.Encoding]::UTF8.GetString($bytes)
        # Start from LF so a file that already has CRLF (the archive script itself) does not gain a second CR.
        [IO.File]::WriteAllBytes($path, [Text.Encoding]::UTF8.GetBytes($text.Replace("`r`n", "`n").Replace("`n", "`r`n")))
    }
}

function Read-ReleaseManifest([string]$Root) {
    return Get-Content -LiteralPath (Join-Path $Root 'local-releases/v0.3.0/release.json') -Raw | ConvertFrom-Json
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
            'scripts/archive-release.ps1', 'docs/ux.md',
            'README.md', 'budget-engine/Cargo.toml', 'budget-mobile/package.json', 'budget-mobile/README.md', 'budget-mobile/ENGINE-CONTRACT.md', 'budget-mobile/SERVER-CONTRACT.md', 'budget-mobile/server/DEPLOYMENT.md', 'budget-mobile/Dockerfile', 'budget-mobile/.dockerignore', 'budget-engine/src/lib.rs', 'budget-engine/tests/parity.mjs', 'budget-mobile/public/app.mjs', 'budget-mobile/public/sw.js', 'budget-mobile/public/index.html', 'budget-mobile/public/styles.css', 'budget-mobile/public/manifest.webmanifest', 'budget-mobile/public/icon-192.png', 'budget-mobile/public/budget_engine.wasm', 'budget-mobile/server/server.mjs', 'budget-mobile/tests/server.test.mjs')) {
            Assert-True ($names -contains $required) "Source archive is missing $required"
        }
        Assert-True (-not ($names | Where-Object { $_ -match '(?i)(^|/)(target|reference|analysis|\.git|local-releases|node_modules|data|credentials|secrets)/|\.bgt$|\.env$|secrets\.json$|cached-budget\.json$|\.sqlite$' })) 'Source archive contains private or generated files.'
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

Test-Case 'archive records the HEAD commit and a clean tree for a committed git work tree' {
    $root = New-Fixture 'committed work tree'
    Initialize-TestRepository $root
    $head = (Invoke-TestGit $root @('rev-parse', 'HEAD'))[0]
    Invoke-Archive $root
    $manifest = Read-ReleaseManifest $root
    Assert-True ($manifest.git_commit -ceq $head) 'release.json must record the HEAD commit.'
    Assert-True ($manifest.git_clean -eq $true) 'A committed, clean work tree must be recorded as clean.'
}

Test-Case 'archive refuses uncommitted changes to a tracked release input and explains the rule' {
    $root = New-Fixture 'uncommitted tracked input'
    Initialize-TestRepository $root
    Write-Text (Join-Path $root 'budget-gui/src/main.rs') "fn edited_after_commit() {}`n"
    # Regenerate the build manifest so only the git rule can refuse this archive.
    New-Executable $root 'budget app.exe' '0.3.0' 'fixture-build-a' 'first'
    Assert-ArchiveFails $root 'uncommitted changes: budget-gui/src/main.rs.*Only committed sources'
    Assert-NoPublishedVersion $root
}

Test-Case 'archive refuses untracked release inputs that no commit contains' {
    $root = New-Fixture 'untracked release input'
    Initialize-TestRepository $root
    Write-Text (Join-Path $root 'budget-core/src/new_module.rs') "pub fn new_module() {}`n"
    Assert-ArchiveFails $root 'uncommitted changes: budget-core/src/new_module.rs'
    Assert-NoPublishedVersion $root
}

Test-Case 'archive records a dirty tree outside the release inputs without blocking' {
    $root = New-Fixture 'dirty notes outside inputs'
    Initialize-TestRepository $root
    $head = (Invoke-TestGit $root @('rev-parse', 'HEAD'))[0]
    Write-Text (Join-Path $root 'reference/source.rs') 'edited reference notes'
    Invoke-Archive $root
    $manifest = Read-ReleaseManifest $root
    Assert-True ($manifest.git_commit -ceq $head) 'A dirty tree must still record the HEAD commit.'
    Assert-True ($manifest.git_clean -eq $false) 'A dirty tree must be recorded as not clean.'
}

Test-Case 'archive refuses a git work tree that has no commit to name' {
    $root = New-Fixture 'git without commit'
    Remove-PlantedGitFolders $root
    [void](Invoke-TestGit $root @('init', '-q', '-b', 'main'))
    Assert-ArchiveFails $root 'HEAD has no commit'
    Assert-NoPublishedVersion $root
}

Test-Case 'archive of a folder that is not a git work tree records null git fields' {
    $root = New-Fixture 'folder without git'
    Invoke-Archive $root
    $manifest = Read-ReleaseManifest $root
    Assert-True ($null -eq $manifest.git_commit -and $null -eq $manifest.git_clean) 'A folder without its own .git must record null git fields.'
}

Test-Case 'a CRLF checkout of the same source archives with the LF build manifest' {
    $root = New-Fixture 'crlf checkout'
    Write-Text (Join-Path $root 'budget-gui/src/main.rs') "fn main() {`n}`n"
    New-Executable $root 'budget app.exe' '0.3.0' 'fixture-build-a' 'first'
    Convert-TextInputsToCrlf $root
    Assert-True ([IO.File]::ReadAllText((Join-Path $root 'budget-gui/src/main.rs')).Contains("`r`n")) 'Fixture did not become a CRLF checkout.'
    Invoke-Archive $root
    Assert-True (Test-Path -LiteralPath (Join-Path $root 'local-releases/v0.3.0/release.json') -PathType Leaf) 'A CRLF checkout of the same commit must archive.'
}

Test-Case 'text digests ignore CRLF, binary digests do not, and text matches the build.rs known answer' {
    $digestRoot = Join-Path $testRoot 'digest-identity'
    Write-Text (Join-Path $digestRoot 'lf.txt') "a`nb`n"
    Write-Text (Join-Path $digestRoot 'crlf.txt') "a`r`nb`r`n"
    [IO.File]::WriteAllBytes((Join-Path $digestRoot 'binary-lf.bin'), [byte[]](0, 10))
    [IO.File]::WriteAllBytes((Join-Path $digestRoot 'binary-crlf.bin'), [byte[]](0, 13, 10))
    # Known answer shared with budget-gui/build.rs text_line_endings_are_normalised_but_binary_...
    Assert-True ([BudgetArchiveTestHash]::FileHash((Join-Path $digestRoot 'crlf.txt')) -ceq '78ed6781f136a14e') 'Text digest must match the build.rs known answer.'
    Assert-True ([BudgetArchiveTestHash]::FileHash((Join-Path $digestRoot 'lf.txt')) -ceq '78ed6781f136a14e') 'LF text must match the same known answer.'
    Assert-True ([BudgetArchiveTestHash]::FileHash((Join-Path $digestRoot 'binary-lf.bin')) -cne [BudgetArchiveTestHash]::FileHash((Join-Path $digestRoot 'binary-crlf.bin'))) 'Binary bytes must be hashed exactly.'
}

Test-Case 'resolves relative archive paths against the repository root, not the current directory' {
    $root = New-Fixture 'relative paths'
    $elsewhere = Join-Path $testRoot 'elsewhere-current-directory'
    [void][IO.Directory]::CreateDirectory($elsewhere)
    Push-Location -LiteralPath $elsewhere
    try {
        & (Join-Path $root 'scripts/archive-release.ps1') -ExecutablePath 'target/budget app.exe' -OutputDirectory 'local-releases' -BuildManifestPath 'target/release/build/budget-gui-fixture/out/build-inputs-fixture-build-a.json' | Out-Null
    } finally { Pop-Location }
    Assert-True (Test-Path -LiteralPath (Join-Path $root 'local-releases/v0.3.0/release.json') -PathType Leaf) 'Relative paths must resolve against the repository root.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $elsewhere 'local-releases'))) 'Relative output must not land in the current directory.'
}

Test-Case 'archive help text says relative paths resolve against the repository root' {
    # Help text wraps across lines, so compare it with every run of whitespace collapsed to one space.
    $description = (((Get-Help -Name $scriptUnderTest -Full).description | ForEach-Object { $_.Text }) -join ' ') -replace '\s+', ' '
    Assert-True ($description -match 'relative paths.*repository root') 'Archive help must say that relative paths resolve against the repository root.'
}

Test-Case 'installer never clobbers or deletes user files and packages only the archived build' {
    $nsi = Get-Content -LiteralPath (Join-Path $repository 'installer/budget.nsi') -Raw
    Assert-True ($nsi -notmatch 'lib\.rs') 'Installer must not ship or delete a stray lib.rs.'
    Assert-True ($nsi -notmatch '(?m)^\s*Delete\s+"\$INSTDIR\\sample\.bgt"') 'Uninstaller must not delete a user-editable sample.bgt.'
    Assert-True ($nsi -match 'IfFileExists\s+"\$INSTDIR\\sample\.bgt"') 'Installer must not unconditionally overwrite sample.bgt.'
    Assert-True ($nsi -notmatch '!define VERSION "') 'Installer must not carry a stale default version.'
    Assert-True ($nsi -match '!ifndef VERSION') 'Installer must fail when VERSION is not supplied.'
    Assert-True ($nsi -match '!ifndef PAYLOAD') 'Installer must fail when the archived executable is not supplied.'
    Assert-True ($nsi -match '!ifndef ICON_PAYLOAD') 'Installer must fail when the archived icon is not supplied.'
    Assert-True ($nsi -match [regex]::Escape('File "/oname=${EXE}" "${PAYLOAD}"')) 'Installer must package the supplied payload.'
    Assert-True ($nsi -match '(?m)^Icon "\$\{ICON_PAYLOAD\}"\s*$') 'Setup must use the verified archived icon.'
    Assert-True ($nsi -match '(?m)^UninstallIcon "\$\{ICON_PAYLOAD\}"\s*$') 'Uninstall must use the verified archived icon.'
    Assert-True ($nsi -notmatch 'budget-gui\\assets\\budget-calendar\.ico') 'Installer must not read the current source icon.'
    Assert-True ($nsi -notmatch 'target\\release\\budget-gui\.exe') 'Installer must not package whatever is in target\release.'
    Assert-True ($nsi -match [regex]::Escape('"DisplayVersion" "${VERSION}"')) 'DisplayVersion must come from VERSION.'
}

Test-Case 'installer saves the previous .bgt association and removes only what it created' {
    $nsi = Get-Content -LiteralPath (Join-Path $repository 'installer/budget.nsi') -Raw
    $installStart = $nsi.IndexOf('Section "Install"', [StringComparison]::Ordinal)
    $uninstallStart = $nsi.IndexOf('Section "Uninstall"', [StringComparison]::Ordinal)
    Assert-True ($installStart -ge 0 -and $uninstallStart -gt $installStart) 'Installer sections are missing.'
    $install = $nsi.Substring($installStart, $uninstallStart - $installStart)
    $uninstall = $nsi.Substring($uninstallStart)
    $read = $install.IndexOf('ReadRegStr $0 HKCU "Software\Classes\.bgt" ""', [StringComparison]::Ordinal)
    $replace = $install.IndexOf('WriteRegStr HKCU "Software\Classes\.bgt" "" "BudgetCalendar.bgt"', [StringComparison]::Ordinal)
    Assert-True ($read -ge 0 -and $replace -gt $read) 'Install must read the previous .bgt association before replacing it.'
    Assert-True ($uninstall.IndexOf('DeleteRegKey HKCU "Software\Classes\.bgt"', [StringComparison]::Ordinal) -lt 0) 'Uninstall must not delete the whole .bgt key.'
    Assert-True ($uninstall.IndexOf('DeleteRegKey /ifempty HKCU "Software\Classes\.bgt"', [StringComparison]::Ordinal) -ge 0) 'Uninstall must delete the .bgt key only when it is empty.'
    $restore = $uninstall.IndexOf('ReadRegStr $0 HKCU "${UNINST_KEY}" "PreviousBgtDefault"', [StringComparison]::Ordinal)
    $removeUninstallKey = $uninstall.IndexOf('DeleteRegKey HKCU "${UNINST_KEY}"', [StringComparison]::Ordinal)
    Assert-True ($restore -ge 0 -and $restore -lt $removeUninstallKey) 'Uninstall must read the saved association before removing its record.'
}

Test-Case 'installer wrapper refuses a VERSION that differs from budget-gui/Cargo.toml before packaging' {
    $caught = $null
    try { & (Join-Path $repository 'installer/build-installer.ps1') -Version '0.0.1' | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match 'does not match budget-gui/Cargo.toml') "Expected a version refusal, received: $caught"
}

function New-InstallerFixture([string]$Name) {
    $root = New-Fixture $Name
    Invoke-Archive $root
    Copy-Item -LiteralPath (Join-Path $repository 'installer/build-installer.ps1') -Destination (Join-Path $root 'installer/build-installer.ps1') -Force
    Copy-Item -LiteralPath (Join-Path $repository 'installer/budget.nsi') -Destination (Join-Path $root 'installer/budget.nsi') -Force
    Write-Text (Join-Path $root 'reference/sample.bgt') 'fixture sample budget, generated for tests'
    return $root
}

Test-Case 'installer wrapper passes the archived executable and icon even when the current icon changes' {
    $root = New-InstallerFixture 'installer passes archive'
    Write-Text (Join-Path $root 'budget-gui/assets/budget-calendar.ico') 'new unarchived artwork'
    $fake = Join-Path $root 'tools/fake-makensis.cmd'
    Write-Text $fake "@echo off`r`necho %*> `"%~dp0args.txt`"`r`n"
    & (Join-Path $root 'installer/build-installer.ps1') -Makensis $fake | Out-Null
    $recorded = Get-Content -LiteralPath (Join-Path $root 'tools/args.txt') -Raw
    Assert-True ($recorded -match '/DVERSION=0\.3\.0') "Compiler did not receive VERSION: $recorded"
    Assert-True ($recorded -match '/DPAYLOAD=.*local-releases.v0\.3\.0.budget-gui\.exe') "Compiler did not receive the archived executable: $recorded"
    $expectedIcon = '/DICON_PAYLOAD=' + (Join-Path $root 'local-releases/v0.3.0/budget-calendar.ico')
    Assert-True ($recorded.Contains($expectedIcon)) "Compiler did not receive the archived icon: $recorded"
    Assert-True (-not $recorded.Contains((Join-Path $root 'budget-gui/assets/budget-calendar.ico'))) 'Compiler received the unarchived current icon.'
    Assert-True ($recorded -match 'budget\.nsi') "Compiler did not receive the script: $recorded"
}

Test-Case 'installer wrapper refuses an archived icon that differs from its release manifest' {
    $root = New-InstallerFixture 'installer tampered icon'
    Write-Text (Join-Path $root 'local-releases/v0.3.0/budget-calendar.ico') 'tampered icon after archiving'
    $caught = $null
    try { & (Join-Path $root 'installer/build-installer.ps1') -Makensis 'unused' | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match 'Archived icon differs') "Expected an icon tamper refusal before compiler lookup, received: $caught"
}

Test-Case 'installer wrapper refuses a missing archived icon even when a current source icon exists' {
    $root = New-InstallerFixture 'installer missing icon'
    Remove-Item -LiteralPath (Join-Path $root 'local-releases/v0.3.0/budget-calendar.ico')
    $caught = $null
    try { & (Join-Path $root 'installer/build-installer.ps1') -Makensis 'unused' | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match 'Archived icon is missing') "Expected a missing archive icon refusal, received: $caught"
}

Test-Case 'installer wrapper refuses missing or malformed archived icon checksums' {
    foreach ($checksum in @($null, 'invalid-checksum')) {
        $name = if ($null -eq $checksum) { 'missing' } else { 'malformed' }
        $root = New-InstallerFixture "installer $name icon checksum"
        $manifestPath = Join-Path $root 'local-releases/v0.3.0/release.json'
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ($null -eq $checksum) { $manifest.files.PSObject.Properties.Remove('budget-calendar.ico') }
        else { $manifest.files.'budget-calendar.ico' = $checksum }
        Write-Text $manifestPath ($manifest | ConvertTo-Json -Depth 5)
        $caught = $null
        try { & (Join-Path $root 'installer/build-installer.ps1') -Makensis 'unused' | Out-Null } catch { $caught = $_.Exception.Message }
        Assert-True ($null -ne $caught -and $caught -match 'missing a valid SHA256 checksum') "Expected a $name icon checksum refusal, received: $caught"
    }
}

Test-Case 'installer wrapper refuses an archived executable that differs from its release manifest' {
    $root = New-InstallerFixture 'installer tampered archive'
    Write-Text (Join-Path $root 'local-releases/v0.3.0/budget-gui.exe') 'tampered after archiving'
    $caught = $null
    try { & (Join-Path $root 'installer/build-installer.ps1') -Makensis 'unused' | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match 'differs') "Expected a tamper refusal, received: $caught"
}

Test-Case 'installer wrapper refuses to package a version that was never archived' {
    $root = New-Fixture 'installer without archive'
    Copy-Item -LiteralPath (Join-Path $repository 'installer/build-installer.ps1') -Destination (Join-Path $root 'installer/build-installer.ps1') -Force
    $caught = $null
    try { & (Join-Path $root 'installer/build-installer.ps1') -Makensis 'unused' | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match 'archived release') "Expected an archive refusal, received: $caught"
}

Test-Case 'installer script compiles with NSIS when it is installed' {
    $makensis = @(
        (Join-Path ([Environment]::GetEnvironmentVariable('ProgramFiles(x86)')) 'NSIS/makensis.exe'),
        (Join-Path $env:ProgramFiles 'NSIS/makensis.exe')
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if ($null -eq $makensis) {
        Write-Host 'SKIP NSIS compile: makensis is not installed.'
        return
    }
    $root = New-InstallerFixture 'installer compiles'
    & (Join-Path $root 'installer/build-installer.ps1') -Makensis $makensis | Out-Null
    Assert-True (Test-Path -LiteralPath (Join-Path $root 'target/BudgetCalendar-0.3.0-setup.exe') -PathType Leaf) 'NSIS did not produce the setup program.'
}

Write-Host "$passed release archive tests passed. Fixtures retained at $testRoot"
