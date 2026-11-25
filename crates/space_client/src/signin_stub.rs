//! Sign-in stub for iOS and for Android without a data folder. Every call returns `ClientError::Unsupported`.
//!
//! Android sign-in is `signin_mobile` (SA.7). The web is `web_signin` (SA.6). iOS needs ASWebAuthenticationSession, which
//! needs Objective-C bindings (decision 0018, proposed).

use space_proto::auth::SessionResponse;

use crate::api::ApiClient;
use crate::error::ClientError;

pub async fn sign_in(_api: &ApiClient) -> Result<SessionResponse, ClientError> {
    Err(ClientError::Unsupported(
        "sign-in on iOS (slice SA, decision 0018)",
    ))
}
