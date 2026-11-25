# Compares Rimelight Media with the apps it replaces: disk size, then memory and CPU while each shows the same file.
#   powershell -ExecutionPolicy Bypass -File tools/bench.ps1                 # sizes + live run (15 s each)
#   tools/bench.ps1 -SizeOnly                                                # only disk sizes
#   tools/bench.ps1 -Video D:\clip.mp4 -Picture D:\photo.jpg -Seconds 30
# The live run opens each app for a moment and closes it again. An app that is already open is skipped, so nothing you
# have open gets closed. Rimelight Media must be installed (tools/pack.ps1 -Install).
param(
    [string]$Video = (Join-Path (Split-Path -Parent $PSScriptRoot) 'samples\sample.mkv'),
    [string]$Picture = (Join-Path (Split-Path -Parent $PSScriptRoot) 'samples\sample.png'),
    [int]$Seconds = 15,
    [switch]$SizeOnly
)
$ErrorActionPreference = 'Continue'

function FolderMB($path) {
    if (-not $path -or -not (Test-Path $path)) { return $null }
    [math]::Round(((Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum) / 1MB, 1)
}
function AppxFolder($name) { (Get-AppxPackage -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1).InstallLocation }

$rimelight = Join-Path $env:LOCALAPPDATA 'RimelightMedia'
$rimelightExe = Join-Path $rimelight 'current\rimelight-media.exe'
$vlcDir = @('C:\Program Files\VideoLAN\VLC', 'C:\Program Files (x86)\VideoLAN\VLC') | Where-Object { Test-Path $_ } | Select-Object -First 1

Write-Host "`nDisk size (MB)"
$sizes = @(
    [pscustomobject]@{ App = 'Rimelight Media'; MB = (FolderMB $rimelight); Where = $rimelight }
    [pscustomobject]@{ App = 'VLC'; MB = (FolderMB $vlcDir); Where = $vlcDir }
    [pscustomobject]@{ App = 'Photos'; MB = (FolderMB (AppxFolder 'Microsoft.Windows.Photos')); Where = (AppxFolder 'Microsoft.Windows.Photos') }
    [pscustomobject]@{ App = 'Snipping Tool'; MB = (FolderMB (AppxFolder 'Microsoft.ScreenSketch')); Where = (AppxFolder 'Microsoft.ScreenSketch') }
    [pscustomobject]@{ App = 'Media Player'; MB = (FolderMB (AppxFolder 'Microsoft.ZuneMusic')); Where = (AppxFolder 'Microsoft.ZuneMusic') }
)
$sizes | Format-Table -AutoSize
Write-Host '(Store apps also share Windows App SDK runtimes that are not counted here.)'
if ($SizeOnly) { return }

$cores = [Environment]::ProcessorCount
function Measure-App($label, [scriptblock]$launch, [string[]]$names) {
    if (Get-Process -Name $names -ErrorAction SilentlyContinue) {
        return [pscustomobject]@{ App = $label; Note = 'skipped: already open' }
    }
    & $launch
    Start-Sleep -Seconds 3
    $procs = Get-Process -Name $names -ErrorAction SilentlyContinue
    if (-not $procs) { return [pscustomobject]@{ App = $label; Note = 'did not start' } }
    $startCpu = ($procs | Measure-Object TotalProcessorTime -Sum).Sum
    if (-not $startCpu) { $startCpu = [TimeSpan]::Zero }
    $t0 = Get-Date
    $peakWs = 0; $wsSum = 0; $privSum = 0; $n = 0
    for ($i = 0; $i -lt $Seconds; $i++) {
        Start-Sleep -Seconds 1
        $procs = Get-Process -Name $names -ErrorAction SilentlyContinue
        $ws = ($procs | Measure-Object WorkingSet64 -Sum).Sum
        $priv = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum
        $wsSum += $ws; $privSum += $priv; $n++
        if ($ws -gt $peakWs) { $peakWs = $ws }
    }
    $procs = Get-Process -Name $names -ErrorAction SilentlyContinue
    $cpu = (($procs | ForEach-Object { $_.TotalProcessorTime }) | Measure-Object TotalMilliseconds -Sum).Sum
    $cpuPct = [math]::Round(($cpu - $startCpu.TotalMilliseconds) / ((Get-Date) - $t0).TotalMilliseconds / $cores * 100, 1)
    foreach ($p in $procs) { $null = $p.CloseMainWindow() }
    Start-Sleep -Seconds 2
    Get-Process -Name $names -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    [pscustomobject]@{
        App = $label
        'CPU %' = $cpuPct
        'Avg memory MB' = [math]::Round($wsSum / [math]::Max($n, 1) / 1MB, 1)
        'Peak memory MB' = [math]::Round($peakWs / 1MB, 1)
        'Private MB' = [math]::Round($privSum / [math]::Max($n, 1) / 1MB, 1)
        Note = ''
    }
}

$results = @()
if (Test-Path $rimelightExe) {
    $results += Measure-App 'Rimelight Media: video' { Start-Process $rimelightExe -ArgumentList "`"$Video`"" } @('rimelight-media')
    $results += Measure-App 'Rimelight Media: picture' { Start-Process $rimelightExe -ArgumentList "`"$Picture`"" } @('rimelight-media')
    $results += Measure-App 'Rimelight Media: idle' { Start-Process $rimelightExe } @('rimelight-media')
} else {
    Write-Host "Rimelight Media isn't installed: run tools/pack.ps1 -Install first."
}
if ($vlcDir) { $results += Measure-App 'VLC: video' { Start-Process (Join-Path $vlcDir 'vlc.exe') -ArgumentList "`"$Video`"" } @('vlc') }
if (AppxFolder 'Microsoft.Windows.Photos') {
    $results += Measure-App 'Photos: picture' { Start-Process "ms-photos:viewer?fileName=$([uri]::EscapeDataString($Picture))" } @('Photos', 'Microsoft.Photos', 'PhotosApp')
}
if (AppxFolder 'Microsoft.ScreenSketch') {
    $results += Measure-App 'Snipping Tool: idle' { Start-Process 'explorer.exe' -ArgumentList 'shell:AppsFolder\Microsoft.ScreenSketch_8wekyb3d8bbwe!App' } @('SnippingTool', 'ScreenSketch')
}
if (AppxFolder 'Microsoft.ZuneMusic') {
    $results += Measure-App 'Media Player: video' { Start-Process 'explorer.exe' -ArgumentList "`"$Video`"" } @('Microsoft.Media.Player', 'Music.UI')
}
Write-Host "`nWhile showing the same file for $Seconds s ($cores logical cores; CPU % is of the whole machine)"
$results | Format-Table -AutoSize
