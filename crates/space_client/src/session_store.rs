//! Session token storage. The OS credential store on desktop, an in-memory
//! store for tests and for any platform without a backend yet.

use serde::{Deserialize, Serialize};
use space_proto::auth::CurrentUser;

use crate::error::ClientError;
use crate::lock;

/// What is kept between launches: the bearer token, its expiry and the user
/// it belongs to, so an offline start can show who is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSession {
    pub session_token: String,
    /// RFC 3339 timestamp, as sent by the backend.
    pub expires_at: String,
    pub user: CurrentUser,
}

pub trait SessionStore: Send + Sync {
    fn save(&self, session: &StoredSession) -> Result<(), ClientError>;
    fn load(&self) -> Result<Option<StoredSession>, ClientError>;
    /// Removes the stored session. Clearing an empty store is not an error.
    fn clear(&self) -> Result<(), ClientError>;
}

/// In-memory store. Used by tests and as a fallback.
#[derive(Default)]
pub struct MemorySessionStore {
    slot: std::sync::Mutex<Option<StoredSession>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for MemorySessionStore {
    fn save(&self, session: &StoredSession) -> Result<(), ClientError> {
        *lock(&self.slot) = Some(session.clone());
        Ok(())
    }

    fn load(&self) -> Result<Option<StoredSession>, ClientError> {
        Ok(lock(&self.slot).clone())
    }

    fn clear(&self) -> Result<(), ClientError> {
        *lock(&self.slot) = None;
        Ok(())
    }
}

/// OS credential store (Windows Credential Manager, macOS Keychain, Linux
/// Secret Service or keyutils). The session is one JSON value under service
/// `rimelight-space`.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub struct KeyringStore;

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
const SERVICE: &str = "rimelight-space";
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
const ACCOUNT: &str = "session";

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl KeyringStore {
    fn entry() -> Result<keyring::Entry, ClientError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| ClientError::Storage(e.to_string()))
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl SessionStore for KeyringStore {
    fn save(&self, session: &StoredSession) -> Result<(), ClientError> {
        let json =
            serde_json::to_string(session).map_err(|e| ClientError::Storage(e.to_string()))?;
        Self::entry()?
            .set_password(&json)
            .map_err(|e| ClientError::Storage(e.to_string()))
    }

    fn load(&self) -> Result<Option<StoredSession>, ClientError> {
        match Self::entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| ClientError::Storage(format!("stored session is unreadable: {e}"))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(ClientError::Storage(e.to_string())),
        }
    }

    fn clear(&self) -> Result<(), ClientError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(ClientError::Storage(e.to_string())),
        }
    }
}

/// A session file on a platform without a credential store (Android, SA.7, decision 0018). The file is written with mode
/// 0600 where the platform has modes. On Android the folder is private to the app; encrypting the file with the Android
/// Keystore is a later hardening step.
#[cfg(not(target_arch = "wasm32"))]
pub struct FileSessionStore {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileSessionStore {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SessionStore for FileSessionStore {
    fn save(&self, session: &StoredSession) -> Result<(), ClientError> {
        let json = serde_json::to_vec(session).map_err(|e| ClientError::Storage(e.to_string()))?;
        write_private(&self.path, &json)
    }

    fn load(&self) -> Result<Option<StoredSession>, ClientError> {
        match std::fs::read_to_string(&self.path) {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| ClientError::Storage(format!("stored session is unreadable: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ClientError::Storage(e.to_string())),
        }
    }

    fn clear(&self) -> Result<(), ClientError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ClientError::Storage(e.to_string())),
        }
    }
}

/// Writes `bytes` to `path` through a temporary file that is renamed into place, so a crash never leaves half a file. The
/// temporary file is created private (mode 0600 on unix) before anything is written to it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), ClientError> {
    use std::io::Write as _;
    let storage = |e: std::io::Error| ClientError::Storage(e.to_string());
    let temp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(storage)?;
    file.write_all(bytes).map_err(storage)?;
    file.sync_all().map_err(storage)?;
    drop(file);
    std::fs::rename(&temp, path).map_err(storage)
}

/// Browser store (SA.6): the session as JSON in `localStorage` under `rimelight-space.session`. A browser that refuses storage
/// gives an error from `save`, and the app shows the sign-in screen again on the next start.
#[cfg(target_arch = "wasm32")]
pub struct WebSessionStore;

#[cfg(target_arch = "wasm32")]
pub const WEB_SESSION_KEY: &str = "rimelight-space.session";

#[cfg(target_arch = "wasm32")]
impl SessionStore for WebSessionStore {
    fn save(&self, session: &StoredSession) -> Result<(), ClientError> {
        use gloo_storage::{LocalStorage, Storage as _};
        LocalStorage::set(WEB_SESSION_KEY, session).map_err(|e| ClientError::Storage(e.to_string()))
    }

    fn load(&self) -> Result<Option<StoredSession>, ClientError> {
        use gloo_storage::{LocalStorage, Storage as _, errors::StorageError};
        match LocalStorage::get::<StoredSession>(WEB_SESSION_KEY) {
            Ok(session) => Ok(Some(session)),
            Err(StorageError::KeyNotFound(_)) => Ok(None),
            Err(e) => Err(ClientError::Storage(e.to_string())),
        }
    }

    fn clear(&self) -> Result<(), ClientError> {
        use gloo_storage::{LocalStorage, Storage as _};
        LocalStorage::delete(WEB_SESSION_KEY);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoredSession {
        StoredSession {
            session_token: "tok_1".to_string(),
            expires_at: "2026-12-15T10:00:00Z".to_string(),
            user: CurrentUser {
                id: "u1".to_string(),
                email: "o@example.com".to_string(),
                display_name: "Owner".to_string(),
                auth0_sub: None,
            },
        }
    }

    #[test]
    fn memory_store_save_load_clear() {
        let store = MemorySessionStore::new();
        assert_eq!(store.load(), Ok(None));
        store.save(&sample()).expect("save");
        assert_eq!(store.load(), Ok(Some(sample())));
        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None));
        store.clear().expect("clearing an empty store is fine");
    }

    #[test]
    fn stored_session_json_round_trips() {
        let json = serde_json::to_string(&sample()).expect("serialize");
        let back: StoredSession = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, sample());
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rimelight-client-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn file_store_save_load_clear() {
        let dir = test_dir("file-store");
        let store = FileSessionStore::new(dir.join("session.json"));
        assert_eq!(store.load(), Ok(None));
        store.save(&sample()).expect("save");
        assert_eq!(store.load(), Ok(Some(sample())));
        store.save(&sample()).expect("overwrite");
        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None));
        store.clear().expect("clearing an empty store is fine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn file_store_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = test_dir("file-mode");
        let path = dir.join("session.json");
        FileSessionStore::new(path.clone())
            .save(&sample())
            .expect("save");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
