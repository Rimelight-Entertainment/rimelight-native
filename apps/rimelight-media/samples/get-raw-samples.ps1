<#
.SYNOPSIS
Downloads one small real sample per camera RAW format and per HEIC/HEIF/HIF into samples\raw\.

.DESCRIPTION
RAW files come from https://raw.pixls.us (CC0 sample files, index at https://raw.pixls.us/data/filelist.sha256).
HEIC and HEIF come from the libheif test data on GitHub. HIF has no public sample in those sources, so it is skipped.
Each file is checked with a HEAD request first and skipped when it is larger than 60 MB. Existing files are kept.
The folder samples\raw\ is git-ignored, so these large files stay out of the repository.

Usage (from the repository root):
  powershell -ExecutionPolicy Bypass -File samples\get-raw-samples.ps1
#>

$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'raw'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$maxBytes = 60MB

$pixls = 'https://raw.pixls.us/data/'
$files = [ordered]@{
    '3fr'  = $pixls + 'Hasselblad/CFV/RAW_HASSELBLAD_CFV.3FR'
    'arw'  = $pixls + 'Sony/DSC-RX0/DSC00009.ARW'
    'cr2'  = $pixls + 'Canon/Canon%20PowerShot%20S2%20IS/IMG_1107.CR2'
    'cr3'  = $pixls + 'Canon/Canon%20EOS%20R6%20Mark%20III/IMG_4244.CR3'
    'dng'  = $pixls + 'Adobe%20DNG%20Converter/Canon%20EOS%205D%20Mark%20III/5G4A9395-compressed-lossy.DNG'
    'erf'  = $pixls + 'Epson/R-D1/_EPS0672.ERF'
    'iiq'  = $pixls + 'Phase%20One/H%2025/H25_Outdoor_.IIQ'
    'kdc'  = $pixls + 'Kodak/DC50/RAW_KODAK_DC50.KDC'
    'mrw'  = $pixls + 'Minolta/Dimage%20Z2/RAW_MINOLTA_DIMAGE_Z2.MRW'
    'nef'  = $pixls + 'Nikon/1%20J2/DSC_0451.NEF'
    'nrw'  = $pixls + 'Nikon/Coolpix%20P7000/RAW_NIKON_P7000.NRW'
    'orf'  = $pixls + 'Olympus/C5050Z/RAW_OLYMPUS_C5050Z.ORF'
    'pef'  = $pixls + 'Pentax/ist%20DS/IMGP2333.pef'
    'raf'  = $pixls + 'Fujifilm/FinePix%20F700/RAW_FUJI_F700.RAF'
    'rw2'  = $pixls + 'Panasonic/DC-FZ83/DC-FZ83_ISO125_4x3.RW2'
    'rwl'  = $pixls + 'Leica/D-LUX%205/L1010451.RWL'
    'sr2'  = $pixls + 'Sony/DSC-R1/_DSC1477.SR2'
    'srf'  = $pixls + 'Sony/DSC-F828/DSC06227.SRF'
    'srw'  = $pixls + 'Samsung/EK-GN120/20231015_094741.srw'
    'x3f'  = $pixls + 'Polaroid/Polaroid%20x530/RAW_POLAROID_X530.X3F'
    'heic' = 'https://raw.githubusercontent.com/strukturag/libheif/master/examples/example.heic'
    'heif' = 'https://raw.githubusercontent.com/strukturag/libheif/master/tests/data/lightning_mini.heif'
}

$report = @()
foreach ($ext in $files.Keys) {
    $url = $files[$ext]
    $target = Join-Path $dir ("sample." + $ext)
    if (Test-Path -LiteralPath $target) {
        $report += [pscustomobject]@{ Ext = $ext; Status = 'exists'; Bytes = (Get-Item -LiteralPath $target).Length }
        continue
    }
    try {
        $head = Invoke-WebRequest -UseBasicParsing -Method Head -Uri $url -TimeoutSec 60
        $len = [int64]$head.Headers['Content-Length']
        if ($len -gt $maxBytes) {
            $report += [pscustomobject]@{ Ext = $ext; Status = 'skipped (over 60 MB)'; Bytes = $len }
            continue
        }
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $target -TimeoutSec 600
        $report += [pscustomobject]@{ Ext = $ext; Status = 'downloaded'; Bytes = (Get-Item -LiteralPath $target).Length }
    } catch {
        if (Test-Path -LiteralPath $target) { Remove-Item -LiteralPath $target -Force }
        $report += [pscustomobject]@{ Ext = $ext; Status = 'failed: ' + $_.Exception.Message; Bytes = 0 }
    }
}

Write-Host ''
Write-Host 'HIF: no public sample found (not on raw.pixls.us or libheif test data); skipped.'
$report | Format-Table -AutoSize
