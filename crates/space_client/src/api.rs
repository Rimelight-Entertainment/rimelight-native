//! HTTP client for the Rimelight Space auth endpoints.
//!
//! The wire is behind [`Transport`] so tests can fake it. On native targets
//! [`ApiClient::new`] uses reqwest with rustls.

use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use space_proto::auth::{ApiError, CurrentUser, ExchangeRequest, SessionResponse};

use crate::error::ClientError;

#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The browser's fetch futures hold JS values and are not `Send`. A wasm32 page runs on one thread, so they need not be.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

/// A binary request body with its media type, for the avatar upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBody {
    pub content_type: String,
    pub bytes: Vec<u8>,
    /// Sent as `X-File-Name` (already percent-encoded) when set. The attachment upload uses it.
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub bearer: Option<String>,
    pub json_body: Option<String>,
    /// Set instead of `json_body` for a binary upload.
    pub raw_body: Option<RawBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The body as text (lossy for binary bodies). JSON replies are parsed from this.
    pub body: String,
    /// The body as sent. The avatar GET (SP.3b) reads the image from here.
    pub bytes: Vec<u8>,
}

/// Sends one HTTP request. Implemented by reqwest on native and faked in tests.
pub trait Transport: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ClientError>>;
}

#[cfg(not(target_arch = "wasm32"))]
struct ReqwestTransport {
    client: reqwest::Client,
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ClientError>> {
        Box::pin(async move {
            let mut builder = match request.method {
                Method::Get => self.client.get(&request.url),
                Method::Post => self.client.post(&request.url),
                Method::Patch => self.client.patch(&request.url),
                Method::Put => self.client.put(&request.url),
                Method::Delete => self.client.delete(&request.url),
            };
            if let Some(token) = &request.bearer {
                builder = builder.bearer_auth(token);
            }
            if let Some(body) = request.json_body {
                builder = builder
                    .header("content-type", "application/json")
                    .body(body);
            }
            if let Some(raw) = request.raw_body {
                builder = builder.header("content-type", raw.content_type);
                if let Some(name) = raw.file_name {
                    builder = builder.header("X-File-Name", name);
                }
                builder = builder.body(raw.bytes);
            }
            let response = builder
                .send()
                .await
                .map_err(|e| ClientError::Network(e.to_string()))?;
            let status = response.status().as_u16();
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ClientError::Network(e.to_string()))?
                .to_vec();
            let body = String::from_utf8_lossy(&bytes).into_owned();
            Ok(HttpResponse {
                status,
                body,
                bytes,
            })
        })
    }
}

/// Browser transport over `fetch` (SA.6). The page's origin must be an allowed web app origin on the backend (`WEB_APP_ORIGINS`).
#[cfg(target_arch = "wasm32")]
struct WebTransport;

#[cfg(target_arch = "wasm32")]
impl Transport for WebTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ClientError>> {
        Box::pin(async move {
            let network = |e: gloo_net::Error| ClientError::Network(e.to_string());
            let builder = match request.method {
                Method::Get => gloo_net::http::Request::get(&request.url),
                Method::Post => gloo_net::http::Request::post(&request.url),
                Method::Patch => gloo_net::http::Request::patch(&request.url),
                Method::Put => gloo_net::http::Request::put(&request.url),
                Method::Delete => gloo_net::http::Request::delete(&request.url),
            };
            let builder = match &request.bearer {
                Some(token) => builder.header("Authorization", &format!("Bearer {token}")),
                None => builder,
            };
            let http = match (request.json_body, request.raw_body) {
                (Some(body), _) => builder
                    .header("Content-Type", "application/json")
                    .body(body)
                    .map_err(network)?,
                (None, Some(raw)) => {
                    // Binary bodies go to fetch as a Uint8Array.
                    let array = js_sys::Uint8Array::from(raw.bytes.as_slice());
                    let mut builder = builder.header("Content-Type", &raw.content_type);
                    if let Some(name) = &raw.file_name {
                        builder = builder.header("X-File-Name", name);
                    }
                    builder.body(array).map_err(network)?
                }
                (None, None) => builder.build().map_err(network)?,
            };
            let response = http.send().await.map_err(network)?;
            let status = response.status();
            let bytes = response.binary().await.map_err(network)?;
            let body = String::from_utf8_lossy(&bytes).into_owned();
            Ok(HttpResponse {
                status,
                body,
                bytes,
            })
        })
    }
}

/// The transport for this platform: reqwest on native, fetch in the browser.
#[cfg(not(target_arch = "wasm32"))]
fn default_transport() -> Arc<dyn Transport> {
    Arc::new(ReqwestTransport {
        client: reqwest::Client::new(),
    })
}

#[cfg(target_arch = "wasm32")]
fn default_transport() -> Arc<dyn Transport> {
    Arc::new(WebTransport)
}

/// Client for `/v1/auth/*` and `/v1/me`.
pub struct ApiClient {
    base_url: String,
    transport: Arc<dyn Transport>,
}

/// `GET /v1/me` is documented as a session check. Accept either the bare
/// user or the session envelope until the backend contract settles.
#[derive(Deserialize)]
#[serde(untagged)]
enum MeReply {
    User(CurrentUser),
    Session(SessionResponse),
}

impl ApiClient {
    /// Client over the platform transport: reqwest on native, fetch in the browser.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_transport(base_url, default_transport())
    }

    /// Client over any transport (used by tests).
    pub fn with_transport(base_url: impl Into<String>, transport: Arc<dyn Transport>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            transport,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Browser URL that starts the Auth0 login through the backend.
    pub fn login_url(&self, redirect_uri: &str, code_challenge: &str, state: &str) -> String {
        format!(
            "{}/v1/auth/login?redirect_uri={}&code_challenge={}&state={}",
            self.base_url,
            encode_query_value(redirect_uri),
            encode_query_value(code_challenge),
            encode_query_value(state),
        )
    }

    /// `POST /v1/auth/exchange`: trades the one-time code for a session.
    pub async fn exchange(
        &self,
        request: &ExchangeRequest,
    ) -> Result<SessionResponse, ClientError> {
        let body =
            serde_json::to_string(request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        let reply = self
            .send(Method::Post, "/v1/auth/exchange", None, Some(body))
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/me` with the session token.
    pub async fn me(&self, token: &str) -> Result<CurrentUser, ClientError> {
        let reply = self.send(Method::Get, "/v1/me", Some(token), None).await?;
        match parse::<MeReply>(&reply.body)? {
            MeReply::User(user) => Ok(user),
            MeReply::Session(session) => Ok(session.user),
        }
    }

    /// `POST /v1/auth/signout` with the session token.
    pub async fn signout(&self, token: &str) -> Result<(), ClientError> {
        self.send(Method::Post, "/v1/auth/signout", Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/health`.
    pub async fn health(&self) -> Result<(), ClientError> {
        self.send(Method::Get, "/v1/health", None, None)
            .await
            .map(|_| ())
    }

    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        json_body: Option<String>,
    ) -> Result<HttpResponse, ClientError> {
        let request = HttpRequest {
            method,
            url: format!("{}{path}", self.base_url),
            bearer: bearer.map(str::to_string),
            json_body,
            raw_body: None,
        };
        let response = self.transport.send(request).await?;
        check(response)
    }

    /// Sends a binary body (the avatar upload).
    pub(crate) async fn send_raw(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        raw_body: RawBody,
    ) -> Result<HttpResponse, ClientError> {
        let request = HttpRequest {
            method,
            url: format!("{}{path}", self.base_url),
            bearer: bearer.map(str::to_string),
            json_body: None,
            raw_body: Some(raw_body),
        };
        let response = self.transport.send(request).await?;
        check(response)
    }
}

/// Maps HTTP status to the client error model. 401 becomes `Unauthorized`.
fn check(response: HttpResponse) -> Result<HttpResponse, ClientError> {
    match response.status {
        200..=299 => Ok(response),
        401 => Err(ClientError::Unauthorized),
        400 if is_invalid_query(&response.body) => {
            let body: QueryError = serde_json::from_str(&response.body)
                .map_err(|e| ClientError::Protocol(e.to_string()))?;
            Err(ClientError::InvalidQuery {
                message: body.message,
                start: body.span.start,
                end: body.span.end,
            })
        }
        status => match serde_json::from_str::<ApiError>(&response.body) {
            Ok(api) => Err(ClientError::Api {
                status,
                error: api.error,
                message: api.message,
            }),
            Err(_) => Err(ClientError::Api {
                status,
                error: "http_error".to_string(),
                message: format!("HTTP {status}"),
            }),
        },
    }
}

/// A 400 `invalid_query` body: the usual error shape plus the span of the offending text (decision 0028).
#[derive(Deserialize)]
struct QueryError {
    message: String,
    span: QuerySpan,
}

#[derive(Deserialize)]
struct QuerySpan {
    start: usize,
    end: usize,
}

fn is_invalid_query(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.as_str())
                .map(|e| e == "invalid_query")
        })
        .unwrap_or(false)
}

pub(crate) fn parse<T: DeserializeOwned>(body: &str) -> Result<T, ClientError> {
    serde_json::from_str(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

/// Percent-encodes a query value (RFC 3986 unreserved characters pass through).
pub(crate) fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Decodes a query value: `+` is a space and `%XX` is a byte.
pub(crate) fn decode_query_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Splits `a=1&b=two` into decoded name/value pairs.
pub(crate) fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (decode_query_value(k), decode_query_value(v)),
            None => (decode_query_value(pair), String::new()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ScriptedTransport, json_response};

    const USER_JSON: &str =
        r#"{"id":"u1","email":"o@example.com","display_name":"Owner","auth0_sub":null}"#;

    fn client(
        replies: Vec<Result<HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("http://localhost:8787/", transport.clone());
        (api, transport)
    }

    #[test]
    fn base_url_loses_trailing_slash() {
        let (api, _) = client(vec![]);
        assert_eq!(api.base_url(), "http://localhost:8787");
    }

    #[test]
    fn login_url_encodes_parameters() {
        let (api, _) = client(vec![]);
        let url = api.login_url("http://127.0.0.1:5000/callback", "abc_DEF-1", "s t/a");
        assert_eq!(
            url,
            "http://localhost:8787/v1/auth/login?redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2Fcallback&code_challenge=abc_DEF-1&state=s%20t%2Fa"
        );
    }

    #[test]
    fn query_round_trips_through_encode_and_decode() {
        let raw = "a b&c=d/é";
        assert_eq!(decode_query_value(&encode_query_value(raw)), raw);
        assert_eq!(decode_query_value("x+y%2Bz"), "x y+z");
        assert_eq!(decode_query_value("bad%zz%"), "bad%zz%");
    }

    #[test]
    fn exchange_posts_json_and_parses_session() {
        let body = format!(
            r#"{{"session_token":"tok_1","expires_at":"2026-12-15T10:00:00Z","user":{USER_JSON}}}"#
        );
        let (api, transport) = client(vec![json_response(200, &body)]);
        let request = ExchangeRequest {
            code: "code-1".to_string(),
            code_verifier: "verifier-1".to_string(),
        };
        let session = pollster::block_on(api.exchange(&request)).expect("exchange");
        assert_eq!(session.session_token, "tok_1");
        assert_eq!(session.user.email, "o@example.com");

        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Post);
        assert_eq!(sent[0].url, "http://localhost:8787/v1/auth/exchange");
        assert_eq!(sent[0].bearer, None);
        let body: ExchangeRequest =
            serde_json::from_str(sent[0].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(body, request);
    }

    #[test]
    fn me_maps_401_to_unauthorized() {
        let (api, transport) = client(vec![json_response(401, "")]);
        let result = pollster::block_on(api.me("tok_old"));
        assert_eq!(result, Err(ClientError::Unauthorized));
        assert_eq!(transport.requests()[0].bearer.as_deref(), Some("tok_old"));
    }

    #[test]
    fn me_accepts_bare_user_and_session_envelope() {
        let (api, _) = client(vec![json_response(200, USER_JSON)]);
        let user = pollster::block_on(api.me("t")).expect("bare user");
        assert_eq!(user.display_name, "Owner");

        let envelope = format!(r#"{{"session_token":"t","expires_at":"x","user":{USER_JSON}}}"#);
        let (api, _) = client(vec![json_response(200, &envelope)]);
        let user = pollster::block_on(api.me("t")).expect("envelope");
        assert_eq!(user.id, "u1");
    }

    #[test]
    fn server_error_body_is_mapped() {
        let (api, _) = client(vec![json_response(
            403,
            r#"{"error":"not_employee","message":"Only employees can sign in."}"#,
        )]);
        let result = pollster::block_on(api.health());
        assert_eq!(
            result,
            Err(ClientError::Api {
                status: 403,
                error: "not_employee".to_string(),
                message: "Only employees can sign in.".to_string(),
            })
        );
    }

    #[test]
    fn network_failure_is_passed_through() {
        let (api, _) = client(vec![Err(ClientError::Network("refused".to_string()))]);
        let result = pollster::block_on(api.me("t"));
        assert_eq!(result, Err(ClientError::Network("refused".to_string())));
    }

    #[test]
    fn signout_sends_bearer() {
        let (api, transport) = client(vec![json_response(204, "")]);
        pollster::block_on(api.signout("tok")).expect("signout");
        let sent = &transport.requests()[0];
        assert_eq!(sent.url, "http://localhost:8787/v1/auth/signout");
        assert_eq!(sent.bearer.as_deref(), Some("tok"));
    }
}
