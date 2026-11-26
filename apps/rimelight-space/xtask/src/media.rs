//! `cargo xtask media pack`: builds Rimelight Media in release mode and packs it with Velopack's `vpk`, the way
//! `cargo xtask release pack` packs Space. The output folder (`apps/rimelight-media/releases`) is what the release workflow
//! uploads to GitHub. `--install` also installs the packed setup on this PC.
//!
//! Windows only for now, like `cargo xtask ffmpeg` (which it runs first when the FFmpeg folder is missing). `vpk` must be on
//! the PATH or installed by `dotnet tool update` (this command does that at `VPK_VERSION`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::ffmpeg;

/// The Velopack CLI this app packs with. The release workflow installs the same version (`cargo xtask media vpk-version`).
const VPK_VERSION: &str = "1.2.161";
const PACK_ID: &str = "RimelightMedia";
const TITLE: &str = "Rimelight Media";
const AUTHORS: &str = "Rimelight Entertainment";
/// The app's exe, as `apps/rimelight-media/Cargo.toml` names its binary. Installed copies look for this name.
const MAIN_EXE: &str = "rimelight-media.exe";
const PACKAGE: &str = "media_desktop";
/// The GitHub repository the build bakes in, so installed copies update from its releases (`option_env!` in the app).
const REPO_ENV: &str = "RIMELIGHT_MEDIA_REPO";
/// The FFmpeg DLLs the app loads, by file-name prefix (`ffmpeg/current/bin`).
const FFMPEG_DLLS: &[&str] = &["avcodec-", "avformat-", "avutil-", "swresample-", "swscale-"];

const USAGE: &str = "\
usage: cargo xtask media pack [--version <x.y.z>] [--repo <owner/name>] [--install]
       cargo xtask media vpk-version

  --version     the version to pack; must equal the version in apps/rimelight-media/Cargo.toml (the default)
  --repo        the GitHub repository baked into the build (default: $RIMELIGHT_MEDIA_REPO; none means no update checks)
  --install     install the packed setup on this PC (silent)
  vpk-version   print the Velopack CLI version the release workflow must install";

pub fn run(args: &[&str]) -> ExitCode {
    match args {
        ["pack", rest @ ..] => finish(pack(rest)),
        ["vpk-version"] => {
            println!("{VPK_VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("FAILED: xtask media: {err}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    version: Option<String>,
    repo: Option<String>,
    install: bool,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut options = Options {
        version: None,
        repo: std::env::var(REPO_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty()),
        install: false,
    };
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match *flag {
            "--version" => options.version = Some(value(&mut iter, flag)?),
            "--repo" => options.repo = Some(value(&mut iter, flag)?),
            "--install" => options.install = true,
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(options)
}

fn value(iter: &mut std::slice::Iter<'_, &str>, flag: &str) -> Result<String, String> {
    iter.next()
        .map(|v| (*v).to_string())
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn pack(args: &[&str]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("media packaging is Windows only for now".into());
    }
    let options = parse(args)?;
    let cargo_version = cargo_version()?;
    let version = options.version.clone().unwrap_or_else(|| cargo_version.clone());
    if version != cargo_version {
        return Err(format!(
            "version {version} doesn't match apps/rimelight-media/Cargo.toml ({cargo_version}); bump it first"
        ));
    }

    let ffmpeg_dir = ffmpeg::ensure()?;
    let target = target_dir();

    let mut build = Command::new("cargo");
    build.args(["build", "--release", "-p", PACKAGE]);
    match &options.repo {
        Some(repo) => {
            build.env(REPO_ENV, repo);
        }
        None => println!("xtask media: no --repo: this build will not check for updates"),
    }
    run_step("cargo build --release", &mut build)?;

    let stage = target.join("media-publish");
    stage_files(&target.join("release"), &ffmpeg_dir, &stage)?;

    let icon = target.join("rimelight-media.ico");
    run_step(
        "write the app icon",
        Command::new(stage.join(MAIN_EXE)).arg("--write-icon").arg(&icon),
    )?;

    run_step(
        "install the vpk CLI",
        Command::new("dotnet").args(["tool", "update", "--global", "vpk", "--version", VPK_VERSION]),
    )?;
    let out = media_dir().join("releases");
    fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
    run_step(
        "vpk pack",
        Command::new("vpk")
            .args(["pack", "--packId", PACK_ID, "--packVersion", version.as_str()])
            .arg("--packDir")
            .arg(&stage)
            .args(["--mainExe", MAIN_EXE, "--packTitle", TITLE, "--packAuthors", AUTHORS])
            .arg("--icon")
            .arg(&icon)
            .arg("--outputDir")
            .arg(&out)
            // The app makes its own Start menu and Desktop shortcuts (src/install.rs), so they follow the setup choices.
            .args(["--shortcuts", "None"]),
    )?;

    let setup = out.join(format!("{PACK_ID}-win-Setup.exe"));
    if !setup.is_file() {
        return Err(format!("vpk did not write {}", setup.display()));
    }
    println!(
        "xtask media: installed size about {} MB; installer {} MB: {}",
        megabytes(&stage)?,
        megabytes(&setup)?,
        setup.display()
    );

    if options.install {
        run_step(
            "install on this PC",
            Command::new(&setup).arg("--silent"),
        )?;
        println!("xtask media: installed to %LOCALAPPDATA%\\RimelightMedia (Start menu: Rimelight Media)");
    }
    Ok(())
}

/// The folder `vpk` packs: the exe, the FFmpeg DLLs it loads, and the licences.
fn stage_files(release_dir: &Path, ffmpeg_dir: &Path, stage: &Path) -> Result<(), String> {
    if stage.exists() {
        fs::remove_dir_all(stage).map_err(|e| format!("clear {}: {e}", stage.display()))?;
    }
    let licenses = stage.join("licenses");
    fs::create_dir_all(&licenses).map_err(|e| format!("create {}: {e}", licenses.display()))?;

    copy(&release_dir.join(MAIN_EXE), &stage.join(MAIN_EXE))?;
    let bin = ffmpeg_dir.join("bin");
    let mut dlls = 0;
    for entry in fs::read_dir(&bin).map_err(|e| format!("read {}: {e}", bin.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.ends_with(".dll") && FFMPEG_DLLS.iter().any(|lib| name.starts_with(lib)) {
            copy(&path, &stage.join(name))?;
            dlls += 1;
        }
    }
    if dlls != FFMPEG_DLLS.len() {
        return Err(format!(
            "found {dlls} FFmpeg DLLs in {}, expected {}; run `cargo xtask ffmpeg`",
            bin.display(),
            FFMPEG_DLLS.len()
        ));
    }

    let root = media_dir();
    let licence_files = [
        (ffmpeg_dir.join("LICENSE.txt"), "FFmpeg-LICENSE.txt"),
        (ffmpeg_dir.join("dav1d-LICENSE.txt"), "dav1d-LICENSE.txt"),
        (ffmpeg_dir.join("BUILD-INFO.txt"), "FFmpeg-BUILD-INFO.txt"),
        (root.join("assets").join("icons").join("LICENSE"), "Lucide-LICENSE.txt"),
    ];
    for (from, name) in licence_files {
        copy(&from, &licenses.join(name))?;
    }
    copy(&root.join("THIRD-PARTY-NOTICES.md"), &stage.join("THIRD-PARTY-NOTICES.md"))?;
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} to {}: {e}", from.display(), to.display()))
}

fn run_step(label: &str, command: &mut Command) -> Result<(), String> {
    println!("==> {label}");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{label} failed ({status})")),
        Err(err) => Err(format!("{label} could not start: {err}")),
    }
}

/// Total size in MB of a file, or of every file under a folder.
fn megabytes(path: &Path) -> Result<String, String> {
    fn total(path: &Path) -> std::io::Result<u64> {
        if path.is_dir() {
            let mut sum = 0;
            for entry in fs::read_dir(path)? {
                sum += total(&entry?.path())?;
            }
            Ok(sum)
        } else {
            Ok(fs::metadata(path)?.len())
        }
    }
    let bytes = total(path).map_err(|e| format!("size of {}: {e}", path.display()))?;
    Ok(format!("{:.1}", bytes as f64 / 1_048_576.0))
}

/// `version` under `[package]` in apps/rimelight-media/Cargo.toml.
fn cargo_version() -> Result<String, String> {
    let manifest = media_dir().join("Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package
            && let Some(rest) = line.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            return Ok(value.trim().trim_matches('"').to_string());
        }
    }
    Err(format!("no version under [package] in {}", manifest.display()))
}

fn media_dir() -> PathBuf {
    crate::ffmpeg::workspace_root()
        .join("apps")
        .join("rimelight-media")
}

/// `CARGO_TARGET_DIR` when set (the shared target folder, see AGENTS.md), else `target/` in the workspace.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| crate::ffmpeg::workspace_root().join("target"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_flags() {
        let options = parse(&["--version", "1.2.3", "--repo", "owner/app", "--install"])
            .expect("parses");
        assert_eq!(options.version.as_deref(), Some("1.2.3"));
        assert_eq!(options.repo.as_deref(), Some("owner/app"));
        assert!(options.install);
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["--version"]).is_err());
    }

    #[test]
    fn the_media_manifest_has_a_version() {
        let version = cargo_version().expect("reads the version");
        assert!(version.split('.').count() >= 3, "got {version}");
    }
}
