mod clean;
mod docs;
mod ffmpeg;
mod media;
mod release;
mod screenshot;
mod targets;

use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  gate quick      cargo fmt --check, clippy -D warnings, lint
  gate test       gate quick, then cargo test --workspace (lib, bins, tests, one thread), then doc tests
  clean           remove incremental, screenshot, tmp and stale build dirs from target/ (--all: cargo clean)
  gate targets    cargo check for installed targets: host, wasm32, android (cargo-ndk)
  lint            static checks (stub: prints checks not yet implemented)
  screenshot      capture PNGs to check a UI change: windows | shell/<state> | gallery-all | android (see `cargo xtask screenshot`)
  docs ia         regenerate the route table in PROJECT.md section 6.5
  ffmpeg          fetch the pinned FFmpeg build the apps link against into ffmpeg/current (Windows, once per machine)
  release pack    build and package Rimelight Space with Velopack (Windows; see `cargo xtask release`)
  media pack      build and package Rimelight Media with Velopack, --install to set it up here (see `cargo xtask media`)
  personal pack  build and package Rimelight Personal (app, Worker bundle, Velopack installer): --version <x.y.z>
  db              not implemented";

/// Checks that `lint` will run once they are implemented (PROJECT.md section 10.1).
const PENDING_LINT_CHECKS: &[&str] = &[
    "dependency rules (PROJECT.md section 7.2)",
    "raw colour, font size and spacing literal ban outside space_ui",
    "i18n key check",
    "navigation manifest check",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["gate", "quick"] => gate_quick(),
        ["gate", "test"] => {
            if gate_quick() != ExitCode::SUCCESS {
                return ExitCode::FAILURE;
            }
            // Every step selects the whole workspace, so all of them share one dependency build (decision 0043).
            // `--lib --bins --tests` leaves out the examples.
            if run_step(
                "cargo test --workspace --lib --bins --tests",
                "cargo",
                &[
                    "test",
                    "--workspace",
                    "--lib",
                    "--bins",
                    "--tests",
                    "--",
                    "--test-threads=1",
                ],
            ) != ExitCode::SUCCESS
            {
                return ExitCode::FAILURE;
            }
            run_step(
                "cargo test --workspace --doc",
                "cargo",
                &["test", "--workspace", "--doc"],
            )
        }
        ["clean", rest @ ..] => clean::run(rest),
        ["gate", "targets"] => targets::run(),
        ["lint"] => lint(),
        ["screenshot", rest @ ..] => screenshot::run(rest),
        ["docs", rest @ ..] => docs::run(rest),
        ["ffmpeg", rest @ ..] => ffmpeg::run(rest),
        ["release", rest @ ..] => release::run(rest),
        ["media", rest @ ..] => media::run(rest),
        ["personal", "pack", "--version", version] => personal_pack(version),
        ["db"] => not_implemented("db"),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn gate_quick() -> ExitCode {
    let steps: [(&str, &str, &[&str]); 2] = [
        (
            "cargo fmt --all --check",
            "cargo",
            &["fmt", "--all", "--check"],
        ),
        (
            "cargo clippy --workspace --all-targets -- -D warnings",
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ];
    for (label, program, args) in steps {
        if run_step(label, program, args) != ExitCode::SUCCESS {
            return ExitCode::FAILURE;
        }
    }
    lint()
}

fn lint() -> ExitCode {
    println!("xtask lint: stub, the following checks are NOT YET IMPLEMENTED:");
    for check in PENDING_LINT_CHECKS {
        println!("  - {check}");
    }
    println!("xtask lint: passed (stub, no checks run)");
    ExitCode::SUCCESS
}

fn run_step(label: &str, program: &str, args: &[&str]) -> ExitCode {
    println!("==> {label}");
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("FAILED: {label} ({status})");
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("FAILED: {label} could not start: {err}");
            ExitCode::FAILURE
        }
    }
}

fn not_implemented(name: &str) -> ExitCode {
    eprintln!("xtask {name}: not implemented");
    ExitCode::from(2)
}

/// The `vpk` version Rimelight Personal packs with (the one the release workflow used with pack-windows.ps1).
const PERSONAL_VPK_VERSION: &str = "1.3.39-g8d2844f";

/// `cargo xtask personal pack --version <x.y.z>`: the release build of Rimelight Personal, the FFmpeg DLLs it loads, the
/// Worker bundle it sets up the owner's backend from, and a Velopack installer and update package in
/// `apps/rimelight-personal/releases`. Windows only.
fn personal_pack(version: &str) -> ExitCode {
    match personal_pack_run(version) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("FAILED: xtask personal pack: {err}");
            ExitCode::FAILURE
        }
    }
}

fn personal_pack_run(version: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Personal packaging is Windows only".into());
    }
    let root = ffmpeg::workspace_root();
    let app = root.join("apps").join("rimelight-personal");
    let stage = app.join("stage");
    let releases = app.join("releases");
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), Into::into);

    for dir in [&stage, &releases] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| format!("clear {}: {e}", dir.display()))?;
        }
    }
    std::fs::create_dir_all(stage.join("worker"))
        .map_err(|e| format!("create {}: {e}", stage.display()))?;
    std::fs::create_dir_all(&releases)
        .map_err(|e| format!("create {}: {e}", releases.display()))?;

    // 1. The app, release build.
    personal_run(
        "cargo build --release -p personal_desktop",
        Command::new("cargo").current_dir(&root).args([
            "build",
            "--release",
            "-p",
            "personal_desktop",
        ]),
    )?;
    copy_file(
        &target.join("release").join("personal_desktop.exe"),
        &stage.join("personal_desktop.exe"),
    )?;

    // 1b. The FFmpeg DLLs the app loads (`cargo xtask ffmpeg` fills ffmpeg/current).
    let ffmpeg_bin = root.join("ffmpeg").join("current").join("bin");
    let mut dlls = 0;
    for entry in std::fs::read_dir(&ffmpeg_bin)
        .map_err(|e| {
            format!(
                "read {}: {e}; run cargo xtask ffmpeg first",
                ffmpeg_bin.display()
            )
        })?
        .flatten()
    {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        {
            copy_file(&path, &stage.join(entry.file_name()))?;
            dlls += 1;
        }
    }
    if dlls == 0 {
        return Err(format!(
            "no FFmpeg DLLs in {}; run cargo xtask ffmpeg first",
            ffmpeg_bin.display()
        ));
    }

    // 2. The Worker bundle, built from the committed config (no account needed). `cmd /C` runs pnpm's Windows shim.
    let worker = app.join("worker");
    personal_run(
        "install the worker's packages",
        Command::new("cmd").current_dir(&worker).args([
            "/C",
            "pnpm",
            "install",
            "--frozen-lockfile",
        ]),
    )?;
    let worker_out = stage.join("worker-out");
    personal_run(
        "build the worker bundle",
        Command::new("cmd")
            .current_dir(&worker)
            .args([
                "/C",
                "pnpm",
                "exec",
                "wrangler",
                "deploy",
                "--config",
                "wrangler.bundle.jsonc",
                "--dry-run",
                "--outdir",
            ])
            .arg(&worker_out),
    )?;
    copy_file(
        &worker_out.join("fetch.js"),
        &stage.join("worker").join("fetch.js"),
    )?;
    std::fs::remove_dir_all(&worker_out)
        .map_err(|e| format!("remove {}: {e}", worker_out.display()))?;

    // 3. Licence text beside the app (optional, as before).
    let license = app.join("LICENSE");
    if license.is_file() {
        copy_file(&license, &stage.join("LICENSE"))?;
    }

    // 4. Velopack: installer and update package.
    personal_run(
        "install vpk",
        Command::new("dotnet").args([
            "tool",
            "update",
            "--global",
            "vpk",
            "--version",
            PERSONAL_VPK_VERSION,
        ]),
    )?;
    personal_run(
        "vpk pack",
        Command::new("vpk")
            .args([
                "pack",
                "--packId",
                "RimelightPersonal",
                "--packVersion",
                version,
            ])
            .arg("--packDir")
            .arg(&stage)
            .args([
                "--mainExe",
                "personal_desktop.exe",
                "--packTitle",
                "Rimelight Personal",
                "--packAuthors",
                "Rimelight Entertainment",
                "--shortcuts",
                "Desktop,StartMenu",
            ])
            .arg("--icon")
            .arg(app.join("assets").join("logo").join("logo_color.ico"))
            .arg("--outputDir")
            .arg(&releases),
    )?;

    let setup = std::fs::read_dir(&releases)
        .map_err(|e| format!("read {}: {e}", releases.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with("Setup.exe"))
        .ok_or_else(|| format!("vpk did not write a setup file into {}", releases.display()))?;
    println!("Packed: {}", setup.display());
    Ok(())
}

fn copy_file(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} to {}: {e}", from.display(), to.display()))
}

fn personal_run(label: &str, command: &mut Command) -> Result<(), String> {
    println!("==> {label}");
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{label} failed ({status})")),
        Err(err) => Err(format!("{label} could not start: {err}")),
    }
}
