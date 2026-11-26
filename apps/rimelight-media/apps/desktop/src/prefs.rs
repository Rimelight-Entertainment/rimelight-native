//! Settings and the recent-files list, saved as small text files in %APPDATA%\Rimelight Media (the rimelight-space pattern of
//! plain text stores). A missing or broken file means defaults.

use std::path::{Path, PathBuf};

use rimelight_platform::UserFolder;

pub const MAX_RECENTS: usize = 12;

#[derive(Debug, Clone)]
pub struct Prefs {
    /// 0 light, 1 dark, 2 follow Windows.
    pub theme: i32,
    pub background: bool,
    pub snip_copy: bool,
    pub snip_save: bool,
    pub snip_open: bool,
    pub rec_system_audio: bool,
    pub rec_mic: bool,
    pub rec_fps: i32,
    pub volume: f32,
    pub screenshot_dir: PathBuf,
    pub recording_dir: PathBuf,
    pub voice_dir: PathBuf,
    /// Where a new canvas is saved (Ctrl+S with no file yet).
    pub canvas_dir: PathBuf,
    pub desktop_shortcut: bool,
    /// File types offered to Windows: pictures, video, music, text.
    pub file_types: [bool; 4],
    /// True when Rimelight Media turned off Windows' own Print Screen snipping (uninstall turns it back on).
    pub printscreen_changed: bool,
    /// The voice recorder's microphone gain in dB (-12 to +24).
    pub voice_gain_db: f32,
    /// The font of plain (non-code) text in the Text tab: 0 Mono (Consolas), 1 Sans (Segoe UI), 2 Serif (Georgia).
    pub text_font: i32,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn user_folder(which: UserFolder, home: &Path, fallback: &str) -> PathBuf {
    rimelight_platform::known_folder(which).unwrap_or_else(|| home.join(fallback))
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(home)
        .join("Rimelight Media")
}

impl Default for Prefs {
    fn default() -> Self {
        let home = home();
        Prefs {
            theme: 2,
            background: false,
            snip_copy: true,
            snip_save: true,
            snip_open: true,
            rec_system_audio: true,
            rec_mic: false,
            rec_fps: 30,
            volume: 1.0,
            // Each kind in its own user folder (where Windows keeps it, OneDrive included).
            screenshot_dir: user_folder(UserFolder::Pictures, &home, "Pictures")
                .join("Screenshots"),
            recording_dir: user_folder(UserFolder::Videos, &home, "Videos").join("Captures"),
            voice_dir: user_folder(UserFolder::Music, &home, "Music").join("Recordings"),
            canvas_dir: user_folder(UserFolder::Pictures, &home, "Pictures").join("Canvas"),
            desktop_shortcut: true,
            file_types: [true; 4],
            printscreen_changed: false,
            voice_gain_db: 0.0,
            text_font: 0,
        }
    }
}

fn flag(v: &str) -> bool {
    v == "1" || v.eq_ignore_ascii_case("true")
}

impl Prefs {
    pub fn load() -> Prefs {
        let mut p = Prefs::default();
        let Ok(text) = std::fs::read_to_string(data_dir().join("settings.txt")) else {
            return p;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "theme" => p.theme = v.parse().unwrap_or(2).clamp(0, 2),
                "background" => p.background = flag(v),
                "snip_copy" => p.snip_copy = flag(v),
                "snip_save" => p.snip_save = flag(v),
                "snip_open" => p.snip_open = flag(v),
                "rec_system_audio" => p.rec_system_audio = flag(v),
                "rec_mic" => p.rec_mic = flag(v),
                "rec_fps" => p.rec_fps = if v == "60" { 60 } else { 30 },
                "volume" => p.volume = v.parse().unwrap_or(1.0f32).clamp(0.0, 1.0),
                "voice_gain_db" => p.voice_gain_db = v.parse().unwrap_or(0.0f32).clamp(-12.0, 24.0),
                "text_font" => p.text_font = v.parse().unwrap_or(0).clamp(0, 2),
                "screenshot_dir" if !v.is_empty() => p.screenshot_dir = PathBuf::from(v),
                "recording_dir" if !v.is_empty() => p.recording_dir = PathBuf::from(v),
                // 0.1.x saved Documents\Sound Recordings as the default: voice notes now go to Music\Recordings.
                "voice_dir" if !v.is_empty() && !v.ends_with(r"Documents\Sound Recordings") => {
                    p.voice_dir = PathBuf::from(v)
                }
                "canvas_dir" if !v.is_empty() => p.canvas_dir = PathBuf::from(v),
                "desktop_shortcut" => p.desktop_shortcut = flag(v),
                "printscreen_changed" => p.printscreen_changed = flag(v),
                "file_types" => {
                    let mut it = v.chars().map(|c| c == '1');
                    p.file_types = [0; 4].map(|_| it.next().unwrap_or(true));
                }
                _ => {}
            }
        }
        p
    }

    pub fn save(&self) {
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "theme={}\nbackground={}\nsnip_copy={}\nsnip_save={}\nsnip_open={}\nrec_system_audio={}\nrec_mic={}\nrec_fps={}\nvolume={}\nscreenshot_dir={}\nrecording_dir={}\nvoice_dir={}\ncanvas_dir={}\ndesktop_shortcut={}\nprintscreen_changed={}\nfile_types={}{}{}{}\nvoice_gain_db={}\ntext_font={}\n",
            self.theme,
            b(self.background),
            b(self.snip_copy),
            b(self.snip_save),
            b(self.snip_open),
            b(self.rec_system_audio),
            b(self.rec_mic),
            self.rec_fps,
            self.volume,
            self.screenshot_dir.display(),
            self.recording_dir.display(),
            self.voice_dir.display(),
            self.canvas_dir.display(),
            b(self.desktop_shortcut),
            b(self.printscreen_changed),
            b(self.file_types[0]),
            b(self.file_types[1]),
            b(self.file_types[2]),
            b(self.file_types[3]),
            self.voice_gain_db,
            self.text_font,
        );
        write(&data_dir().join("settings.txt"), &text);
    }
}

fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn load_recents() -> Vec<PathBuf> {
    std::fs::read_to_string(data_dir().join("recents.txt"))
        .map(|t| {
            t.lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_recents(list: &[PathBuf]) {
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    write(&data_dir().join("recents.txt"), &text);
}

/// The folders pinned to the Files tab's sidebar, one per line (pinned.txt).
pub fn load_pinned() -> Vec<PathBuf> {
    std::fs::read_to_string(data_dir().join("pinned.txt"))
        .map(|t| {
            t.lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_pinned(list: &[PathBuf]) {
    let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
    write(&data_dir().join("pinned.txt"), &text);
}

/// Puts `path` first, without duplicates, keeping at most MAX_RECENTS.
pub fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENTS);
}
