//! Desktop sign-in: system browser plus a one-shot loopback callback.
//!
//! Flow: bind `127.0.0.1:0`, open `/v1/auth/login` in the system browser with
//! `redirect_uri=http://127.0.0.1:<port>/callback`, accept one callback that
//! carries `code` and `state`, check `state`, then exchange the code with the
//! PKCE verifier.
//!
//! The callback wait blocks the calling thread (up to the timeout). Call
//! [`sign_in`] from a worker thread or a blocking-capable executor, never from
//! the UI thread.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use space_proto::auth::{ExchangeRequest, SessionResponse};

use crate::api::{ApiClient, parse_query};
use crate::error::ClientError;
use crate::pkce;

/// How long the user has to finish signing in in the browser.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

const CALLBACK_PATH: &str = "/callback";
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEAD_BYTES: usize = 8192;

const SUCCESS_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Rimelight Space</title></head><body><p>You are signed in to Rimelight Space. You can close this window.</p></body></html>";
const FAILED_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Rimelight Space</title></head><body><p>Sign-in was not completed. Return to Rimelight Space and try again.</p></body></html>";

/// Opens a URL in the user's browser. Injectable so tests open nothing.
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), ClientError>;
}

/// Opens the URL with the platform's default browser.
pub struct SystemBrowser;

impl BrowserOpener for SystemBrowser {
    fn open(&self, url: &str) -> Result<(), ClientError> {
        open::that(url).map_err(|e| ClientError::Browser(e.to_string()))
    }
}

/// Full sign-in with the system browser and the default timeout.
pub async fn sign_in(api: &ApiClient) -> Result<SessionResponse, ClientError> {
    sign_in_with(api, &SystemBrowser, SIGN_IN_TIMEOUT).await
}

/// Full sign-in with an injected browser and timeout.
pub async fn sign_in_with(
    api: &ApiClient,
    browser: &dyn BrowserOpener,
    timeout: Duration,
) -> Result<SessionResponse, ClientError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
    let port = listener.local_addr().map_err(io_error)?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

    let pkce = pkce::new_pair()?;
    let state = pkce::new_state()?;
    let login_url = api.login_url(&redirect_uri, &pkce.challenge, &state);
    browser.open(&login_url)?;

    let code = accept_callback(&listener, &state, timeout)?;
    drop(listener);

    api.exchange(&ExchangeRequest {
        code,
        code_verifier: pkce.verifier,
    })
    .await
}

fn io_error(e: std::io::Error) -> ClientError {
    ClientError::Io(e.to_string())
}

/// Waits for one callback with a matching `state` and returns its `code`.
///
/// Requests for other paths, or without a `state`, get a 404 and are ignored.
/// A `state` that does not match fails the sign-in with `StateMismatch`.
pub(crate) fn accept_callback(
    listener: &TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, ClientError> {
    listener.set_nonblocking(true).map_err(io_error)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(code) = handle_connection(stream, expected_state)? {
                    return Ok(code);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(ClientError::SignInTimeout);
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(e) => return Err(io_error(e)),
        }
    }
}

/// Reads one request. `Ok(Some(code))` on the callback, `Ok(None)` for a
/// request to ignore, `Err` on a failed sign-in.
fn handle_connection(
    mut stream: TcpStream,
    expected_state: &str,
) -> Result<Option<String>, ClientError> {
    stream.set_nonblocking(false).map_err(io_error)?;
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(io_error)?;

    let Some(head) = read_head(&mut stream) else {
        return Ok(None);
    };
    let target = head.lines().next().and_then(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("GET"), Some(target)) => Some(target.to_string()),
            _ => None,
        }
    });
    let Some(target) = target else {
        respond(&mut stream, "400 Bad Request", FAILED_PAGE);
        return Ok(None);
    };

    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    if path != CALLBACK_PATH {
        respond(&mut stream, "404 Not Found", FAILED_PAGE);
        return Ok(None);
    }

    let params = parse_query(query);
    let get = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };

    let Some(state) = get("state") else {
        respond(&mut stream, "404 Not Found", FAILED_PAGE);
        return Ok(None);
    };
    if state != expected_state {
        respond(&mut stream, "400 Bad Request", FAILED_PAGE);
        return Err(ClientError::StateMismatch);
    }
    if let Some(error) = get("error") {
        respond(&mut stream, "400 Bad Request", FAILED_PAGE);
        return Err(ClientError::SignInDenied(error.to_string()));
    }
    match get("code") {
        Some(code) if !code.is_empty() => {
            respond(&mut stream, "200 OK", SUCCESS_PAGE);
            Ok(Some(code.to_string()))
        }
        _ => {
            respond(&mut stream, "400 Bad Request", FAILED_PAGE);
            Err(ClientError::Protocol("callback had no code".to_string()))
        }
    }
}

/// Reads until the blank line that ends the request head (or a size cap).
fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < MAX_HEAD_BYTES && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::testing::{ScriptedTransport, json_response};

    /// Sends one raw HTTP request to the listener and returns the full reply.
    fn send_raw(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.write_all(request.as_bytes()).expect("write");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).expect("read");
        reply
    }

    fn bound() -> (TcpListener, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        (listener, port)
    }

    #[test]
    fn callback_returns_code_when_state_matches() {
        let (listener, port) = bound();
        let client = thread::spawn(move || {
            send_raw(
                port,
                "GET /callback?code=abc123&state=st-1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
        });
        let code = accept_callback(&listener, "st-1", Duration::from_secs(5)).expect("code");
        assert_eq!(code, "abc123");
        let reply = client.join().expect("client");
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.contains("You can close this window"));
    }

    #[test]
    fn callback_with_wrong_state_is_rejected() {
        let (listener, port) = bound();
        let client = thread::spawn(move || {
            send_raw(
                port,
                "GET /callback?code=abc123&state=attacker HTTP/1.1\r\nHost: x\r\n\r\n",
            )
        });
        let result = accept_callback(&listener, "st-1", Duration::from_secs(5));
        assert_eq!(result, Err(ClientError::StateMismatch));
        assert!(client.join().expect("client").starts_with("HTTP/1.1 400"));
    }

    #[test]
    fn unrelated_requests_are_ignored_until_the_callback() {
        let (listener, port) = bound();
        let client = thread::spawn(move || {
            let favicon = send_raw(port, "GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n");
            assert!(favicon.starts_with("HTTP/1.1 404"));
            send_raw(
                port,
                "GET /callback?code=later&state=st-2 HTTP/1.1\r\nHost: x\r\n\r\n",
            );
        });
        let code = accept_callback(&listener, "st-2", Duration::from_secs(5)).expect("code");
        assert_eq!(code, "later");
        client.join().expect("client");
    }

    #[test]
    fn provider_denial_is_reported() {
        let (listener, port) = bound();
        let client = thread::spawn(move || {
            send_raw(
                port,
                "GET /callback?error=access_denied&state=st-3 HTTP/1.1\r\nHost: x\r\n\r\n",
            )
        });
        let result = accept_callback(&listener, "st-3", Duration::from_secs(5));
        assert_eq!(
            result,
            Err(ClientError::SignInDenied("access_denied".to_string()))
        );
        client.join().expect("client");
    }

    #[test]
    fn backend_failure_codes_are_reported_as_the_code() {
        for code in [
            "not_provisioned",
            "not_employee",
            "access_denied",
            "server_error",
        ] {
            let (listener, port) = bound();
            let target = format!("/callback?error={code}&state=st-5");
            let client = thread::spawn(move || {
                send_raw(port, &format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n"))
            });
            let result = accept_callback(&listener, "st-5", Duration::from_secs(5));
            assert_eq!(result, Err(ClientError::SignInDenied(code.to_string())));
            client.join().expect("client");
        }
    }

    #[test]
    fn no_callback_times_out() {
        let (listener, _port) = bound();
        let result = accept_callback(&listener, "st-4", Duration::from_millis(150));
        assert_eq!(result, Err(ClientError::SignInTimeout));
    }

    /// Browser double: reads `redirect_uri` and `state` from the login URL and
    /// plays the callback from a thread (the flow is waiting on this thread).
    struct CallbackBrowser {
        seen_url: Mutex<Option<String>>,
        state_override: Option<&'static str>,
    }

    impl BrowserOpener for CallbackBrowser {
        fn open(&self, url: &str) -> Result<(), ClientError> {
            *self.seen_url.lock().expect("lock") = Some(url.to_string());
            let query = url.split_once('?').map_or("", |(_, q)| q);
            let params = parse_query(query);
            let value = |name: &str| {
                params
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let redirect = value("redirect_uri");
            let port: u16 = redirect
                .strip_prefix("http://127.0.0.1:")
                .and_then(|rest| rest.split('/').next())
                .and_then(|p| p.parse().ok())
                .expect("redirect_uri has a loopback port");
            let state = self
                .state_override
                .map_or_else(|| value("state"), str::to_string);
            thread::spawn(move || {
                let request = format!(
                    "GET /callback?code=auth-code-1&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
                );
                send_raw(port, &request);
            });
            Ok(())
        }
    }

    fn session_body() -> String {
        r#"{"session_token":"tok_1","expires_at":"2026-12-15T10:00:00Z","user":{"id":"u1","email":"o@example.com","display_name":"Owner","auth0_sub":null}}"#
            .to_string()
    }

    #[test]
    fn full_sign_in_exchanges_code_with_matching_verifier() {
        let transport = Arc::new(ScriptedTransport::new(vec![json_response(
            200,
            &session_body(),
        )]));
        let api = ApiClient::with_transport("http://localhost:8787", transport.clone());
        let browser = CallbackBrowser {
            seen_url: Mutex::new(None),
            state_override: None,
        };

        let session = pollster::block_on(sign_in_with(&api, &browser, Duration::from_secs(5)))
            .expect("sign in");
        assert_eq!(session.session_token, "tok_1");

        let login = browser.seen_url.lock().expect("lock").clone().expect("url");
        let challenge = parse_query(login.split_once('?').expect("query").1)
            .into_iter()
            .find(|(k, _)| k == "code_challenge")
            .map(|(_, v)| v)
            .expect("challenge");

        let sent = transport.requests();
        assert_eq!(sent.len(), 1);
        let body: ExchangeRequest =
            serde_json::from_str(sent[0].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(body.code, "auth-code-1");
        assert_eq!(pkce::challenge_for(&body.code_verifier), challenge);
    }

    #[test]
    fn state_mismatch_stops_sign_in_before_exchange() {
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let api = ApiClient::with_transport("http://localhost:8787", transport.clone());
        let browser = CallbackBrowser {
            seen_url: Mutex::new(None),
            state_override: Some("forged"),
        };

        let result = pollster::block_on(sign_in_with(&api, &browser, Duration::from_secs(5)));
        assert_eq!(result.err(), Some(ClientError::StateMismatch));
        assert!(
            transport.requests().is_empty(),
            "no exchange after a bad state"
        );
    }
}
