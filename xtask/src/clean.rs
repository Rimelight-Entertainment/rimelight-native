//! `cargo xtask clean`: removes the parts of `target/` that are safe to regenerate and grow with every build.
//! `cargo xtask clean --all` runs `cargo clean`. Both print how much space they freed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

pub fn run(args: &[&str]) -> ExitCode {
    let all = match args {
        [] => false,
        ["--all"] => true,
        _ => {
            eprintln!("usage: cargo xtask clean [--all]");
            return ExitCode::from(2);
        }
    };
    let target = target_dir();
    let before = size_of(&target);
    if all {
        println!("==> cargo clean");
        match Command::new("cargo").arg("clean").status() {
            Ok(status) if status.success() => {}
            _ => {
                eprintln!("FAILED: cargo clean");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let mut plan = Vec::new();
        for entry in dirs_in(&target) {
            if matches!(
                entry.file_name().and_then(|n| n.to_str()),
                Some("screenshot" | "tmp")
            ) {
                plan.push(entry);
            }
        }
        for profile in profile_dirs(&target) {
            plan.push(profile.join("incremental"));
            plan.extend(stale_build_copies(&profile.join("build")));
        }
        for path in plan.iter().filter(|p| p.exists()) {
            let freed = size_of(path);
            if let Err(err) = remove(path) {
                eprintln!("FAILED: could not remove {}: {err}", path.display());
                return ExitCode::FAILURE;
            }
            println!("removed {} ({})", path.display(), human(freed));
        }
    }
    let after = size_of(&target);
    println!(
        "xtask clean: freed {} (target/ {} -> {})",
        human(before.saturating_sub(after)),
        human(before),
        human(after)
    );
    ExitCode::SUCCESS
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::ffmpeg::workspace_root().join("target"))
}

/// Profile directories: `target/debug` and `target/<triple>/debug` (and the same for release).
fn profile_dirs(target: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in dirs_in(target) {
        match dir.file_name().and_then(|n| n.to_str()) {
            Some("debug" | "release") => found.push(dir),
            _ => {
                for sub in dirs_in(&dir) {
                    if matches!(
                        sub.file_name().and_then(|n| n.to_str()),
                        Some("debug" | "release")
                    ) {
                        found.push(sub);
                    }
                }
            }
        }
    }
    found
}

/// Build-script output dirs are named `<package>-<hash>`. Each rebuild with a new hash leaves an old copy, so this
/// keeps the newest copy of each package and returns the older ones.
fn stale_build_copies(build: &Path) -> Vec<PathBuf> {
    let mut groups: Vec<(String, Vec<(SystemTime, PathBuf)>)> = Vec::new();
    for dir in dirs_in(build) {
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let package = name
            .rsplit_once('-')
            .map_or(name, |(pkg, _)| pkg)
            .to_string();
        let modified = fs::metadata(&dir)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        match groups.iter_mut().find(|(p, _)| *p == package) {
            Some((_, copies)) => copies.push((modified, dir)),
            None => groups.push((package, vec![(modified, dir)])),
        }
    }
    let mut stale = Vec::new();
    for (_, mut copies) in groups {
        copies.sort_by_key(|copy| std::cmp::Reverse(copy.0));
        stale.extend(copies.into_iter().skip(1).map(|(_, dir)| dir));
    }
    stale
}

fn dirs_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
        .collect()
}

fn remove(path: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Total size of a file or directory tree. Symbolic links are not followed.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| size_of(&e.path()))
        .sum()
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
