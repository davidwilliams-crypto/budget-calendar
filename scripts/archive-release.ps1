<#
.SYNOPSIS
Preserves a freshly built local Budget Calendar release without replacing earlier builds.
.DESCRIPTION
Archives target/release/budget-gui.exe into local-releases/vVERSION with its build input manifest,
a source ZIP of the release inputs, and release.json (build identity, SHA256 checksums, and the git
commit with whether the work tree was clean). Relative paths in -ExecutablePath, -OutputDirectory and
-BuildManifestPath are resolved against the repository root, not the current directory.
When the repository is a git work tree, uncommitted changes to release inputs are refused.
.PARAMETER ExecutablePath
Built executable to archive. Default: target/release/budget-gui.exe under the repository root.
.PARAMETER OutputDirectory
Folder that receives vVERSION. Default: local-releases under the repository root.
.PARAMETER BuildManifestPath
Build input manifest that matches the executable. Default: the single matching manifest under target/release/build.
.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/archive-release.ps1
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExecutablePath,
    [string]$OutputDirectory,
    [string]$BuildManifestPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')

function Resolve-RepositoryPath([string]$Path) {
    # Relative paths belong to the repository, so the archive behaves the same from any directory.
    return [IO.Path]::GetFullPath([IO.Path]::Combine($repository, $Path))
}

function Assert-InRepository([string]$Path, [bool]$AllowRoot = $false) {
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    if (($AllowRoot -and $full.Equals($repository, [StringComparison]::OrdinalIgnoreCase)) -or
        $full.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        return $full
    }
    throw "Path must stay within this repository: $full"
}

function Assert-NoReparsePoint([string]$Path) {
    $full = Assert-InRepository $Path $true
    while ($full.Length -ge $repository.Length) {
        if (Test-Path -LiteralPath $full) {
            $item = Get-Item -LiteralPath $full -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Archive paths may not traverse a link or reparse point: $full"
            }
        }
        if ($full.Equals($repository, [StringComparison]::OrdinalIgnoreCase)) { break }
        $full = [IO.Path]::GetDirectoryName($full)
    }
}

function Require-File([string]$Path, [string]$Description) {
    $full = Assert-InRepository $Path
    Assert-NoReparsePoint $full
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "Missing $Description`: $full" }
    return $full
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ExecutableMetadata([string]$Path) {
    $tempRoot = Assert-InRepository (Join-Path $repository 'target')
    Assert-NoReparsePoint $tempRoot
    [void][IO.Directory]::CreateDirectory($tempRoot)
    $stem = Join-Path $tempRoot ('archive-metadata-' + [Guid]::NewGuid().ToString('N'))
    $stdout = $stem + '.json'
    $stderr = $stem + '.stderr'
    try {
        # --version-json exits before GUI creation or budget loading. Keep console fixtures hidden too.
        $process = Start-Process -FilePath $Path -ArgumentList '--version-json' -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $null = $process.Handle # cache the handle so ExitCode is readable after exit (Windows PowerShell 5.1)
        if (-not $process.WaitForExit(15000)) {
            $process.Kill()
            $process.WaitForExit()
            throw 'Executable --version-json did not finish within 15 seconds.'
        }
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Executable --version-json failed with exit code $($process.ExitCode)." }
        $metadata = [IO.File]::ReadAllText($stdout) | ConvertFrom-Json
        foreach ($field in @('app', 'version', 'build_id', 'target', 'profile')) {
            if ($null -eq $metadata.PSObject.Properties[$field] -or
                $metadata.$field -isnot [string] -or [string]::IsNullOrWhiteSpace($metadata.$field)) {
                throw "Executable metadata is missing string field '$field'."
            }
        }
        if ($metadata.app -cne 'Budget Calendar') { throw 'Executable metadata identifies a different application.' }
        if ($metadata.version -cnotmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {
            throw 'Executable metadata contains an invalid semantic version.'
        }
        if ($metadata.build_id -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') {
            throw 'Executable metadata contains an unsafe build identifier.'
        }
        return $metadata
    } finally {
        foreach ($file in @($stdout, $stderr)) {
            if (Test-Path -LiteralPath $file -PathType Leaf) { Remove-Item -LiteralPath $file -Force }
        }
    }
}

function Get-SourceFiles {
    # Deliberately enumerate application inputs, never the repository as a whole.
    # In particular, reference/, analysis/, target/, .git/, and user budgets are excluded.
    # Keep this allowlist aligned with budget-gui/build.rs source_inputs.
    $files = [Collections.Generic.SortedDictionary[string, string]]::new([StringComparer]::Ordinal)
    foreach ($relative in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.gitattributes', '.gitignore',
        'SPEC.md', 'CHANGELOG.md', 'AGENTS.md', 'budget-gui/Cargo.toml', 'budget-gui/build.rs',
        'budget-core/Cargo.toml', 'vendor/webbrowser/Cargo.toml', 'docs/ux.md',
        'README.md', 'budget-engine/Cargo.toml', 'budget-mobile/package.json', 'budget-mobile/README.md', 'budget-mobile/ENGINE-CONTRACT.md', 'budget-mobile/SERVER-CONTRACT.md', 'budget-mobile/server/DEPLOYMENT.md', 'budget-mobile/Dockerfile', 'budget-mobile/.dockerignore')) {
        $files.Add($relative, (Require-File (Join-Path $repository $relative) 'source input'))
    }
    foreach ($rule in @(
        @{ Directory = 'budget-gui/src'; Extensions = @('.rs') },
        @{ Directory = 'budget-gui/assets'; Extensions = @('.ico', '.png', '.svg') },
        @{ Directory = 'budget-core/src'; Extensions = @('.rs') },
        @{ Directory = 'budget-core/tests'; Extensions = @('.rs') },
        @{ Directory = 'budget-engine/src'; Extensions = @('.rs') },
        @{ Directory = 'budget-engine/tests'; Extensions = @('.mjs') },
        @{ Directory = 'budget-mobile/public'; Extensions = @('.mjs', '.js', '.html', '.css', '.webmanifest', '.png', '.wasm') },
        @{ Directory = 'budget-mobile/server'; Extensions = @('.mjs') },
        @{ Directory = 'budget-mobile/tests'; Extensions = @('.mjs') },
        @{ Directory = 'vendor/webbrowser/src'; Extensions = @('.rs') },
        @{ Directory = 'scripts'; Extensions = @('.ps1') },
        @{ Directory = 'installer'; Extensions = @('.nsi', '.ps1') }
    )) {
        $directory = Assert-InRepository (Join-Path $repository $rule.Directory)
        Assert-NoReparsePoint $directory
        if (-not (Test-Path -LiteralPath $directory -PathType Container)) { throw "Missing source directory: $directory" }
        Add-SourceDirectory $directory $rule.Extensions $files
    }
    foreach ($name in @('LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE.txt', 'README.md')) {
        $relative = 'vendor/webbrowser/' + $name
        $path = Join-Path $repository $relative
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $files.Add($relative, (Require-File $path 'vendor documentation'))
        }
    }
    return ,$files
}

function Add-SourceDirectory([string]$Directory, [string[]]$Extensions, $Files) {
    foreach ($item in Get-ChildItem -LiteralPath $Directory -Force) {
        if (@('target', 'local-releases', '.git', 'docs', 'reference', 'analysis', 'node_modules', 'data', 'private', 'secrets', 'credentials', 'backups') -contains $item.Name) { continue }
        if ($item.PSIsContainer) {
            Assert-NoReparsePoint $item.FullName
            Add-SourceDirectory $item.FullName $Extensions $Files
        } elseif ($Extensions -contains $item.Extension.ToLowerInvariant()) {
            $full = Require-File $item.FullName 'source input'
            $relative = $full.Substring($repository.Length + 1).Replace('\', '/')
            $Files.Add($relative, $full)
        }
    }
}

function Initialize-InputHash {
    if ($null -eq ('BudgetReleaseArchiveHash' -as [type])) {
        # Same dependency-free FNV-1a algorithm and input identity as build.rs: a file with no NUL byte is
        # text, hashed with CRLF read as LF, so a Windows checkout and an LF checkout digest identically.
        # Unchecked arithmetic is intentional.
        Add-Type -TypeDefinition @'
using System;
using System.IO;
public static class BudgetReleaseArchiveHash {
    public static string BytesHash(byte[] bytes) {
        bool text = Array.IndexOf(bytes, (byte)0) < 0;
        ulong value = 14695981039346656037UL;
        for (int i = 0; i < bytes.Length; i++) {
            if (text && bytes[i] == 13 && i + 1 < bytes.Length && bytes[i + 1] == 10) continue;
            value = unchecked((value ^ bytes[i]) * 1099511628211UL);
        }
        return value.ToString("x16");
    }
    public static string StreamHash(Stream stream) {
        using (MemoryStream buffer = new MemoryStream()) {
            stream.CopyTo(buffer);
            return BytesHash(buffer.ToArray());
        }
    }
    public static string FileHash(string path) {
        return BytesHash(File.ReadAllBytes(path));
    }
}
'@
    }
}

function Invoke-Git([string[]]$Arguments) {
    # Git writes warnings to stderr; the exit code decides success, so stderr is discarded here.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(& git -C $repository @Arguments 2>$null)
        $exitCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
    return [pscustomobject]@{ ExitCode = $exitCode; Output = $output }
}

function Get-SourceControlState($AllowedSources) {
    # Only a folder that holds git metadata can name a commit: a .git file (linked worktree) or a .git
    # directory with HEAD. Other folders, including test fixtures that sit inside this repository, record
    # null and are not checked against git. Once git metadata exists, every git failure is an error.
    $marker = Join-Path $repository '.git'
    $hasGitMetadata = (Test-Path -LiteralPath $marker -PathType Leaf) -or
        (Test-Path -LiteralPath (Join-Path $marker 'HEAD') -PathType Leaf)
    if (-not $hasGitMetadata) {
        return @{ Commit = $null; Clean = $null }
    }
    if ($null -eq (Get-Command git -CommandType Application -ErrorAction SilentlyContinue)) {
        throw 'Git is required to check this work tree before archiving. Install Git, then archive again.'
    }
    $top = Invoke-Git @('rev-parse', '--show-toplevel')
    $topPath = if ($top.Output.Count -eq 1) { [IO.Path]::GetFullPath(([string]$top.Output[0]).Replace('/', '\')).TrimEnd('\') } else { '' }
    if ($top.ExitCode -ne 0 -or -not $topPath.Equals($repository, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Git does not treat $repository as its work tree root; refusing to archive."
    }
    $head = Invoke-Git @('rev-parse', '--verify', 'HEAD')
    if ($head.ExitCode -ne 0 -or $head.Output.Count -ne 1 -or ([string]$head.Output[0]) -cnotmatch '^[0-9a-f]{40,64}$') {
        throw 'Git HEAD has no commit to name. Commit the release sources, rebuild, then archive again.'
    }
    $status = Invoke-Git @('status', '--porcelain', '--untracked-files=all')
    if ($status.ExitCode -ne 0) { throw 'git status failed; the release sources cannot be verified as committed.' }
    $changed = @()
    foreach ($line in $status.Output) {
        $text = [string]$line
        if ($text.Length -lt 4) { continue }
        foreach ($part in $text.Substring(3).Split(@(' -> '), [StringSplitOptions]::None)) {
            $changed += $part.Trim('"')
        }
    }
    $uncommitted = @($changed | Where-Object { $AllowedSources.ContainsKey($_) })
    if ($uncommitted.Count -gt 0) {
        throw ('Refusing to archive: release inputs have uncommitted changes: ' + ($uncommitted -join ', ') +
            '. Only committed sources can be archived, because the source ZIP and build identity must match a commit. ' +
            'Commit these files, rebuild, then archive again.')
    }
    return @{ Commit = [string]$head.Output[0]; Clean = ($changed.Count -eq 0) }
}

function Get-BuildInputs($Metadata, $AllowedSources) {
    $path = $BuildManifestPath
    if ([string]::IsNullOrWhiteSpace($path)) {
        $buildDirectory = Assert-InRepository (Join-Path $repository 'target/release/build')
        Assert-NoReparsePoint $buildDirectory
        $candidates = @()
        if (Test-Path -LiteralPath $buildDirectory -PathType Container) {
            foreach ($directory in Get-ChildItem -LiteralPath $buildDirectory -Directory -Filter 'budget-gui-*') {
                $candidate = Join-Path $directory.FullName ('out/build-inputs-' + $Metadata.build_id + '.json')
                if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                    $candidates += Require-File $candidate 'build input manifest'
                }
            }
        }
        if ($candidates.Count -eq 0) {
            throw "No build input manifest for $($Metadata.build_id). Build the release from the final sources first."
        }
        $path = @($candidates | Sort-Object)[0]
        foreach ($candidate in $candidates) {
            if ((Get-Sha256 $candidate) -cne (Get-Sha256 $path)) {
                throw 'Multiple different build input manifests claim this build identity.'
            }
        }
    }
    $path = Require-File $path 'build input manifest'
    $text = [IO.File]::ReadAllText($path)
    try { $inputs = $text | ConvertFrom-Json }
    catch { throw 'Build input manifest is invalid JSON.' }
    foreach ($field in @('build_id', 'target', 'profile')) {
        if ($null -eq $inputs.PSObject.Properties[$field] -or $inputs.$field -cne $Metadata.$field) {
            throw "Build input manifest '$field' does not match the executable."
        }
    }
    if ($null -eq $inputs.PSObject.Properties['compiler'] -or [string]::IsNullOrWhiteSpace($inputs.compiler) -or
        $null -eq $inputs.PSObject.Properties['inputs'] -or $inputs.inputs -isnot [array]) {
        throw 'Build input manifest is missing compiler or source inventory.'
    }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $validated = [Collections.Generic.SortedDictionary[string, object]]::new([StringComparer]::Ordinal)
    Initialize-InputHash
    foreach ($inputFile in $inputs.inputs) {
        if ($null -eq $inputFile.PSObject.Properties['path'] -or $inputFile.path -isnot [string] -or
            [string]::IsNullOrWhiteSpace($inputFile.path) -or $inputFile.path.Contains('\') -or
            $inputFile.path.Contains(':') -or $inputFile.path.StartsWith('/') -or
            @($inputFile.path.Split('/') | Where-Object { $_ -eq '..' -or $_ -eq '.' -or $_ -eq '' }).Count -gt 0) {
            throw 'Build input manifest contains an unsafe source path.'
        }
        $relative = $inputFile.path
        if (-not $seen.Add($relative)) { throw "Build input manifest contains a duplicate path: $relative" }
        if (-not $AllowedSources.ContainsKey($relative)) { throw "Build input path is outside the source allowlist: $relative" }
        if ($null -eq $inputFile.PSObject.Properties['fnv1a'] -or $inputFile.fnv1a -isnot [string] -or
            $inputFile.fnv1a -cnotmatch '^[a-f0-9]{16}$') {
            throw "Build input manifest contains an invalid digest: $relative"
        }
        $source = $AllowedSources[$relative]
        if ([BudgetReleaseArchiveHash]::FileHash($source) -cne $inputFile.fnv1a) {
            throw "Source input changed since this executable was built: $relative. Rebuild before archiving."
        }
        $validated.Add($relative, @{ Source = $source; Digest = $inputFile.fnv1a })
    }
    if ($seen.Count -ne $AllowedSources.Count) { throw 'Build input manifest does not contain the complete source inventory.' }
    return @{ Text = $text; Files = $validated }
}

function Assert-ExistingRelease([string]$Directory, $Metadata, [string]$ExecutableHash) {
    Assert-NoReparsePoint $Directory
    $manifestPath = Require-File (Join-Path $Directory 'release.json') 'existing release manifest'
    try { $saved = [IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json }
    catch { throw "Existing release has an unreadable manifest; it will not be overwritten: $Directory" }
    foreach ($field in @('app', 'version', 'build_id', 'target', 'profile')) {
        if ($null -eq $saved.PSObject.Properties[$field] -or $saved.$field -cne $Metadata.$field) {
            throw "Version $($Metadata.version) already contains a different build. Bump the version before archiving."
        }
    }
    if ($null -eq $saved.PSObject.Properties['sha256'] -or $saved.sha256 -cne $ExecutableHash) {
        throw "Version $($Metadata.version) already contains a different executable. Bump the version before archiving."
    }
    foreach ($name in @('budget-gui.exe', 'budget-calendar.ico', 'CHANGELOG.md', 'source.zip', 'build-inputs.json')) {
        $path = Require-File (Join-Path $Directory $name) 'existing archived file'
        if ($null -eq $saved.PSObject.Properties['files'] -or $null -eq $saved.files.PSObject.Properties[$name] -or
            (Get-Sha256 $path) -cne $saved.files.$name) {
            throw "Existing release file differs from its manifest; it will not be overwritten: $name"
        }
    }
    if ((Get-Sha256 (Join-Path $Directory 'budget-gui.exe')) -cne $ExecutableHash) {
        throw 'Existing archived executable differs from this build; it will not be overwritten.'
    }
}

if ([string]::IsNullOrWhiteSpace($ExecutablePath)) { $ExecutablePath = Join-Path $repository 'target/release/budget-gui.exe' }
else { $ExecutablePath = Resolve-RepositoryPath $ExecutablePath }
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { $OutputDirectory = Join-Path $repository 'local-releases' }
else { $OutputDirectory = Resolve-RepositoryPath $OutputDirectory }
if (-not [string]::IsNullOrWhiteSpace($BuildManifestPath)) { $BuildManifestPath = Resolve-RepositoryPath $BuildManifestPath }
$executable = Require-File $ExecutablePath 'executable'
$output = Assert-InRepository $OutputDirectory
Assert-NoReparsePoint $output
$initialExecutableHash = Get-Sha256 $executable
$metadata = Get-ExecutableMetadata $executable
$manifest = Require-File (Join-Path $repository 'budget-gui/Cargo.toml') 'GUI Cargo manifest'
$packageText = [IO.File]::ReadAllText($manifest)
$packageSection = [regex]::Match($packageText, '(?ms)^\[package\]\s*\r?\n(?<content>.*?)(?=^\[|\z)')
$versionMatch = [regex]::Match($packageSection.Groups['content'].Value, '(?m)^version\s*=\s*"(?<version>[^"]+)"\s*$')
if (-not $versionMatch.Success -or $versionMatch.Groups['version'].Value -cne $metadata.version) {
    throw "Executable version $($metadata.version) does not match budget-gui/Cargo.toml. Build the current version first."
}
$changelog = Require-File (Join-Path $repository 'CHANGELOG.md') 'changelog'
$dateMatch = [regex]::Match([IO.File]::ReadAllText($changelog), '(?m)^## \[' + [regex]::Escape($metadata.version) + '\] - (?<date>\d{4}-\d{2}-\d{2})\s*$')
$releaseDate = [datetime]::MinValue
if (-not $dateMatch.Success -or -not [datetime]::TryParseExact($dateMatch.Groups['date'].Value, 'yyyy-MM-dd',
    [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::None, [ref]$releaseDate)) {
    throw "CHANGELOG.md needs a dated '## [$($metadata.version)] - YYYY-MM-DD' section."
}
$icon = Require-File (Join-Path $repository 'budget-gui/assets/budget-calendar.ico') 'application icon'
$allowedSources = Get-SourceFiles
$sourceControl = Get-SourceControlState $allowedSources
$buildInputs = Get-BuildInputs $metadata $allowedSources
$executableHash = Get-Sha256 $executable
if ($executableHash -cne $initialExecutableHash) {
    throw 'Executable changed while reading its build identity; rebuild before archiving.'
}
$release = Assert-InRepository (Join-Path $output ('v' + $metadata.version))
if (Test-Path -LiteralPath $release) {
    Assert-ExistingRelease $release $metadata $executableHash
    Write-Output "Already archived identical release: $release"
    return
}

[void][IO.Directory]::CreateDirectory($output)
Assert-NoReparsePoint $output
$staging = Assert-InRepository (Join-Path $output ('.v' + $metadata.version + '.staging-' + [Guid]::NewGuid().ToString('N')))
try {
    [void][IO.Directory]::CreateDirectory($staging)
    Copy-Item -LiteralPath $executable -Destination (Join-Path $staging 'budget-gui.exe')
    Copy-Item -LiteralPath $icon -Destination (Join-Path $staging 'budget-calendar.ico')
    Copy-Item -LiteralPath $changelog -Destination (Join-Path $staging 'CHANGELOG.md')
    [IO.File]::WriteAllText((Join-Path $staging 'build-inputs.json'), $buildInputs.Text, [Text.UTF8Encoding]::new($false))
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::Open((Join-Path $staging 'source.zip'), [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($entry in $buildInputs.Files.GetEnumerator()) {
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $entry.Value.Source, $entry.Key, [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally { $zip.Dispose() }
    $zip = [IO.Compression.ZipFile]::OpenRead((Join-Path $staging 'source.zip'))
    try {
        foreach ($entry in $zip.Entries) {
            $stream = $entry.Open()
            try { $digest = [BudgetReleaseArchiveHash]::StreamHash($stream) }
            finally { $stream.Dispose() }
            if ($digest -cne $buildInputs.Files[$entry.FullName].Digest) {
                throw "Source input changed while archiving: $($entry.FullName). Rebuild before publishing."
            }
        }
    } finally { $zip.Dispose() }
    foreach ($pair in @(
        @{ File = 'budget-calendar.ico'; Source = 'budget-gui/assets/budget-calendar.ico' },
        @{ File = 'CHANGELOG.md'; Source = 'CHANGELOG.md' }
    )) {
        if ([BudgetReleaseArchiveHash]::FileHash((Join-Path $staging $pair.File)) -cne $buildInputs.Files[$pair.Source].Digest) {
            throw "Source asset changed while archiving: $($pair.File). Rebuild before publishing."
        }
    }
    if ((Get-Sha256 (Join-Path $staging 'budget-gui.exe')) -cne $executableHash) {
        throw 'Executable changed while archiving; build again before publishing a release.'
    }
    $fileHashes = [ordered]@{}
    foreach ($name in @('budget-gui.exe', 'budget-calendar.ico', 'CHANGELOG.md', 'source.zip', 'build-inputs.json')) {
        $fileHashes[$name] = Get-Sha256 (Join-Path $staging $name)
    }
    $releaseManifest = [ordered]@{
        schema_version = 1
        app = $metadata.app
        version = $metadata.version
        build_id = $metadata.build_id
        release_date = $releaseDate.ToString('yyyy-MM-dd')
        archived_at_utc = [datetime]::UtcNow.ToString('o')
        target = $metadata.target
        profile = $metadata.profile
        sha256 = $executableHash
        git_commit = $sourceControl.Commit
        git_clean = $sourceControl.Clean
        files = $fileHashes
    }
    [IO.File]::WriteAllText((Join-Path $staging 'release.json'), ($releaseManifest | ConvertTo-Json -Depth 5) + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    # Same-parent rename publishes the complete directory at once and never overwrites an existing version.
    Assert-NoReparsePoint $staging
    Assert-NoReparsePoint $release
    [IO.Directory]::Move($staging, $release)
    Write-Output "Archived $($metadata.app) v$($metadata.version) ($($metadata.build_id)): $release"
} finally {
    if (Test-Path -LiteralPath $staging -PathType Container) {
        $checkedStaging = Assert-InRepository $staging
        Assert-NoReparsePoint $checkedStaging
        if ([IO.Path]::GetDirectoryName($checkedStaging) -cne $output -or
            [IO.Path]::GetFileName($checkedStaging) -cnotmatch '^\.v[0-9].*\.staging-[a-f0-9]{32}$') {
            throw "Refusing cleanup of unexpected staging path: $checkedStaging"
        }
        Remove-Item -LiteralPath $checkedStaging -Recurse -Force
    }
}
