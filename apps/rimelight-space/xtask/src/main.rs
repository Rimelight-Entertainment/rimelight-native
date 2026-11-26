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
