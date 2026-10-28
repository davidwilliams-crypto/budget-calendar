# Run with powershell.exe -NoProfile -File scripts/tests/icon-assets.Tests.ps1.
# All generated fixtures stay under target/; no Pester or external dependencies.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -eq 'Core') {
    throw 'Run icon tests with Windows PowerShell: powershell.exe -NoProfile -File scripts/tests/icon-assets.Tests.ps1'
}
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$scriptUnderTest = Join-Path $repository 'scripts/prepare-icon.ps1'
$assets = Join-Path $repository 'budget-gui/assets'
$testRoot = Join-Path $repository ('target/icon-assets-tests-' + [Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;

public static class BudgetIconTests {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadImage(IntPtr instance, string path, uint type, int width, int height, uint flags);
    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr icon);

    private static void Check(bool condition, string message) {
        if (!condition) throw new Exception(message);
    }

    private static void CheckAlpha(Bitmap bitmap, string label) {
        int clear = 0, partial = 0, opaque = 0;
        for (int y = 0; y < bitmap.Height; y++) {
            for (int x = 0; x < bitmap.Width; x++) {
                byte alpha = bitmap.GetPixel(x, y).A;
                if (alpha == 0) clear++;
                else if (alpha == 255) opaque++;
                else partial++;
            }
        }
        Check(clear > 0 && partial > 0 && opaque > 0, label + " must preserve transparent, antialiased and opaque pixels.");
        Check(bitmap.GetPixel(0, 0).A == 0, label + " top-left corner must be transparent.");
    }

    public static void CheckPng(string path, int expectedSize) {
        using (Bitmap bitmap = new Bitmap(path)) {
            Check(bitmap.RawFormat.Guid == ImageFormat.Png.Guid, "Expected PNG: " + path);
            Check(bitmap.Width == bitmap.Height && bitmap.Width >= 256, "PNG must be square and at least 256px: " + path);
            if (expectedSize > 0) Check(bitmap.Width == expectedSize, "Unexpected PNG dimensions: " + path);
            CheckAlpha(bitmap, Path.GetFileName(path));
        }
    }

    public static void CheckIco(string path, string pngPath) {
        int[] sizes = { 16, 20, 24, 32, 40, 48, 64, 96, 128, 256 };
        byte[] bytes = File.ReadAllBytes(path);
        byte[] png = File.ReadAllBytes(pngPath);
        Check(BitConverter.ToUInt16(bytes, 0) == 0 && BitConverter.ToUInt16(bytes, 2) == 1, "Invalid ICO signature.");
        Check(BitConverter.ToUInt16(bytes, 4) == sizes.Length, "ICO must contain all ten sizes.");
        int nextOffset = 6 + sizes.Length * 16;
        for (int i = 0; i < sizes.Length; i++) {
            int size = sizes[i], entry = 6 + i * 16;
            int width = bytes[entry] == 0 ? 256 : bytes[entry];
            int height = bytes[entry + 1] == 0 ? 256 : bytes[entry + 1];
            int length = BitConverter.ToInt32(bytes, entry + 8), offset = BitConverter.ToInt32(bytes, entry + 12);
            Check(width == size && height == size, "Wrong ICO dimensions for " + size);
            Check(bytes[entry + 2] == 0 && bytes[entry + 3] == 0, "Unexpected ICO palette or reserved field.");
            Check(BitConverter.ToUInt16(bytes, entry + 4) == 1 && BitConverter.ToUInt16(bytes, entry + 6) == 32, "ICO must be 32-bit.");
            Check(offset == nextOffset && length > 0 && length <= bytes.Length - offset, "Invalid ICO image bounds.");
            nextOffset += length;
            if (size == 256) {
                Check(length == png.Length, "256px ICO PNG length differs from runtime PNG.");
                for (int k = 0; k < length; k++) Check(bytes[offset + k] == png[k], "256px ICO PNG differs from runtime PNG.");
            } else {
                Check(BitConverter.ToInt32(bytes, offset) == 40, "Small ICO entries must be DIBs.");
                Check(BitConverter.ToInt32(bytes, offset + 4) == size && BitConverter.ToInt32(bytes, offset + 8) == size * 2, "Wrong DIB dimensions.");
                Check(BitConverter.ToUInt16(bytes, offset + 12) == 1 && BitConverter.ToUInt16(bytes, offset + 14) == 32, "Wrong DIB pixel format.");
                Check(BitConverter.ToUInt32(bytes, offset + 16) == 0, "DIB must be uncompressed.");
                int stride = ((size + 31) / 32) * 4, mask = offset + 40 + size * size * 4;
                Check(length == 40 + size * size * 4 + stride * size, "Wrong DIB/mask length.");
                using (Bitmap decoded = new Bitmap(size, size, PixelFormat.Format32bppArgb)) {
                    for (int y = 0; y < size; y++) {
                        for (int x = 0; x < size; x++) {
                            int pixel = offset + 40 + ((size - y - 1) * size + x) * 4;
                            byte alpha = bytes[pixel + 3];
                            bool masked = (bytes[mask + (size - y - 1) * stride + x / 8] & (0x80 >> (x % 8))) != 0;
                            Check(masked == (alpha == 0), "AND mask does not match transparent pixels at size " + size);
                            decoded.SetPixel(x, y, Color.FromArgb(alpha, bytes[pixel + 2], bytes[pixel + 1], bytes[pixel]));
                        }
                    }
                    CheckAlpha(decoded, "DIB " + size);
                    using (Bitmap native = LoadNative(path, size)) {
                        CheckAlpha(native, "Win32 " + size);
                        // Native loading independently verifies bottom-up row order and BGRA channels.
                        for (int y = 0; y < size; y++) {
                            for (int x = 0; x < size; x++) {
                                Color pixel = decoded.GetPixel(x, y);
                                if (pixel.A == 255) Check(native.GetPixel(x, y).ToArgb() == pixel.ToArgb(), "Win32 pixel differs from DIB at size " + size);
                            }
                        }
                    }
                }
            }
            if (size == 256) {
                using (Bitmap native = LoadNative(path, size)) CheckAlpha(native, "Win32 256");
            }
        }
        Check(nextOffset == bytes.Length, "Unexpected trailing ICO bytes.");
    }

    public static Bitmap LoadNative(string path, int size) {
        IntPtr handle = LoadImage(IntPtr.Zero, path, 1, size, size, 0x10);
        if (handle == IntPtr.Zero) throw new Exception("Win32 LoadImage failed for " + size + "px: " + Marshal.GetLastWin32Error());
        try {
            using (Icon icon = Icon.FromHandle(handle)) {
                Check(icon.Width == size && icon.Height == size, "Win32 loaded the wrong icon size.");
                return icon.ToBitmap();
            }
        } finally { DestroyIcon(handle); }
    }

    public static void WriteFixture(string path, bool square) {
        using (Bitmap bitmap = new Bitmap(256, square ? 256 : 128, PixelFormat.Format32bppArgb))
        using (Graphics graphics = Graphics.FromImage(bitmap)) {
            graphics.Clear(Color.Transparent);
            graphics.FillRectangle(Brushes.Red, 32, 32, 96, 96);
            graphics.FillRectangle(Brushes.Green, 128, 32, 96, 96);
            graphics.FillRectangle(Brushes.Blue, 32, 128, 96, 96);
            using (Brush brush = new SolidBrush(Color.FromArgb(128, 255, 220, 20))) graphics.FillRectangle(brush, 128, 128, 96, 96);
            bitmap.Save(path, ImageFormat.Png);
        }
    }

    public static void WritePreview(string iconPath, string outputPath) {
        int[] sizes = { 16, 24, 32, 48, 64, 128, 256 };
        using (Bitmap bitmap = new Bitmap(840, 620))
        using (Graphics graphics = Graphics.FromImage(bitmap))
        using (Font font = new Font("Segoe UI", 11))
        using (Brush dark = new SolidBrush(Color.FromArgb(24, 31, 43))) {
            graphics.Clear(Color.FromArgb(246, 248, 252));
            graphics.FillRectangle(dark, 0, 310, 840, 310);
            for (int row = 0; row < 2; row++) {
                int x = 22, top = row * 310;
                foreach (int size in sizes) {
                    using (Bitmap icon = LoadNative(iconPath, size)) graphics.DrawImageUnscaled(icon, x, top + 14 + (256 - size) / 2);
                    graphics.DrawString(size.ToString(), font, row == 0 ? Brushes.Black : Brushes.White, x, top + 278);
                    x += Math.Max(size, 36) + 12;
                }
            }
            bitmap.Save(outputPath, ImageFormat.Png);
        }
    }
}
'@

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Assert-Rejected([scriptblock]$Action, [string]$MessageFragment) {
    try { & $Action | Out-Null } catch {
        Assert-True ($_.Exception.Message -like "*$MessageFragment*") "Unexpected rejection: $($_.Exception.Message)"
        return
    }
    throw "Expected rejection containing: $MessageFragment"
}

$master = Join-Path $assets 'budget-calendar-master.png'
$png = Join-Path $assets 'budget-calendar.png'
$ico = Join-Path $assets 'budget-calendar.ico'
[BudgetIconTests]::CheckPng($master, 0)
[BudgetIconTests]::CheckPng($png, 256)
[BudgetIconTests]::CheckIco($ico, $png)
Write-Output 'PASS: production master, PNG, all ten ICO entries and Win32 decoding with alpha.'

$masterHash = (Get-FileHash -LiteralPath $master -Algorithm SHA256).Hash
$repeated = Join-Path $testRoot 'repeated'
& $scriptUnderTest -OutputDirectory $repeated | Out-Null
foreach ($name in @('budget-calendar.png', 'budget-calendar.ico')) {
    Assert-True ((Get-FileHash -LiteralPath (Join-Path $assets $name)).Hash -eq (Get-FileHash -LiteralPath (Join-Path $repeated $name)).Hash) "Generated $name is not reproducible."
}
Assert-True ((Get-FileHash -LiteralPath $master -Algorithm SHA256).Hash -eq $masterHash) 'Packaging changed the master artwork.'
Write-Output 'PASS: deterministic PNG/ICO bytes and unchanged master artwork.'

$fixture = Join-Path $testRoot 'fixture.png'
$fixtureOutput = Join-Path $testRoot 'fixture-output'
[BudgetIconTests]::WriteFixture($fixture, $true)
& $scriptUnderTest -SourcePath $fixture -OutputDirectory $fixtureOutput | Out-Null
[BudgetIconTests]::CheckIco((Join-Path $fixtureOutput 'budget-calendar.ico'), (Join-Path $fixtureOutput 'budget-calendar.png'))
Write-Output 'PASS: generated color/alpha fixture, BGRA channels, row order and AND masks.'

$badPng = Join-Path $testRoot 'bad.png'
[IO.File]::WriteAllText($badPng, 'not a PNG')
$nonsquare = Join-Path $testRoot 'nonsquare.png'
[BudgetIconTests]::WriteFixture($nonsquare, $false)
Assert-Rejected { & $scriptUnderTest -SourcePath 'target/missing-icon.png' -OutputDirectory $fixtureOutput } 'existing PNG'
Assert-Rejected { & $scriptUnderTest -SourcePath $badPng -OutputDirectory $fixtureOutput } 'Could not package icon'
Assert-Rejected { & $scriptUnderTest -SourcePath $nonsquare -OutputDirectory $fixtureOutput } 'square PNG'
Assert-Rejected { & $scriptUnderTest -OutputDirectory (Join-Path $repository '..') } 'inside the repository'
Assert-Rejected { & $scriptUnderTest -OutputDirectory 'scripts' } 'output must be'
Assert-Rejected { & $scriptUnderTest -SourcePath $png } 'cannot also be'
Write-Output 'PASS: missing/malformed/nonsquare input and unsafe output paths are rejected.'

$previewRoot = Join-Path $repository 'target/icon-review'
[void][IO.Directory]::CreateDirectory($previewRoot)
[BudgetIconTests]::WritePreview($ico, (Join-Path $previewRoot 'packaged-icons.png'))
Write-Output "Icon asset tests passed. Fixtures: $testRoot"
Write-Output "Native-size preview: $previewRoot/packaged-icons.png"
