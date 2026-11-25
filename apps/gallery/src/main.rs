//! Space's component gallery: the shared `rimelight_gallery` (rimelight-rs `apps/gallery`) with Space's own stories as its
//! app stories. The stories folder is `crates/space_ui/ui/stories` (`stories.txt` and `stories.slint`), used as the
//! default `--stories` folder, so `--stories` is not needed.
//!
//! Every flag of the shared gallery works (`--story`, `--theme`, `--locale`, `--viewport`, `--text-scale`, `--size`,
//! `--screenshot`, `--list-stories`, `--foundations`, ...). See rimelight-rs `apps/gallery/README.md`.
//!
//! Space adds one capture: the whole app. `--story shell/<state> --screenshot <png>` builds Space's real shell in the
//! state `space_ui::slint_ui::shell_capture` names, and writes the frame. `--theme light|dark|system` (default system)
//! and `--size WxH` (default 1280x800) apply to it. This path is handled here, not by the shared gallery.
//!
//! Exit codes: 0 ok, 2 bad arguments, 3 the window or the capture failed, 4 the captured image is blank (shared gallery
//! only; the shell capture refuses a blank frame with exit 3).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use space_ui::slint_ui::shell_capture;
use space_ui::theme::ThemeMode;

/// The app's story folder: `ui/stories` in space_ui, next to the markup it is compiled with.
const SPACE_STORIES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/space_ui/ui/stories"
);

/// The usage of the whole-app capture. The shared gallery prints its own usage for its flags.
const USAGE: &str = "usage: space_gallery --story shell/<state> --screenshot <png> [--theme light|dark|system] [--size WxH]\n\
       space_gallery [the shared gallery flags, see rimelight-rs apps/gallery/README.md]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match shell_state(&args) {
        Some(state) => capture_shell(&args, state),
        None => rimelight_gallery::run(&args, Some(Path::new(SPACE_STORIES))),
    }
}

/// The state of `--story shell/<state>`, when the command line asks for a whole-shell capture.
fn shell_state(args: &[String]) -> Option<&str> {
    args.windows(2)
        .find(|pair| pair[0] == "--story")
        .and_then(|pair| pair[1].strip_prefix("shell/"))
}

/// Captures the shell in `state` to `--screenshot`, with `--theme` and `--size`. Any other flag is refused.
fn capture_shell(args: &[String], state: &str) -> ExitCode {
    let mut theme = ThemeMode::System;
    let mut size = (1280, 800);
    let mut out: Option<PathBuf> = None;
    let mut iter = args.iter().skip(1);
    while let Some(flag) = iter.next() {
        let Some(value) = iter.next() else {
            return usage_error(&format!("{flag} needs a value"));
        };
        match flag.as_str() {
            "--story" => {}
            "--theme" => {
                theme = match value.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    "system" => ThemeMode::System,
                    other => return usage_error(&format!("unknown theme {other:?}")),
                };
            }
            "--size" => match parse_size(value) {
                Ok(parsed) => size = parsed,
                Err(message) => return usage_error(&message),
            },
            "--screenshot" => out = Some(PathBuf::from(value)),
            other => return usage_error(&format!("unknown flag {other} for a shell capture")),
        }
    }
    let Some(out) = out else {
        return usage_error("a shell capture needs --screenshot <png>");
    };

    if let Err(message) = rimelight_gallery::harness::install_headless() {
        eprintln!("space_gallery: {message}");
        return ExitCode::from(3);
    }
    match shell_capture::capture(state, theme, size, &out) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("space_gallery: {message}");
            ExitCode::from(3)
        }
    }
}

/// Parses `WxH` in logical pixels, at least 100 on each side.
fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let (w, h) = value
        .split_once('x')
        .ok_or_else(|| "size must be WxH".to_string())?;
    let w: u32 = w.parse().map_err(|_| "bad width".to_string())?;
    let h: u32 = h.parse().map_err(|_| "bad height".to_string())?;
    if w < 100 || h < 100 {
        return Err("size must be at least 100x100".to_string());
    }
    Ok((w, h))
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("space_gallery: {message}\n{USAGE}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Space's stories compile with the shared gallery's markup and libraries, and the story list parses with the
    /// list empty (the issues board is the shared BoardPage).
    #[test]
    fn space_stories_compile() {
        rimelight_gallery::harness::install_headless().expect("install the testing backend");
        rimelight_gallery::harness::compile(Some(Path::new(SPACE_STORIES)))
            .expect("Space's stories compile");
        let stories = rimelight_gallery::registry::load_app_stories(Path::new(SPACE_STORIES))
            .expect("stories.txt parses");
        assert!(stories.is_empty(), "stories.txt lists no Space-only story");
    }

    #[test]
    fn shell_state_is_read_from_the_story_flag() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shell_state(&args(&["g", "--story", "shell/desktop-default"])),
            Some("desktop-default")
        );
        assert_eq!(
            shell_state(&args(&["g", "--story", "components/board-page"])),
            None
        );
        assert_eq!(shell_state(&args(&["g", "--list-stories"])), None);
    }
}
