//! Saved workspaces (planned feature B3; IA part B). The window loads them at start, saves about a second after a change,
//! and saves on exit. The store is a spy in memory, so no test touches the disk or the app-data folder.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use space_ui::slint_ui::app_shell::{self, App, NoLocation};
use space_ui::slint_ui::workspace::{Store, Workspaces};
use space_ui::theme::ThemeMode;

/// A store in memory that counts its writes.
#[derive(Default)]
struct SpyStore {
    text: RefCell<Option<String>>,
    writes: Cell<u32>,
}

impl SpyStore {
    /// A spy that already holds `text`, as a save from an earlier run would.
    fn holding(text: &str) -> Rc<Self> {
        Rc::new(Self {
            text: RefCell::new(Some(text.to_owned())),
            writes: Cell::new(0),
        })
    }
}

impl Store for SpyStore {
    fn load(&self) -> Option<String> {
        self.text.borrow().clone()
    }

    fn save(&self, text: &str) -> Result<(), String> {
        self.writes.set(self.writes.get() + 1);
        *self.text.borrow_mut() = Some(text.to_owned());
        Ok(())
    }
}

/// Builds the window on `store` with no deep link.
fn open(store: Rc<SpyStore>) -> App {
    app_shell::build_with_store(None, ThemeMode::Light, Rc::new(NoLocation), store)
        .expect("window builds")
}

/// The number of workspaces in a saved text, or `usize::MAX` when the text does not decode.
fn saved_count(text: &str) -> usize {
    Workspaces::decode(text).map_or(usize::MAX, |ws| ws.len())
}

/// Runs the timers that are due on the mock clock.
fn tick() {
    slint::platform::update_timers_and_animations();
}

pub fn saved_workspaces_load_at_start_and_the_exit_save_writes_them_back() {
    crate::support::init();
    // A window makes a state (a second workspace, named and pinned), and closing the window saves it.
    let spy = Rc::new(SpyStore::default());
    let app = open(spy.clone());
    app.ui.invoke_new_workspace_requested();
    app.ui.invoke_workspace_renamed(1, "Sales".into());
    app.ui.invoke_workspace_menu_picked(1, "pin".into());
    app.flush();
    let saved = spy.text.borrow().clone().expect("the exit save writes");
    assert_eq!(saved, app.state_text().expect("encodes"));
    assert_eq!(saved_count(&saved), 2);
    drop(app);

    // The next start loads the same state, and an exit with no change writes nothing.
    let again_spy = SpyStore::holding(&saved);
    let again = open(again_spy.clone());
    assert_eq!(again.state_text().expect("encodes"), saved);
    again.flush();
    assert_eq!(
        again_spy.writes.get(),
        0,
        "an unchanged state is not written"
    );
}

pub fn a_corrupt_save_starts_with_one_workspace_and_does_not_crash() {
    crate::support::init();
    let spy = SpyStore::holding("v9\nthis is not a workspace file");
    let app = app_shell::build_with_store(
        Some("/home/todos"),
        ThemeMode::Light,
        Rc::new(NoLocation),
        spy.clone(),
    )
    .expect("window builds on a corrupt save");
    let state = app.state_text().expect("encodes");
    assert_eq!(saved_count(&state), 1, "one workspace");
    assert_eq!(app.ui.get_shell_area(), 0, "the Home area shows");
}

/// A change is not saved on the spot: the save waits for the debounce timer (about a second) or for the exit. `flush` saves
/// at once. The one-second timer itself is not checked here: the testing backend does not fire Slint timers from mock time
/// or from the real clock (checked 2026-11-13), so the delay is verified only by reading the code and by a real run.
pub fn a_change_waits_for_the_debounce_and_flush_saves_it_at_once() {
    crate::support::init();
    // Start from a saved one-workspace state, so the start itself writes nothing new.
    let first = open(Rc::new(SpyStore::default()));
    let text = first.state_text().expect("encodes");
    drop(first);
    let spy = SpyStore::holding(&text);
    let app = open(spy.clone());
    tick();

    app.ui.invoke_new_workspace_requested();
    tick();
    assert_eq!(spy.writes.get(), 0, "a change is not written on the spot");

    app.flush();
    assert_eq!(spy.writes.get(), 1, "flush writes the change at once");
    let saved = spy.text.borrow().clone().expect("saved");
    assert_eq!(saved_count(&saved), 2, "the new workspace is in the save");
    app.flush();
    assert_eq!(
        spy.writes.get(),
        1,
        "a second flush with no change writes nothing"
    );
}

pub fn closing_every_workspace_saves_the_empty_state() {
    crate::support::init();
    let spy = Rc::new(SpyStore::default());
    let app = open(spy.clone());
    app.ui.invoke_close_workspace_requested();
    app.flush();
    let saved = spy.text.borrow().clone().expect("the exit save writes");
    assert_eq!(saved_count(&saved), 0, "the empty state is saved");
    drop(app);

    let again = open(SpyStore::holding(&saved));
    assert_eq!(saved_count(&again.state_text().expect("encodes")), 0);
}
