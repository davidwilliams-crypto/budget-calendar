#Requires -Version 5.1
# Run directly with Windows PowerShell; no Pester or other dependencies are required.
# Generated fixtures only; the gate is driven with stub steps, so it never runs itself.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$fixture = Join-Path $root ('target/release-gate-tests-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$powershell = Join-Path $PSHOME 'powershell.exe'
$script:checks = 0
function Assert-True([bool]$Value, [string]$Message) { if (-not $Value) { throw $Message }; $script:checks++ }
function Assert-Fails([scriptblock]$Action, [string]$Pattern) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match $Pattern) "Expected '$Pattern', got '$caught'"
}
function Write-Fixture([string]$Name, [string]$Text) {
    $path = Join-Path $fixture $Name
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
    [IO.File]::WriteAllText($path, $Text, [Text.UTF8Encoding]::new($false))
}

# --- Gate reporting -------------------------------------------------------------------------
function Invoke-Gate([object[]]$Steps, [string[]]$Extra = @()) {
    $file = Join-Path $fixture ([Guid]::NewGuid().ToString('N') + '.json')
    [IO.File]::WriteAllText($file, (ConvertTo-Json -InputObject @($Steps) -Depth 4), [Text.UTF8Encoding]::new($false))
    $log = Join-Path $fixture ([Guid]::NewGuid().ToString('N') + '.log')
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $root 'scripts/release-gate.ps1'), '-StepsFile', $file) + $Extra
    $process = Start-Process -FilePath $powershell -ArgumentList $arguments -NoNewWindow -PassThru -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    [void]$process.Handle
    if (-not $process.WaitForExit(120000)) { $process.Kill(); throw 'The gate did not finish.' }
    return [pscustomobject]@{ Code = $process.ExitCode; Text = [IO.File]::ReadAllText($log) + [IO.File]::ReadAllText("$log.err") }
}
$pass = @{ name = 'passes'; file = 'cmd.exe'; arguments = @('/c', 'exit 0') }
$fail = @{ name = 'fails'; file = 'cmd.exe'; arguments = @('/c', 'echo the-failure-detail & exit 3') }
$skip = @{ name = 'missing tool'; skip = 'The tool is not installed.' }
$after = @{ name = 'runs after'; file = 'cmd.exe'; arguments = @('/c', 'exit 0') }

$result = Invoke-Gate @($pass)
Assert-True ($result.Code -eq 0 -and $result.Text -match 'RELEASE GATE PASSED') "All passing steps must pass: $($result.Text)"

$result = Invoke-Gate @($pass, $fail, $after)
Assert-True ($result.Code -ne 0) 'A failed step must fail the gate.'
Assert-True ($result.Text -match 'RELEASE GATE FAILED') 'A failure must be loud.'
Assert-True ($result.Text -match 'the-failure-detail') 'A failure must show the step output.'
Assert-True ($result.Text -match 'runs after') 'Later steps still report after a failure.'

$result = Invoke-Gate @($fail) @('-AllowSkip')
Assert-True ($result.Code -ne 0) '-AllowSkip must never excuse a failed step.'

$result = Invoke-Gate @($pass, $skip)
Assert-True ($result.Code -ne 0) 'A skipped step must fail the gate by default.'
Assert-True ($result.Text -match 'SKIPPED' -and $result.Text -match 'The tool is not installed') 'A skip must be reported with its reason.'

$result = Invoke-Gate @($pass, $skip) @('-AllowSkip')
Assert-True ($result.Code -eq 0) "-AllowSkip must accept a skip: $($result.Text)"
Assert-True ($result.Text -match 'SKIPPED' -and $result.Text -match 'PASSED WITH SKIPPED') '-AllowSkip must not hide the skip.'

$result = Invoke-Gate @(@{ name = 'no such program'; file = (Join-Path $fixture 'missing.exe'); arguments = @() })
Assert-True ($result.Code -ne 0) 'A step that cannot start must fail the gate.'

$result = Invoke-Gate @(@{ name = 'hangs'; file = 'cmd.exe'; arguments = @('/c', 'ping -n 30 127.0.0.1 >nul') }) @('-StepTimeoutMinutes', '0.02')
Assert-True ($result.Code -ne 0 -and $result.Text -match 'timed out') 'A hung step must fail the gate.'

# --- Engine stamp ---------------------------------------------------------------------------
Write-Fixture 'scripts/stamp-mobile-engine.ps1' ([IO.File]::ReadAllText((Join-Path $root 'scripts/stamp-mobile-engine.ps1')))
$sources = @{
    'Cargo.toml' = "[workspace]`n"; 'budget-core/Cargo.toml' = "[package]`n"; 'budget-engine/Cargo.toml' = "[package]`n"
    'budget-core/src/lib.rs' = "pub fn a() {}`n"; 'budget-core/src/nested/deep.rs' = "pub fn b() {}`n"; 'budget-engine/src/lib.rs' = "pub fn c() {}`n"
    'Cargo.lock' = "[[package]]`nname = `"budget-core`"`nversion = `"0.1.0`"`n`n[[package]]`nname = `"budget-gui`"`nversion = `"0.8.0`"`n"
}
foreach ($entry in $sources.GetEnumerator()) { Write-Fixture $entry.Key $entry.Value }
$wasm = Join-Path $fixture 'budget-mobile/public/budget_engine.wasm'
$module = [byte[]](0,97,115,109,1,0,0,0, 1,1,0, 0,3,1,97,98)
function Write-Module { [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($wasm)); [IO.File]::WriteAllBytes($wasm, $module) }
$stamp = Join-Path $fixture 'scripts/stamp-mobile-engine.ps1'
Write-Module
Assert-Fails { & $stamp -Check } 'build-mobile'
$built = & $stamp -SourceHashOnly
$printed = & $stamp -BuiltFrom $built
Assert-True ($printed -ceq $built) 'Stamping must print the source hash.'
Assert-True ((& $stamp -Check) -ceq $built) 'A freshly stamped engine must pass.'
$bytes = [IO.File]::ReadAllBytes($wasm)
Assert-True ($bytes.Length -gt $module.Length -and $bytes[$module.Length] -eq 0) 'The stamp must be an ignorable custom section after the module.'
Assert-Fails { & $stamp } 'already stamped'
Write-Fixture 'budget-core/src/lib.rs' "pub fn a() {}`r`n"
Assert-True ((& $stamp -Check) -ceq $built) 'Line endings alone must not make the engine stale.'
Write-Fixture 'Cargo.lock' ($sources['Cargo.lock'] -replace '0\.8\.0', '0.9.0')
Assert-True ((& $stamp -Check) -ceq $built) 'A desktop version bump must not make the engine stale.'
Write-Fixture 'budget-core/src/nested/deep.rs' "pub fn b() { changed() }`n"
Assert-Fails { & $stamp -Check } 'does not match'
Write-Fixture 'budget-core/src/nested/deep.rs' $sources['budget-core/src/nested/deep.rs']
Write-Fixture 'budget-engine/src/added.rs' "pub fn added() {}`n"
Assert-Fails { & $stamp -Check } 'does not match'
[IO.File]::Delete((Join-Path $fixture 'budget-engine/src/added.rs'))
Write-Fixture 'Cargo.lock' ($sources['Cargo.lock'] -replace 'budget-core', 'budget-dependency')
Assert-Fails { & $stamp -Check } 'does not match'
Write-Fixture 'Cargo.lock' $sources['Cargo.lock']
Write-Module
Assert-Fails { & $stamp -BuiltFrom ('0' * 64) } 'changed during the build'
Assert-Fails { & $stamp -Check } 'build-mobile'
[IO.File]::WriteAllBytes($wasm, [byte[]](1, 2, 3))
Assert-Fails { & $stamp -Check } 'not a WebAssembly'

# --- Three-client integration cannot lose its Windows step ----------------------------------
$node = (Get-Command node -CommandType Application | Select-Object -First 1).Source
$integration = Join-Path $root 'budget-mobile/tests/three-client-integration.mjs'
function Invoke-Integration([hashtable]$Environment) {
    $saved = @{}
    foreach ($name in $Environment.Keys) { $saved[$name] = [Environment]::GetEnvironmentVariable($name); [Environment]::SetEnvironmentVariable($name, $Environment[$name]) }
    $log = Join-Path $fixture ([Guid]::NewGuid().ToString('N') + '.log')
    try {
        $process = Start-Process -FilePath $node -ArgumentList "`"$integration`"" -NoNewWindow -PassThru -RedirectStandardOutput $log -RedirectStandardError "$log.err"
        [void]$process.Handle
        if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'The integration preflight did not finish.' }
    } finally { foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) } }
    return [pscustomobject]@{ Code = $process.ExitCode; Text = [IO.File]::ReadAllText("$log.err") }
}
$result = Invoke-Integration @{ BUDGET_CARGO = (Join-Path $fixture 'no-such-cargo.exe') }
Assert-True ($result.Code -ne 0 -and $result.Text -match 'Windows client step') "A missing BUDGET_CARGO must stop the run: $($result.Text)"
$result = Invoke-Integration @{ BUDGET_CARGO = $null; PATH = $fixture }
Assert-True ($result.Code -ne 0 -and $result.Text -match 'Windows client step') "Cargo missing from PATH must stop the run: $($result.Text)"

Remove-Item -LiteralPath $fixture -Recurse -Force
Write-Host "$script:checks release gate checks passed."
