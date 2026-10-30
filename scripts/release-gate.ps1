#Requires -Version 5.1
<#
.SYNOPSIS
The one command that decides whether this tree may be released.
.DESCRIPTION
Runs, in Windows PowerShell from any directory: the shipped-engine and service-worker checks, the workspace
tests, the build-script tests, native/WASM parity against the shipped engine, every mobile unit test, every
browser test, the three-client integration test and every scripts/tests/*.Tests.ps1.

Every step runs even after a failure, so one run shows everything that is wrong. Any failed step fails the gate.
A step that cannot run is reported as SKIPPED and also fails the gate unless -AllowSkip is passed; -AllowSkip
never excuses a failure.

Needs cargo, rustc and node on PATH, with the offline Cargo cache in place. Browser steps need Playwright and a
Chromium build: PLAYWRIGHT_MODULE_PATH and BUDGET_CHROME choose them, otherwise common locations are searched.
Logs and a summary.json go under target/release-gate/. HOME and USERPROFILE point inside that run folder, so no
test can reach personal settings. All test data is generated under target/.
.PARAMETER StepsFile
For scripts/tests/release-gate.Tests.ps1 only: JSON stub steps to run instead of the real ones.
#>
[CmdletBinding()]
param([switch]$AllowSkip, [string]$Cargo = 'cargo', [double]$StepTimeoutMinutes = 45, [string]$StepsFile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location -LiteralPath $root
$run = Join-Path $root ('target/release-gate/' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 6))
[void][IO.Directory]::CreateDirectory($run)

function New-Step([string]$Name, [string]$File, [string[]]$Arguments, [string]$Skip) {
    return [pscustomobject]@{ name = $Name; file = $File; arguments = $Arguments; skip = $Skip }
}
function New-Script([string]$Name, [string]$Script, [string[]]$Arguments = @()) {
    return New-Step $Name 'powershell.exe' (@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $Arguments)
}
function Find-Playwright {
    if ($env:PLAYWRIGHT_MODULE_PATH) { return $env:PLAYWRIGHT_MODULE_PATH }
    $places = @((Join-Path $root 'budget-mobile/node_modules'), (Join-Path $root 'target/ci-tools/node_modules'))
    $node = Get-Command node -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($node) {
        $global = Join-Path (Split-Path $node.Source) 'node_modules'
        $places += $global
        if (Test-Path -LiteralPath $global) { $places += @(Get-ChildItem -LiteralPath $global -Directory | ForEach-Object { Join-Path $_.FullName 'node_modules' }) }
    }
    foreach ($place in $places) {
        foreach ($package in @('playwright', 'playwright-core')) {
            $candidate = Join-Path $place "$package/index.mjs"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        }
    }
    return $null
}
function Find-Chromium {
    if ($env:BUDGET_CHROME) { return $env:BUDGET_CHROME }
    $store = Join-Path $env:LOCALAPPDATA 'ms-playwright'
    if (Test-Path -LiteralPath $store) {
        $builds = @(Get-ChildItem -LiteralPath $store -Directory -Filter 'chromium-*' | Sort-Object { [int]($_.Name -replace '\D', '') } -Descending)
        foreach ($build in $builds) {
            foreach ($folder in @('chrome-win64', 'chrome-win')) {
                $candidate = Join-Path $build.FullName "$folder/chrome.exe"
                if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
            }
        }
    }
    foreach ($candidate in @("$env:ProgramFiles\Google\Chrome\Application\chrome.exe", "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe")) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return $null
}
function Get-RealSteps {
    $steps = [Collections.Generic.List[object]]::new()
    $cargoPath = (Get-Command $Cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1)
    $rustc = (Get-Command rustc -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1)
    $node = (Get-Command node -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1)
    $noCargo = if ($cargoPath) { $null } else { "Cargo was not found ($Cargo)." }
    $noRustc = if ($rustc) { $null } else { 'rustc is not on PATH.' }
    $noNode = if ($node) { $null } else { 'Node is not on PATH.' }
    $cargoFile = if ($cargoPath) { $cargoPath.Source } else { $Cargo }
    $playwright = Find-Playwright; $chromium = Find-Chromium
    $noBrowser = if ($noNode) { $noNode } elseif (-not $playwright) { 'Playwright was not found; set PLAYWRIGHT_MODULE_PATH.' } elseif (-not $chromium) { 'No Chromium browser was found; set BUDGET_CHROME.' } else { $null }
    if (-not $noBrowser) {
        # The browser tests call plain chromium.launch(), so the chosen browser is applied here.
        $shim = Join-Path $run 'playwright-shim.mjs'
        $module = ([Uri][IO.Path]::GetFullPath($playwright)).AbsoluteUri
        $browser = $chromium.Replace('\', '/').Replace("'", "\'")
        [IO.File]::WriteAllText($shim, "import * as loaded from '$module';`nconst pw=loaded.chromium?loaded:loaded.default,launch=pw.chromium.launch.bind(pw.chromium);`nexport const chromium=new Proxy(pw.chromium,{get:(t,k)=>k==='launch'?(o={})=>launch({...o,executablePath:'$browser'}):t[k]});`n", [Text.UTF8Encoding]::new($false))
        $env:PLAYWRIGHT_MODULE_PATH = $shim
    }
    $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $wasm = Join-Path $root 'budget-mobile/public/budget_engine.wasm'
    $env:BUDGET_ENGINE_WASM = $wasm
    if (-not $env:BUDGET_ENGINE_NATIVE) { $env:BUDGET_ENGINE_NATIVE = Join-Path $targetDirectory 'debug/budget-engine-cli.exe' }
    $env:BUDGET_CARGO = $cargoFile
    $buildTests = Join-Path $run 'version-build-tests.exe'

    $steps.Add((New-Script 'Shipped phone engine matches Rust sources' 'scripts/stamp-mobile-engine.ps1' @('-Check')))
    $steps.Add((New-Script 'Service worker cache name is current' 'scripts/stamp-mobile-worker.ps1' @('-Check')))
    $steps.Add((New-Step 'Workspace tests' $cargoFile @('test', '--workspace', '--offline') $noCargo))
    $steps.Add((New-Step 'Build-script tests: compile' 'rustc' @('--edition=2021', '--test', 'budget-gui/build.rs', '-o', $buildTests) $noRustc))
    $steps.Add((New-Step 'Build-script tests: run' $buildTests @() $noRustc))
    $steps.Add((New-Step 'Native engine for parity' $cargoFile @('build', '-p', 'budget-engine', '--bin', 'budget-engine-cli', '--offline') $noCargo))
    $paritySkip = if ($noNode) { $noNode } else { $noCargo }
    $steps.Add((New-Step 'Native and shipped WASM parity' 'node' @('--test', 'budget-engine/tests/parity.mjs') $paritySkip))
    $unit = @(Get-ChildItem -LiteralPath (Join-Path $root 'budget-mobile/tests') -Filter '*.test.mjs' | Sort-Object Name | ForEach-Object { 'budget-mobile/tests/' + $_.Name })
    if ($unit.Count -eq 0) { throw 'No mobile unit tests were found.' }
    $steps.Add((New-Step "Mobile unit tests ($($unit.Count) files)" 'node' (@('--test') + $unit) $noNode))
    $browser = @(Get-ChildItem -LiteralPath (Join-Path $root 'budget-mobile/tests') -Filter '*.browser.mjs' | Sort-Object Name)
    if ($browser.Count -eq 0) { throw 'No browser tests were found.' }
    foreach ($item in $browser) { $steps.Add((New-Step "Browser: $($item.BaseName -replace '\.browser$', '')" 'node' @("budget-mobile/tests/$($item.Name)") $noBrowser)) }
    $integrationSkip = if ($noBrowser) { $noBrowser } else { $noCargo }
    $steps.Add((New-Step 'Three-client integration, Windows step included' 'node' @('budget-mobile/tests/three-client-integration.mjs') $integrationSkip))
    foreach ($item in @(Get-ChildItem -LiteralPath (Join-Path $root 'scripts/tests') -Filter '*.Tests.ps1' | Sort-Object Name)) {
        $steps.Add((New-Script "Script tests: $($item.Name -replace '\.Tests\.ps1$', '')" "scripts/tests/$($item.Name)"))
    }
    return $steps.ToArray()
}
function Format-Argument([string]$Value) { if ($Value -match '[\s&|<>^()]') { return '"' + $Value + '"' }; return $Value }
function Invoke-Step($Step, [string]$Log) {
    $line = (@($Step.arguments) | Where-Object { $null -ne $_ } | ForEach-Object { Format-Argument $_ }) -join ' '
    $command = '/d /s /c ""' + $Step.file + '" ' + $line + ' > "' + $Log + '" 2>&1"'
    $process = Start-Process -FilePath "$env:SystemRoot\System32\cmd.exe" -ArgumentList $command -NoNewWindow -PassThru
    [void]$process.Handle
    if (-not $process.WaitForExit([int]($StepTimeoutMinutes * 60000))) {
        & "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F 2>&1 | Out-Null
        [void]$process.WaitForExit(10000)
        return "timed out after $StepTimeoutMinutes minutes"
    }
    if ($process.ExitCode -ne 0) { return "exit code $($process.ExitCode)" }
    return $null
}

# Keep Cargo and rustup where they were, then isolate everything else from personal settings.
if (-not $StepsFile) {
    if (-not $env:CARGO_HOME) { $env:CARGO_HOME = Join-Path $env:USERPROFILE '.cargo' }
    if (-not $env:RUSTUP_HOME) { $env:RUSTUP_HOME = Join-Path $env:USERPROFILE '.rustup' }
    $steps = @(Get-RealSteps)
    $home_ = Join-Path $run 'home'
    [void][IO.Directory]::CreateDirectory($home_)
    $env:HOME = $home_; $env:USERPROFILE = $home_
} else {
    $parsed = [IO.File]::ReadAllText($StepsFile) | ConvertFrom-Json
    $steps = @($parsed | ForEach-Object { $_ })
}

$results = [Collections.Generic.List[object]]::new()
$index = 0
foreach ($step in $steps) {
    $index++
    $reason = $null
    if ($step.PSObject.Properties['skip']) { $reason = $step.skip }
    Write-Host ''
    Write-Host ("[{0}/{1}] {2}" -f $index, $steps.Count, $step.name)
    if ($reason) {
        Write-Host "    SKIPPED: $reason" -ForegroundColor Yellow
        $results.Add([pscustomobject]@{ name = $step.name; status = 'skipped'; detail = $reason; seconds = 0; log = $null })
        continue
    }
    $log = Join-Path $run ('{0:00}-{1}.log' -f $index, (($step.name -replace '[^A-Za-z0-9]+', '-').Trim('-')))
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $problem = Invoke-Step $step $log
    $watch.Stop()
    $seconds = [Math]::Round($watch.Elapsed.TotalSeconds)
    if ($problem) {
        Write-Host "    FAILED ($problem, ${seconds}s)  $log" -ForegroundColor Red
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Tail 40 | ForEach-Object { Write-Host "    | $_" } }
        $results.Add([pscustomobject]@{ name = $step.name; status = 'failed'; detail = $problem; seconds = $seconds; log = $log })
    } else {
        Write-Host "    passed (${seconds}s)" -ForegroundColor Green
        $results.Add([pscustomobject]@{ name = $step.name; status = 'passed'; detail = $null; seconds = $seconds; log = $log })
    }
}

$failed = @($results | Where-Object status -eq 'failed')
$skipped = @($results | Where-Object status -eq 'skipped')
$passed = @($results | Where-Object status -eq 'passed')
$verdict = if ($failed.Count -gt 0 -or ($skipped.Count -gt 0 -and -not $AllowSkip)) { 'RELEASE GATE FAILED' } elseif ($skipped.Count -gt 0) { 'RELEASE GATE PASSED WITH SKIPPED STEPS' } else { 'RELEASE GATE PASSED' }
$summary = [ordered]@{ verdict = $verdict; allowSkip = [bool]$AllowSkip; finished = (Get-Date).ToString('o'); steps = @($results.ToArray()) }
[IO.File]::WriteAllText((Join-Path $run 'summary.json'), ($summary | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
Write-Host ''
Write-Host '================================================================'
foreach ($result in $results) {
    $detail = if ($result.detail) { " - $($result.detail)" } else { '' }
    Write-Host ('{0,-8} {1}{2}' -f $result.status.ToUpperInvariant(), $result.name, $detail)
}
Write-Host '================================================================'
$colour = if ($verdict -ceq 'RELEASE GATE PASSED') { 'Green' } elseif ($verdict -ceq 'RELEASE GATE FAILED') { 'Red' } else { 'Yellow' }
Write-Host "$verdict`: $($passed.Count) passed, $($failed.Count) failed, $($skipped.Count) skipped. Logs: $run" -ForegroundColor $colour
if ($verdict -ceq 'RELEASE GATE FAILED') { exit 1 }
exit 0
