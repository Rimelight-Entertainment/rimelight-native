//! Windows integration around the Velopack install (its hooks run in main.rs): the shortcuts (Start menu > Rimelight, Desktop),
//! file types and default-app capabilities, the start-with-Windows entry and Windows' own Print Screen setting. All per
//! user, no admin. Velopack itself adds the Installed apps entry and removes the program files on uninstall.

use std::path::{Path, PathBuf};

use crate::files::{AUDIO_EXTS, IMAGE_EXTS, TEXT_EXTS, VIDEO_EXTS};

pub const APP_NAME: &str = "Rimelight Media";
const EXE_NAME: &str = "rimelight-media.exe";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const CAPS_KEY: &str = r"Software\RimelightMedia\Capabilities";
/// The per-category ProgIDs earlier versions registered. Kept on install (a person's UserChoice may still name them, and
/// they still open the app); removed on uninstall only.
const OLD_PROGIDS: [&str; 4] = [
    "RimelightMedia.Image",
    "RimelightMedia.Video",
    "RimelightMedia.Audio",
    "RimelightMedia.Text",
];
/// Script types: "Open with" only. Never offered in Default apps, never claimed as the default, and not part of the
/// not-default check, so double-click keeps running them.
const SCRIPT_EXTS: [&str; 3] = ["bat", "ps1", "sh"];
/// The categories in the order of the home page: the icon's name (rimelight-media-<name>.ico) and the extensions.
static CATEGORIES: [(&str, &[&str]); 4] = [
    ("picture", IMAGE_EXTS),
    ("video", VIDEO_EXTS),
    ("music", AUDIO_EXTS),
    ("text", TEXT_EXTS),
];
/// The glyph of each category (the same icons as the home page's file-type tiles, ui/home.slint), its SVG and the two
/// colours of the tile (the shared extension-file-kind-* tokens in rimelight_design: picture, video, music, text).
const KIND_ICONS: [(&str, &str, &str, &str); 4] = [
    (
        "picture",
        include_str!("../assets/icons/image.svg"),
        "#7DD3FC",
        "#0284C7",
    ),
    (
        "video",
        include_str!("../assets/icons/video.svg"),
        "#C4B5FD",
        "#7C3AED",
    ),
    (
        "music",
        include_str!("../assets/icons/music.svg"),
        "#FDA4AF",
        "#E11D48",
    ),
    (
        "text",
        include_str!("../assets/icons/type.svg"),
        "#86EFAC",
        "#16A34A",
    ),
];

/// Every extension the app opens, in category order.
pub fn all_exts() -> impl Iterator<Item = &'static str> {
    CATEGORIES.iter().flat_map(|(_, exts)| exts.iter().copied())
}

/// One ProgID per extension, so Explorer's Type column names the format.
fn progid(ext: &str) -> String {
    format!("RimelightMedia.{ext}")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_script(ext: &str) -> bool {
    SCRIPT_EXTS.contains(&ext)
}

/// What Explorer's Type column shows for an extension, one scheme for every format: "<FORMAT> image", "<FORMAT> video",
/// "<FORMAT> audio" or "<FORMAT> text" ("JPEG image", "MP4 video", "Ogg audio", "JSON text"). The category comes from
/// the extension lists in files.rs; the format name is the real one from this table. Used for each ProgID's
/// FriendlyTypeName and description, so Explorer and the app agree.
pub fn type_name(ext: &str) -> String {
    let word = if IMAGE_EXTS.contains(&ext) {
        "image"
    } else if VIDEO_EXTS.contains(&ext) {
        "video"
    } else if AUDIO_EXTS.contains(&ext) {
        "audio"
    } else {
        "text"
    };
    let format = match ext {
        "png" => "PNG",
        "jpg" | "jpeg" | "jfif" => "JPEG",
        "webp" => "WebP",
        "svg" => "SVG",
        "gif" => "GIF",
        "bmp" => "BMP",
        "ico" => "ICO",
        "avif" => "AVIF",
        "heic" => "HEIC",
        "heif" | "hif" => "HEIF",
        "dng" => "DNG",
        "cr2" | "cr3" => "Canon RAW",
        "nef" | "nrw" => "Nikon RAW",
        "arw" | "srf" | "sr2" => "Sony RAW",
        "raf" => "Fujifilm RAW",
        "orf" => "Olympus RAW",
        "rw2" => "Panasonic RAW",
        "rwl" => "Leica RAW",
        "pef" => "Pentax RAW",
        "srw" => "Samsung RAW",
        "3fr" => "Hasselblad RAW",
        "erf" => "Epson RAW",
        "kdc" => "Kodak RAW",
        "mrw" => "Minolta RAW",
        "x3f" => "Sigma RAW",
        "iiq" => "Phase One RAW",
        "mp4" => "MP4",
        "m4v" => "M4V",
        "mkv" => "Matroska",
        "webm" => "WebM",
        "mov" => "QuickTime",
        "avi" => "AVI",
        "wmv" => "WMV",
        "mpg" | "mpeg" => "MPEG",
        "m2ts" | "mts" => "AVCHD",
        "flv" => "FLV",
        "3gp" => "3GP",
        "ogv" | "oga" | "ogg" => "Ogg",
        "mp3" => "MP3",
        "flac" => "FLAC",
        "wav" => "WAV",
        "opus" => "Opus",
        "m4a" => "M4A",
        "aac" => "AAC",
        "wma" => "WMA",
        "aif" | "aiff" => "AIFF",
        "mka" => "Matroska",
        "txt" | "text" => "Plain",
        "md" | "markdown" => "Markdown",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "ini" => "INI",
        "cfg" | "conf" => "Config",
        "log" => "Log",
        "csv" => "CSV",
        "xml" => "XML",
        "env" => "ENV",
        "rs" => "Rust",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "html" => "HTML",
        "css" => "CSS",
        "bat" => "Batch",
        "ps1" => "PowerShell",
        "sh" => "Shell",
        _ => return format!("{} {word}", ext.to_ascii_uppercase()),
    };
    format!("{format} {word}")
}

/// The SVG drawn at size x size, as PNG bytes.
fn render_png(svg: &str, size: u32) -> Option<Vec<u8>> {
    let tree =
        resvg::usvg::Tree::from_data(svg.as_bytes(), &resvg::usvg::Options::default()).ok()?;
    let scale = size as f32 / tree.size().width();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let img = image::RgbaImage::from_raw(size, size, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .ok()?;
    Some(buf.into_inner())
}

/// A coloured tile with the category's white glyph, as an SVG (the glyph's own <svg> wrapper is dropped).
fn kind_icon_svg(glyph: &str, top: &str, bottom: &str) -> String {
    let open_end = glyph
        .find("<svg")
        .and_then(|i| glyph[i..].find('>').map(|j| i + j + 1))
        .unwrap_or(0);
    let close = glyph.rfind("</svg>").unwrap_or(glyph.len());
    let body = &glyph[open_end..close];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient></defs><rect x="8" y="8" width="240" height="240" rx="56" fill="url(#g)"/><g transform="translate(53 53) scale(6.25)" fill="none" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</g></svg>"##
    )
}

/// A multi-size .ico (PNG frames, Windows Vista and later). `frames` are (size, PNG bytes).
fn write_ico(path: &Path, frames: &[(u32, Vec<u8>)]) -> bool {
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * frames.len() as u32;
    for (size, png) in frames {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in frames {
        out.extend_from_slice(png);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, out).is_ok()
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from(EXE_NAME))
}

fn icon_path() -> PathBuf {
    crate::prefs::data_dir().join("rimelight-media.ico")
}

/// Writes the app icon (the logo) as an .ico file, for file types and shortcuts.
pub fn write_icon(path: &Path) -> bool {
    crate::picture::logo(256).is_some_and(|pic| write_picture_icon(path, pic))
}

fn write_picture_icon(path: &Path, pic: rimelight_picture::Picture) -> bool {
    let Some(img) = image::RgbaImage::from_raw(pic.w, pic.h, pic.rgba) else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    image::DynamicImage::ImageRgba8(img)
        .save_with_format(path, image::ImageFormat::Ico)
        .is_ok()
}

#[cfg(windows)]
mod reg {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn set(path: &str, name: &str, value: &str) -> std::io::Result<()> {
        let (key, _) = hkcu().create_subkey(path)?;
        key.set_value(name, &value)
    }

    pub fn register_file_types(exe: &Path, icon: &Path, kinds: [bool; 4]) -> std::io::Result<()> {
        remove_current_progids();
        let exe_s = exe.display().to_string();
        let open = format!("\"{exe_s}\" \"%1\"");
        let icon_s = if icon.exists() {
            icon.display().to_string()
        } else {
            format!("{exe_s},0")
        };
        for ((kind, exts), on) in CATEGORIES.iter().zip(kinds) {
            if !on {
                continue;
            }
            // Each kind of file shows its glyph in its colour (pictures light blue, video purple, music red, text green).
            let kind_icon = icon.with_file_name(format!("rimelight-media-{kind}.ico"));
            let type_icon = if kind_icon.exists() {
                kind_icon.display().to_string()
            } else {
                icon_s.clone()
            };
            for ext in exts.iter() {
                let progid = progid(ext);
                let name = type_name(ext);
                let base = format!(r"Software\Classes\{progid}");
                set(&base, "", &name)?;
                set(&base, "FriendlyTypeName", &name)?;
                set(&format!(r"{base}\DefaultIcon"), "", &type_icon)?;
                set(&format!(r"{base}\shell\open\command"), "", &open)?;
                // Without this, Explorer hides Open for more than 15 files and Enter runs another verb (for pictures, the
                // desktop slideshow). Player: one start per file, gathered by app::queue_open.
                set(&format!(r"{base}\shell\open"), "MultiSelectModel", "Player")?;
                set(&format!(r"{base}\shell"), "", "open")?;
                set(
                    &format!(r"Software\Classes\.{ext}\OpenWithProgids"),
                    &progid,
                    "",
                )?;
                if is_script(ext) {
                    continue;
                }
                set(
                    &format!(r"{CAPS_KEY}\FileAssociations"),
                    &format!(".{ext}"),
                    &progid,
                )?;
                set(
                    &format!(r"Software\Classes\Applications\{EXE_NAME}\SupportedTypes"),
                    &format!(".{ext}"),
                    "",
                )?;
            }
        }
        set(
            &format!(r"Software\Classes\Applications\{EXE_NAME}\shell\open\command"),
            "",
            &open,
        )?;
        set(
            &format!(r"Software\Classes\Applications\{EXE_NAME}"),
            "FriendlyAppName",
            APP_NAME,
        )?;
        set(CAPS_KEY, "ApplicationName", APP_NAME)?;
        set(CAPS_KEY, "ApplicationIcon", &icon_s)?;
        set(
            CAPS_KEY,
            "ApplicationDescription",
            "Pictures, video, music, screenshots and screen recording in one small app.",
        )?;
        set(r"Software\RegisteredApplications", APP_NAME, CAPS_KEY)?;
        rimelight_platform::file_types_changed();
        Ok(())
    }

    /// Removes the current per-extension ProgIDs; register_file_types writes them again. The old per-category ProgIDs stay.
    fn remove_current_progids() {
        let hk = hkcu();
        for ext in all_exts() {
            let progid = progid(ext);
            let _ = hk.delete_subkey_all(format!(r"Software\Classes\{progid}"));
            if let Ok(k) = hk.open_subkey_with_flags(
                format!(r"Software\Classes\.{ext}\OpenWithProgids"),
                KEY_ALL_ACCESS,
            ) {
                let _ = k.delete_value(&progid);
            }
        }
    }

    /// Uninstall: removes every ProgID, the old per-category ones included.
    pub fn unregister_file_types() {
        remove_current_progids();
        let hk = hkcu();
        for ext in all_exts() {
            if let Ok(k) = hk.open_subkey_with_flags(
                format!(r"Software\Classes\.{ext}\OpenWithProgids"),
                KEY_ALL_ACCESS,
            ) {
                for old in OLD_PROGIDS {
                    let _ = k.delete_value(old);
                }
            }
        }
        for old in OLD_PROGIDS {
            let _ = hk.delete_subkey_all(format!(r"Software\Classes\{old}"));
        }
        let _ = hk.delete_subkey_all(format!(r"Software\Classes\Applications\{EXE_NAME}"));
        let _ = hk.delete_subkey_all(r"Software\RimelightMedia");
        if let Ok(k) = hk.open_subkey_with_flags(r"Software\RegisteredApplications", KEY_ALL_ACCESS)
        {
            let _ = k.delete_value(APP_NAME);
        }
        rimelight_platform::file_types_changed();
    }

    /// Windows' own "Use the Print screen key to open screen capture" (on when the value is missing, Windows 11).
    pub fn windows_printscreen_on() -> bool {
        hkcu()
            .open_subkey(r"Control Panel\Keyboard")
            .and_then(|k| k.get_value::<u32, _>("PrintScreenKeyForSnippingEnabled"))
            .map_or(true, |v| v != 0)
    }

    pub fn set_windows_printscreen(on: bool) {
        if let Ok((k, _)) = hkcu().create_subkey(r"Control Panel\Keyboard") {
            let _ = k.set_value("PrintScreenKeyForSnippingEnabled", &u32::from(on));
        }
    }

    /// The ms-screenclip: link Windows starts for Win+Shift+S (and for Print Screen when its own snipping is on). The
    /// Snipping Tool owns it while installed; once it is removed, Windows starts Rimelight Media instead.
    pub fn set_screenclip(exe: Option<&Path>) {
        let base = r"Software\Classes\ms-screenclip";
        match exe {
            Some(exe) => {
                let _ = set(base, "", "URL:Screen snip");
                let _ = set(base, "URL Protocol", "");
                let _ = set(
                    &format!(r"{base}\shell\open\command"),
                    "",
                    &format!("\"{}\" \"%1\"", exe.display()),
                );
            }
            None => {
                let _ = hkcu().delete_subkey_all(base);
            }
        }
    }

    pub fn set_run_at_login(exe: Option<&Path>) {
        let Ok((key, _)) = hkcu().create_subkey(RUN_KEY) else {
            return;
        };
        match exe {
            Some(exe) => {
                let _ = key.set_value(APP_NAME, &format!("\"{}\" --background", exe.display()));
            }
            None => {
                let _ = key.delete_value(APP_NAME);
            }
        }
    }
}

#[cfg(not(windows))]
mod reg {
    use std::path::Path;
    pub fn windows_printscreen_on() -> bool {
        false
    }
    pub fn set_windows_printscreen(_: bool) {}
    pub fn register_file_types(_: &Path, _: &Path, _: [bool; 4]) -> std::io::Result<()> {
        Ok(())
    }
    pub fn unregister_file_types() {}
    pub fn set_run_at_login(_: Option<&Path>) {}
    pub fn set_screenclip(_: Option<&Path>) {}
}

pub use reg::{set_run_at_login, set_screenclip, set_windows_printscreen, windows_printscreen_on};

/// Registers the chosen file types (pictures, video, music) for this exe; the others are removed. An installed copy keeps
/// the same path across updates.
pub fn register_file_types(kinds: [bool; 4]) -> Result<(), String> {
    let icon = icon_path();
    write_icon(&icon);
    // Each category's glyph in white on its colour, at the sizes Explorer uses (16, 32, 48 and 256 px).
    for (kind, glyph, top, bottom) in KIND_ICONS {
        let svg = kind_icon_svg(glyph, top, bottom);
        let frames: Vec<(u32, Vec<u8>)> = [16u32, 32, 48, 256]
            .into_iter()
            .filter_map(|size| render_png(&svg, size).map(|png| (size, png)))
            .collect();
        if frames.len() == 4 {
            write_ico(
                &icon.with_file_name(format!("rimelight-media-{kind}.ico")),
                &frames,
            );
        }
    }
    reg::register_file_types(&current_exe(), &icon, kinds).map_err(|e| e.to_string())
}

/// What shortcuts start: the installer's launcher ("Rimelight Media.exe" in the install folder, always the newest
/// version), else this exe.
pub fn launcher_path() -> PathBuf {
    let exe = current_exe();
    exe.parent()
        .and_then(Path::parent)
        .map(|root| root.join("Rimelight Media.exe"))
        .filter(|p| p.exists())
        .unwrap_or(exe)
}

fn powershell(script: &str) {
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.status();
}

fn quote(p: &Path) -> String {
    p.display().to_string().replace('\'', "''")
}

/// The Start menu folder for every Rimelight app ("Rimelight", like JetBrains' folder).
const START_FOLDER: &str = "$([Environment]::GetFolderPath('Programs'))\\Rimelight";
const DESKTOP: &str = "$([Environment]::GetFolderPath('Desktop'))";

fn shortcut_script(folder: &str) -> String {
    format!(
        "New-Item -ItemType Directory -Force \"{folder}\" | Out-Null; $s=(New-Object -ComObject WScript.Shell).CreateShortcut(\"{folder}\\{APP_NAME}.lnk\"); $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.IconLocation='{},0'; $s.Description='Pictures, video, music and screen capture'; $s.Save()",
        quote(&launcher_path()),
        quote(
            &current_exe()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        ),
        quote(&current_exe()),
    )
}

/// Start menu > Rimelight > Rimelight Media, and the Desktop shortcut when desktop. Also removes the shortcut 0.1.0's
/// installer put at the top of the Start menu.
pub fn set_shortcuts(desktop: bool) {
    let mut script = shortcut_script(START_FOLDER);
    script.push_str("; Remove-Item -Force -ErrorAction SilentlyContinue \"$([Environment]::GetFolderPath('Programs'))\\Rimelight Media.lnk\"");
    if desktop {
        script.push_str("; ");
        script.push_str(&shortcut_script(DESKTOP));
    } else {
        script.push_str(&format!(
            "; Remove-Item -Force -ErrorAction SilentlyContinue \"{DESKTOP}\\{APP_NAME}.lnk\""
        ));
    }
    powershell(&script);
}

/// Removes both shortcuts, and the Rimelight Start menu folder when nothing else is in it.
fn remove_shortcuts() {
    powershell(&format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue \"{START_FOLDER}\\{APP_NAME}.lnk\",\"{DESKTOP}\\{APP_NAME}.lnk\"; if ((Test-Path \"{START_FOLDER}\") -and -not (Get-ChildItem \"{START_FOLDER}\")) {{ Remove-Item -Force \"{START_FOLDER}\" }}"
    ));
}

/// After install: the shortcuts, all file types, start with Windows (so the Print Screen keys work), using the choices
/// saved in the settings (defaults on a first install). The first-run window then lets the person change them.
pub fn after_install() {
    let p = crate::prefs::Prefs::load();
    set_shortcuts(p.desktop_shortcut);
    let _ = register_file_types(p.file_types);
    let mut p = p;
    p.background = true;
    p.save();
    set_run_at_login(Some(&launcher_path()));
    set_screenclip(Some(&launcher_path()));
}

/// After an update: the same integration, kept as the person chose it.
pub fn after_update() {
    let p = crate::prefs::Prefs::load();
    set_shortcuts(p.desktop_shortcut);
    let _ = register_file_types(p.file_types);
    if p.background {
        set_run_at_login(Some(&launcher_path()));
    }
    set_screenclip(Some(&launcher_path()));
}

/// Removes everything this module added, and the app's settings folder (the uninstall hook runs it). Pictures,
/// recordings and voice notes the person made are theirs and stay.
pub fn unregister_all() {
    let p = crate::prefs::Prefs::load();
    if p.printscreen_changed {
        set_windows_printscreen(true);
    }
    remove_shortcuts();
    reg::unregister_file_types();
    set_run_at_login(None);
    set_screenclip(None);
    let _ = std::fs::remove_dir_all(crate::prefs::data_dir());
}

/// Windows Settings > Default apps, on Rimelight Media's page. (Started through the shell: Explorer given an
/// ms-settings link with an encoded space opened the Documents folder instead.)
pub fn open_default_apps_settings() {
    if !rimelight_platform::open_uri(&format!(
        "ms-settings:defaultapps?registeredAppUser={APP_NAME}"
    )) {
        rimelight_platform::open_uri("ms-settings:defaultapps");
    }
}

/// The "Set as default app" button. Windows 10 and 11 don't let an app set its own defaults silently (UserChoice is
/// protected), so this (re)writes the app's registrations for the chosen file types (ProgIDs, RegisteredApplications and
/// its Capabilities, which is what Default apps lists) and opens this app's Default apps page for the person to pick.
/// No message box: the caller's toast says what to do.
pub fn register_defaults(kinds: [bool; 4]) -> Result<(), String> {
    register_file_types(kinds)?;
    open_default_apps_settings();
    Ok(())
}
