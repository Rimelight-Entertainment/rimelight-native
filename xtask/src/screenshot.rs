//! `cargo xtask screenshot`: capture PNGs to check a UI change (PLAN.md section 10.2, decision 0006).
//!
//! - `windows`: builds the example or binary (`--screenshot` flag, `Window::render_to_image`), runs it, and
//!   validates the PNG. `--bin` with `--story` targets the gallery (`space_gallery`, P0.15).
//! - `shell/<state>`: builds `space_gallery` and captures Space's whole shell in that state (`--story shell/<state>
//!   --screenshot`), then validates the PNG. The owner-approved command for a whole-app capture.
//! - `gallery-all`: asks the gallery for its ids (stories and variants) and captures each in light and dark, chrome on by default.
//! - `android`: uses `adb` against a device or emulator that is already connected (never started here), sets night
//!   mode for `--theme`, installs and launches the app, runs `adb exec-out screencap -p`, restores night mode, and
//!   validates the PNG.
//!
//! std only. PNG validation checks the signature, the IHDR and IEND chunks, and a minimum size. Blank-image detection
//! is done inside the Windows app (fewer than 8 distinct colours in a sample fails with exit code 4). Android
//! screenshots get the size and structure checks only, because decoding zlib would need a dependency.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// A flat or near-empty PNG of a window is far smaller than this. Real UI captures are well above it.
const MIN_PNG_BYTES: u64 = 4096;
const WINDOWS_RUN_TIMEOUT: Duration = Duration::from_secs(60);
const ANDROID_SETTLE: Duration = Duration::from_secs(8);

const USAGE: &str = "\
usage:
  cargo xtask screenshot windows --package <crate> --example <name> [--features <list>] --theme <light|dark> --size <WxH> --out <png>
  cargo xtask screenshot windows --package <crate> --bin <name> [--features <list>] [--story <id>] [--viewport <v>] [--chrome <on|off>] [--locale <l>] [--text-scale <n>] --theme <light|dark> --size <WxH> --out <png>
  cargo xtask screenshot shell/<state> --out <png> [--theme <light|dark>] [--size <WxH>]
  cargo xtask screenshot gallery-all --out-dir <dir> [--size <WxH>] [--chrome <on|off>] [--package <crate>] [--bin <name>] [--features <list>]
  cargo xtask screenshot android --apk <path> --package <id> --activity <name> --theme <light|dark> --out <png>

  --example   a workspace example with a `--screenshot` flag, built with `cargo build -p <crate> --example <name>`
              (run from the repo root; the binary is read from ./target/debug/examples)
  --bin       a workspace binary with a `--screenshot` flag (the gallery is `--package space_gallery --bin space_gallery`)
  shell/<state> the whole Space shell in a state (`space_gallery --story shell/<state> --screenshot`); theme defaults to
              light, size to 1280x800
  --story     story id passed to the binary as `--story <id>` (gallery and examples)
  gallery-all captures every id from `<bin> --list-stories` (story and variant) in light and dark, with the window chrome on unless `--chrome off`, writing
              `<out-dir>/<section>-<name>.<theme>.headless.png`, the visual baseline set (default size 1280x800)
  --activity  class name, or `.Name` relative to the package";

/// Entry point for `cargo xtask screenshot <args...>`.
pub fn run(args: &[&str]) -> ExitCode {
    match args {
        ["windows", rest @ ..] => {
            match parse_opts(
                rest,
                &[
                    "package",
                    "example",
                    "bin",
                    "features",
                    "theme",
                    "size",
                    "out",
                    "locale",
                    "text-scale",
                    "story",
                    "viewport",
                    "chrome",
                    "reduced-motion",
                ],
                &["theme", "size", "out"],
            ) {
                Ok(opts) => windows(&opts),
                Err(err) => usage_error(&err),
            }
        }
        [state, rest @ ..] if state.starts_with("shell/") => {
            match parse_opts(rest, &["out", "theme", "size"], &["out"]) {
                Ok(opts) => shell(state, &opts),
                Err(err) => usage_error(&err),
            }
        }
        ["gallery-all", rest @ ..] => {
            match parse_opts(
                rest,
                &["out-dir", "size", "package", "bin", "features", "chrome"],
                &["out-dir"],
            ) {
                Ok(opts) => gallery_all(&opts),
                Err(err) => usage_error(&err),
            }
        }
        ["android", rest @ ..] => {
            match parse_opts(
                rest,
                &["apk", "package", "activity", "theme", "out"],
                &["apk", "package", "activity", "theme", "out"],
            ) {
                Ok(opts) => android(&opts),
                Err(err) => usage_error(&err),
            }
        }
        _ => usage_error(
            "expected `windows`, `shell/<state>`, `gallery-all` or `android` after `screenshot`",
        ),
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("xtask screenshot: {message}\n{USAGE}");
    ExitCode::from(2)
}

/// Parses `--key value` pairs. Rejects unknown keys, missing values, repeats and missing required keys.
fn parse_opts(
    raw: &[&str],
    allowed: &[&str],
    required: &[&str],
) -> Result<HashMap<String, String>, String> {
    let mut opts = HashMap::new();
    let mut i = 0;
    while i < raw.len() {
        let key = raw[i];
        let Some(name) = key.strip_prefix("--") else {
            return Err(format!("expected a --flag, got {key:?}"));
        };
        if !allowed.contains(&name) {
            return Err(format!("unknown option --{name}"));
        }
        let value = raw
            .get(i + 1)
            .ok_or_else(|| format!("--{name} needs a value"))?;
        if opts.insert(name.to_string(), value.to_string()).is_some() {
            return Err(format!("--{name} given twice"));
        }
        i += 2;
    }
    for name in required {
        if !opts.contains_key(*name) {
            return Err(format!("missing required option --{name}"));
        }
    }
    Ok(opts)
}

fn check_theme(theme: &str) -> Result<(), String> {
    match theme {
        "light" | "dark" => Ok(()),
        other => Err(format!("--theme must be light or dark, got {other:?}")),
    }
}

fn check_size(size: &str) -> Result<(u32, u32), String> {
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("--size must be WxH, got {size:?}"))?;
    let w: u32 = w
        .parse()
        .map_err(|_| format!("bad width in --size {size:?}"))?;
    let h: u32 = h
        .parse()
        .map_err(|_| format!("bad height in --size {size:?}"))?;
    if w < 100 || h < 100 {
        return Err(format!("--size {size:?} is too small (minimum 100x100)"));
    }
    Ok((w, h))
}

/// Makes the output path absolute and creates its folder. Removes any old file so a stale PNG cannot pass.
fn prepare_out(out: &str) -> Result<PathBuf, String> {
    let path = std::env::current_dir()
        .map_err(|e| format!("cannot read current dir: {e}"))?
        .join(out);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("cannot remove old {}: {e}", path.display()))?;
    }
    Ok(path)
}

fn windows(opts: &HashMap<String, String>) -> ExitCode {
    let result = (|| -> Result<String, String> {
        let theme = &opts["theme"];
        check_theme(theme)?;
        let (w, h) = check_size(&opts["size"])?;
        let out = prepare_out(&opts["out"])?;
        let exe = match (opts.get("example"), opts.get("bin")) {
            (Some(example), None) => build_example(opts, example)?,
            (None, Some(bin)) => build_bin(opts, bin)?,
            _ => {
                return Err(
                    "give exactly one of --package with --example, or --package with --bin"
                        .to_string(),
                );
            }
        };
        let mut extra = vec![
            "--theme".to_string(),
            theme.clone(),
            "--size".to_string(),
            format!("{w}x{h}"),
        ];
        extra.extend(extra_args(opts));
        run_capture(&exe, &out, &extra)?;
        validate_png(&out)
    })();
    finish("windows", result)
}

/// `cargo xtask screenshot shell/<state> --out <png> [--theme light|dark] [--size WxH]`: builds `space_gallery` and
/// captures Space's whole shell in `state` (`--story shell/<state> --screenshot <png>`). The shell refuses a blank frame
/// itself. The PNG is validated like the other captures.
fn shell(story: &str, opts: &HashMap<String, String>) -> ExitCode {
    let result = (|| -> Result<String, String> {
        let theme = opts.get("theme").map(String::as_str).unwrap_or("light");
        check_theme(theme)?;
        let (w, h) = check_size(opts.get("size").map(String::as_str).unwrap_or("1280x800"))?;
        let out = prepare_out(&opts["out"])?;
        let exe = build_bin(opts, "space_gallery")?;
        let extra = [
            "--story".to_string(),
            story.to_string(),
            "--theme".to_string(),
            theme.to_string(),
            "--size".to_string(),
            format!("{w}x{h}"),
        ];
        run_capture(&exe, &out, &extra)?;
        validate_png(&out)
    })();
    finish(story, result)
}

/// Runs `exe --screenshot <out> <extra...>` and waits for it, with a timeout. Fails when the app exits non-zero.
fn run_capture(exe: &Path, out: &Path, extra: &[String]) -> Result<(), String> {
    let mut child = Command::new(exe)
        .arg("--screenshot")
        .arg(out)
        .args(extra)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("wait failed: {e}"))? {
            break status;
        }
        if started.elapsed() > WINDOWS_RUN_TIMEOUT {
            let _ = child.kill();
            return Err(format!(
                "app did not exit within {}s and was killed",
                WINDOWS_RUN_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        return Err(format!("app exited with {status} (see its message above)"));
    }
    Ok(())
}

/// `cargo xtask screenshot gallery-all --out-dir <dir>`: builds the gallery, asks it
/// for its story ids, and captures every id in light and dark (chrome on unless `--chrome off`). Each PNG is validated.
fn gallery_all(opts: &HashMap<String, String>) -> ExitCode {
    let result = (|| -> Result<String, String> {
        let (w, h) = check_size(opts.get("size").map(String::as_str).unwrap_or("1280x800"))?;
        let out_dir = std::env::current_dir()
            .map_err(|e| format!("cannot read current dir: {e}"))?
            .join(&opts["out-dir"]);
        fs::create_dir_all(&out_dir)
            .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
        let package = opts
            .get("package")
            .map(String::as_str)
            .unwrap_or("space_gallery");
        let bin = opts
            .get("bin")
            .map(String::as_str)
            .unwrap_or("space_gallery");
        let chrome = opts
            .get("chrome")
            .cloned()
            .unwrap_or_else(|| "on".to_string());
        if chrome != "on" && chrome != "off" {
            return Err(format!("--chrome must be on or off, got {chrome:?}"));
        }
        let exe = build_bin(opts, bin)?;

        let listing = Command::new(&exe)
            .arg("--list-stories")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run {} --list-stories: {e}", exe.display()))?;
        if !listing.status.success() {
            return Err(format!("--list-stories failed ({})", listing.status));
        }
        let ids: Vec<String> = String::from_utf8_lossy(&listing.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        if ids.is_empty() {
            return Err(format!("{package} lists no stories"));
        }

        let mut captured = Vec::new();
        for id in &ids {
            for theme in ["light", "dark"] {
                let file = format!("{}.{theme}.headless.png", id.replace(['/', '#'], "-"));
                let out = out_dir.join(&file);
                if out.exists() {
                    fs::remove_file(&out)
                        .map_err(|e| format!("cannot remove old {}: {e}", out.display()))?;
                }
                let extra = [
                    "--story".to_string(),
                    id.clone(),
                    "--theme".to_string(),
                    theme.to_string(),
                    "--size".to_string(),
                    format!("{w}x{h}"),
                    "--chrome".to_string(),
                    chrome.clone(),
                ];
                run_capture(&exe, &out, &extra).map_err(|e| format!("{id} {theme}: {e}"))?;
                validate_png(&out).map_err(|e| format!("{id} {theme}: {e}"))?;
                captured.push(file);
            }
        }
        Ok(format!(
            "{} stories x 2 themes = {} PNGs in {}",
            ids.len(),
            captured.len(),
            out_dir.display()
        ))
    })();
    finish("gallery-all", result)
}

/// Builds one binary of a workspace package (run from the repo root) and returns it. Used for `--bin` and the gallery.
/// The target folder that screenshot builds use. It is separate from `target/debug`, so a capture build never replaces
/// a binary the owner may be running.
pub const SCREENSHOT_TARGET_DIR: &str = "target/screenshot";

fn build_bin(opts: &HashMap<String, String>, bin: &str) -> Result<PathBuf, String> {
    let package = opts
        .get("package")
        .map(String::as_str)
        .unwrap_or("space_gallery");
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "-p",
        package,
        "--bin",
        bin,
        "--target-dir",
        SCREENSHOT_TARGET_DIR,
    ]);
    if let Some(features) = opts.get("features") {
        cmd.args(["--features", features]);
    }
    let build = cmd
        .status()
        .map_err(|e| format!("cannot run cargo build: {e}"))?;
    if !build.success() {
        return Err(format!("cargo build of --bin {bin} failed ({build})"));
    }
    let exe = Path::new(SCREENSHOT_TARGET_DIR)
        .join("debug")
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    if !exe.is_file() {
        return Err(format!(
            "built binary not found at {} (run from the repo root)",
            exe.display()
        ));
    }
    Ok(exe)
}

/// Builds one example of a workspace package (run from the repo root) and returns its binary.
fn build_example(opts: &HashMap<String, String>, example: &str) -> Result<PathBuf, String> {
    let package = opts
        .get("package")
        .ok_or_else(|| "--example needs --package".to_string())?;
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", package, "--example", example]);
    if let Some(features) = opts.get("features") {
        cmd.args(["--features", features]);
    }
    let build = cmd
        .status()
        .map_err(|e| format!("cannot run cargo build: {e}"))?;
    if !build.success() {
        return Err(format!("cargo build of example {example} failed ({build})"));
    }
    let exe = Path::new("target")
        .join("debug")
        .join("examples")
        .join(format!("{example}{}", std::env::consts::EXE_SUFFIX));
    if !exe.is_file() {
        return Err(format!(
            "built example not found at {} (run from the repo root)",
            exe.display()
        ));
    }
    Ok(exe)
}

fn android(opts: &HashMap<String, String>) -> ExitCode {
    let result = (|| -> Result<String, String> {
        let theme = &opts["theme"];
        check_theme(theme)?;
        let apk = PathBuf::from(&opts["apk"]);
        if !apk.is_file() {
            return Err(format!("--apk not found: {}", apk.display()));
        }
        let package = &opts["package"];
        let activity = &opts["activity"];
        // `am start -n` takes `package/class`; a leading dot in --activity means "relative to the package".
        let class = if activity.starts_with('.') {
            format!("{package}{activity}")
        } else {
            activity.clone()
        };
        let component = format!("{package}/{class}");
        let out = prepare_out(&opts["out"])?;

        let adb = Adb::find()?;
        let serial = adb.pick_device()?;
        let night_yes = theme == "dark";
        let previous = adb.night_mode(&serial)?;
        let capture = (|| -> Result<(), String> {
            adb.run(
                &serial,
                &[
                    "shell",
                    "cmd",
                    "uimode",
                    "night",
                    if night_yes { "yes" } else { "no" },
                ],
            )?;
            adb.run(&serial, &["install", "-r", &apk.display().to_string()])
                .and_then(|out| {
                    if out.contains("Success") {
                        Ok(())
                    } else {
                        Err(format!("adb install did not report Success: {out}"))
                    }
                })?;
            adb.run(&serial, &["shell", "am", "force-stop", package])?;
            let start = adb.run(&serial, &["shell", "am", "start", "-W", "-n", &component])?;
            if !start.contains("Status: ok") {
                return Err(format!("am start did not report Status: ok: {start}"));
            }
            thread::sleep(ANDROID_SETTLE);
            let png = adb.capture_png(&serial)?;
            fs::write(&out, &png).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            Ok(())
        })();
        // Always restore night mode and stop the app, even when the capture failed.
        let restore = adb.run(&serial, &["shell", "cmd", "uimode", "night", &previous]);
        let _ = adb.run(&serial, &["shell", "am", "force-stop", package]);
        capture?;
        restore.map_err(|e| format!("night mode was not restored to {previous}: {e}"))?;
        validate_png(&out)
    })();
    finish("android", result)
}

struct Adb {
    path: PathBuf,
}

impl Adb {
    /// Looks for adb in ANDROID_HOME, ANDROID_SDK_ROOT, the default Windows SDK folder, then PATH.
    fn find() -> Result<Self, String> {
        let mut candidates = Vec::new();
        for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            if let Ok(root) = std::env::var(var) {
                candidates.push(PathBuf::from(root).join("platform-tools").join(adb_name()));
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local)
                    .join("Android/Sdk/platform-tools")
                    .join(adb_name()),
            );
        }
        for candidate in candidates {
            if candidate.is_file() {
                return Ok(Self { path: candidate });
            }
        }
        // Fall back to PATH; the first command will fail clearly if it is missing.
        Ok(Self {
            path: PathBuf::from(adb_name()),
        })
    }

    /// Chooses the device: ANDROID_SERIAL if set, otherwise the only attached device. Never starts an emulator.
    fn pick_device(&self) -> Result<String, String> {
        let listing = self.output(&["devices"])?;
        let devices: Vec<&str> = listing
            .lines()
            .skip(1)
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(serial), Some("device")) => Some(serial),
                    _ => None,
                }
            })
            .collect();
        if let Ok(serial) = std::env::var("ANDROID_SERIAL") {
            if devices.contains(&serial.as_str()) {
                return Ok(serial);
            }
            return Err(format!("ANDROID_SERIAL={serial} is not an attached device"));
        }
        match devices.as_slice() {
            [] => Err(
                "no Android device or emulator is connected (adb devices shows none). \
                 Start one first, for example `emulator -avd Medium_Phone_API_37.0`, then retry."
                    .to_string(),
            ),
            [one] => Ok(one.to_string()),
            many => Err(format!(
                "{} devices connected ({}); set ANDROID_SERIAL to choose one",
                many.len(),
                many.join(", ")
            )),
        }
    }

    /// Reads the current night mode (`yes`, `no` or `auto`) so it can be restored.
    fn night_mode(&self, serial: &str) -> Result<String, String> {
        let text = self.run(serial, &["shell", "cmd", "uimode", "night"])?;
        for word in ["yes", "no", "auto"] {
            if text.trim_end().ends_with(word) {
                return Ok(word.to_string());
            }
        }
        Err(format!(
            "cannot read night mode from `cmd uimode night`: {text:?}"
        ))
    }

    fn output(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(&self.path)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run {} {}: {e}", self.path.display(), args.join(" ")))?;
        if !out.status.success() {
            return Err(format!(
                "adb {} failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Runs `adb -s <serial> <args>` and returns stdout.
    fn run(&self, serial: &str, args: &[&str]) -> Result<String, String> {
        let mut full = vec!["-s", serial];
        full.extend_from_slice(args);
        self.output(&full)
    }

    /// `adb exec-out screencap -p`, checked for the PNG signature before it is returned.
    fn capture_png(&self, serial: &str) -> Result<Vec<u8>, String> {
        let out = Command::new(&self.path)
            .args(["-s", serial, "exec-out", "screencap", "-p"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("cannot run screencap: {e}"))?;
        if !out.status.success() {
            return Err(format!("screencap failed ({})", out.status));
        }
        if out.stdout.len() < 8 || out.stdout[..8] != PNG_SIGNATURE {
            return Err("screencap did not return a PNG".to_string());
        }
        Ok(out.stdout)
    }
}

fn adb_name() -> &'static str {
    if cfg!(windows) { "adb.exe" } else { "adb" }
}

/// Checks a PNG file: signature, IHDR first, IEND last, minimum size. Returns a one-line summary.
pub fn validate_png(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if (bytes.len() as u64) < MIN_PNG_BYTES {
        return Err(format!(
            "{} is {} bytes, below the {MIN_PNG_BYTES}-byte minimum (blank or truncated?)",
            path.display(),
            bytes.len()
        ));
    }
    if bytes[..8] != PNG_SIGNATURE {
        return Err(format!(
            "{} does not start with the PNG signature",
            path.display()
        ));
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(format!("{} has no IHDR chunk first", path.display()));
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 {
        return Err(format!("{} has zero size", path.display()));
    }
    if &bytes[bytes.len() - 8..bytes.len() - 4] != b"IEND" {
        return Err(format!(
            "{} does not end with an IEND chunk (truncated?)",
            path.display()
        ));
    }
    Ok(format!(
        "{} ({width}x{height}, {} bytes)",
        path.display(),
        bytes.len()
    ))
}

fn finish(platform: &str, result: Result<String, String>) -> ExitCode {
    match result {
        Ok(summary) => {
            println!("xtask screenshot {platform}: ok, wrote {summary}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("FAILED: xtask screenshot {platform}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Passes the optional story flags (`--story`, `--locale`, `--text-scale`, `--viewport`, `--chrome`) through to the
/// example or gallery binary.
fn extra_args(opts: &HashMap<String, String>) -> Vec<String> {
    [
        "story",
        "locale",
        "text-scale",
        "viewport",
        "chrome",
        "reduced-motion",
    ]
    .iter()
    .filter_map(|key| opts.get(*key).map(|v| vec![format!("--{key}"), v.clone()]))
    .flatten()
    .collect()
}
