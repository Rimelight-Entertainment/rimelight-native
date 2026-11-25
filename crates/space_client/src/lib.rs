//! space_client: the Rimelight Space client core (decision 0014).
//!
//! Covers PKCE, the HTTP API client, the OS credential store, the desktop
//! loopback sign-in flow, the Android browser return and the signed-in state machine. The UI reads
//! [`auth::AuthState`]; nothing here draws anything.

pub mod api;
pub mod auth;
pub mod canvas_live;
pub mod canvases;
pub mod content;
pub mod content_live;
pub mod error;
pub mod people;
pub mod pkce;
pub mod projects;
pub mod session_store;
pub mod web_signin;

// Desktop: system browser plus loopback callback.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub mod signin;

// Android sign-in (SA.7, decision 0018): the system browser and the rimelight-space://auth return. Native only; the browser
// is passed in by the app, so this module has no Android code.
#[cfg(not(target_arch = "wasm32"))]
pub mod signin_mobile;

// iOS: documented stub until its sign-in is built (decision 0018). The web uses web_signin (SA.6) instead.
#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
#[path = "signin_stub.rs"]
pub mod signin;

#[cfg(test)]
mod testing;

/// Runs a client future to completion on a small current-thread tokio runtime. Blocks the calling thread, so call it from a
/// worker thread, never from the UI thread. Native targets only (reqwest and the desktop sign-in need the runtime).
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, error::ClientError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| error::ClientError::Io(e.to_string()))?;
    Ok(runtime.block_on(future))
}

/// Locks a mutex, recovering the guard if a previous holder panicked.
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
