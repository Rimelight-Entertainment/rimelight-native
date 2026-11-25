//! Obtains an Access token for the desktop app, by the same login `cloudflared access login` uses (token/token.go and
//! token/transfer.go in cloudflare/cloudflared, read 2026-11-23).
//!
//! The client makes a Curve25519 key pair. It opens the browser on the Access CLI login page of the application, with the
//! public key in the URL. When the user finishes, Access stores the token for that public key at the transfer service.
//! The client polls the transfer service with the same public key and gets the token encrypted to it (a NaCl box, so only
//! this client can read it). [`cli_login`] runs on a worker thread and blocks until the token arrives or the timeout passes.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use crypto_box::aead::Aead;
use crypto_box::{Nonce, PublicKey, SalsaBox, SecretKey};

use crate::auth::{
    ACCESS_APP_URL, ACCESS_AUD, APP_ORIGIN, AuthError, TRANSFER_ORIGIN, percent_encode,
};

/// How long the user has to finish the login in the browser.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// The pause between two polls that were answered at once (a non-200 or an empty body). The transfer service also holds a
/// poll open for a while on its own.
const POLL_EVERY: Duration = Duration::from_millis(1500);

/// The Access CLI login page, relative to the app origin (cloudflared's `cdn-cgi/access/cli`).
const LOGIN_PATH: &str = "/cdn-cgi/access/cli";
/// The nonce at the start of a NaCl box.
const NONCE_LEN: usize = 24;

/// The login URL for a public key (base64url with padding, as cloudflared encodes it). `redirect_url` is the application
/// URL carrying the same `aud` and `token`, which is what Access checks it against.
fn login_url(public_key: &str) -> String {
    let redirect = format!(
        "{ACCESS_APP_URL}?aud={}&token={}",
        percent_encode(ACCESS_AUD),
        percent_encode(public_key)
    );
    format!(
        "{APP_ORIGIN}{LOGIN_PATH}?aud={}&edge_token_transfer=true&redirect_url={}&send_org_token=true&token={}",
        percent_encode(ACCESS_AUD),
        percent_encode(&redirect),
        percent_encode(public_key)
    )
}

/// The transfer poll URL for a public key.
fn poll_url(public_key: &str) -> String {
    format!("{TRANSFER_ORIGIN}/transfer/{}", percent_encode(public_key))
}

/// A fresh key pair. The secret stays with the caller; the public key (base64url) is what the URLs carry.
fn new_key_pair() -> Result<(SecretKey, String), AuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AuthError::Unavailable(e.to_string()))?;
    let secret = SecretKey::from(bytes);
    let public = URL_SAFE.encode(secret.public_key().as_bytes());
    Ok((secret, public))
}

/// Opens the transfer service's answer. `body` is base64 of nonce (24 bytes) and box; `service_key` is the
/// `service-public-key` header, the service's public key in base64url. The plaintext is JSON with an `app_token`.
fn open_transfer(secret: &SecretKey, service_key: &str, body: &str) -> Result<String, AuthError> {
    let service = URL_SAFE_NO_PAD
        .decode(service_key.trim().trim_end_matches('='))
        .map_err(|e| AuthError::Protocol(format!("service key: {e}")))?;
    let service: [u8; 32] = service
        .try_into()
        .map_err(|_| AuthError::Protocol("service key is not 32 bytes".to_owned()))?;
    let data = STANDARD
        .decode(body.trim())
        .map_err(|e| AuthError::Protocol(format!("transfer body: {e}")))?;
    if data.len() <= NONCE_LEN {
        return Err(AuthError::Protocol("transfer body is too short".to_owned()));
    }
    let (nonce, boxed) = data.split_at(NONCE_LEN);
    let plain = SalsaBox::new(&PublicKey::from(service), secret)
        .decrypt(Nonce::from_slice(nonce), boxed)
        .map_err(|_| AuthError::Protocol("the transfer could not be decrypted".to_owned()))?;
    let value: serde_json::Value = serde_json::from_slice(&plain)
        .map_err(|e| AuthError::Protocol(format!("transfer json: {e}")))?;
    value
        .get("app_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| AuthError::Protocol("the transfer holds no app_token".to_owned()))
}

/// Runs the CLI login: opens the browser, polls for the token, and returns it. Blocks the calling thread; call it from a
/// worker thread. `open_browser` is injected so tests open nothing.
pub fn cli_login(open_browser: &dyn Fn(&str) -> Result<(), String>) -> Result<String, AuthError> {
    let (secret, public) = new_key_pair()?;
    open_browser(&login_url(&public)).map_err(AuthError::Browser)?;

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let url = poll_url(&public);
    loop {
        if let Some(token) = poll_once(&secret, &url)? {
            return Ok(token);
        }
        if Instant::now() >= deadline {
            return Err(AuthError::Timeout);
        }
        std::thread::sleep(POLL_EVERY);
    }
}

/// One poll. `Ok(Some(token))` when the transfer is ready, `Ok(None)` while it is not (a non-200 answer or an empty body).
/// A server error is a failure.
fn poll_once(secret: &SecretKey, url: &str) -> Result<Option<String>, AuthError> {
    let (tx, rx) = mpsc::channel();
    ehttp::fetch(ehttp::Request::get(url), move |result| {
        let _ = tx.send(result);
    });
    let response = rx
        .recv()
        .map_err(|e| AuthError::Protocol(e.to_string()))?
        .map_err(|e| AuthError::Unavailable(format!("network: {e}")))?;
    if response.status >= 500 {
        return Err(AuthError::Unavailable(format!(
            "the transfer service answered {}",
            response.status
        )));
    }
    if !response.ok {
        return Ok(None);
    }
    let body = String::from_utf8_lossy(&response.bytes).trim().to_owned();
    if body.is_empty() {
        return Ok(None);
    }
    let service_key = response.headers.get("service-public-key").ok_or_else(|| {
        AuthError::Protocol("the transfer has no service-public-key header".to_owned())
    })?;
    open_transfer(secret, service_key, &body).map(Some)
}

/// Opens `url` in the system browser. The `open` crate on desktop.
pub fn open_system_browser(url: &str) -> Result<(), String> {
    open::that(url).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_url_follows_cloudflared() {
        let url = login_url("KEY=");
        assert!(url.starts_with("https://idantity.me/cdn-cgi/access/cli?aud="));
        assert!(url.contains("&edge_token_transfer=true&"));
        assert!(url.contains("&send_org_token=true&"));
        assert!(url.ends_with("&token=KEY%3D"));
        // The redirect is the application URL (inside the Access app), not "/".
        assert!(url.contains("redirect_url=https%3A%2F%2Fidantity.me%2Fapi%2Fpersonal%3Faud%3D"));
        assert!(!url.contains("redirect_url=%2F&"));
    }

    #[test]
    fn the_poll_url_is_on_the_transfer_service() {
        assert_eq!(
            poll_url("a+b="),
            "https://login.cloudflareaccess.org/transfer/a%2Bb%3D"
        );
    }

    #[test]
    fn a_new_key_pair_has_a_32_byte_public_key() {
        let (_, public) = new_key_pair().expect("key pair");
        assert_eq!(URL_SAFE.decode(public).expect("base64url").len(), 32);
    }

    /// The service side of the transfer: encrypts `plain` from `service` to `client_public`, as the transfer service does.
    fn transfer_body(service: &SecretKey, client_public: &PublicKey, plain: &[u8]) -> String {
        let nonce = [7u8; NONCE_LEN];
        let boxed = SalsaBox::new(client_public, service)
            .encrypt(Nonce::from_slice(&nonce), plain)
            .expect("encrypt");
        let mut data = nonce.to_vec();
        data.extend(boxed);
        STANDARD.encode(data)
    }

    #[test]
    fn the_transfer_is_opened_with_the_client_secret() {
        let (client, _) = new_key_pair().expect("client");
        let (service, _) = new_key_pair().expect("service");
        let body = transfer_body(
            &service,
            &client.public_key(),
            br#"{"app_token":"the.jwt.token","org_token":"org"}"#,
        );
        let key = URL_SAFE.encode(service.public_key().as_bytes());
        assert_eq!(
            open_transfer(&client, &key, &body),
            Ok("the.jwt.token".to_owned())
        );
    }

    #[test]
    fn a_transfer_for_another_key_is_refused() {
        let (client, _) = new_key_pair().expect("client");
        let (other, _) = new_key_pair().expect("other");
        let (service, _) = new_key_pair().expect("service");
        let body = transfer_body(&service, &other.public_key(), br#"{"app_token":"x"}"#);
        let key = URL_SAFE.encode(service.public_key().as_bytes());
        assert!(matches!(
            open_transfer(&client, &key, &body),
            Err(AuthError::Protocol(_))
        ));
    }

    #[test]
    fn a_transfer_without_an_app_token_is_refused() {
        let (client, _) = new_key_pair().expect("client");
        let (service, _) = new_key_pair().expect("service");
        let body = transfer_body(&service, &client.public_key(), br#"{"org_token":"org"}"#);
        let key = URL_SAFE.encode(service.public_key().as_bytes());
        assert!(matches!(
            open_transfer(&client, &key, &body),
            Err(AuthError::Protocol(_))
        ));
    }
}
