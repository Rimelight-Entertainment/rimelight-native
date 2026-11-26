//! Sign-in and the session for the personal API, which sits behind a Cloudflare Access application on idantity.me.
//!
//! Desktop: Cloudflare Access issues a JWT (`CF_Authorization`). The app obtains it through the Access CLI login (see
//! [`crate::signin`]), keeps it in the OS credential store, and sends it as the `cf-access-token` header on every API call.
//! A token whose `exp` claim has passed is treated as absent, so the next start signs in again.
//!
//! Web: the browser holds the `CF_Authorization` cookie for idantity.me once Access has signed the user in, and
//! same-origin requests send it. The web build sends no header; sign-in navigates to the Access login and sign-out to the
//! Access logout (both in the `apps/web` shell, see [`web_login_url`] and [`web_logout_url`]).
//!
//! The UI reads [`Auth::state`]-style results through callbacks; nothing here draws anything.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::api::{BearerSource, HeaderToken};

/// The request header the personal API expects the Access token in.
pub const ACCESS_HEADER: &str = "cf-access-token";

/// The Access application's origin (the same host as the API). Empty is not allowed; see [`AppUrl::new`].
pub const APP_ORIGIN: &str = "https://idantity.me";

/// The Access application the personal API sits behind: its URL (host and path), as Access scopes it.
pub const ACCESS_APP_URL: &str = "https://idantity.me/api/personal";

/// The application's AUD tag. Access prints it as `kid` in the login redirect and as `aud` in the token; it is not a secret.
pub const ACCESS_AUD: &str = "f55835872c87ed965c6f06fa02bed51bb36fa5b714ea7f3c20205d89624d6391";

/// The team's Access login host (where Access signs a user in).
pub const ACCESS_TEAM_ORIGIN: &str = "https://idantitydotme.cloudflareaccess.com";

/// The token transfer service the CLI login polls (the same one `cloudflared access login` uses).
pub const TRANSFER_ORIGIN: &str = "https://login.cloudflareaccess.org";

/// Failures in sign-in and the session store. Shown to the user as text, never with the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The browser could not be opened, or the sign-in page was not reached.
    Browser(String),
    /// The user did not finish signing in before the timeout.
    Timeout,
    /// The sign-in service answered with something this client does not understand.
    Protocol(String),
    /// The OS credential store refused a read or write.
    Storage(String),
    /// The stored token is unreadable (corrupt value).
    Corrupt,
    /// The platform cannot run this step (for example no loopback listener could be bound).
    Unavailable(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Browser(e) => write!(f, "could not open the sign-in page: {e}"),
            Self::Timeout => write!(f, "sign-in was not finished in time"),
            Self::Protocol(e) => write!(f, "unexpected sign-in response: {e}"),
            Self::Storage(e) => write!(f, "could not use the credential store: {e}"),
            Self::Corrupt => write!(f, "the stored sign-in is unreadable"),
            Self::Unavailable(e) => write!(f, "sign-in is not available here: {e}"),
        }
    }
}

/// The Access token and when it expires (seconds since the Unix epoch, from the JWT `exp` claim).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccess {
    pub token: String,
    pub expires_at: u64,
}

impl StoredAccess {
    /// Wraps a token, reading its `exp` claim. A token without a readable `exp` is kept with an expiry of zero, so it is
    /// never reused (the next start signs in again).
    pub fn from_token(token: String) -> Self {
        let expires_at = jwt_exp(&token).unwrap_or(0);
        Self { token, expires_at }
    }

    /// True when the token is still valid at `now` (seconds since the epoch), with a 60 second margin.
    pub fn is_valid_at(&self, now: u64) -> bool {
        self.expires_at > now.saturating_add(60)
    }
}

/// Reads the `exp` claim of a JWT without checking its signature (the server checks it on every call).
pub fn jwt_exp(token: &str) -> Option<u64> {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("exp")?.as_u64()
}

/// Seconds since the Unix epoch.
pub fn now_unix() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
}

/// The in-memory session: the current Access token, read by the API client on each request. Shared with an `Arc`.
#[derive(Debug, Default)]
pub struct AccessSession {
    access: Mutex<Option<StoredAccess>>,
}

impl AccessSession {
    pub fn set(&self, access: Option<StoredAccess>) {
        *self.access.lock().unwrap_or_else(|p| p.into_inner()) = access;
    }

    /// True when a token is held and still valid now.
    pub fn is_signed_in(&self) -> bool {
        self.access
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|a| a.is_valid_at(now_unix()))
    }
}

impl BearerSource for AccessSession {
    fn credential(&self) -> Option<HeaderToken> {
        let guard = self.access.lock().unwrap_or_else(|p| p.into_inner());
        let access = guard.as_ref()?;
        // An expired token is not sent: the API answers as signed out, and the shell shows the sign-in page.
        access.is_valid_at(now_unix()).then(|| HeaderToken {
            name: ACCESS_HEADER.to_owned(),
            value: access.token.clone(),
        })
    }
}

/// Where the Access token is kept between launches.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredAccess>, AuthError>;
    fn save(&self, access: &StoredAccess) -> Result<(), AuthError>;
    /// Removing an empty store is not an error.
    fn clear(&self) -> Result<(), AuthError>;
}

/// In-memory store, for tests and for a platform without a store yet.
#[derive(Default)]
pub struct MemoryStore {
    slot: Mutex<Option<StoredAccess>>,
}

impl SessionStore for MemoryStore {
    fn load(&self) -> Result<Option<StoredAccess>, AuthError> {
        Ok(self.slot.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }

    fn save(&self, access: &StoredAccess) -> Result<(), AuthError> {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(access.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), AuthError> {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
}

/// The OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service), the same way Rimelight
/// Space keeps its session. One JSON value under service `rimelight-personal`.
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
impl KeyringStore {
    fn entry() -> Result<keyring::Entry, AuthError> {
        keyring::Entry::new("rimelight-personal", "access").map_err(|e| AuthError::Storage(e.to_string()))
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl SessionStore for KeyringStore {
    fn load(&self) -> Result<Option<StoredAccess>, AuthError> {
        match Self::entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|_| AuthError::Corrupt),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AuthError::Storage(e.to_string())),
        }
    }

    fn save(&self, access: &StoredAccess) -> Result<(), AuthError> {
        let json = serde_json::to_string(access).map_err(|e| AuthError::Storage(e.to_string()))?;
        Self::entry()?
            .set_password(&json)
            .map_err(|e| AuthError::Storage(e.to_string()))
    }

    fn clear(&self) -> Result<(), AuthError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AuthError::Storage(e.to_string())),
        }
    }
}

/// The sign-in state the app holds: the store and the in-memory session, kept together so sign-out clears both.
pub struct Auth {
    pub session: Arc<AccessSession>,
    store: Arc<dyn SessionStore>,
}

impl Auth {
    pub fn new(store: Arc<dyn SessionStore>) -> Self {
        Self {
            session: Arc::new(AccessSession::default()),
            store,
        }
    }

    /// Loads a stored token into the session when it is still valid. Returns whether the user is signed in. An expired
    /// token is cleared from the store.
    pub fn restore(&self) -> Result<bool, AuthError> {
        match self.store.load() {
            Ok(Some(access)) if access.is_valid_at(now_unix()) => {
                self.session.set(Some(access));
                Ok(true)
            }
            Ok(Some(_)) => {
                let _ = self.store.clear();
                Ok(false)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Keeps a token from a finished sign-in, in memory and in the store.
    pub fn accept_token(&self, token: String) -> Result<(), AuthError> {
        let access = StoredAccess::from_token(token);
        if !access.is_valid_at(now_unix()) {
            return Err(AuthError::Protocol(
                "the sign-in token has already expired".to_owned(),
            ));
        }
        self.store.save(&access)?;
        self.session.set(Some(access));
        Ok(())
    }

    /// Clears the token from memory and from the store. The caller navigates the web build to the Access logout.
    pub fn sign_out(&self) -> Result<(), AuthError> {
        self.session.set(None);
        self.store.clear()
    }
}

/// The web sign-in page: Access shows its login and returns to `return_to` (an absolute URL on the app origin). Access
/// takes a path on the application host, not a full URL (a full URL is refused as "Invalid redirect URL"), so only the
/// path and query of `return_to` are sent. A `return_to` that is not on the app origin returns to `/`.
pub fn web_login_url(return_to: &str) -> String {
    let path = return_to
        .strip_prefix(APP_ORIGIN)
        .filter(|rest| rest.starts_with('/'))
        .unwrap_or("/");
    format!(
        "{ACCESS_TEAM_ORIGIN}/cdn-cgi/access/login/idantity.me?kid={ACCESS_AUD}&redirect_url={}",
        percent_encode(path)
    )
}

/// The web sign-out: Access clears its cookie and returns to the app origin.
pub fn web_logout_url() -> String {
    format!("{APP_ORIGIN}/cdn-cgi/access/logout")
}

/// Encodes a query value (RFC 3986 unreserved characters pass through).
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte))
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JWT-shaped token with only the payload that matters: `{"exp":<exp>}`. The signature is not checked by the client.
    fn token_with_exp(exp: u64) -> String {
        use base64::Engine as _;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let payload = URL_SAFE_NO_PAD.encode(format!("{{\"exp\":{exp}}}"));
        format!("e30.{payload}.sig")
    }

    #[test]
    fn reads_exp_from_the_payload() {
        assert_eq!(jwt_exp(&token_with_exp(1_900_000_000)), Some(1_900_000_000));
        assert_eq!(jwt_exp("not-a-jwt"), None);
    }

    #[test]
    fn a_token_near_expiry_is_not_valid() {
        let access = StoredAccess {
            token: "t".into(),
            expires_at: 1000,
        };
        assert!(access.is_valid_at(900));
        assert!(!access.is_valid_at(950), "inside the 60 second margin");
    }

    #[test]
    fn expired_session_sends_no_header() {
        let session = AccessSession::default();
        session.set(Some(StoredAccess {
            token: "old".into(),
            expires_at: 1,
        }));
        assert_eq!(session.credential(), None);
    }

    #[test]
    fn accept_then_sign_out_clears_store_and_session() {
        let auth = Auth::new(Arc::new(MemoryStore::default()));
        let token = token_with_exp(now_unix() + 3600);
        auth.accept_token(token.clone()).expect("accept");
        let header = auth.session.credential().expect("header");
        assert_eq!(header.name, ACCESS_HEADER);
        assert_eq!(header.value, token);
        auth.sign_out().expect("sign out");
        assert_eq!(auth.session.credential(), None);
        assert_eq!(auth.store.load(), Ok(None));
    }

    #[test]
    fn restore_drops_an_expired_stored_token() {
        let store = Arc::new(MemoryStore::default());
        store
            .save(&StoredAccess {
                token: "old".into(),
                expires_at: 1,
            })
            .expect("save");
        let auth = Auth::new(store.clone());
        assert_eq!(auth.restore(), Ok(false));
        assert_eq!(store.load(), Ok(None));
    }

    #[test]
    fn web_login_url_sends_a_path_with_the_app_aud() {
        assert_eq!(
            web_login_url("https://idantity.me/todos"),
            format!(
                "https://idantitydotme.cloudflareaccess.com/cdn-cgi/access/login/idantity.me?kid={ACCESS_AUD}&redirect_url=%2Ftodos"
            )
        );
        assert!(web_login_url("https://elsewhere.example/x").ends_with("redirect_url=%2F"));
    }
}
