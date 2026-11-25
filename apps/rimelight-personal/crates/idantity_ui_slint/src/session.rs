//! The sign-in driver: connects the `Session` global (ui/sign-in.slint) to the Access session in `idantity_core::auth`.
//!
//! The shell calls [`wire`] once, after it creates the window. The window shows the shared `SignInScreen` over everything
//! until the phase is `SignedIn`, as Rimelight Space does. [`wire`]:
//! - starts the session: it first asks whether the server answers (phase `Loading`, then `Unreachable` when it does not,
//!   retried on its own), then desktop loads the stored Access token and web asks the API whether the browser's Access
//!   cookie is accepted;
//! - answers `Session.sign-in` (desktop: the Access CLI login on a worker thread; web: navigate to the Access login),
//!   `Session.cancel`, `Session.retry` and `Session.sign-out`;
//! - checks the server every few seconds: while signed in, losing it sets `Session.connection` to offline, which blocks
//!   the app with the "Connection lost" overlay until it answers again.
//!
//! The returned [`Auth`] carries the session the API client sends with each request: build the client with
//! `ApiClient::new(base).with_bearer(auth.session.clone())`. On the web the session sends no header (the cookie does).

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use idantity_core::auth::{Auth, SessionStore};
use idantity_core::{ApiClient, DEFAULT_API_BASE};
use slint::ComponentHandle as _;

use crate::generated::{ConnectionState, IdantityShell, Session, SessionPhase};

/// How often the server is checked while signed in and connected, in seconds.
const CHECK_CONNECTED: u64 = 10;
/// How often it is checked while the connection is lost or the unreachable screen shows, in seconds.
const CHECK_LOST: u64 = 3;

thread_local! {
    /// The check timer. It lives as long as the window's thread.
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// Connects the sign-in screen to the Access session and starts it. Returns the session for the API client.
pub fn wire(ui: &IdantityShell) -> Arc<Auth> {
    let auth = Arc::new(Auth::new(default_store()));
    ui.global::<Session>().set_phase(SessionPhase::Loading);
    start(ui, &auth);
    start_timer(ui, &auth);

    let weak = ui.as_weak();
    let for_sign_in = auth.clone();
    ui.global::<Session>().on_sign_in(move || {
        if let Some(ui) = weak.upgrade() {
            begin_sign_in(&ui, for_sign_in.clone());
        }
    });

    // Cancel: back to the sign-in button. A browser sign-in already under way is dropped when it finishes (see
    // `finish_sign_in`).
    let weak = ui.as_weak();
    ui.global::<Session>().on_cancel(move || {
        if let Some(ui) = weak.upgrade() {
            let session = ui.global::<Session>();
            session.set_error("".into());
            session.set_phase(SessionPhase::SignedOut);
        }
    });

    let weak = ui.as_weak();
    let for_retry = auth.clone();
    ui.global::<Session>().on_retry(move || {
        if let Some(ui) = weak.upgrade() {
            start(&ui, &for_retry);
        }
    });

    let weak = ui.as_weak();
    let for_sign_out = auth.clone();
    ui.global::<Session>().on_sign_out(move || {
        if let Some(ui) = weak.upgrade() {
            let session = ui.global::<Session>();
            session.set_phase(SessionPhase::SignedOut);
            session.set_connection(ConnectionState::Connected);
            session.set_error("".into());
            if let Err(e) = for_sign_out.sign_out() {
                session.set_error(e.to_string().into());
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ =
                    idantity_core::web_session::navigate(&idantity_core::auth::web_logout_url());
            }
        }
    });

    auth
}

/// The OS credential store on desktop; the browser keeps no token of its own (the Access cookie), so web uses memory.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn default_store() -> Arc<dyn SessionStore> {
    Arc::new(idantity_core::auth::KeyringStore)
}

#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
fn default_store() -> Arc<dyn SessionStore> {
    Arc::new(idantity_core::auth::MemoryStore::default())
}

/// Sets the first state: asks whether the server answers, then decides between signed in and signed out (or shows the
/// unreachable screen). Also the unreachable screen's Retry and its timer.
fn start(ui: &IdantityShell, auth: &Arc<Auth>) {
    let session = ui.global::<Session>();
    session.set_error("".into());
    // The unreachable screen keeps showing while it retries; every other start shows the loading spinner.
    if session.get_phase() != SessionPhase::Unreachable {
        session.set_phase(SessionPhase::Loading);
    }
    let weak = ui.as_weak();
    let auth = auth.clone();
    ApiClient::new(DEFAULT_API_BASE).reachable(move |online| {
        let _ = weak.upgrade_in_event_loop(move |ui| {
            if online {
                decide(&ui, &auth);
            } else {
                ui.global::<Session>().set_phase(SessionPhase::Unreachable);
            }
        });
    });
}

/// The server answered: a stored token that is still valid signs the user in without a browser (desktop); on the web the
/// API says whether the browser's Access cookie is accepted.
fn decide(ui: &IdantityShell, auth: &Arc<Auth>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // A store that cannot be read counts as signed out; the next sign-in reports its error.
        let signed_in = auth.restore().unwrap_or(false);
        let session = ui.global::<Session>();
        session.set_connection(ConnectionState::Connected);
        session.set_phase(if signed_in {
            SessionPhase::SignedIn
        } else {
            SessionPhase::SignedOut
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let weak = ui.as_weak();
        idantity_core::web_session::probe_cookie_session(move |signed_in| {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let session = ui.global::<Session>();
                session.set_connection(ConnectionState::Connected);
                session.set_phase(if signed_in {
                    SessionPhase::SignedIn
                } else {
                    SessionPhase::SignedOut
                });
            });
        });
        // The browser keeps no token here: the Access cookie is the session.
        let _ = auth;
    }
}

/// The periodic check. A tick every second counts up to the interval of the state the window shows: the unreachable screen
/// and a lost connection check often, a connected app rarely. Nothing is checked on the sign-in button or while loading
/// or waiting (the sign-in itself reports a network failure).
fn start_timer(ui: &IdantityShell, auth: &Arc<Auth>) {
    let weak = ui.as_weak();
    let auth = auth.clone();
    let mut seconds = 0u64;
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let session = ui.global::<Session>();
            let interval = match (session.get_phase(), session.get_connection()) {
                (SessionPhase::Unreachable, _) => CHECK_LOST,
                (SessionPhase::SignedIn, ConnectionState::Connected) => CHECK_CONNECTED,
                (SessionPhase::SignedIn, _) => CHECK_LOST,
                _ => {
                    seconds = 0;
                    return;
                }
            };
            seconds += 1;
            if seconds < interval {
                return;
            }
            seconds = 0;
            if session.get_phase() == SessionPhase::Unreachable {
                start(&ui, &auth);
                return;
            }
            let weak = ui.as_weak();
            ApiClient::new(DEFAULT_API_BASE).reachable(move |online| {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let session = ui.global::<Session>();
                    // A sign-out that happened while the check ran wins.
                    if session.get_phase() == SessionPhase::SignedIn {
                        session.set_connection(if online {
                            ConnectionState::Connected
                        } else {
                            ConnectionState::Offline
                        });
                    }
                });
            });
        },
    );
    TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
}

/// Starts the sign-in: the screen shows "finish signing in in your browser" until the flow finishes or is cancelled.
fn begin_sign_in(ui: &IdantityShell, auth: Arc<Auth>) {
    let session = ui.global::<Session>();
    session.set_error("".into());
    session.set_phase(SessionPhase::Waiting);

    #[cfg(not(target_arch = "wasm32"))]
    {
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result: Result<(), idantity_core::auth::AuthError> =
                idantity_core::signin::cli_login(&idantity_core::signin::open_system_browser)
                    .and_then(|token| auth.accept_token(token));
            let _ = weak.upgrade_in_event_loop(move |ui| finish_sign_in(&ui, &auth, result));
        });
    }

    #[cfg(target_arch = "wasm32")]
    {
        drop(auth);
        let page = idantity_core::web_session::page_address().unwrap_or_default();
        // The page leaves for Access; if the browser refuses, the error shows here.
        if let Err(message) =
            idantity_core::web_session::navigate(&idantity_core::auth::web_login_url(&page))
        {
            session.set_phase(SessionPhase::SignedOut);
            session.set_error(message.into());
        }
    }
}

/// Shows the outcome of a finished desktop sign-in. A sign-in the user cancelled meanwhile is dropped: its token is cleared
/// again, so cancelling really leaves the app signed out.
#[cfg(not(target_arch = "wasm32"))]
fn finish_sign_in(
    ui: &IdantityShell,
    auth: &Arc<Auth>,
    result: Result<(), idantity_core::auth::AuthError>,
) {
    let session = ui.global::<Session>();
    if session.get_phase() != SessionPhase::Waiting {
        if result.is_ok() {
            let _ = auth.sign_out();
        }
        return;
    }
    match result {
        Ok(()) => {
            session.set_error("".into());
            session.set_connection(ConnectionState::Connected);
            session.set_phase(SessionPhase::SignedIn);
        }
        Err(e) => {
            session.set_phase(SessionPhase::SignedOut);
            session.set_error(e.to_string().into());
        }
    }
}
