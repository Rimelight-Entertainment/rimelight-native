//! Sign-in on Android (SA.7, decision 0018): the system browser and the return address `rimelight-space://auth`.
//!
//! [`begin`] keeps the PKCE verifier and a `state` in a private file in the app's data folder, then opens `/v1/auth/login`
//! in the browser with the return address. The browser comes back to the app through the custom scheme, and the app passes
//! that address to [`finish_return`], which checks the `state` and exchanges the code. The file is kept because the app can
//! be stopped while the browser is open.
//!
//! This module does not touch Android. The app supplies the function that opens a URL, and the app passes the return
//! address it received.

use std::path::Path;

use space_proto::auth::{ExchangeRequest, SessionResponse};

use crate::api::ApiClient;
use crate::error::ClientError;
use crate::pkce;
use crate::session_store::write_private;
use crate::web_signin::{PendingWebSignIn, WebReturn, matching_pending, parse_return};

/// The return address the backend sends the browser back to. The app's intent filter has the same scheme and host.
pub const RETURN_ADDRESS: &str = "rimelight-space://auth";

/// The file in the app's data folder that keeps the sign-in in progress.
pub const PENDING_FILE: &str = "signin-pending.json";

/// Starts the sign-in: writes the pending sign-in privately, then opens the login page with `open`. If `open` fails the
/// pending sign-in is removed again.
pub fn begin(
    api: &ApiClient,
    dir: &Path,
    open: &dyn Fn(&str) -> Result<(), ClientError>,
) -> Result<(), ClientError> {
    let pair = pkce::new_pair()?;
    let state = pkce::new_state()?;
    let pending = PendingWebSignIn {
        verifier: pair.verifier,
        state: state.clone(),
    };
    let json = serde_json::to_vec(&pending).map_err(|e| ClientError::Storage(e.to_string()))?;
    let path = dir.join(PENDING_FILE);
    write_private(&path, &json)?;
    let url = api.login_url(RETURN_ADDRESS, &pair.challenge, &state);
    if let Err(error) = open(&url) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(())
}

/// The query of a sign-in return address, without the `?`. `None` when `url` is not the return address.
pub fn return_query(url: &str) -> Option<&str> {
    let rest = url.strip_prefix(RETURN_ADDRESS)?;
    match rest.strip_prefix('?') {
        Some(query) => Some(query),
        None if rest.is_empty() => Some(""),
        None => None,
    }
}

/// Finishes the sign-in with the address the app received.
///
/// `None` when the address is not a sign-in return, or when no sign-in is pending (a return the app already used, or one from
/// an earlier launch): such an address is ignored. Otherwise the pending sign-in is used once. A `state` that does not match
/// is `StateMismatch`, a denial from the backend is `SignInDenied` with its code, and a code is exchanged for the session.
pub async fn finish_return(
    api: &ApiClient,
    dir: &Path,
    url: &str,
) -> Option<Result<SessionResponse, ClientError>> {
    let returned = parse_return(return_query(url)?)?;
    let pending = match take_pending(dir) {
        Ok(Some(pending)) => pending,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let returned_state = match &returned {
        WebReturn::Code { state, .. } | WebReturn::Error { state, .. } => state,
    };
    let pending = match matching_pending(Some(pending), returned_state) {
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

/// Reads and removes the pending sign-in. `Ok(None)` when there is none.
fn take_pending(dir: &Path) -> Result<Option<PendingWebSignIn>, ClientError> {
    let path = dir.join(PENDING_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ClientError::Storage(e.to_string())),
    };
    let _ = std::fs::remove_file(&path);
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| ClientError::Storage(format!("pending sign-in is unreadable: {e}")))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use super::*;
    use crate::testing::{ScriptedTransport, json_response};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rimelight-signin-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn session_body() -> String {
        r#"{"session_token":"tok_m","expires_at":"2026-12-15T10:00:00Z","user":{"id":"u1","email":"o@example.com","display_name":"Owner","auth0_sub":null}}"#
            .to_string()
    }

    fn param(url: &str, name: &str) -> String {
        let query = url.split_once('?').map_or("", |(_, q)| q);
        crate::api::parse_query(query)
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
            .expect("parameter")
    }

    #[test]
    fn only_the_return_address_is_a_return() {
        assert_eq!(return_query("rimelight-space://auth"), Some(""));
        assert_eq!(
            return_query("rimelight-space://auth?code=c&state=s"),
            Some("code=c&state=s")
        );
        assert_eq!(return_query("rimelight-space://other?code=c"), None);
        assert_eq!(return_query("rimelight-space://authx?code=c"), None);
        assert_eq!(return_query("https://example.com/?code=c"), None);
    }

    #[test]
    fn sign_in_goes_out_with_the_return_address_and_comes_back_once() {
        let dir = test_dir("round-trip");
        let transport = Arc::new(ScriptedTransport::new(vec![json_response(
            200,
            &session_body(),
        )]));
        let api = ApiClient::with_transport("http://10.0.2.2:8787", transport.clone());
        let opened = RefCell::new(None::<String>);
        begin(&api, &dir, &|url: &str| {
            *opened.borrow_mut() = Some(url.to_string());
            Ok(())
        })
        .expect("begin");

        let login = opened.borrow().clone().expect("opened");
        assert_eq!(param(&login, "redirect_uri"), RETURN_ADDRESS);
        let challenge = param(&login, "code_challenge");
        let state = param(&login, "state");

        let back = format!("{RETURN_ADDRESS}?code=auth-code-m&state={state}");
        let session = pollster::block_on(finish_return(&api, &dir, &back))
            .expect("a sign-in return")
            .expect("session");
        assert_eq!(session.session_token, "tok_m");

        let sent = transport.requests();
        let body: ExchangeRequest =
            serde_json::from_str(sent[0].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(body.code, "auth-code-m");
        assert_eq!(pkce::challenge_for(&body.code_verifier), challenge);

        assert!(
            pollster::block_on(finish_return(&api, &dir, &back)).is_none(),
            "a second use of the same return is ignored"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_return_with_the_wrong_state_is_refused_before_the_exchange() {
        let dir = test_dir("forged");
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let api = ApiClient::with_transport("http://10.0.2.2:8787", transport.clone());
        begin(&api, &dir, &|_| Ok(())).expect("begin");

        let back = format!("{RETURN_ADDRESS}?code=c&state=forged");
        let result = pollster::block_on(finish_return(&api, &dir, &back));
        assert_eq!(result, Some(Err(ClientError::StateMismatch)));
        assert!(transport.requests().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_denial_is_reported_with_its_code() {
        let dir = test_dir("denied");
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let api = ApiClient::with_transport("http://10.0.2.2:8787", transport.clone());
        let state = RefCell::new(String::new());
        begin(&api, &dir, &|url: &str| {
            *state.borrow_mut() = param(url, "state");
            Ok(())
        })
        .expect("begin");

        let back = format!(
            "{RETURN_ADDRESS}?error=not_employee&state={}",
            state.borrow()
        );
        let result = pollster::block_on(finish_return(&api, &dir, &back));
        assert_eq!(
            result,
            Some(Err(ClientError::SignInDenied("not_employee".to_string())))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_return_with_nothing_pending_is_ignored() {
        let dir = test_dir("stale");
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let api = ApiClient::with_transport("http://10.0.2.2:8787", transport.clone());
        let back = format!("{RETURN_ADDRESS}?code=c&state=s");
        assert!(pollster::block_on(finish_return(&api, &dir, &back)).is_none());
        assert!(
            pollster::block_on(finish_return(&api, &dir, "https://x/?code=c&state=s")).is_none()
        );
        assert!(transport.requests().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_open_removes_the_pending_sign_in() {
        let dir = test_dir("open-fails");
        let api = ApiClient::with_transport(
            "http://10.0.2.2:8787",
            Arc::new(ScriptedTransport::new(vec![])),
        );
        let result = begin(&api, &dir, &|_| {
            Err(ClientError::Browser("no browser".into()))
        });
        assert_eq!(result, Err(ClientError::Browser("no browser".into())));
        assert!(!dir.join(PENDING_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
