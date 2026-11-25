//! Sign-in driver (SA.4 desktop, SA.6 web). Runs the client's async calls and sets the shell's properties on the UI thread.
//!
//! Start-up: the window shows the loading phase. On the web, a page that came back from the sign-in carries the result,
//! which is checked and exchanged first. Otherwise `restore` checks the stored session, then the shell (signed in), the
//! sign-in screen, or the "Can't reach Rimelight Space" screen when the server cannot be reached (SA.8: no offline mode). That
//! screen keeps the stored session and retries on its own. Desktop sign-in runs the browser flow on a worker thread (it blocks
//! for up to five minutes). Web sign-in leaves the page for the backend's login, and the page comes back on start-up. Cancel
//! ends the wait on the screen; a late result from the cancelled attempt is dropped.
//!
//! Once signed in, the server is checked by a one-second tick: every [`CHECK_INTERVAL`] while connected, every
//! [`RECONNECT_INTERVAL`] once the connection is lost (the overlay says so and blocks the app, SA.9). A check asks for `health` and, when a session is stored, `me`: a 401 clears the session and shows
//! the sign-in screen. When the connection returns, the open pages ask again (`reload_pages`).
//!
//! Native: each task runs on a worker thread with its own runtime, and workers post `Update`s with
//! `slint::invoke_from_event_loop`. Web: each task is a local future on the page's only thread, and the update is applied
//! directly. Either way the window is touched only on the UI thread.
//!
//! Android (SA.7, decision 0018) uses [`attach_mobile`]: the sign-in opens the system browser and does not wait; the browser
//! returns `rimelight-space://auth` to the app, and the app passes that address to [`Session::sign_in_returned`].
//!
//! The desktop and web apps call [`attach`] before they run the window.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use slint::{ComponentHandle as _, Timer, TimerMode, Weak};
use space_client::api::ApiClient;
use space_client::auth::{AuthState, restore};
use space_client::error::ClientError;
use space_client::session_store::{SessionStore, StoredSession};
#[cfg(not(target_arch = "wasm32"))]
use space_client::signin;
#[cfg(not(target_arch = "wasm32"))]
use space_client::signin_mobile;
#[cfg(target_arch = "wasm32")]
use space_client::web_signin;

use super::app_shell::App;
use super::session::{SignInProblem, sign_in_error_text};
use super::{AdminOrgState, AdminRolesState, AdminUsersState, HomeState, LocationsState};
use super::{AppShell, ConnectionState, SessionPhase};
use space_page_kit::page_data::DataSource;
use space_page_kit::person::initials;
use space_page_kit::task::spawn_task;

/// How often the server is checked while connected.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// How often the server is checked while the connection is lost, signed in (SA.8).
pub const RECONNECT_INTERVAL: Duration = Duration::from_secs(5);
/// How often start-up is tried again on the "Can't reach Rimelight Space" screen (SA.8).
pub const RETRY_INTERVAL: Duration = Duration::from_secs(10);
/// The tick the periodic check counts in. Each interval above is a whole number of ticks.
const TICK: Duration = Duration::from_secs(1);

/// What a task tells the UI thread.
#[derive(Debug)]
enum Update {
    /// Show the sign-in screen, with the error line when there is one.
    SignedOut(Option<SignInProblem>),
    /// The server could not be reached at start-up: show the "Can't reach Rimelight Space" screen (SA.8).
    Unreachable,
    /// The browser sign-in is running: show the wait with Cancel.
    Waiting,
    /// Show the shell for this person.
    SignedIn {
        id: String,
        name: String,
        email: String,
    },
    /// Whether the server answered: the status bar and the connection overlay follow.
    Online(bool),
}

/// Finishes a browser sign-in with the address the app received.
type ReturnHandler = Box<dyn Fn(&str)>;

/// Opens a URL in the system browser. Its error is the client's browser error.
pub type BrowserOpener = Rc<dyn Fn(&str) -> Result<(), ClientError>>;

/// Keeps the periodic check alive. Drop it to stop the checks.
pub struct Session {
    _timer: Timer,
    /// Finishes a browser sign-in with the address the app received. `None` where the browser does not return to the app.
    returned: Option<ReturnHandler>,
}

impl Session {
    /// Finishes the browser sign-in with the address the app received (`rimelight-space://auth?...`, SA.7). Any other address,
    /// or a return with no sign-in pending, does nothing. Call it on the UI thread.
    pub fn sign_in_returned(&self, url: &str) {
        if let Some(finish) = &self.returned {
            finish(url);
        }
    }
}

/// What a platform whose browser returns to the app needs (Android, SA.7, decision 0018).
pub struct Mobile {
    /// Opens the login page in the system browser.
    pub opener: BrowserOpener,
    /// The app's private data folder: the pending sign-in and the session file are kept here.
    pub data_dir: PathBuf,
    /// The sign-in return the app was started with, if any.
    pub launch_return: Option<String>,
}

/// Wires the sign-in screen and the shell's sign-out to `api` and `store`, restores the stored session, and starts the
/// periodic check. Call it on the UI thread once the window exists. The window keeps its `loading` phase until the restore
/// answers.
pub fn attach(app: &App, api: Arc<ApiClient>, store: Arc<dyn SessionStore>) -> Session {
    wire(app, api, store, None)
}

/// [`attach`] for a platform whose browser returns to the app (Android). Sign-in opens the browser and waits for
/// [`Session::sign_in_returned`].
pub fn attach_mobile(
    app: &App,
    api: Arc<ApiClient>,
    store: Arc<dyn SessionStore>,
    mobile: Mobile,
) -> Session {
    wire(app, api, store, Some(mobile))
}

fn wire(
    app: &App,
    api: Arc<ApiClient>,
    store: Arc<dyn SessionStore>,
    mobile: Option<Mobile>,
) -> Session {
    let ui = &app.ui;
    let weak = ui.as_weak();
    // The pages load through the same client and store (SP.3b, src/slint_ui/page_data.rs).
    *app.data.borrow_mut() = Some(Arc::new(DataSource::new(api.clone(), store.clone())));
    // Each sign-in attempt takes a number. Cancel takes one too, so a late result from an earlier attempt is dropped.
    let attempt = Arc::new(AtomicU64::new(0));
    // The return the app started with, read by start-up. The returned-address handler is for later returns.
    let launch = mobile
        .as_ref()
        .and_then(|m| m.launch_return.clone().map(|url| (m.data_dir.clone(), url)));
    #[cfg(not(target_arch = "wasm32"))]
    let returned = mobile
        .as_ref()
        .map(|m| return_handler(api.clone(), store.clone(), weak.clone(), m.data_dir.clone()));
    #[cfg(target_arch = "wasm32")]
    let returned: Option<ReturnHandler> = None;
    let mobile = mobile.map(Rc::new);

    let (a, s, w, n, m) = (
        api.clone(),
        store.clone(),
        weak.clone(),
        attempt.clone(),
        mobile.clone(),
    );
    ui.on_sign_in_clicked(move || {
        start_sign_in(a.clone(), s.clone(), w.clone(), n.clone(), m.clone())
    });

    let (w, n) = (weak.clone(), attempt.clone());
    ui.on_sign_in_cancelled(move || {
        n.fetch_add(1, Ordering::SeqCst);
        post(&w, Update::SignedOut(None));
    });

    let (a, s, w) = (api.clone(), store.clone(), weak.clone());
    ui.on_sign_out_clicked(move || sign_out(a.clone(), s.clone(), w.clone()));

    let (a, s, w) = (api.clone(), store.clone(), weak.clone());
    ui.on_sign_in_retry_clicked(move || retry_startup(a.clone(), s.clone(), w.clone()));

    let (a, s, w) = (api.clone(), store.clone(), weak.clone());
    spawn_task(move || async move { startup(&a, s.as_ref(), &w, launch).await });

    // The tick counts seconds since the last check. The interval depends on the state the window shows: the unreachable
    // screen retries start-up, a lost connection is checked more often, and a connected app is checked every CHECK_INTERVAL.
    let timer = Timer::default();
    let (a, s, w) = (api, store, weak);
    let waited = Rc::new(Cell::new(Duration::ZERO));
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = w.upgrade() else {
            return;
        };
        let unreachable = ui.get_session_phase() == SessionPhase::Unreachable;
        let interval = if unreachable {
            RETRY_INTERVAL
        } else if ui.get_connection() == ConnectionState::Connected {
            CHECK_INTERVAL
        } else {
            RECONNECT_INTERVAL
        };
        let elapsed = waited.get() + TICK;
        if elapsed < interval {
            waited.set(elapsed);
            return;
        }
        waited.set(Duration::ZERO);
        if unreachable {
            retry_startup(a.clone(), s.clone(), w.clone());
        } else {
            check(a.clone(), s.clone(), w.clone());
        }
    });
    Session {
        _timer: timer,
        returned,
    }
}

/// The handler for a sign-in return the app receives while it runs. Native only: the Android sign-in is native.
#[cfg(not(target_arch = "wasm32"))]
fn return_handler(
    api: Arc<ApiClient>,
    store: Arc<dyn SessionStore>,
    weak: Weak<AppShell>,
    dir: PathBuf,
) -> ReturnHandler {
    Box::new(move |url: &str| {
        let (api, store, weak, dir, url) = (
            api.clone(),
            store.clone(),
            weak.clone(),
            dir.clone(),
            url.to_string(),
        );
        spawn_task(move || async move {
            if let Some(result) = signin_mobile::finish_return(&api, &dir, &url).await {
                finish_sign_in(result.map(to_stored), store.as_ref(), &weak);
            }
        });
    })
}

/// Sends an update to the window. Native: from any thread, dropped when the window is gone. Web: already on the UI thread.
#[cfg(not(target_arch = "wasm32"))]
fn post(weak: &Weak<AppShell>, update: Update) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            apply(&ui, update);
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn post(weak: &Weak<AppShell>, update: Update) {
    if let Some(ui) = weak.upgrade() {
        apply(&ui, update);
    }
}

/// Applies an update. Runs on the UI thread only.
fn apply(ui: &AppShell, update: Update) {
    match update {
        Update::SignedOut(problem) => {
            let text = problem.map(|p| sign_in_error_text(&p)).unwrap_or_default();
            ui.set_sign_in_error(text.into());
            ui.set_session_phase(SessionPhase::SignedOut);
        }
        Update::Waiting => {
            ui.set_sign_in_error("".into());
            ui.set_session_phase(SessionPhase::Waiting);
        }
        Update::Unreachable => {
            ui.set_sign_in_error("".into());
            ui.set_session_phase(SessionPhase::Unreachable);
        }
        Update::SignedIn { id, name, email } => {
            ui.set_user_id(id.into());
            ui.set_user_initials(initials(&name).into());
            ui.set_user_name(name.into());
            ui.set_user_email(email.into());
            ui.set_sign_in_error("".into());
            ui.set_session_phase(SessionPhase::SignedIn);
            reload_pages(ui);
        }
        Update::Online(online) => {
            let was = ui.get_connection();
            let state = if online {
                ConnectionState::Connected
            } else {
                ConnectionState::Offline
            };
            ui.set_connection(state);
            // The connection came back: the open pages ask again, as they do after sign-in (SA.8).
            if online && was != ConnectionState::Connected {
                reload_pages(ui);
            }
        }
    }
}

/// Asks every page that loads from the server to load again. Runs after sign-in and when the connection returns.
fn reload_pages(ui: &AppShell) {
    // A page that failed without a session (or opened before one) asks again now that there is one (SP.3b).
    // The Dashboard loads both of its cards again (S2).
    ui.global::<HomeState>().invoke_opened();
    ui.invoke_employees_opened();
    // The project list (the Projects sidebar and switcher) and the issue list ask again too (S1.3).
    ui.invoke_projects_refresh();
    ui.invoke_overview_retry();
    ui.invoke_issues_retry();
    ui.invoke_issue_retry();
    // The profile page asks again as well, so a profile that failed before sign-in loads now (SP.3c).
    ui.invoke_profile_retry();
    // The teams, team and departments pages ask again too (SP.3d).
    ui.invoke_teams_retry();
    ui.invoke_team_retry();
    ui.invoke_dept_retry();
    ui.global::<LocationsState>().invoke_retry();
    // The admin pages ask again too (SP.3e), so a page that opened before sign-in loads now.
    ui.global::<AdminUsersState>().invoke_retry();
    ui.global::<AdminOrgState>().invoke_retry();
    ui.global::<AdminRolesState>().invoke_retry();
}

/// Start-up: on the web, a sign-in return is finished first. Otherwise asks the server who the stored session belongs to,
/// then shows the shell or the sign-in screen.
async fn startup(
    api: &ApiClient,
    store: &dyn SessionStore,
    weak: &Weak<AppShell>,
    launch: Option<(PathBuf, String)>,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = launch;
        if let Some(result) = web_signin::complete_web_sign_in(api).await {
            finish_sign_in(result.map(to_stored), store, weak);
            return;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((dir, url)) = launch
        && let Some(result) = signin_mobile::finish_return(api, &dir, &url).await
    {
        finish_sign_in(result.map(to_stored), store, weak);
        return;
    }
    show_restored(restore(api, store).await, weak);
}

/// Shows the state `restore` found.
fn show_restored(state: AuthState, weak: &Weak<AppShell>) {
    match state {
        AuthState::SignedIn(user) => {
            post(
                weak,
                Update::SignedIn {
                    id: user.id,
                    name: user.display_name,
                    email: user.email,
                },
            );
            post(weak, Update::Online(true));
        }
        AuthState::Unreachable => post(weak, Update::Unreachable),
        AuthState::SignedOut | AuthState::SigningIn => post(weak, Update::SignedOut(None)),
    }
}

/// Checks the stored session again from the start (the unreachable screen's Retry, and its timer). The stored session is
/// kept when the server still cannot be reached.
fn retry_startup(api: Arc<ApiClient>, store: Arc<dyn SessionStore>, weak: Weak<AppShell>) {
    spawn_task(move || async move { show_restored(restore(&api, store.as_ref()).await, &weak) });
}

/// Starts the browser sign-in and shows the wait. Desktop: the wait is on a worker. Web: the page goes to the backend's login.
#[cfg(not(target_arch = "wasm32"))]
fn start_sign_in(
    api: Arc<ApiClient>,
    store: Arc<dyn SessionStore>,
    weak: Weak<AppShell>,
    attempt: Arc<AtomicU64>,
    mobile: Option<Rc<Mobile>>,
) {
    if let Some(mobile) = mobile {
        // The browser returns through `Session::sign_in_returned`, so nothing waits here.
        post(&weak, Update::Waiting);
        if let Err(e) = signin_mobile::begin(&api, &mobile.data_dir, &*mobile.opener) {
            post(&weak, Update::SignedOut(Some(problem_of(&e))));
        }
        return;
    }
    let number = attempt.fetch_add(1, Ordering::SeqCst) + 1;
    post(&weak, Update::Waiting);
    spawn_task(move || async move {
        // Waits for up to the client's timeout (five minutes).
        let result = signin::sign_in(&api).await;
        if attempt.load(Ordering::SeqCst) != number {
            return;
        }
        finish_sign_in(result.map(to_stored), store.as_ref(), &weak);
    });
}

#[cfg(target_arch = "wasm32")]
fn start_sign_in(
    api: Arc<ApiClient>,
    _store: Arc<dyn SessionStore>,
    weak: Weak<AppShell>,
    _attempt: Arc<AtomicU64>,
    _mobile: Option<Rc<Mobile>>,
) {
    post(&weak, Update::Waiting);
    if let Err(e) = web_signin::begin_web_sign_in(&api) {
        post(&weak, Update::SignedOut(Some(problem_of(&e))));
    }
}

/// Keeps a finished sign-in: saves the session and shows the shell, or shows the sign-in screen with the problem.
fn finish_sign_in(
    result: Result<StoredSession, ClientError>,
    store: &dyn SessionStore,
    weak: &Weak<AppShell>,
) {
    match result {
        Ok(stored) => match store.save(&stored) {
            Ok(()) => {
                post(
                    weak,
                    Update::SignedIn {
                        id: stored.user.id,
                        name: stored.user.display_name,
                        email: stored.user.email,
                    },
                );
                post(weak, Update::Online(true));
            }
            Err(e) => post(
                weak,
                Update::SignedOut(Some(SignInProblem::Other(e.to_string()))),
            ),
        },
        Err(e) => post(weak, Update::SignedOut(Some(problem_of(&e)))),
    }
}

/// The session the backend returned, in the form the store keeps.
fn to_stored(session: space_proto::auth::SessionResponse) -> StoredSession {
    StoredSession {
        session_token: session.session_token,
        expires_at: session.expires_at,
        user: session.user,
    }
}

/// Sign out: tells the server, clears the stored session and shows the sign-in screen. A failed server call still clears the
/// session on this machine.
fn sign_out(api: Arc<ApiClient>, store: Arc<dyn SessionStore>, weak: Weak<AppShell>) {
    spawn_task(move || async move {
        if let Ok(Some(stored)) = store.load() {
            let _ = api.signout(&stored.session_token).await;
        }
        let _ = store.clear();
        post(&weak, Update::SignedOut(None));
    });
}

/// The periodic check: `health` for the connection, and `me` for a stored session. A 401 on `me` ends the session.
fn check(api: Arc<ApiClient>, store: Arc<dyn SessionStore>, weak: Weak<AppShell>) {
    spawn_task(move || async move {
        match api.health().await {
            Err(_) => post(&weak, Update::Online(false)),
            Ok(()) => {
                post(&weak, Update::Online(true));
                let Ok(Some(stored)) = store.load() else {
                    return;
                };
                if let Err(ClientError::Unauthorized) = api.me(&stored.session_token).await {
                    let _ = store.clear();
                    post(&weak, Update::SignedOut(Some(SignInProblem::SessionEnded)));
                }
            }
        }
    });
}

/// Maps a client error from the browser sign-in to the message the screen shows.
fn problem_of(error: &ClientError) -> SignInProblem {
    match error {
        ClientError::SignInTimeout => SignInProblem::Timeout,
        // The backend's codes (backend/api `service.rs`, ERR_*): an account without access is "not provisioned" or "not employee".
        ClientError::SignInDenied(code) if code == "not_provisioned" || code == "not_employee" => {
            SignInProblem::NotProvisioned
        }
        ClientError::SignInDenied(code) if code == "server_error" => SignInProblem::Other(
            "Rimelight Space could not complete sign-in. Try again in a moment.".to_string(),
        ),
        ClientError::SignInDenied(_) => SignInProblem::Denied,
        ClientError::Network(_) => SignInProblem::Unreachable,
        other => SignInProblem::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeout_and_a_denial_have_their_own_messages() {
        assert_eq!(
            problem_of(&ClientError::SignInTimeout),
            SignInProblem::Timeout
        );
        assert_eq!(
            problem_of(&ClientError::SignInDenied("access_denied".into())),
            SignInProblem::Denied
        );
        assert_eq!(
            problem_of(&ClientError::SignInDenied("not_provisioned".into())),
            SignInProblem::NotProvisioned
        );
    }

    #[test]
    fn backend_codes_map_to_their_problems() {
        assert_eq!(
            problem_of(&ClientError::SignInDenied("not_employee".into())),
            SignInProblem::NotProvisioned
        );
        assert_eq!(
            problem_of(&ClientError::SignInDenied("access_denied".into())),
            SignInProblem::Denied
        );
        assert_eq!(
            problem_of(&ClientError::SignInDenied("server_error".into())),
            SignInProblem::Other(
                "Rimelight Space could not complete sign-in. Try again in a moment.".into()
            )
        );
    }

    #[test]
    fn a_network_failure_is_unreachable_and_the_rest_keep_their_text() {
        assert_eq!(
            problem_of(&ClientError::Network("refused".into())),
            SignInProblem::Unreachable
        );
        assert_eq!(
            problem_of(&ClientError::Unsupported("sign-in on mobile and web")),
            SignInProblem::Other("not supported yet: sign-in on mobile and web".into())
        );
    }
}
