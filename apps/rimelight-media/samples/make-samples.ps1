<#
.SYNOPSIS
Makes the media samples in this folder with FFmpeg: 1.5 s clips and 64 x 64 pictures.

.DESCRIPTION
The text samples (.txt, .json, .env, .svg and so on) are already here. RAW camera files and HEIC/HEIF/HIF can't be
made from scratch; see README.md for where real ones come from.

Usage (from the repository root):
  powershell -ExecutionPolicy Bypass -File samples\make-samples.ps1
  powershell -ExecutionPolicy Bypass -File samples\make-samples.ps1 -Ffmpeg C:\path\to\ffmpeg.exe

Without -Ffmpeg it looks for ffmpeg on the PATH, then under ffmpeg\ and tools\. A format whose encoder this FFmpeg build
lacks is skipped with a warning.
#>
param([string]$Ffmpeg)

$ErrorActionPreference = 'Stop'
$dir = $PSScriptRoot

if (-not $Ffmpeg) {
    $cmd = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($cmd) {
        $Ffmpeg = $cmd.Source
    } else {
        $found = Get-ChildItem -Path (Join-Path $dir '..\ffmpeg'), (Join-Path $dir '..\tools') -Recurse -Filter ffmpeg.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $Ffmpeg = $found.FullName }
    }
}
if (-not $Ffmpeg -or -not (Test-Path -LiteralPath $Ffmpeg)) {
    throw 'ffmpeg.exe not found. Pass -Ffmpeg <path to ffmpeg.exe>.'
}

function New-Sample {
    param([string]$Name, [string[]]$FfArgs)
    $out = Join-Path $dir $Name
    & $Ffmpeg -hide_banner -loglevel error -y @FfArgs $out
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $out)) {
        Write-Warning "$Name was not made (this FFmpeg build may lack its encoder)"
    } else {
        Write-Host "made $Name"
    }
}

# Inputs: a 64 x 64 test picture, a 1.5 s tone and a 160 x 120 test clip (with the tone as its sound).
$still = @('-f', 'lavfi', '-i', 'testsrc=size=64x64:rate=1:duration=1', '-frames:v', '1')
$tone = @('-f', 'lavfi', '-i', 'sine=frequency=440:duration=1.5')
$clip = @('-f', 'lavfi', '-i', 'testsrc=size=160x120:rate=10:duration=1.5', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=1.5', '-map', '0:v', '-map', '1:a', '-shortest')
$clip176 = @('-f', 'lavfi', '-i', 'testsrc=size=176x144:rate=10:duration=1.5', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=1.5', '-map', '0:v', '-map', '1:a', '-shortest')

# Pictures
New-Sample 'sample.png' $still
New-Sample 'sample.jpg' ($still + @('-q:v', '3'))
Copy-Item -LiteralPath (Join-Path $dir 'sample.jpg') -Destination (Join-Path $dir 'sample.jpeg') -Force
Copy-Item -LiteralPath (Join-Path $dir 'sample.jpg') -Destination (Join-Path $dir 'sample.jfif') -Force
New-Sample 'sample.webp' $still
New-Sample 'sample.gif' $still
New-Sample 'sample.bmp' $still
New-Sample 'sample.avif' ($still + @('-c:v', 'libaom-av1', '-still-picture', '1'))

# Icon: a blue tile, written with System.Drawing (FFmpeg has no .ico muxer).
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 64, 64
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(77, 160, 255))
$g.Dispose()
$icon = [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
$fs = [System.IO.File]::Create((Join-Path $dir 'sample.ico'))
$icon.Save($fs)
$fs.Close()
Write-Host 'made sample.ico'

# Sound
New-Sample 'sample.mp3' ($tone + @('-c:a', 'libmp3lame', '-b:a', '128k'))
New-Sample 'sample.wav' $tone
New-Sample 'sample.flac' $tone
New-Sample 'sample.aac' ($tone + @('-c:a', 'aac', '-f', 'adts'))
New-Sample 'sample.m4a' ($tone + @('-c:a', 'aac', '-f', 'ipod'))
New-Sample 'sample.aif' ($tone + @('-c:a', 'pcm_s16be', '-f', 'aiff'))
New-Sample 'sample.aiff' ($tone + @('-c:a', 'pcm_s16be', '-f', 'aiff'))
New-Sample 'sample.ogg' ($tone + @('-c:a', 'libvorbis', '-f', 'ogg'))
New-Sample 'sample.oga' ($tone + @('-c:a', 'libvorbis', '-f', 'ogg'))
New-Sample 'sample.opus' ($tone + @('-c:a', 'libopus', '-f', 'ogg'))
New-Sample 'sample.wma' ($tone + @('-c:a', 'wmav2', '-f', 'asf'))

# Video
New-Sample 'sample.mp4' ($clip + @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-f', 'mp4'))
New-Sample 'sample.m4v' ($clip + @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-f', 'mp4'))
New-Sample 'sample.mkv' ($clip + @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-f', 'matroska'))
New-Sample 'sample.webm' ($clip + @('-c:v', 'libvpx', '-c:a', 'libvorbis', '-f', 'webm'))
New-Sample 'sample.mov' ($clip + @('-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-f', 'mov'))
New-Sample 'sample.avi' ($clip + @('-c:v', 'mpeg4', '-c:a', 'pcm_s16le', '-f', 'avi'))
New-Sample 'sample.wmv' ($clip + @('-c:v', 'wmv2', '-c:a', 'wmav2', '-f', 'asf'))
New-Sample 'sample.flv' ($clip + @('-c:v', 'flv', '-c:a', 'pcm_s16le', '-f', 'flv'))
New-Sample 'sample.mpg' ($clip + @('-c:v', 'mpeg2video', '-c:a', 'mp2', '-f', 'mpeg'))
New-Sample 'sample.mpeg' ($clip + @('-c:v', 'mpeg2video', '-c:a', 'mp2', '-f', 'mpeg'))
New-Sample 'sample.m2ts' ($clip + @('-c:v', 'mpeg2video', '-c:a', 'mp2', '-f', 'mpegts'))
New-Sample 'sample.mts' ($clip + @('-c:v', 'mpeg2video', '-c:a', 'mp2', '-f', 'mpegts'))
New-Sample 'sample.ogv' ($clip + @('-c:v', 'libtheora', '-c:a', 'libvorbis', '-f', 'ogg'))
New-Sample 'sample.3gp' ($clip176 + @('-c:v', 'mpeg4', '-c:a', 'aac', '-f', '3gp'))
