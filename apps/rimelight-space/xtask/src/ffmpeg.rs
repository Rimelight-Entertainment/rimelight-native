//! `cargo xtask ffmpeg`: fetches the pinned FFmpeg build that video playback links against (CV-V.2, decision 0046)
//! into `<target dir>/ffmpeg/current` (the import libraries, headers and DLLs). The shared ffmpeg-sys-next (rimelight-rs
//! `vendor/`) finds it there and copies the DLLs next to the built programs. Run it once per machine, and again when the pin changes.
//!
//! Windows only for now (video plays on Windows first). It uses the `curl.exe` and `tar.exe` that Windows ships and
//! PowerShell's `Get-FileHash`, so xtask needs no extra crates.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The pinned build: the trimmed LGPL FFmpeg 9.0 for win64 shared by the Rimelight apps (rimelight-rs
/// `tools/ffmpeg`, options in `tools/ffmpeg/options.txt`). Keep these in step with rimelight-rs `tools/ffmpeg/pin.txt`
/// and the `PIN` in its `vendor/ffmpeg-sys-next/build.rs`, which warns on a mismatch.
const RELEASE: &str = "ffmpeg-n9.0.2-r1";
const ASSET: &str = "rimelight-ffmpeg-n9.0.2-r1-win64-lgpl-shared.zip";
const SHA256: &str = "013136dbfe3ae75d6f2eadcd6bec35f7a2fc49d2163996f01625ef5a912d0de0";
/// Where the asset is published (still rimelight-media's release, until rimelight-rs publishes its own).
const URL_BASE: &str =
    "https://github.com/Rimelight-Entertainment/rimelight-media/releases/download";

pub fn run(args: &[&str]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask ffmpeg");
        return ExitCode::from(2);
    }
    if !cfg!(windows) {
        eprintln!(
            "xtask ffmpeg: video plays on Windows only for now (decision 0046); nothing to fetch here"
        );
        return ExitCode::SUCCESS;
    }
    let root = target_dir().join("ffmpeg");
    let current = root.join("current");
    let stamp = current.join("PINNED_RELEASE.txt");
    let wanted = format!("{RELEASE}/{ASSET}");
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == wanted) {
        println!("xtask ffmpeg: {ASSET} is already in {}", current.display());
        return ExitCode::SUCCESS;
    }
    match fetch(&root, &current, &stamp, &wanted) {
        Ok(()) => {
            println!("xtask ffmpeg: {ASSET} is in {}", current.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("FAILED: xtask ffmpeg: {err}");
            ExitCode::FAILURE
        }
    }
}

fn fetch(root: &Path, current: &Path, stamp: &Path, wanted: &str) -> Result<(), String> {
    let dl = root.join("dl");
    fs::create_dir_all(&dl).map_err(|e| format!("create {}: {e}", dl.display()))?;
    let zip = dl.join(ASSET);
    let url = format!("{URL_BASE}/{RELEASE}/{ASSET}");
    println!("==> downloading {url}");
    let status = Command::new("curl.exe")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&zip)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl.exe could not start: {e}"))?;
    if !status.success() {
        return Err(format!(
            "the download failed ({status}); check the pin against rimelight-rs `tools/ffmpeg/pin.txt`"
        ));
    }
    let hash = sha256_of(&zip)?;
    if !hash.eq_ignore_ascii_case(SHA256) {
        let _ = fs::remove_file(&zip);
        return Err(format!(
            "{ASSET} has SHA-256 {hash}, expected {SHA256}; the file was removed"
        ));
    }
    let unpack = dl.join("unpack");
    let _ = fs::remove_dir_all(&unpack);
    fs::create_dir_all(&unpack).map_err(|e| format!("create {}: {e}", unpack.display()))?;
    println!("==> unpacking");
    let status = Command::new("tar.exe")
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&unpack)
        .status()
        .map_err(|e| format!("tar.exe could not start: {e}"))?;
    if !status.success() {
        return Err(format!("unpacking {ASSET} failed ({status})"));
    }
    // The zip holds one folder named like the asset.
    let inner = fs::read_dir(&unpack)
        .map_err(|e| format!("read {}: {e}", unpack.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.join("lib").is_dir() && p.join("bin").is_dir())
        .ok_or("the zip has no folder with lib/ and bin/")?;
    let _ = fs::remove_dir_all(current);
    fs::rename(&inner, current).map_err(|e| format!("move into {}: {e}", current.display()))?;
    fs::write(stamp, wanted).map_err(|e| format!("write {}: {e}", stamp.display()))?;
    let _ = fs::remove_dir_all(&dl);
    Ok(())
}

/// Lower-case hex SHA-256 of a file, through PowerShell's Get-FileHash.
fn sha256_of(path: &Path) -> Result<String, String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "(Get-FileHash -Algorithm SHA256 -LiteralPath '{}').Hash",
            path.display().to_string().replace('\'', "''")
        ))
        .output()
        .map_err(|e| format!("powershell.exe could not start: {e}"))?;
    if !out.status.success() {
        return Err("Get-FileHash failed".to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
}

/// `CARGO_TARGET_DIR` (the shared target folder, see AGENTS.md), else `target/` in the workspace.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map_or_else(|| PathBuf::from("target"), |p| p.join("target"))
        },
        PathBuf::from,
    )
}
