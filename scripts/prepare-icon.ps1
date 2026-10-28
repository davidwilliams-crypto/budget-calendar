# Mechanical packaging only: preserve the master artwork and its transparency.
# Run with Windows PowerShell; System.Drawing is included with Windows.
[CmdletBinding()]
param(
    [string]$SourcePath = 'budget-gui/assets/budget-calendar-master.png',
    [string]$OutputDirectory = 'budget-gui/assets'
)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -eq 'Core') {
    throw 'Use Windows PowerShell for deterministic System.Drawing packaging: powershell.exe -NoProfile -File scripts/prepare-icon.ps1'
}
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

function Resolve-RepositoryPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'An icon path cannot be empty.' }
    if (-not [IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $repository $Path }
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    if (-not $fullPath.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Icon paths must stay inside the repository: $Path"
    }
    # Do not follow junctions or symlinks out of the permitted asset/fixture folders.
    $ancestor = $fullPath
    while ($ancestor.Length -gt $repository.Length) {
        if (Test-Path -LiteralPath $ancestor) {
            if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Icon paths cannot traverse a reparse point: $ancestor"
            }
        }
        $ancestor = [IO.Path]::GetDirectoryName($ancestor)
    }
    return $fullPath
}

$source = Resolve-RepositoryPath $SourcePath
$output = Resolve-RepositoryPath $OutputDirectory
$assets = [IO.Path]::GetFullPath((Join-Path $repository 'budget-gui/assets'))
$target = [IO.Path]::GetFullPath((Join-Path $repository 'target'))
if ($output -ne $assets -and -not $output.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Icon output must be budget-gui/assets or a fixture directory under target/.'
}
if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or [IO.Path]::GetExtension($source) -ine '.png') {
    throw "The icon master must be an existing PNG file: $source"
}
$pngPath = Resolve-RepositoryPath (Join-Path $output 'budget-calendar.png')
$icoPath = Resolve-RepositoryPath (Join-Path $output 'budget-calendar.ico')
if ($source -ieq $pngPath) { throw 'The icon master cannot also be the generated PNG output.' }

Add-Type -AssemblyName System.Drawing
if (-not ('BudgetIconPackaging' -as [type])) {
    Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;

public static class BudgetIconPackaging {
    public static readonly int[] Sizes = { 16, 20, 24, 32, 40, 48, 64, 96, 128, 256 };

    private static Bitmap Resize(Image source, int size) {
        Bitmap result = new Bitmap(size, size, PixelFormat.Format32bppArgb);
        using (Graphics graphics = Graphics.FromImage(result))
        using (ImageAttributes attributes = new ImageAttributes()) {
            graphics.CompositingMode = CompositingMode.SourceCopy;
            graphics.CompositingQuality = CompositingQuality.HighQuality;
            graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            graphics.SmoothingMode = SmoothingMode.HighQuality;
            attributes.SetWrapMode(WrapMode.TileFlipXY);
            graphics.DrawImage(source, new Rectangle(0, 0, size, size),
                0, 0, source.Width, source.Height, GraphicsUnit.Pixel, attributes);
        }
        return result;
    }

    private static byte[] EncodePng(Bitmap bitmap) {
        using (MemoryStream stream = new MemoryStream()) {
            bitmap.Save(stream, ImageFormat.Png);
            return stream.ToArray();
        }
    }

    private static byte[] EncodeDib(Bitmap bitmap) {
        int size = bitmap.Width;
        int maskStride = ((size + 31) / 32) * 4;
        using (MemoryStream stream = new MemoryStream())
        using (BinaryWriter writer = new BinaryWriter(stream)) {
            writer.Write((uint)40); // BITMAPINFOHEADER, without a BMP file header.
            writer.Write(size);
            writer.Write(size * 2); // XOR pixels plus the AND transparency mask.
            writer.Write((ushort)1);
            writer.Write((ushort)32);
            writer.Write((uint)0); // Uncompressed BGRA.
            writer.Write((uint)(size * size * 4));
            writer.Write(0);
            writer.Write(0);
            writer.Write((uint)0);
            writer.Write((uint)0);
            for (int y = size - 1; y >= 0; y--) {
                for (int x = 0; x < size; x++) {
                    Color pixel = bitmap.GetPixel(x, y);
                    writer.Write(pixel.B);
                    writer.Write(pixel.G);
                    writer.Write(pixel.R);
                    writer.Write(pixel.A);
                }
            }
            for (int y = size - 1; y >= 0; y--) {
                byte[] mask = new byte[maskStride];
                for (int x = 0; x < size; x++) {
                    if (bitmap.GetPixel(x, y).A == 0) mask[x / 8] |= (byte)(0x80 >> (x % 8));
                }
                writer.Write(mask);
            }
            return stream.ToArray();
        }
    }

    public static void Write(string sourcePath, string pngPath, string icoPath) {
        List<byte[]> images = new List<byte[]>();
        using (Image source = Image.FromFile(sourcePath)) {
            if (source.RawFormat.Guid != ImageFormat.Png.Guid || source.Width != source.Height || source.Width < 256)
                throw new ArgumentException("The icon master must be a square PNG at least 256 pixels wide.");
            foreach (int size in Sizes) {
                using (Bitmap bitmap = Resize(source, size)) {
                    images.Add(size == 256 ? EncodePng(bitmap) : EncodeDib(bitmap));
                }
            }
        }
        byte[] ico;
        using (MemoryStream stream = new MemoryStream())
        using (BinaryWriter writer = new BinaryWriter(stream)) {
            writer.Write((ushort)0);
            writer.Write((ushort)1);
            writer.Write((ushort)Sizes.Length);
            uint offset = (uint)(6 + Sizes.Length * 16);
            for (int i = 0; i < Sizes.Length; i++) {
                writer.Write((byte)(Sizes[i] == 256 ? 0 : Sizes[i]));
                writer.Write((byte)(Sizes[i] == 256 ? 0 : Sizes[i]));
                writer.Write((byte)0);
                writer.Write((byte)0);
                writer.Write((ushort)1);
                writer.Write((ushort)32);
                writer.Write((uint)images[i].Length);
                writer.Write(offset);
                offset += (uint)images[i].Length;
            }
            foreach (byte[] image in images) writer.Write(image);
            ico = stream.ToArray();
        }
        Directory.CreateDirectory(Path.GetDirectoryName(pngPath));
        File.WriteAllBytes(pngPath, images[images.Count - 1]);
        File.WriteAllBytes(icoPath, ico);
    }
}
'@
}

try {
    [BudgetIconPackaging]::Write($source, $pngPath, $icoPath)
} catch {
    throw "Could not package icon '$source': $($_.Exception.GetBaseException().Message)"
}
[pscustomobject]@{
    Source = $source
    Png = $pngPath
    Icon = $icoPath
    Sizes = [BudgetIconPackaging]::Sizes -join ', '
}
