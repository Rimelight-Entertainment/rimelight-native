//! What kind of file a path is, sibling pictures in a folder, and the small text formats the UI shows.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Text,
}

/// Pictures the app opens. HEIC/HEIF/HIF/AVIF and camera RAW are view-only: "Save as" writes PNG or JPEG.
pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "svg", "gif", "bmp", "ico", "heic", "heif", "hif",
    "avif", "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef",
    "srw", "3fr", "erf", "kdc", "mrw", "x3f", "iiq", "rwl",
];
/// Video the Player opens. ".ts" is not here: it is TypeScript as often as MPEG transport stream, so it is text.
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "mpg", "mpeg", "m2ts", "mts", "flv", "3gp",
    "ogv",
];
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma", "aiff", "aif", "mka",
];
pub const SUBTITLE_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa"];
/// Plain text the Text tab edits (Notepad's job): notes, Markdown, JSON and config or log files.
pub const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "json", "toml", "yaml", "yml", "ini", "cfg", "conf", "log", "csv",
    "xml", "env", "text", "rs", "js", "ts", "html", "css", "bat", "ps1", "sh",
];

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = ext_of(path);
    let ext = ext.as_str();
    if IMAGE_EXTS.contains(&ext) {
        Some(Kind::Image)
    } else if VIDEO_EXTS.contains(&ext) {
        Some(Kind::Video)
    } else if AUDIO_EXTS.contains(&ext) {
        Some(Kind::Audio)
    } else if TEXT_EXTS.contains(&ext) {
        Some(Kind::Text)
    } else {
        None
    }
}

pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Image => "image",
        Kind::Text => "text",
        Kind::Video => "video",
        Kind::Audio => "audio",
    }
}

/// The pictures in the same folder, sorted by name the way Explorer roughly does (case-insensitive, numbers in order).
pub fn sibling_pictures(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && kind_of(p) == Some(Kind::Image))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)));
    if out.is_empty() {
        out.push(path.to_path_buf());
    }
    out
}

/// The files of one kind (audio or video) in the same folder, in the same natural order as `sibling_pictures`. The open
/// file is always in the list, even when its extension is not the kind asked for.
pub fn sibling_media(path: &Path, want: Kind) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && kind_of(p) == Some(want))
                .collect()
        })
        .unwrap_or_default();
    if !out.iter().any(|p| p == path) {
        out.push(path.to_path_buf());
    }
    out.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)));
    out
}

fn natural_key(p: &Path) -> Vec<(u8, u64, String)> {
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut key = Vec::new();
    let mut digits = String::new();
    let mut text = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                key.push((1, 0, std::mem::take(&mut text)));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                key.push((
                    0,
                    digits.parse().unwrap_or(u64::MAX),
                    std::mem::take(&mut digits),
                ));
            }
            text.push(c);
        }
    }
    if !digits.is_empty() {
        key.push((0, digits.parse().unwrap_or(u64::MAX), digits));
    }
    if !text.is_empty() {
        key.push((1, 0, text));
    }
    key
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

pub fn folder_name(path: &Path) -> String {
    path.parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// 75_000 ms -> "1:15"; an hour or more -> "1:02:03".
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "Screenshot 2026-11-20 153012" style names, local time.
pub fn stamp(prefix: &str) -> String {
    let [y, mo, d, h, mi, s] = rimelight_platform::local_time();
    format!("{prefix} {y:04}-{mo:02}-{d:02} {h:02}{mi:02}{s:02}")
}

/// A free path `dir/name.ext` (adds " (2)" and so on when taken).
pub fn free_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let first = dir.join(format!("{name}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name} ({n}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Opens Explorer with the file selected (or the folder).
pub fn reveal(path: &Path) {
    let mut cmd = std::process::Command::new("explorer");
    if path.is_file() {
        // Explorer reads `/select,"path"` as one raw argument. Rust's own quoting of `/select,<path>` breaks on a path
        // with spaces, and Explorer then opens Documents instead.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        }
        #[cfg(not(windows))]
        cmd.arg(format!("/select,{}", path.display()));
    } else {
        cmd.arg(path);
    }
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_clock() {
        assert_eq!(kind_of(Path::new("a/B.JPG")), Some(Kind::Image));
        assert_eq!(kind_of(Path::new("x.mkv")), Some(Kind::Video));
        assert_eq!(kind_of(Path::new("x.flac")), Some(Kind::Audio));
        assert_eq!(kind_of(Path::new("x.txt")), Some(Kind::Text));
        assert_eq!(kind_of(Path::new("x.exe")), None);
        assert_eq!(clock(75_000), "1:15");
        assert_eq!(clock(3_723_000), "1:02:03");
    }

    #[test]
    fn natural_order() {
        let mut v = vec![
            PathBuf::from("img10.png"),
            PathBuf::from("img2.png"),
            PathBuf::from("Img1.png"),
        ];
        v.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)));
        assert_eq!(v[0], PathBuf::from("Img1.png"));
        assert_eq!(v[2], PathBuf::from("img10.png"));
    }
}
