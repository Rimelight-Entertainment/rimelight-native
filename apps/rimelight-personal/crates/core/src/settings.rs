//! The instance this installed copy connects to, saved by setup (see [`crate::provision`]). Native only.
//!
//! The file is `settings.json` in the app's data folder: `%LOCALAPPDATA%\Rimelight Personal` on Windows,
//! `~/Library/Application Support/Rimelight Personal` on macOS, `$XDG_CONFIG_HOME` or `~/.config` elsewhere. It holds
//! no secret: the audience and the team name are public identifiers, and the owner's sign-in token is in the OS
//! keychain, not here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::instance::InstanceConfig;

const FOLDER: &str = "Rimelight Personal";
const FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub api_base: String,
    pub access_aud: String,
    pub access_team_origin: String,
}

impl Settings {
    pub fn to_instance(&self) -> Result<InstanceConfig, String> {
        InstanceConfig::from_values(
            self.api_base.clone(),
            self.access_aud.clone(),
            self.access_team_origin.clone(),
        )
    }
}

/// The folder the settings file lives in, or `None` when the platform has no such folder.
pub fn settings_folder() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join(FOLDER))
}

pub fn settings_path() -> Option<PathBuf> {
    settings_folder().map(|folder| folder.join(FILE))
}

/// Reads the saved settings. `Ok(None)` when setup has not run yet.
pub fn load(path: &Path) -> Result<Option<Settings>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("settings are unreadable: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("could not read the settings: {e}")),
    }
}

/// Writes the settings, creating the folder when it is missing.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not create the settings folder: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("could not save the settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Settings {
        Settings {
            api_base: "https://rimelight-personal.example.workers.dev/api".to_owned(),
            access_aud: "aud-1".to_owned(),
            access_team_origin: "https://team.cloudflareaccess.com".to_owned(),
        }
    }

    #[test]
    fn settings_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("rp-settings-{}", std::process::id()));
        let path = dir.join(FILE);
        save(&path, &sample()).expect("save");
        assert_eq!(load(&path).expect("load"), Some(sample()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_means_setup_has_not_run() {
        let path = std::env::temp_dir().join("rp-settings-missing-does-not-exist.json");
        assert_eq!(load(&path), Ok(None));
    }

    #[test]
    fn saved_settings_become_an_instance() {
        let instance = sample().to_instance().expect("valid");
        assert_eq!(instance.api_base, "https://rimelight-personal.example.workers.dev/api");
        assert_eq!(instance.access_aud, "aud-1");
    }

    #[test]
    fn a_damaged_file_is_reported_not_ignored() {
        let dir = std::env::temp_dir().join(format!("rp-settings-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(FILE);
        std::fs::write(&path, "{ not json").expect("write");
        assert!(load(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
