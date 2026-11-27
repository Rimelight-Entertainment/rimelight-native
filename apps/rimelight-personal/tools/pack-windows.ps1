# Packs Rimelight Personal for Windows: the release build of the app, the Worker bundle it sets up the owner's backend
# from, and a Velopack installer. Run from anywhere; needs Rust, Node with pnpm, and (to publish) dotnet.
#
#   pwsh apps/rimelight-personal/tools/pack-windows.ps1 -Version 0.1.0
#
# Output: apps/rimelight-personal/releases (the setup and the update package). The release workflow uploads them.

param(
    [Parameter(Mandatory = $true)] [string] $Version,
    [string] $VpkVersion = "1.3.39-g8d2844f"
)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$app = Resolve-Path (Join-Path $here "..")
$root = Resolve-Path (Join-Path $app "..\..")
$stage = Join-Path $app "stage"
$releases = Join-Path $app "releases"

function Step($label, [scriptblock] $work) {
    Write-Host "==> $label"
    & $work
    if ($LASTEXITCODE -ne 0) { throw "$label failed (exit $LASTEXITCODE)" }
}

Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $stage, $releases
New-Item -ItemType Directory -Force $stage, (Join-Path $stage "worker"), $releases | Out-Null

# 1. The app, release build.
Step "build the app (release)" { Push-Location $root; cargo build --release -p personal_desktop; Pop-Location }
Copy-Item (Join-Path $root "target\release\personal_desktop.exe") (Join-Path $stage "personal_desktop.exe")

# 1b. The FFmpeg DLLs the app loads (its Storage previews and audio go through rimelight_media). The same folder Media
# packs from: `cargo xtask ffmpeg` fills it.
$ffmpegBin = Join-Path $root "ffmpeg\current\bin"
$dlls = @(Get-ChildItem $ffmpegBin -Filter "*.dll" -ErrorAction SilentlyContinue)
if ($dlls.Count -eq 0) { throw "no FFmpeg DLLs in $ffmpegBin; run cargo xtask ffmpeg first" }
$dlls | Copy-Item -Destination $stage

# 2. The Worker bundle, built from the committed config (no account needed).
$worker = Join-Path $app "worker"
Step "install the worker's packages" { Push-Location $worker; pnpm install --frozen-lockfile; Pop-Location }
Step "build the worker bundle" {
    Push-Location $worker
    ./node_modules/.bin/wrangler deploy --config wrangler.bundle.jsonc --dry-run --outdir (Join-Path $stage "worker-out")
    Pop-Location
}
Copy-Item (Join-Path $stage "worker-out\fetch.js") (Join-Path $stage "worker\fetch.js")
Remove-Item -Recurse -Force (Join-Path $stage "worker-out")

# 3. Licence text beside the app.
Copy-Item (Join-Path $app "LICENSE") (Join-Path $stage "LICENSE") -ErrorAction SilentlyContinue

# 4. Velopack: installer and update package.
Step "install vpk" { dotnet tool update --global vpk --version $VpkVersion }
Step "vpk pack" {
    vpk pack --packId RimelightPersonal --packVersion $Version --packDir $stage `
        --mainExe personal_desktop.exe --packTitle "Rimelight Personal" --packAuthors "Rimelight Entertainment" `
        --outputDir $releases
}

$setup = Get-ChildItem $releases -Filter "*Setup.exe" | Select-Object -First 1
if (-not $setup) { throw "vpk did not write a setup file into $releases" }
Write-Host "Packed: $($setup.FullName)"
