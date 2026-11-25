//! Auth contract types for the sign-in flow (decision 0014).
//!
//! `ExchangeRequest` is sent by the app to `/v1/auth/exchange` with the
//! one-time code and PKCE verifier. `SessionResponse` is the reply and carries
//! the session token the app stores in the OS credential store.

use serde::{Deserialize, Serialize};

/// Body of `POST /v1/auth/exchange`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
    pub code_verifier: String,
}

/// Reply to a successful exchange and to `GET /v1/me` session checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResponse {
    pub session_token: String,
    /// RFC 3339 timestamp.
    pub expires_at: String,
    pub user: CurrentUser,
}

/// The signed-in employee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentUser {
    /// UUID in canonical text form.
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub auth0_sub: Option<String>,
}

/// Error body returned by auth endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, value, "round trip changed the value: {json}");
    }

    fn sample_user(auth0_sub: Option<&str>) -> CurrentUser {
        CurrentUser {
            id: "0190a6d2-7c1e-7b3a-9f4e-2d5c8a1b6e10".to_string(),
            email: "owner@example.com".to_string(),
            display_name: "Owner".to_string(),
            auth0_sub: auth0_sub.map(str::to_string),
        }
    }

    #[test]
    fn exchange_request_round_trips() {
        round_trip(&ExchangeRequest {
            code: "abc123".to_string(),
            code_verifier: "verifier-value".to_string(),
        });
    }

    #[test]
    fn session_response_round_trips() {
        round_trip(&SessionResponse {
            session_token: "tok_example".to_string(),
            expires_at: "2026-12-15T10:00:00Z".to_string(),
            user: sample_user(Some("auth0|123")),
        });
    }

    #[test]
    fn current_user_round_trips_with_and_without_auth0_sub() {
        round_trip(&sample_user(Some("auth0|123")));
        round_trip(&sample_user(None));
    }

    #[test]
    fn current_user_null_auth0_sub_deserializes_to_none() {
        let json = r#"{"id":"x","email":"e","display_name":"d","auth0_sub":null}"#;
        let user: CurrentUser = serde_json::from_str(json).expect("deserialize");
        assert_eq!(user.auth0_sub, None);
    }

    #[test]
    fn api_error_round_trips() {
        round_trip(&ApiError {
            error: "invalid_code".to_string(),
            message: "The sign-in code has expired.".to_string(),
        });
    }
}
