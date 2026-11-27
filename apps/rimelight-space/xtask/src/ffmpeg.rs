//! `cargo xtask ffmpeg`: fetches the pinned FFmpeg build that the apps link against (CV-V.2, decision 0046) into
//! `ffmpeg/current` at the workspace root (the import libraries, headers and DLLs). The pin is `ffmpeg-pin.txt` at the
//! workspace root, the only place its version is written; the vendored ffmpeg-sys-next build script reads the same file
//! and copies the DLLs next to the built programs. Run it once per machine, and again when the pin changes. Media
//! packaging (`cargo xtask media pack`) fetches it too when missing.
//!
//! Windows only for now (video plays on Windows first). It uses the `curl.exe` and `tar.exe` that Windows ships and
//! PowerShell's `Get-FileHash`, so xtask needs no extra crates.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The pin file: `release=`, `asset=` and `sha256=` lines. Read at compile time, so xtask and the build scripts agree.
const PIN: &str = include_str!("../../../../ffmpeg-pin.txt");
/// Where the asset is published (still rimelight-media's release, until rimelight-rs publishes its own).
const URL_BASE: &str =
    "https://github.com/Rimelight-Entertainment/rimelight-media/releases/download";

/// The value of one `key=` line of the pin file.
fn pin(key: &str) -> &'static str {
    PIN.lines()
        .find_map(|line| line.strip_prefix(key))
        .unwrap_or_else(|| panic!("ffmpeg-pin.txt has no `{key}` line"))
}

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
    match ensure() {
        Ok(current) => {
            println!(
                "xtask ffmpeg: {} is in {}",
                pin("asset="),
                current.display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("FAILED: xtask ffmpeg: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The FFmpeg folder the apps link against (`ffmpeg/current` at the workspace root). Fetches the pinned build when that
/// folder does not hold it yet, and returns the folder. Windows only.
pub fn ensure() -> Result<PathBuf, String> {
    let root = workspace_root().join("ffmpeg");
    let current = root.join("current");
    let stamp = current.join("PINNED_RELEASE.txt");
    let wanted = format!("{}/{}", pin("release="), pin("asset="));
    if fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == wanted) {
        return Ok(current);
    }
    fetch(&root, &current, &stamp, &wanted)?;
    Ok(current)
}

fn fetch(root: &Path, current: &Path, stamp: &Path, wanted: &str) -> Result<(), String> {
    let asset = pin("asset=");
    let dl = root.join("dl");
    fs::create_dir_all(&dl).map_err(|e| format!("create {}: {e}", dl.display()))?;
    let zip = dl.join(asset);
    let url = format!("{URL_BASE}/{}/{asset}", pin("release="));
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
            "the download failed ({status}); check the pin in ffmpeg-pin.txt"
        ));
    }
    let hash = sha256_of(&zip)?;
    let expected = pin("sha256=");
    if !hash.eq_ignore_ascii_case(expected) {
        let _ = fs::remove_file(&zip);
        return Err(format!(
            "{asset} has SHA-256 {hash}, expected {expected}; the file was removed"
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
        return Err(format!("unpacking {asset} failed ({status})"));
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

/// Lower-case hex SHA-256 of a file, through Windows' `certutil.exe` (present on every Windows install; the
/// GitHub runner's `powershell.exe` does not have `Get-FileHash`).
fn sha256_of(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("{} is missing after the download", path.display()));
    }
    let out = Command::new("certutil.exe")
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|e| format!("certutil.exe could not start: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!(
            "certutil failed for {} ({}): {}{}",
            path.display(),
            out.status,
            stdout.trim(),
            stderr.trim()
        ));
    }
    // certutil prints a header line, then the hash (possibly with spaces), then a status line.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| line.split_whitespace().collect::<String>())
        .find(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|hex| hex.to_lowercase())
        .ok_or_else(|| format!("certutil printed no SHA-256 for {}", path.display()))
}

/// The monorepo root (the folder holding the workspace `Cargo.toml`): this crate is `apps/rimelight-space/xtask`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}
