//! `cargo xtask gate targets`: `cargo check` for every target that is installed locally.
//!
//! Host and wasm32 run whenever their rustup target is present. Android runs through cargo-ndk only when both
//! cargo-ndk and the aarch64 Android target are installed. Anything missing is skipped with a message, so the
//! gate never silently passes a target it did not check.

use std::process::{Command, ExitCode};

use crate::run_step;

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const ANDROID_TARGET: &str = "aarch64-linux-android";

pub fn run() -> ExitCode {
    let installed = installed_rustup_targets();
    let mut failed = false;
    let mut checked = Vec::new();
    let mut skipped = Vec::new();

    // Host: the whole workspace for the toolchain's default target.
    if run_step(
        "cargo check --workspace (host)",
        "cargo",
        &["check", "--workspace"],
    ) == ExitCode::SUCCESS
    {
        checked.push("host");
    } else {
        failed = true;
    }

    // wasm32: the web shell, the same crate CI builds with trunk.
    match &installed {
        Some(list) if list.iter().any(|t| t == WASM_TARGET) => {
            if run_step(
                "cargo check -p rimelight_web --target wasm32-unknown-unknown",
                "cargo",
                &["check", "-p", "rimelight_web", "--target", WASM_TARGET],
            ) == ExitCode::SUCCESS
            {
                checked.push("wasm32");
            } else {
                failed = true;
            }
        }
        _ => skipped.push(format!(
            "wasm32: target {WASM_TARGET} is not installed (rustup target add {WASM_TARGET})"
        )),
    }

    // Android: cargo-ndk check for arm64-v8a, the ABI CI builds.
    let has_ndk = cargo_ndk_available();
    let has_android = installed
        .as_ref()
        .is_some_and(|list| list.iter().any(|t| t == ANDROID_TARGET));
    if has_ndk && has_android {
        if run_step(
            "cargo ndk -t arm64-v8a check -p rimelight_android",
            "cargo",
            &["ndk", "-t", "arm64-v8a", "check", "-p", "rimelight_android"],
        ) == ExitCode::SUCCESS
        {
            checked.push("android (arm64-v8a)");
        } else {
            failed = true;
        }
    } else {
        let mut reasons = Vec::new();
        if !has_ndk {
            reasons.push("cargo-ndk is not installed (cargo install cargo-ndk)".to_string());
        }
        if !has_android {
            reasons.push(format!(
                "target {ANDROID_TARGET} is not installed (rustup target add {ANDROID_TARGET})"
            ));
        }
        skipped.push(format!("android: {}", reasons.join("; ")));
    }

    println!();
    println!("xtask gate targets summary");
    println!("  checked: {}", list_or_none(&checked));
    for skip in &skipped {
        println!("  SKIPPED {skip}");
    }
    if installed.is_none() {
        println!("  note: could not read installed rustup targets; wasm and android were skipped");
    }

    if failed {
        eprintln!("xtask gate targets: FAILED");
        return ExitCode::FAILURE;
    }
    println!("xtask gate targets: passed");
    ExitCode::SUCCESS
}

/// Lists the installed targets of the active toolchain, or `None` when rustup cannot be queried.
fn installed_rustup_targets() -> Option<Vec<String>> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().map(|line| line.trim().to_string()).collect())
}

fn cargo_ndk_available() -> bool {
    Command::new("cargo")
        .args(["ndk", "--version"])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn list_or_none(items: &[&str]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}
