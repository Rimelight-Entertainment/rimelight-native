//! Web sign-in (decision 0014, SA.6): a same-tab redirect through the backend.
//!
//! [`begin_web_sign_in`] keeps the PKCE verifier and a `state` in sessionStorage and sends the page to `/v1/auth/login`
//! with the page's own address as the return address. The backend sends the browser back there with `code` and `state`, or
//! `error` and `state`. [`complete_web_sign_in`] runs when the page starts: it checks the `state`, removes the sign-in
//! parameters from the address bar, and exchanges the code.
//!
//! The pure parts (reading the return, the state check, the return address) compile on every target and are unit-tested
//! natively. The browser calls are wasm32 only.

use serde::{Deserialize, Serialize};
#[cfg(target_arch = "wasm32")]
use space_proto::auth::{ExchangeRequest, SessionResponse};

#[cfg(target_arch = "wasm32")]
use crate::api::ApiClient;
use crate::api::parse_query;
use crate::error::ClientError;

/// sessionStorage key for the sign-in in progress.
pub const PENDING_KEY: &str = "rimelight-space.web-signin";

/// What the page keeps while the browser is away at the sign-in: the verifier for the exchange and the `state` to check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWebSignIn {
    pub verifier: String,
    pub state: String,
}

/// The sign-in parameters the backend put on the return address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebReturn {
    Code { code: String, state: String },
    Error { error: String, state: String },
}

/// Reads the sign-in return from a query string (without the leading `?`). `error` wins over `code`. Returns `None` when
/// there is no `state`, or when there is neither a non-empty `code` nor an `error`: the page is then not a sign-in return.
pub fn parse_return(query: &str) -> Option<WebReturn> {
    let params = parse_query(query);
    let get = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let state = get("state")?;
    if let Some(error) = get("error") {
        return Some(WebReturn::Error { error, state });
    }
    let code = get("code").filter(|code| !code.is_empty())?;
    Some(WebReturn::Code { code, state })
}

/// The pending sign-in if its `state` matches the one the browser returned. A missing pending sign-in never matches.
pub fn matching_pending(
    pending: Option<PendingWebSignIn>,
    returned_state: &str,
) -> Result<PendingWebSignIn, ClientError> {
    match pending {
        Some(pending) if pending.state == returned_state => Ok(pending),
        _ => Err(ClientError::StateMismatch),
    }
}

/// The return address for the login: this page's origin and path, with no query or fragment. An empty path is `/`.
pub fn return_address(origin: &str, pathname: &str) -> String {
    let path = if pathname.is_empty() { "/" } else { pathname };
    format!("{}{path}", origin.trim_end_matches('/'))
}

/// Sends the page to Auth0 through the backend. The page leaves, so nothing runs after this in the page.
#[cfg(target_arch = "wasm32")]
pub fn begin_web_sign_in(api: &ApiClient) -> Result<(), ClientError> {
    use gloo_storage::{SessionStorage, Storage as _};

    let pair = crate::pkce::new_pair()?;
    let state = crate::pkce::new_state()?;
    let (origin, pathname) = page_location()?;
    let return_to = return_address(&origin, &pathname);
    let pending = PendingWebSignIn {
        verifier: pair.verifier,
        state: state.clone(),
    };
    SessionStorage::set(PENDING_KEY, &pending).map_err(|e| ClientError::Storage(e.to_string()))?;
    let url = api.login_url(&return_to, &pair.challenge, &state);
    location()?.set_href(&url).map_err(browser_error)
}

/// Runs at start-up. `None` when the page address carries no sign-in return (the normal start). Otherwise checks the
/// `state`, removes the sign-in parameters from the address bar, and exchanges the code for a session. The pending sign-in
/// is used once, whatever the outcome.
#[cfg(target_arch = "wasm32")]
pub async fn complete_web_sign_in(api: &ApiClient) -> Option<Result<SessionResponse, ClientError>> {
    let query = page_query()?;
    let returned = parse_return(&query)?;
    strip_query();
    let pending = take_pending();
    let returned_state = match &returned {
        WebReturn::Code { state, .. } | WebReturn::Error { state, .. } => state,
    };
    let pending = match matching_pending(pending, returned_state) {
        Ok(pending) => pending,
        Err(error) => return Some(Err(error)),
    };
    Some(match returned {
        WebReturn::Error { error, .. } => Err(ClientError::SignInDenied(error)),
        WebReturn::Code { code, .. } => {
            api.exchange(&ExchangeRequest {
                code,
                code_verifier: pending.verifier,
            })
            .await
        }
    })
}

/// A browser refusal (no window, a blocked storage call) as a client error. `JsValue` is `Debug`.
#[cfg(target_arch = "wasm32")]
fn browser_error<E: std::fmt::Debug>(error: E) -> ClientError {
    ClientError::Browser(format!("{error:?}"))
}

#[cfg(target_arch = "wasm32")]
fn location() -> Result<web_sys::Location, ClientError> {
    web_sys::window()
        .map(|window| window.location())
        .ok_or_else(|| ClientError::Browser("no window".to_string()))
}

/// The page's origin and path.
#[cfg(target_arch = "wasm32")]
fn page_location() -> Result<(String, String), ClientError> {
    let location = location()?;
    let origin = location.origin().map_err(browser_error)?;
    let pathname = location.pathname().map_err(browser_error)?;
    Ok((origin, pathname))
}

/// The page's query without the leading `?`, or `None` when there is none.
#[cfg(target_arch = "wasm32")]
fn page_query() -> Option<String> {
    let search = location().ok()?.search().ok()?;
    let query = search.strip_prefix('?').unwrap_or(&search);
    (!query.is_empty()).then(|| query.to_string())
}

/// Replaces the address with the page's path, so a reload does not repeat the return.
#[cfg(target_arch = "wasm32")]
fn strip_query() {
    let Ok((_, pathname)) = page_location() else {
        return;
    };
    if let Some(window) = web_sys::window()
        && let Ok(history) = window.history()
    {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&pathname));
    }
}

/// Reads and removes the pending sign-in. A missing or unreadable value is `None`.
#[cfg(target_arch = "wasm32")]
fn take_pending() -> Option<PendingWebSignIn> {
    use gloo_storage::{SessionStorage, Storage as _};
    let pending = SessionStorage::get::<PendingWebSignIn>(PENDING_KEY).ok();
    SessionStorage::delete(PENDING_KEY);
    pending
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = "st-1";

    #[test]
    fn a_code_return_is_read_with_its_state() {
        assert_eq!(
            parse_return("code=abc%2B1&state=st-1"),
            Some(WebReturn::Code {
                code: "abc+1".to_string(),
                state: STATE.to_string(),
            })
        );
    }

    #[test]
    fn an_error_return_is_read_and_wins_over_a_code() {
        assert_eq!(
            parse_return("error=access_denied&state=st-1"),
            Some(WebReturn::Error {
                error: "access_denied".to_string(),
                state: STATE.to_string(),
            })
        );
        assert_eq!(
            parse_return("code=x&error=not_employee&state=st-1"),
            Some(WebReturn::Error {
                error: "not_employee".to_string(),
                state: STATE.to_string(),
            })
        );
    }

    #[test]
    fn a_page_without_a_sign_in_return_is_not_one() {
        assert_eq!(parse_return(""), None);
        assert_eq!(parse_return("calendar=week"), None);
        assert_eq!(parse_return("code=abc"), None, "no state");
        assert_eq!(parse_return("state=st-1"), None, "no code or error");
        assert_eq!(parse_return("code=&state=st-1"), None, "empty code");
    }

    #[test]
    fn the_state_must_match_the_pending_sign_in() {
        let pending = PendingWebSignIn {
            verifier: "v".repeat(43),
            state: STATE.to_string(),
        };
        assert_eq!(
            matching_pending(Some(pending.clone()), "st-1"),
            Ok(pending.clone())
        );
        assert_eq!(
            matching_pending(Some(pending), "forged"),
            Err(ClientError::StateMismatch)
        );
        assert_eq!(
            matching_pending(None, "st-1"),
            Err(ClientError::StateMismatch),
            "a return with nothing pending (for example after a reload) is refused"
        );
    }

    #[test]
    fn the_return_address_is_origin_and_path() {
        assert_eq!(
            return_address("http://localhost:8080", "/calendar"),
            "http://localhost:8080/calendar"
        );
        assert_eq!(
            return_address("https://space.example.com/", "/"),
            "https://space.example.com/"
        );
        assert_eq!(
            return_address("http://localhost:8080", ""),
            "http://localhost:8080/"
        );
    }

    #[test]
    fn the_pending_sign_in_survives_a_json_round_trip() {
        let pending = PendingWebSignIn {
            verifier: "verifier-1".to_string(),
            state: "state-1".to_string(),
        };
        let json = serde_json::to_string(&pending).expect("serialize");
        let back: PendingWebSignIn = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, pending);
    }
}
