//! Signed-in state and the start-up restore.

use space_proto::auth::CurrentUser;

use crate::api::ApiClient;
use crate::error::ClientError;
use crate::session_store::SessionStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    SignedOut,
    SigningIn,
    SignedIn(CurrentUser),
    /// A stored session exists but the server could not be reached. Rimelight Space works only online (SA.8), so the
    /// shell does not open: the UI shows "Can't reach Rimelight Space". The stored session is kept.
    Unreachable,
}

/// Start-up check. Loads the stored session and asks `/v1/me` who it is.
///
/// - No stored session, or the store cannot be read: `SignedOut`.
/// - `/v1/me` accepts the token: `SignedIn`.
/// - `/v1/me` answers 401: the stored session is cleared and `SignedOut`.
/// - Any other failure (network, server error): `Unreachable`. The stored session is kept.
pub async fn restore(api: &ApiClient, store: &dyn SessionStore) -> AuthState {
    let stored = match store.load() {
        Ok(Some(stored)) => stored,
        Ok(None) | Err(_) => return AuthState::SignedOut,
    };
    match api.me(&stored.session_token).await {
        Ok(user) => AuthState::SignedIn(user),
        Err(ClientError::Unauthorized) => {
            // A rejected session is worthless; drop it. A failed clear is
            // harmless here because the next start checks the server again.
            let _ = store.clear();
            AuthState::SignedOut
        }
        Err(_) => AuthState::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::api::HttpRequest;
    use crate::session_store::{MemorySessionStore, StoredSession};
    use crate::testing::{ScriptedTransport, json_response};

    const USER_JSON: &str =
        r#"{"id":"u1","email":"o@example.com","display_name":"Owner","auth0_sub":null}"#;

    fn owner() -> CurrentUser {
        CurrentUser {
            id: "u1".to_string(),
            email: "o@example.com".to_string(),
            display_name: "Owner".to_string(),
            auth0_sub: None,
        }
    }

    fn stored_store() -> MemorySessionStore {
        let store = MemorySessionStore::new();
        store
            .save(&StoredSession {
                session_token: "tok_1".to_string(),
                expires_at: "2026-12-15T10:00:00Z".to_string(),
                user: owner(),
            })
            .expect("save");
        store
    }

    fn api(
        replies: Vec<Result<crate::api::HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        (
            ApiClient::with_transport("http://localhost:8787", transport.clone()),
            transport,
        )
    }

    fn sent(transport: &ScriptedTransport) -> Vec<HttpRequest> {
        transport.requests()
    }

    #[test]
    fn no_stored_session_is_signed_out_without_calling_server() {
        let (api, transport) = api(vec![]);
        let state = pollster::block_on(restore(&api, &MemorySessionStore::new()));
        assert_eq!(state, AuthState::SignedOut);
        assert!(sent(&transport).is_empty());
    }

    #[test]
    fn valid_session_is_signed_in() {
        let (api, transport) = api(vec![json_response(200, USER_JSON)]);
        let store = stored_store();
        let state = pollster::block_on(restore(&api, &store));
        assert_eq!(state, AuthState::SignedIn(owner()));
        assert_eq!(sent(&transport)[0].bearer.as_deref(), Some("tok_1"));
        assert!(store.load().expect("load").is_some(), "session kept");
    }

    #[test]
    fn rejected_session_is_signed_out_and_cleared() {
        let (api, _) = api(vec![json_response(401, "")]);
        let store = stored_store();
        let state = pollster::block_on(restore(&api, &store));
        assert_eq!(state, AuthState::SignedOut);
        assert_eq!(store.load(), Ok(None), "store cleared on 401");
    }

    #[test]
    fn network_failure_is_unreachable_and_keeps_session() {
        let (api, _) = api(vec![Err(ClientError::Network(
            "connection refused".to_string(),
        ))]);
        let store = stored_store();
        let state = pollster::block_on(restore(&api, &store));
        assert_eq!(state, AuthState::Unreachable);
        assert!(store.load().expect("load").is_some(), "session kept");
    }

    #[test]
    fn server_error_is_unreachable_not_signed_out() {
        let (api, _) = api(vec![json_response(503, "")]);
        let state = pollster::block_on(restore(&api, &stored_store()));
        assert_eq!(state, AuthState::Unreachable);
    }
}
