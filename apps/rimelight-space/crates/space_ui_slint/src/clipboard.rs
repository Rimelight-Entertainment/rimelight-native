//! The clipboard writer of Copy link (gaps D2), here because the area glue macros (`wire_projects!`) expand in this crate and
//! take it by path. `space_ui::slint_ui::clipboard` re-exports it and keeps the toast.
//!
//! Desktop and Android: the AppWindow's `copy-link` function (`ui/components/clipboard-writer.slint`) selects its hidden
//! TextInput's text and calls the TextInput's `copy()`, which Slint hands to the platform clipboard. That path is synchronous,
//! so its outcome is known at once.
//!
//! Web: Slint's `copy()` only works inside a browser copy event, so it does nothing from a menu pick. The web app registers
//! its own writer (`apps/web`, a `navigator.clipboard.writeText` call). The browser answers later, from a promise, so the
//! writer takes a completion callback and calls it with `true` (written) or `false` (refused). A writer is registered once per
//! thread, before the window is built.

use std::cell::RefCell;
use std::rc::Rc;

use crate::AppWindow;

/// A browser writer: takes the text of a link and a callback that gets the outcome (`true` when the browser took it).
type BrowserWriter = Rc<dyn Fn(&str, Box<dyn FnOnce(bool)>)>;

thread_local! {
    /// The browser writer, when the web app registered one. `None` on desktop and Android.
    static BROWSER_WRITER: RefCell<Option<BrowserWriter>> = const { RefCell::new(None) };
}

/// Registers the browser writer for this thread. The web app calls it once before it builds the window. Copy link then
/// calls `write` with the link and a completion callback, instead of the Slint clipboard path.
pub fn set_browser_writer(write: impl Fn(&str, Box<dyn FnOnce(bool)>) + 'static) {
    BROWSER_WRITER.with(|slot| *slot.borrow_mut() = Some(Rc::new(write)));
}

/// Writes `link` to the clipboard, then calls `done` with whether it was written. On desktop and Android the write is
/// synchronous, so `done` runs before this returns, with `true` (the AppWindow's `last-copied` property records the text). With
/// a browser writer, `done` runs when the browser answers.
pub fn write_link(ui: &AppWindow, link: &str, done: impl FnOnce(bool) + 'static) {
    let browser = BROWSER_WRITER.with(|slot| slot.borrow().clone());
    match browser {
        Some(write) => write(link, Box::new(done)),
        None => {
            ui.invoke_copy_link(link.into());
            done(true);
        }
    }
}
