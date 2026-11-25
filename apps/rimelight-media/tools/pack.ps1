# Builds Rimelight Media for release and packs the Velopack installer and update package into releases/.
#   powershell -ExecutionPolicy Bypass -File tools/pack.ps1              # pack the version in Cargo.toml
#   powershell -ExecutionPolicy Bypass -File tools/pack.ps1 -Install     # ...and install it on this PC (silent)
#   tools/pack.ps1 -Version 0.2.0 -Repo owner/rimelight-media            # what the release workflow runs
# -Repo bakes the GitHub repository into the build so installed copies update from its releases.
param(
    [string]$Version,
    [string]$Repo = $env:RIMELIGHT_MEDIA_REPO,
    [switch]$Install
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $cargoVersion = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    if (-not $Version) { $Version = $cargoVersion }
    if ($Version -ne $cargoVersion) { throw "Version $Version doesn't match Cargo.toml ($cargoVersion). Bump Cargo.toml first." }

    & (Join-Path $PSScriptRoot 'get-ffmpeg.ps1')

    # Always this app's own target folder, whatever the shell has set.
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    if ($Repo) { $env:RIMELIGHT_MEDIA_REPO = $Repo } else { Write-Host 'No -Repo: this build will not check for updates.' }
    cargo build --release
    if ($LASTEXITCODE) { throw 'cargo build failed' }

    # The files that ship: the exe, the five FFmpeg libraries it uses, and the licences.
    $publish = Join-Path $root 'publish'
    if (Test-Path $publish) { Remove-Item -Recurse -Force $publish }
    New-Item -ItemType Directory -Force (Join-Path $publish 'licenses') | Out-Null
    $release = Join-Path $root 'target\release'
    Copy-Item (Join-Path $release 'rimelight-media.exe') $publish
    foreach ($lib in 'avcodec-', 'avformat-', 'avutil-', 'swresample-', 'swscale-') {
        Get-ChildItem (Join-Path $root 'ffmpeg\current\bin') -Filter "$lib*.dll" | Copy-Item -Destination $publish
    }
    Copy-Item (Join-Path $root 'ffmpeg\current\LICENSE.txt') (Join-Path $publish 'licenses\FFmpeg-LICENSE.txt')
    Copy-Item (Join-Path $root 'ffmpeg\current\dav1d-LICENSE.txt') (Join-Path $publish 'licenses\dav1d-LICENSE.txt')
    Copy-Item (Join-Path $root 'ffmpeg\current\BUILD-INFO.txt') (Join-Path $publish 'licenses\FFmpeg-BUILD-INFO.txt')
    Copy-Item (Join-Path $root 'assets\icons\LICENSE') (Join-Path $publish 'licenses\Lucide-LICENSE.txt')
    Copy-Item (Join-Path $root 'THIRD-PARTY-NOTICES.md') $publish

    $icon = Join-Path $root 'target\rimelight-media.ico'
    Start-Process -FilePath (Join-Path $publish 'rimelight-media.exe') -ArgumentList '--write-icon', "`"$icon`"" -Wait -NoNewWindow

    # The Velopack CLI, pinned here and in .github/workflows/release.yml (VPK_VERSION).
    $vpkVersion = '1.2.161'
    dotnet tool update --global vpk --version $vpkVersion | Out-Null
    $out = Join-Path $root 'releases'
    vpk pack --packId RimelightMedia --packVersion $Version --packDir $publish --mainExe rimelight-media.exe `
        --packTitle 'Rimelight Media' --packAuthors 'Rimelight Entertainment' --icon $icon --outputDir $out `
        --shortcuts None
    # No installer-made shortcuts: the app makes Start menu > Rimelight > Rimelight Media and the optional Desktop one
    # (src/install.rs), so they follow the setup choices.
    if ($LASTEXITCODE) { throw 'vpk pack failed' }

    $installed = [math]::Round(((Get-ChildItem $publish -Recurse -File | Measure-Object Length -Sum).Sum) / 1MB, 1)
    $setup = Get-Item (Join-Path $out 'RimelightMedia-win-Setup.exe')
    Write-Host ("Installed size about {0} MB; installer {1} MB: {2}" -f $installed, [math]::Round($setup.Length / 1MB, 1), $setup.FullName)

    if ($Install) {
        Start-Process -FilePath $setup.FullName -ArgumentList '--silent' -Wait
        Write-Host "Installed to $env:LOCALAPPDATA\RimelightMedia (Start menu: Rimelight Media)."
    }
}
finally {
    Pop-Location
}
