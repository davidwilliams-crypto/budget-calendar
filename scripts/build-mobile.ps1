[CmdletBinding()]
param([string]$Cargo = 'cargo', [switch]$Online)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $root
try {
    $engineStamp = Join-Path $PSScriptRoot 'stamp-mobile-engine.ps1'
    $builtFrom = & $engineStamp -SourceHashOnly
    $arguments = @('build', '-p', 'budget-engine', '--lib', '--release', '--target', 'wasm32-unknown-unknown')
    if (-not $Online) { $arguments += '--offline' }
    & $Cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Mobile calculation engine build failed.' }
    $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    Copy-Item -LiteralPath (Join-Path $targetDirectory 'wasm32-unknown-unknown/release/budget_engine.wasm') -Destination (Join-Path $root 'budget-mobile/public/budget_engine.wasm')
    & $engineStamp -BuiltFrom $builtFrom | Out-Null
    Add-Type -AssemblyName System.Drawing
    $source = [Drawing.Image]::FromFile((Join-Path $root 'budget-gui/assets/budget-calendar-master.png'))
    try {
        foreach ($size in @(192, 512)) {
            $bitmap = New-Object Drawing.Bitmap($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
                $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $graphics.DrawImage($source, (New-Object Drawing.Rectangle(0,0,$size,$size)))
                $bitmap.Save((Join-Path $root "budget-mobile/public/icon-$size.png"), [Drawing.Imaging.ImageFormat]::Png)
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
        }
    } finally { $source.Dispose() }
    $cacheName = & (Join-Path $PSScriptRoot 'stamp-mobile-worker.ps1')
    Write-Output "Mobile WebAssembly and Android icons built. Service worker cache: $cacheName"
} finally { Pop-Location }
