//! The web app: the Slint app shell in a browser canvas.
//!
//! Build: `trunk build --release` in `apps/web` (needs trunk and the `wasm32-unknown-unknown` target). Trunk loads the
//! wasm and calls `web_main` on start, which opens the shell on the page's `<canvas id="canvas">`. Native builds open the
//! same shell in a window, as apps/desktop does.
//!
//! The browser address bar follows the active page tab (planned feature A3, `browser`): a page in the address bar is the
//! deep link the start opens (as a new tab when it is not the saved active page), each route change is a history entry,
//! and Back or Forward in the browser moves the tab.
//!
//! Saving (planned feature B3, per-tab since D11): each browser tab keeps its own workspaces in sessionStorage under
//! `rimelight-space.workspaces`. A reload of that tab restores them, and they are gone when the tab closes. Every save also
//! writes the same text to localStorage under `rimelight-space.workspaces.last`, the seed for a new tab and for a browser
//! restart. A tab starts from its own sessionStorage state, else `.last`, else the old B3a localStorage key
//! `rimelight-space.workspaces` (read once; never written again). Tabs never read each other's state after start-up, and
//! there is no storage-event handling. Every storage call is in try/catch: a refused write is an error and the app runs on.
//! Saves are about a second after a change, and when the page is hidden (`pagehide`, or `visibilitychange` to hidden).

#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;

#[cfg(not(target_arch = "wasm32"))]
use space_ui::slint_ui::app_shell;
#[cfg(not(target_arch = "wasm32"))]
use space_ui::slint_ui::workspace::{APP_FOLDER, FileStore, MemoryStore, Store};
#[cfg(not(target_arch = "wasm32"))]
use space_ui::theme::ThemeMode;

#[cfg(not(target_arch = "wasm32"))]
fn open() -> Result<(), slint::PlatformError> {
    let store: Rc<dyn Store> = match FileStore::in_app_data(APP_FOLDER) {
        Some(store) => Rc::new(store),
        None => Rc::new(MemoryStore::default()),
    };
    app_shell::run(None, ThemeMode::System, store)
}

/// The Rimelight Space API for the web build. `RIMELIGHT_API_URL` at build time overrides it (for a staging backend).
#[cfg(target_arch = "wasm32")]
const API_URL: &str = match option_env!("RIMELIGHT_API_URL") {
    Some(url) => url,
    None => "https://rimelight.com",
};

#[cfg(target_arch = "wasm32")]
mod browser {
    //! The browser address bar and the saved workspaces. The few DOM calls are inline JavaScript, so this crate adds no
    //! web-sys dependency; the wasm-bindgen dependency it already has provides the glue.

    use std::rc::Rc;
    use std::sync::Arc;

    use space_client::api::ApiClient;
    use space_client::session_store::{SessionStore, WebSessionStore};
    use space_ui::slint_ui::app_shell::{self, App, Location};
    use space_ui::slint_ui::session_driver;
    use space_ui::slint_ui::workspace::{Store, choose_start_text};
    use space_ui::slint_ui::{SessionPhase, clipboard, history};
    use space_ui::theme::ThemeMode;

    use super::API_URL;
    use slint::ComponentHandle as _;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen(inline_js = "
        export function current_location() { return window.location.pathname + window.location.search; }
        export function push_location(route) { window.history.pushState(route, '', route); }
        export function replace_location(route) { window.history.replaceState(route, '', route); }
        export function on_popstate(handler) {
            window.addEventListener('popstate', () => handler(window.location.pathname + window.location.search));
        }
        function storage_area(area) {
            return area === 'session' ? window.sessionStorage : window.localStorage;
        }
        export function storage_load(area, key) {
            try {
                const value = storage_area(area).getItem(key);
                return value === null ? undefined : value;
            } catch (error) {
                return undefined;
            }
        }
        export function storage_save(area, key, text) {
            try {
                storage_area(area).setItem(key, text);
                return true;
            } catch (error) {
                return false;
            }
        }
        export function on_page_hide(handler) {
            window.addEventListener('pagehide', () => handler());
            document.addEventListener('visibilitychange', () => {
                if (document.visibilityState === 'hidden') handler();
            });
        }
        // Copy link (gaps D2). Called from a menu pick, which is a user gesture. `done` gets true when the browser wrote the
        // text and false when it refused (no permission, no secure context, no clipboard API), so the toast can tell them apart.
        export function copy_text(text, done) {
            try {
                if (navigator.clipboard && navigator.clipboard.writeText) {
                    navigator.clipboard.writeText(text).then(() => done(true), () => done(false));
                } else {
                    done(false);
                }
            } catch (error) {
                done(false);
            }
        }
    ")]
    extern "C" {
        fn current_location() -> String;
        fn push_location(route: &str);
        fn replace_location(route: &str);
        fn on_popstate(handler: &Closure<dyn FnMut(String)>);
        fn storage_load(area: &str, key: &str) -> Option<String>;
        fn storage_save(area: &str, key: &str, text: &str) -> bool;
        fn on_page_hide(handler: &Closure<dyn FnMut()>);
        fn copy_text(text: &str, done: JsValue);
    }

    /// The page's address bar, written from the active tab.
    struct AddressBar;

    impl Location for AddressBar {
        fn push(&self, route: &str) {
            push_location(route);
        }

        fn replace(&self, route: &str) {
            replace_location(route);
        }
    }

    /// This tab's workspaces (sessionStorage), and the key of the old B3a localStorage save.
    const TAB_KEY: &str = "rimelight-space.workspaces";
    /// The last saved workspaces of any tab (localStorage): the seed for a new tab.
    const LAST_KEY: &str = "rimelight-space.workspaces.last";
    /// The B3a key, read once as a migration source and never written.
    const LEGACY_KEY: &str = "rimelight-space.workspaces";

    /// The per-tab store (D11). A save writes this tab's sessionStorage and the localStorage seed. A browser that refuses
    /// storage (private mode, blocked site data) loads nothing and its saves fail, and the app runs on.
    struct TabStore;

    impl Store for TabStore {
        fn load(&self) -> Option<String> {
            let session = storage_load("session", TAB_KEY);
            let last = storage_load("local", LAST_KEY);
            let legacy = storage_load("local", LEGACY_KEY);
            let start = choose_start_text(session.as_deref(), last.as_deref(), legacy.as_deref());
            start.text.map(str::to_owned)
        }

        fn save(&self, text: &str) -> Result<(), String> {
            if !storage_save("session", TAB_KEY, text) {
                return Err("sessionStorage refused the save".to_owned());
            }
            if !storage_save("local", LAST_KEY, text) {
                return Err("localStorage refused the seed save".to_owned());
            }
            Ok(())
        }
    }

    /// Opens the shell on the deep link the address bar shows (none for the bare root or an unknown path), with the saved
    /// workspaces. Follows Back and Forward in the browser, saves when the page is hidden, and runs the event loop.
    pub fn open() -> Result<(), slint::PlatformError> {
        let location = current_location();
        let start = history::deep_link(&location);
        // Copy link writes through the browser: Slint's copy only works inside a browser copy event.
        clipboard::set_browser_writer(|link, done| {
            let done = Closure::once_into_js(move |copied: bool| done(copied));
            copy_text(link, done);
        });
        let app: Rc<App> = Rc::new(app_shell::build_with_store(
            start.as_deref(),
            ThemeMode::System,
            Rc::new(AddressBar),
            Rc::new(TabStore),
        )?);
        // The browser's tabs do the job of the workspace strip (IA part B, P5). The workspaces still exist, and the switcher
        // sheet is the way to them once two or more are open.
        app.show_workspace_strip(false);
        let popstate_app = app.clone();
        let on_pop = Closure::<dyn FnMut(String)>::new(move |location: String| {
            popstate_app.location_changed(&location);
        });
        on_popstate(&on_pop);
        // The listener lives as long as the page does.
        on_pop.forget();
        let hide_app = app.clone();
        let on_hide = Closure::<dyn FnMut()>::new(move || hide_app.flush());
        on_page_hide(&on_hide);
        on_hide.forget();
        // Sign-in (SA.6), wired the same way as apps/desktop: the window opens on the loading phase, the driver finishes a
        // sign-in return or restores the stored session, and sets the next phase.
        app.ui.set_session_phase(SessionPhase::Loading);
        let api = Arc::new(ApiClient::new(API_URL));
        let sessions: Arc<dyn SessionStore> = Arc::new(WebSessionStore);
        let session = session_driver::attach(&app, api, sessions);
        // The periodic server check lives as long as the page does (dropping the timer would stop it).
        std::mem::forget(session);
        app.ui.run()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn web_main() {
    browser::open().expect("rimelight_web: the Slint window failed to run");
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    match open() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rimelight_web: {e}");
            ExitCode::FAILURE
        }
    }
}
