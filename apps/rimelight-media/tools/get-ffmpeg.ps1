# Downloads the trimmed FFmpeg Rimelight Media links against (built by rimelight-rs tools/ffmpeg/build.sh and published as a
# release asset of this repository), checks its SHA-256 and unpacks it to ffmpeg/current. Run once per machine
# (tools/pack.ps1 runs it too):
#   powershell -ExecutionPolicy Bypass -File tools/get-ffmpeg.ps1
# The pin below mirrors rimelight-rs tools/ffmpeg/pin.txt (release, asset, sha256). Change both together.
$ErrorActionPreference = 'Stop'
$Release = 'ffmpeg-n9.0.2-r1'
$Asset = 'rimelight-ffmpeg-n9.0.2-r1-win64-lgpl-shared.zip'
$Sha256 = '013136dbfe3ae75d6f2eadcd6bec35f7a2fc49d2163996f01625ef5a912d0de0'

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'ffmpeg'
$current = Join-Path $dest 'current'
if (Test-Path (Join-Path $current 'lib')) {
    if ((Get-Content (Join-Path $current 'PINNED_RELEASE.txt') -ErrorAction SilentlyContinue) -eq "$Release/$Asset") {
        Write-Host "FFmpeg is already set up in $current"
        exit 0
    }
}
New-Item -ItemType Directory -Force $dest | Out-Null
$zip = Join-Path $dest $Asset
if (-not (Test-Path $zip)) {
    $url = "https://github.com/Rimelight-Entertainment/rimelight-media/releases/download/$Release/$Asset"
    Write-Host "Downloading $url"
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
}
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash
if ($hash -ne $Sha256) {
    Remove-Item $zip
    throw "$Asset has SHA-256 $hash, expected $Sha256; the file was removed"
}
$tmp = Join-Path $dest 'unpack'
if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
Expand-Archive -LiteralPath $zip -DestinationPath $tmp
$inner = Get-ChildItem $tmp -Directory | Select-Object -First 1
if (Test-Path $current) { Remove-Item -Recurse -Force $current }
Move-Item $inner.FullName $current
Remove-Item -Recurse -Force $tmp
Remove-Item $zip
Set-Content -Path (Join-Path $current 'PINNED_RELEASE.txt') -Value "$Release/$Asset" # read by the up-to-date check above
Write-Host "FFmpeg is set up in $current"
