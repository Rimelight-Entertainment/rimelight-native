# Rimelight Media

A high-performance unified media application built with Rust and Slint.

## Overview

**Rimelight Media** is a high-performance desktop application for opening and lightly editing images, video, audio, and text files. It also supports screen capture, screen recording, audio recording, webcam, and drawings.

## Features

## Local Development

### Prerequisites

- **Rust:**

### Build Steps

Run from the repository root:

```powershell
# 1. Download the pinned trimmed LGPL FFmpeg build (one-time setup)
cargo xtask ffmpeg

# 2. Run debug build
cargo run -p media_desktop

# 3. Build & install local release package via Velopack
cargo xtask media pack --install
```

### Releases & Distribution

Use **Velopack + GitHub Releases** with releases built by GitHub Actions when a version tag is pushed:

- **Installer:** `RimelightMedia-win-Setup.exe`. Installs per user (no admin, no UAC prompt) to `%LOCALAPPDATA%\RimelightMedia`, adds Start menu shortcut, registers file types (pictures, video, music), and registers with Default Apps.
- **Updates:** Installed copies check the repository's latest release a few seconds after startup, download delta packages in the background, and prompt to restart.
- **Making a Release:**
  1. Bump `version` in `Cargo.toml`.
  2. Commit and push to main.
  3. Create and push tag `vX.Y.Z`:
     ```powershell
     git tag v0.X.X
     git push origin v0.X.X
     ```
  4. GitHub Actions builds the release and attaches `RimelightMedia-win-Setup.exe` and `.nupkg` assets to the release.

## License

MIT License. See [LICENSE.md](LICENSE.md) for more details.
