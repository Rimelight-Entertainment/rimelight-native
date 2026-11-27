//! Start menu and Desktop shortcuts for the Rimelight apps, Windows only. Every app shortcut sits in one company folder,
//! "Rimelight Entertainment" (as JetBrains' tools sit in "JetBrains"). Velopack's own shortcuts are turned off at pack
//! time (`--shortcuts None`), so these functions are the only place shortcuts are made.

use std::path::{Path, PathBuf};

/// The Start menu folder every Rimelight app's shortcut goes in.
pub const COMPANY_FOLDER: &str = "Rimelight Entertainment";
/// The folder the earlier builds used. Removed when shortcuts are set up again.
const OLD_FOLDER: &str = "Rimelight";

/// One app's shortcut: its name (the shortcut's file name), the program it starts, and its description.
pub struct Shortcut<'a> {
    pub name: &'a str,
    pub target: &'a Path,
    pub description: &'a str,
}

fn quote(p: &Path) -> String {
    p.display().to_string().replace('\'', "''")
}

fn programs_folder() -> String {
    "$([Environment]::GetFolderPath('Programs'))".to_owned()
}

fn desktop_folder() -> String {
    "$([Environment]::GetFolderPath('Desktop'))".to_owned()
}

fn run_powershell(script: &str) {
    use std::os::windows::process::CommandExt as _;
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status();
}

/// The script that writes one `.lnk` into `folder`.
fn shortcut_script(folder: &str, app: &Shortcut<'_>) -> String {
    let dir = app
        .target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    format!(
        "New-Item -ItemType Directory -Force '{folder}' | Out-Null; \
         $s=(New-Object -ComObject WScript.Shell).CreateShortcut('{folder}\\{name}.lnk'); \
         $s.TargetPath='{target}'; $s.WorkingDirectory='{dir}'; $s.IconLocation='{target},0'; \
         $s.Description='{desc}'; $s.Save()",
        name = app.name,
        target = quote(app.target),
        dir = quote(&dir),
        desc = app.description.replace('\'', "''"),
    )
}

/// Makes the Start menu shortcut (inside [`COMPANY_FOLDER`]) and, when `desktop` is set, the Desktop shortcut. Also
/// removes this app's shortcut from the old `Rimelight` folder and from the Start menu root, where earlier builds put it.
pub fn set_up(app: &Shortcut<'_>, desktop: bool) {
    let start = format!("{}\\{COMPANY_FOLDER}", programs_folder());
    let old = format!("{}\\{OLD_FOLDER}", programs_folder());
    let mut script = shortcut_script(&start, app);
    script.push_str(&format!(
        "; Remove-Item -Force -ErrorAction SilentlyContinue '{old}\\{name}.lnk','{root}\\{name}.lnk'; \
         if ((Test-Path '{old}') -and -not (Get-ChildItem '{old}')) {{ Remove-Item -Force '{old}' }}",
        name = app.name,
        root = programs_folder(),
    ));
    if desktop {
        script.push_str("; ");
        script.push_str(&shortcut_script(&desktop_folder(), app));
    } else {
        script.push_str(&format!(
            "; Remove-Item -Force -ErrorAction SilentlyContinue '{}\\{}.lnk'",
            desktop_folder(),
            app.name
        ));
    }
    run_powershell(&script);
}

/// Removes this app's shortcuts, and the company folder when nothing else is in it.
pub fn remove(app_name: &str) {
    let start = format!("{}\\{COMPANY_FOLDER}", programs_folder());
    run_powershell(&format!(
        "Remove-Item -Force -ErrorAction SilentlyContinue '{start}\\{app_name}.lnk','{}\\{app_name}.lnk'; \
         if ((Test-Path '{start}') -and -not (Get-ChildItem '{start}')) {{ Remove-Item -Force '{start}' }}",
        desktop_folder()
    ));
}

/// The app's program in its install folder: the Velopack launcher one level above `current` (for example
/// `%LOCALAPPDATA%\RimelightPersonal\Rimelight Personal.exe`), which starts the newest version. Falls back to `exe`.
pub fn launcher_for(exe: &Path, launcher_name: &str) -> PathBuf {
    exe.parent()
        .and_then(Path::parent)
        .map(|install| install.join(launcher_name))
        .filter(|p| p.exists())
        .unwrap_or_else(|| exe.to_path_buf())
}
