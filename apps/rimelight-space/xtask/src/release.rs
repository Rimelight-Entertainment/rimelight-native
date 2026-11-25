//! `cargo xtask release pack`: builds the desktop app in release mode and packages it with Velopack's `vpk` (REL.2, decision
//! 0047). The output folder holds the installer (`RimelightSpace-stable-Setup.exe`), the full and delta packages and the
//! feed files, ready to upload to `space/releases/win/` in R2 (the release workflow does that).
//!
//! Windows only. It needs `vpk` on the PATH, at the version pinned in `VPK_VERSION` (the workflow installs it with
//! `dotnet tool install --global vpk --version <pin>`), and, for video, `cargo xtask ffmpeg` run once before.
//!
//! Signing is off until a certificate exists: set `RIMELIGHT_SIGN_PARAMS` to a `signtool.exe` argument string and it is
//! passed to `vpk pack --signParams`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The `vpk` version this repository packs with. It must equal the `velopack` crate that `rimelight_update` (rimelight-rs) depends on.
const VPK_VERSION: &str = "1.3.39-g8d2844f";
const PACK_ID: &str = "RimelightSpace";
const TITLE: &str = "Rimelight Space";
const AUTHORS: &str = "Rimelight Entertainment";
const CHANNEL: &str = "stable";
const MAIN_EXE: &str = "rimelight_desktop.exe";
/// Where the installed copies read the feed (apps/desktop/src/update.rs). `pack` downloads the previous releases from here
/// so `vpk` can build delta packages.
const FEED_URL: &str = "https://rimelight.com/v1/releases/win";

const USAGE: &str = "\
usage: cargo xtask release pack [--expect-version <x.y.z>] [--out <dir>] [--no-download]
       cargo xtask release version

  --expect-version  fail unless the workspace version is exactly this (the workflow passes the tag without its `v`)
  --out             output folder (default: target/release-out under the target folder)
  --no-download     do not fetch the previous releases (no delta packages)
  version           print the workspace version";

pub fn run(args: &[&str]) -> ExitCode {
    match args {
        ["pack", rest @ ..] => match pack(rest) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("FAILED: xtask release: {err}");
                ExitCode::FAILURE
            }
        },
        ["version"] => match workspace_version() {
            Ok(version) => {
                println!("{version}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("FAILED: xtask release: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

struct Options {
    expect_version: Option<String>,
    out: Option<PathBuf>,
    download: bool,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut options = Options {
        expect_version: None,
        out: None,
        download: true,
    };
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match *flag {
            "--expect-version" => {
                let value = iter.next().ok_or("--expect-version needs a value")?;
                options.expect_version = Some((*value).to_string());
            }
            "--out" => {
                let value = iter.next().ok_or("--out needs a value")?;
                options.out = Some(PathBuf::from(*value));
            }
            "--no-download" => options.download = false,
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(options)
}

/// `version = "x.y.z"` under `[workspace.package]` in the root Cargo.toml.
fn workspace_version() -> Result<String, String> {
    let manifest = repo_root().join("Cargo.toml");
    let text =
        fs::read_to_string(&manifest).map_err(|e| format!("read {}: {e}", manifest.display()))?;
    version_from_manifest(&text).ok_or_else(|| {
        format!(
            "no version under [workspace.package] in {}",
            manifest.display()
        )
    })
}

fn version_from_manifest(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
            continue;
        }
        if in_package && let Some(rest) = line.strip_prefix("version") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| repo_root().join("target"), PathBuf::from)
}

fn run_step(label: &str, command: &mut Command) -> Result<(), String> {
    println!("==> {label}");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{label} failed ({status})")),
        Err(err) => Err(format!("{label} could not start: {err}")),
    }
}

fn pack(args: &[&str]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("release packaging is Windows only for now (decision 0047)".into());
    }
    let options = parse(args)?;
    println!("xtask release: expects vpk {VPK_VERSION} on the PATH");
    let version = workspace_version()?;
    if let Some(expected) = &options.expect_version
        && *expected != version
    {
        return Err(format!(
            "the tag says {expected} but the workspace version is {version}; bump `[workspace.package] version` in Cargo.toml first"
        ));
    }
    let out = options
        .out
        .unwrap_or_else(|| target_dir().join("release-out"));
    let stage = target_dir().join("release-stage");

    // Whole workspace selected, narrowed by a target flag (decision 0043).
    run_step(
        "cargo build --workspace --release --bin rimelight_desktop",
        Command::new("cargo").args([
            "build",
            "--workspace",
            "--release",
            "--bin",
            "rimelight_desktop",
        ]),
    )?;

    stage_files(&target_dir().join("release"), &stage)?;
    fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;

    if options.download {
        // The first release has nothing to download; `--allowEmptyChannel` makes that a success.
        run_step(
            "vpk download http (previous releases, for delta packages)",
            Command::new("vpk")
                .args(["download", "http", "--url", FEED_URL, "--channel", CHANNEL])
                .arg("--outputDir")
                .arg(&out)
                .arg("--allowEmptyChannel"),
        )?;
    }

    let notes = repo_root().join("release").join("notes.md");
    let mut pack_cmd = Command::new("vpk");
    pack_cmd
        .arg("pack")
        .args(["--packId", PACK_ID])
        .args(["--packVersion", &version])
        .args(["--packTitle", TITLE])
        .args(["--packAuthors", AUTHORS])
        .args(["--mainExe", MAIN_EXE])
        .args(["--channel", CHANNEL])
        .arg("--packDir")
        .arg(&stage)
        .arg("--outputDir")
        .arg(&out);
    if notes.is_file() {
        pack_cmd.arg("--releaseNotes").arg(&notes);
    }
    if let Ok(params) = std::env::var("RIMELIGHT_SIGN_PARAMS")
        && !params.trim().is_empty()
    {
        pack_cmd.args(["--signParams", &params]);
    } else {
        println!("xtask release: RIMELIGHT_SIGN_PARAMS is not set, the files are NOT signed");
    }
    run_step("vpk pack", &mut pack_cmd)?;

    // Feed files that are not Velopack's own: the minimum version (read by the running app).
    let min = fs::read_to_string(repo_root().join("release").join("min-version.txt"))
        .map_err(|e| format!("read release/min-version.txt: {e}"))?;
    fs::write(
        out.join(format!("min-version.{CHANNEL}.txt")),
        min.trim().to_string() + "\n",
    )
    .map_err(|e| format!("write the minimum version file: {e}"))?;

    println!(
        "xtask release: version {version} packaged in {}",
        out.display()
    );
    Ok(())
}

/// The folder `vpk` packs: the app exe and every DLL next to it (the FFmpeg ones, copied by the vendored ffmpeg-sys-next).
fn stage_files(release_dir: &Path, stage: &Path) -> Result<(), String> {
    if stage.exists() {
        fs::remove_dir_all(stage).map_err(|e| format!("clear {}: {e}", stage.display()))?;
    }
    fs::create_dir_all(stage).map_err(|e| format!("create {}: {e}", stage.display()))?;
    let exe = release_dir.join(MAIN_EXE);
    if !exe.is_file() {
        return Err(format!("{} was not built", exe.display()));
    }
    fs::copy(&exe, stage.join(MAIN_EXE)).map_err(|e| format!("copy {}: {e}", exe.display()))?;
    let mut dlls = 0;
    let entries =
        fs::read_dir(release_dir).map_err(|e| format!("read {}: {e}", release_dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
            && let Some(name) = path.file_name()
        {
            fs::copy(&path, stage.join(name))
                .map_err(|e| format!("copy {}: {e}", path.display()))?;
            dlls += 1;
        }
    }
    if dlls == 0 {
        return Err(format!(
            "no FFmpeg DLLs next to {MAIN_EXE} in {}; run `cargo xtask ffmpeg` and build again",
            release_dir.display()
        ));
    }
    println!(
        "xtask release: staged {MAIN_EXE} and {dlls} DLLs in {}",
        stage.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_workspace_package_version() {
        let text = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace.dependencies]\nversion = \"9\"\n";
        assert_eq!(version_from_manifest(text).as_deref(), Some("0.1.0"));
    }

    #[test]
    fn no_workspace_package_section_gives_none() {
        assert_eq!(
            version_from_manifest("[package]\nversion = \"1.0.0\"\n"),
            None
        );
    }

    #[test]
    fn the_real_manifest_has_a_version() {
        assert!(workspace_version().is_ok());
    }

    #[test]
    fn parse_reads_flags() {
        let options = parse(&["--expect-version", "1.2.3", "--no-download"]).expect("parses");
        assert_eq!(options.expect_version.as_deref(), Some("1.2.3"));
        assert!(!options.download);
        assert!(parse(&["--bogus"]).is_err());
    }
}
