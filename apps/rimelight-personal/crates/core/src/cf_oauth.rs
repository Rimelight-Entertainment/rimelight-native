//! Cloudflare sign-in for setting up the owner's backend: OAuth 2.0 authorization code with PKCE, as a public client.
//!
//! The app opens the Cloudflare Allow page in the browser, catches the redirect on a loopback listener on
//! `127.0.0.1`, and exchanges the code for an access token. A desktop app cannot keep a client secret, so none is
//! used; PKCE protects the exchange. The client is registered in the Rimelight Cloudflare account
//! (`CLIENT_ID`), and its redirect URI is [`REDIRECT_URI`].
//!
//! The token is opaque (not a JWT) and Cloudflare refuses `offline_access` for this client, so there is no refresh
//! token: the session lasts [`StoredAccess`]'s `expires_at` (about an hour), then the owner signs in again.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ehttp::{Headers, Method, Request};
use sha2::{Digest, Sha256};

use crate::api::percent_encode;
use crate::auth::StoredAccess;

/// The public OAuth client "Rimelight Personal", owned by the Rimelight Cloudflare account.
pub const CLIENT_ID: &str = "00c4300e99277376ebaeeaaf87e11f2b";
/// The loopback port the app listens on for the redirect. It must match the client's registered redirect URI.
pub const CALLBACK_PORT: u16 = 53682;
pub const REDIRECT_URI: &str = "http://127.0.0.1:53682/callback";
pub const AUTHORIZE_URL: &str = "https://dash.cloudflare.com/oauth2/auth";
pub const TOKEN_URL: &str = "https://dash.cloudflare.com/oauth2/token";
pub const USERINFO_URL: &str = "https://dash.cloudflare.com/oauth2/userinfo";

/// The scopes the setup asks for: D1, Workers scripts and routes, R2, and Access applications and policies.
pub const SCOPES: [&str; 19] = [
    "d1.read",
    "d1.write",
    "workers-scripts.read",
    "workers-scripts.write",
    "workers-scripts.edit",
    "workers-scripts.metadata_read",
    "workers-routes.read",
    "workers-routes.write",
    "workers-r2.read",
    "workers-r2.write",
    "access.read",
    "access.write",
    "access-app.read",
    "access-app.write",
    "access-policy.read",
    "access-policy.write",
    "access-acct.read",
    "zone.read",
    "user-details.read",
];

/// How long the app waits for the owner to finish the browser step.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInError {
    /// The loopback listener could not start (for example the port is taken).
    Listener(String),
    /// The browser could not be opened.
    Browser(String),
    /// The owner did not finish in time.
    Timeout,
    /// Cloudflare sent back an error instead of a code (for example the owner declined).
    Denied { error: String, description: String },
    /// The redirect's `state` did not match the one this app sent.
    StateMismatch,
    /// The redirect had no code.
    MissingCode,
    /// The token request failed, or its answer was not understood.
    Token(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Listener(e) => write!(f, "could not listen for the sign-in redirect: {e}"),
            Self::Browser(e) => write!(f, "could not open the browser: {e}"),
            Self::Timeout => write!(f, "sign-in timed out; try again"),
            Self::Denied { error, description } => {
                write!(
                    f,
                    "Cloudflare did not approve the sign-in ({error}: {description})"
                )
            }
            Self::StateMismatch => write!(f, "sign-in was not started by this app; try again"),
            Self::MissingCode => write!(f, "Cloudflare did not send a sign-in code"),
            Self::Token(e) => write!(f, "could not get the Cloudflare token: {e}"),
        }
    }
}

/// The values one sign-in attempt uses. The verifier stays on this device; only its hash goes to Cloudflare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

/// Random PKCE values for one attempt.
pub fn new_pkce() -> Result<Pkce, SignInError> {
    let verifier = random_url_safe(32)?;
    let challenge = challenge_for(&verifier);
    let state = random_url_safe(16)?;
    Ok(Pkce {
        verifier,
        challenge,
        state,
    })
}

fn random_url_safe(bytes: usize) -> Result<String, SignInError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| SignInError::Listener(format!("no random source: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The S256 code challenge for a verifier (RFC 7636): base64url(SHA-256(verifier)), without padding.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The Allow page address for one attempt.
pub fn authorize_url(pkce: &Pkce) -> String {
    format!(
        "{AUTHORIZE_URL}?response_type=code&client_id={CLIENT_ID}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(REDIRECT_URI),
        percent_encode(&SCOPES.join(" ")),
        percent_encode(&pkce.state),
        percent_encode(&pkce.challenge),
    )
}

/// Decodes `%XX` escapes and `+` (as a space) in a query component.
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `key=value` pairs of a query string, decoded.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// Checks the redirect's query and returns the authorization code. Pure, so it can be tested without a browser.
pub fn code_from_redirect(query: &str, expected_state: &str) -> Result<String, SignInError> {
    let pairs = parse_query(query);
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    if let Some(error) = get("error") {
        return Err(SignInError::Denied {
            error,
            description: get("error_description").unwrap_or_default(),
        });
    }
    if get("state").as_deref() != Some(expected_state) {
        return Err(SignInError::StateMismatch);
    }
    get("code")
        .filter(|c| !c.is_empty())
        .ok_or(SignInError::MissingCode)
}

/// Runs one full sign-in: opens the Allow page with `open_browser`, waits for the redirect on the loopback listener,
/// exchanges the code for a token, and returns the session to store.
pub fn sign_in(
    open_browser: impl FnOnce(&str) -> Result<(), String>,
) -> Result<StoredAccess, SignInError> {
    let pkce = new_pkce()?;
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .map_err(|e| SignInError::Listener(e.to_string()))?;

    // The request line (`GET /callback?... HTTP/1.1`) is read on a thread so the wait can time out.
    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let result = listener
            .accept()
            .map_err(|e| e.to_string())
            .and_then(|(mut stream, _)| {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or_default().to_owned();
                let body = "<!doctype html><meta charset=utf-8><title>Rimelight Personal</title>\
                            <p>Signed in. You can close this tab and return to Rimelight Personal.</p>";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                Ok(line)
            });
        let _ = tx.send(result);
    });

    open_browser(&authorize_url(&pkce)).map_err(SignInError::Browser)?;

    let line = match rx.recv_timeout(SIGN_IN_TIMEOUT) {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => return Err(SignInError::Listener(e)),
        Err(_) => return Err(SignInError::Timeout),
    };
    // "GET /callback?code=...&state=... HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or_default();
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let code = code_from_redirect(query, &pkce.state)?;

    exchange_code(&code, &pkce.verifier)
}

/// Swaps the authorization code for an access token. The token is returned, never logged.
fn exchange_code(code: &str, verifier: &str) -> Result<StoredAccess, SignInError> {
    let form = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={CLIENT_ID}&code_verifier={}",
        percent_encode(code),
        percent_encode(REDIRECT_URI),
        percent_encode(verifier),
    );
    let mut request = Request::new(
        Method::POST,
        TOKEN_URL.to_owned(),
        Headers::new(&[("Content-Type", "application/x-www-form-urlencoded")]),
    );
    request.body = form.into_bytes();

    let (tx, rx) = mpsc::channel();
    ehttp::fetch(request, move |result| {
        let _ = tx.send(result);
    });
    let response = rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| SignInError::Token("no answer from Cloudflare".to_owned()))?
        .map_err(|e| SignInError::Token(e.to_string()))?;
    if !response.ok {
        return Err(SignInError::Token(format!("HTTP {}", response.status)));
    }
    stored_from_token_response(&response.bytes, now_unix())
}

/// Reads the access token and its lifetime from the token endpoint's JSON answer. Pure, so it can be tested.
pub fn stored_from_token_response(bytes: &[u8], now: u64) -> Result<StoredAccess, SignInError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| SignInError::Token(format!("bad answer: {e}")))?;
    let token = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| SignInError::Token("no access token in the answer".to_owned()))?;
    let lifetime = value
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);
    Ok(StoredAccess {
        token: token.to_owned(),
        expires_at: now + lifetime,
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_authorize_url_asks_for_pkce_and_this_client() {
        let pkce = Pkce {
            verifier: "v".repeat(43),
            challenge: "chal".to_owned(),
            state: "state-value-1234".to_owned(),
        };
        let url = authorize_url(&pkce);
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=state-value-1234"));
        assert!(!url.contains("offline_access"));
    }

    #[test]
    fn percent_decoding_handles_escapes_and_plus() {
        assert_eq!(percent_decode("a%2Bb+c%3D"), "a+b c=");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn a_good_redirect_gives_the_code() {
        let code = code_from_redirect("code=abc123&scope=d1.read&state=s1", "s1").expect("code");
        assert_eq!(code, "abc123");
    }

    #[test]
    fn a_redirect_with_the_wrong_state_is_refused() {
        assert_eq!(
            code_from_redirect("code=abc&state=other", "s1"),
            Err(SignInError::StateMismatch)
        );
    }

    #[test]
    fn a_declined_sign_in_reports_the_reason() {
        let err = code_from_redirect("error=access_denied&error_description=no&state=s1", "s1")
            .unwrap_err();
        assert_eq!(
            err,
            SignInError::Denied {
                error: "access_denied".to_owned(),
                description: "no".to_owned()
            }
        );
    }

    #[test]
    fn a_redirect_without_a_code_is_refused() {
        assert_eq!(
            code_from_redirect("state=s1", "s1"),
            Err(SignInError::MissingCode)
        );
    }

    #[test]
    fn the_token_answer_gives_the_token_and_its_lifetime() {
        let stored =
            stored_from_token_response(br#"{"access_token":"cfoat_x","expires_in":3599}"#, 1000)
                .unwrap();
        assert_eq!(stored.token, "cfoat_x");
        assert_eq!(stored.expires_at, 4599);
    }

    #[test]
    fn a_token_answer_without_a_token_is_an_error() {
        assert!(stored_from_token_response(br#"{"expires_in":3599}"#, 1000).is_err());
    }
}
