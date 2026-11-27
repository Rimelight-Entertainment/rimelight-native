//! The live [`Cloudflare`] implementation for the setup: sends each call with the sign-in token, and reads the answer.
//! Native only: the desktop app runs the setup. The token is never logged or put in an error.

use std::sync::mpsc;
use std::time::Duration;

use ehttp::{Headers, Method, Request};
use serde_json::Value;

use crate::cf_oauth::USERINFO_URL;
use crate::provision::{API_BASE, Body, CfError, CfMethod, Cloudflare, multipart_body};

/// Calls the Cloudflare API with one OAuth access token.
pub struct BearerCloudflare {
    token: String,
}

impl BearerCloudflare {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

fn ehttp_method(method: &CfMethod) -> Method {
    match method {
        CfMethod::Get => Method::GET,
        CfMethod::Post => Method::POST,
        CfMethod::Put => Method::PUT,
        CfMethod::Patch => Method::PATCH,
    }
}

/// Sends one request and returns the parsed answer. Errors carry Cloudflare's own code and message, never the token.
pub fn send(
    method: Method,
    url: String,
    token: &str,
    headers: Vec<(&str, String)>,
    body: Vec<u8>,
) -> Result<Value, CfError> {
    let mut all = vec![("Authorization", format!("Bearer {token}"))];
    all.extend(headers);
    let pairs: Vec<(&str, &str)> = all.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut request = Request::new(method, url, Headers::new(&pairs));
    request.body = body;

    let (tx, rx) = mpsc::channel();
    ehttp::fetch(request, move |result| {
        let _ = tx.send(result);
    });
    let response = rx
        .recv_timeout(Duration::from_secs(120))
        .map_err(|_| CfError::Transport("no answer in time".to_owned()))?
        .map_err(|e| CfError::Transport(e.to_string()))?;

    let parsed: Value = serde_json::from_slice(&response.bytes).unwrap_or(Value::Null);
    if !response.ok {
        let first = parsed
            .get("errors")
            .and_then(Value::as_array)
            .and_then(|e| e.first());
        return Err(CfError::Api {
            status: response.status,
            code: first.and_then(|e| e.get("code")).and_then(Value::as_u64),
            message: first
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("no message")
                .to_owned(),
        });
    }
    if parsed.get("success") == Some(&Value::Bool(false)) {
        return Err(CfError::Shape("success was false".to_owned()));
    }
    Ok(parsed)
}

impl Cloudflare for BearerCloudflare {
    fn call(&self, method: CfMethod, path: &str, body: Body) -> Result<Value, CfError> {
        let url = format!("{API_BASE}{path}");
        match body {
            Body::None => send(
                ehttp_method(&method),
                url,
                &self.token,
                Vec::new(),
                Vec::new(),
            ),
            Body::Json(value) => send(
                ehttp_method(&method),
                url,
                &self.token,
                vec![("Content-Type", "application/json".to_owned())],
                value.to_string().into_bytes(),
            ),
            Body::WorkerUpload { metadata, module } => {
                let (boundary, bytes) = multipart_body(&metadata, &module);
                send(
                    ehttp_method(&method),
                    url,
                    &self.token,
                    vec![(
                        "Content-Type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )],
                    bytes,
                )
            }
        }
    }
}

/// The owner's email, read from the sign-in's user info. It becomes the Access policy's only allowed identity.
pub fn owner_email(token: &str) -> Result<String, CfError> {
    let answer = send(
        Method::GET,
        USERINFO_URL.to_owned(),
        token,
        Vec::new(),
        Vec::new(),
    )?;
    answer
        .get("email")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CfError::Shape("the sign-in has no email".to_owned()))
}

/// The account this sign-in can see, when there is exactly one. With several, the caller must choose one.
pub fn only_account(cf: &dyn Cloudflare) -> Result<String, CfError> {
    let answer = cf.call(CfMethod::Get, "/accounts", Body::None)?;
    let accounts = answer
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match accounts.as_slice() {
        [one] => one
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| CfError::Shape("the account has no id".to_owned())),
        [] => Err(CfError::Shape("this sign-in can see no account".to_owned())),
        many => {
            let names: Vec<&str> = many
                .iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect();
            Err(CfError::Shape(format!(
                "this sign-in can see several accounts ({}); pass --account <id> to choose one",
                names.join(", ")
            )))
        }
    }
}

/// The signed-in user's email, from the user details (the `user-details.read` scope). It is the owner's identity for the
/// Access policy and the owner id on the rows.
pub fn user_email(cf: &dyn Cloudflare) -> Result<String, CfError> {
    let answer = cf.call(CfMethod::Get, "/user", Body::None)?;
    answer
        .get("result")
        .and_then(|r| r.get("email"))
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CfError::Shape("the sign-in has no email on the user details".to_owned()))
}
