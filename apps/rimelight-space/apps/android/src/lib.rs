//! rimelight_android: the Android app. android-activity calls `android_main` from the NativeActivity and this opens the
//! same app shell as the desktop app; below the phone breakpoint the shell shows bottom tabs and a drawer (PLAN 6.2).
//!
//! Build: `cargo ndk -t x86_64 -o apps/android/android/app/src/main/jniLibs build -p rimelight_android`, then Gradle
//! (`apps/android/android/gradlew assembleDebug`).
//!
//! Saving (planned feature B3): the workspaces are kept in `workspaces.txt` in the app's internal data folder. They are
//! saved about a second after a change, and when the activity saves its state, pauses or stops.
//!
//! Sign-in (SA.7, decision 0018): the system browser opens the login, and the browser returns `rimelight-space://auth` to
//! the activity. The return arrives as the activity's intent (`RimelightActivity` keeps the newest one); `android_main`
//! reads it at start and on each resume and hands it to the session driver. The session is kept in `session.json` in the
//! internal data folder.

#[cfg(target_os = "android")]
use std::cell::RefCell;
#[cfg(target_os = "android")]
use std::rc::Rc;
#[cfg(target_os = "android")]
use std::sync::Arc;

#[cfg(target_os = "android")]
use rimelight_platform::android as platform;
#[cfg(target_os = "android")]
use slint::ComponentHandle;
#[cfg(target_os = "android")]
use slint::android::android_activity::{MainEvent, PollEvent};
#[cfg(target_os = "android")]
use space_area_projects::issue;
#[cfg(target_os = "android")]
use space_client::api::ApiClient;
#[cfg(target_os = "android")]
use space_client::error::ClientError;
#[cfg(target_os = "android")]
use space_client::session_store::{FileSessionStore, MemorySessionStore, SessionStore};
#[cfg(target_os = "android")]
use space_ui::slint_ui::SessionPhase;
#[cfg(target_os = "android")]
use space_ui::slint_ui::app_shell::{self, App};
#[cfg(target_os = "android")]
use space_ui::slint_ui::session_driver::{self, BrowserOpener, Mobile, Session};
#[cfg(target_os = "android")]
use space_ui::slint_ui::workspace::{FileStore, MemoryStore, Store};
#[cfg(target_os = "android")]
use space_ui::theme::ThemeMode;

/// The file of the saved workspaces, in the app's internal data folder.
#[cfg(target_os = "android")]
const SAVE_FILE: &str = "workspaces.txt";

/// The file of the stored session, in the app's internal data folder (decision 0018).
#[cfg(target_os = "android")]
const SESSION_FILE: &str = "session.json";

/// The Rimelight Space API. The emulator reaches the host's localhost at 10.0.2.2. A physical device needs an address it can
/// reach (decision 0018). `RIMELIGHT_API_URL` at build time overrides it.
#[cfg(target_os = "android")]
const DEFAULT_API_URL: &str = "https://rimelight.com";

/// Entry point called by android-activity. Opens the shell with the saved workspaces, or one Home workspace.
///
/// The only unsafe item in this crate: the export symbol android-activity looks up. See PROJECT.md section 15.4.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    // The data folder is read before the app is handed to Slint. Without one, the workspaces are kept in memory only.
    let data_dir = app.internal_data_path();
    let store: Rc<dyn Store> = match &data_dir {
        Some(dir) => Rc::new(FileStore::at(dir.join(SAVE_FILE))),
        None => Rc::new(MemoryStore::default()),
    };
    // The sign-in return the app was started with, if any. The launch intent is read before the window opens.
    let launch_return = platform::intent_data(&app).ok().flatten();
    // The activity's save, pause and stop events save the workspaces. The listener runs on this thread, so it can hold
    // the app (which is not Send) once it is built.
    let running: Rc<RefCell<Option<Rc<App>>>> = Rc::new(RefCell::new(None));
    let listener_running = running.clone();
    // The session driver, once it is attached, and the last sign-in return handled (so a return is used once).
    let session: Rc<RefCell<Option<Session>>> = Rc::new(RefCell::new(None));
    let listener_session = session.clone();
    let last_return: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(launch_return.clone()));
    let listener_last_return = last_return.clone();
    let listener_app = app.clone();
    let opener_app = app.clone();
    let saver_app = app.clone();
    slint::android::init_with_event_listener(app, move |event| match event {
        PollEvent::Main(MainEvent::SaveState { .. } | MainEvent::Pause | MainEvent::Stop) => {
            if let Some(app) = listener_running.borrow().as_ref() {
                app.flush();
            }
        }
        // The browser's return arrives as a new intent while the activity runs; it is read here, once per address.
        PollEvent::Main(MainEvent::Resume { .. }) => {
            let Ok(Some(url)) = platform::intent_data(&listener_app) else {
                return;
            };
            if listener_last_return.borrow().as_deref() == Some(url.as_str()) {
                return;
            }
            *listener_last_return.borrow_mut() = Some(url.clone());
            if let Some(session) = listener_session.borrow().as_ref() {
                session.sign_in_returned(&url);
            }
        }
        _ => {}
    })
    .expect("android init");
    let app = Rc::new(
        app_shell::build_with_store(
            None,
            ThemeMode::System,
            Rc::new(app_shell::NoLocation),
            store,
        )
        .expect("app shell"),
    );
    *running.borrow_mut() = Some(app.clone());
    // The window opens on the loading phase; the session driver checks the stored session and sets the next phase.
    app.ui.set_session_phase(SessionPhase::Loading);
    let base = option_env!("RIMELIGHT_API_URL").unwrap_or(DEFAULT_API_URL);
    let api = Arc::new(ApiClient::new(base));
    let attached = match data_dir {
        Some(dir) => {
            let opener: BrowserOpener = Rc::new(move |url: &str| {
                platform::open_url(&opener_app, url).map_err(ClientError::Browser)
            });
            let mobile = Mobile {
                opener,
                data_dir: dir.clone(),
                launch_return,
            };
            let sessions: Arc<dyn SessionStore> =
                Arc::new(FileSessionStore::new(dir.join(SESSION_FILE)));
            session_driver::attach_mobile(&app, api, sessions, mobile)
        }
        // Without a data folder there is nowhere to keep the pending sign-in, so the sign-in screen reports that it cannot
        // sign in, and the session lasts as long as the app.
        None => session_driver::attach(&app, api, Arc::new(MemorySessionStore::new())),
    };
    *session.borrow_mut() = Some(attached);
    // Attachment downloads write to the Downloads folder through the activity (issue.rs save_attachment).
    issue::set_android_saver(move |name, bytes| platform::save_download(&saver_app, name, bytes));
    app.ui.run().expect("app shell");
}
