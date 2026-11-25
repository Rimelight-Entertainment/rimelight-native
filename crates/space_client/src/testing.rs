//! Test doubles shared by the client's unit tests.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::api::{BoxFuture, HttpRequest, HttpResponse, Transport};
use crate::error::ClientError;
use crate::lock;

/// Transport that replays queued replies and records every request.
pub(crate) struct ScriptedTransport {
    replies: Mutex<VecDeque<Result<HttpResponse, ClientError>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl ScriptedTransport {
    pub(crate) fn new(replies: Vec<Result<HttpResponse, ClientError>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        lock(&self.requests).clone()
    }
}

impl Transport for ScriptedTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ClientError>> {
        lock(&self.requests).push(request);
        let reply = lock(&self.replies)
            .pop_front()
            .unwrap_or_else(|| Err(ClientError::Network("no scripted reply".to_string())));
        Box::pin(async move { reply })
    }
}

pub(crate) fn json_response(status: u16, body: &str) -> Result<HttpResponse, ClientError> {
    Ok(HttpResponse {
        status,
        body: body.to_string(),
        bytes: body.as_bytes().to_vec(),
    })
}

/// A binary reply, such as an avatar image.
pub(crate) fn bytes_response(status: u16, bytes: &[u8]) -> Result<HttpResponse, ClientError> {
    Ok(HttpResponse {
        status,
        body: String::from_utf8_lossy(bytes).into_owned(),
        bytes: bytes.to_vec(),
    })
}
