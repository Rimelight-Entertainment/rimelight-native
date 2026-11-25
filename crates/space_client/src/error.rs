//! Errors returned by the client core.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The server rejected the session (HTTP 401).
    Unauthorized,
    /// The server answered with a non-2xx status and an error body (or none).
    Api {
        status: u16,
        error: String,
        message: String,
    },
    /// The server rejected a search query (HTTP 400 `invalid_query`). `start` and `end` are the 0-based character
    /// offsets of the text it is about, end exclusive.
    InvalidQuery {
        message: String,
        start: usize,
        end: usize,
    },
    /// The request did not reach the server or the reply was not received.
    Network(String),
    /// The reply did not have the expected shape.
    Protocol(String),
    /// The loopback callback carried a state other than the one we sent.
    StateMismatch,
    /// The identity provider or backend reported a denial on the callback.
    SignInDenied(String),
    /// No callback arrived before the deadline.
    SignInTimeout,
    /// The system browser could not be opened.
    Browser(String),
    /// The OS credential store failed.
    Storage(String),
    /// The OS random source failed.
    Random(String),
    /// Local socket error on the loopback listener.
    Io(String),
    /// Not available on this platform yet.
    Unsupported(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "the session is not authorised"),
            Self::Api {
                status,
                error,
                message,
            } => write!(f, "server error {status} ({error}): {message}"),
            Self::InvalidQuery { message, .. } => write!(f, "invalid query: {message}"),
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Protocol(e) => write!(f, "unexpected reply: {e}"),
            Self::StateMismatch => write!(f, "sign-in state did not match"),
            Self::SignInDenied(e) => write!(f, "sign-in was denied: {e}"),
            Self::SignInTimeout => write!(f, "sign-in timed out"),
            Self::Browser(e) => write!(f, "could not open the browser: {e}"),
            Self::Storage(e) => write!(f, "credential store error: {e}"),
            Self::Random(e) => write!(f, "random source error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Unsupported(what) => write!(f, "not supported yet: {what}"),
        }
    }
}

impl std::error::Error for ClientError {}
