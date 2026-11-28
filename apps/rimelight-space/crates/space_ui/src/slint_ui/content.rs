//! The Content area's window glue (CMS.6): `wire_content!` expanded for the generated window, the block editor's binder for
//! the page, and the route dispatch the shell calls when a content page reports `content-opened`.
//!
//! The content pages are one set of globals per window. A page is wired for its route when it opens (the wiring replaces the
//! callbacks of the one before). UNSURE: two tile groups that both show content pages share the globals, as the other areas
//! do; the newer route's state wins.

use std::cell::RefCell;
use std::rc::Rc;

use rimelight_editor::{EditorController, HostEvent, RemoteCursor};
use space_area_content::page::{self, EditorBinder, EditorHandle, PageDeps};
use space_area_content::presence;
use space_area_content::routes::{self, Route};
use space_area_content::scope::Scope;
use space_area_content::{collections, home, media, review};
use space_page_kit::page_data::DataCell;
use space_page_kit::projects;

use crate::slint_ui::AppWindow;
use crate::slint_ui::block_editor::{BlockEditorBinding, bind_block_editor};
use crate::slint_ui::webviews::WebHost;
use slint::ComponentHandle as _;

// wire_content! is expanded in space_ui_slint, where AppWindow is generated (orphan rule), as the other areas do.

/// The editor binding over the window's `EditorBridge`. The page's controller and host callback bind each time a page opens:
/// one editor is live per window, and the newest binding takes over its callbacks (the editor's README).
struct Bound(Rc<BlockEditorBinding>);

impl EditorHandle for Bound {
    fn refresh(&self) {
        self.0.refresh();
    }

    fn set_remote_cursors(&self, cursors: Vec<RemoteCursor>) {
        self.0.set_remote_cursors(cursors);
    }
}

/// The handle when the window is gone: the page keeps its state, and nothing is drawn.
struct Detached;

impl EditorHandle for Detached {
    fn refresh(&self) {}

    fn set_remote_cursors(&self, _cursors: Vec<RemoteCursor>) {}
}

/// The editor binder of the content page: binds the controller to the window's editor.
fn editor_binder(window: slint::Weak<AppWindow>) -> EditorBinder {
    Box::new(
        move |controller: Rc<RefCell<EditorController>>,
              on_host: Box<dyn Fn(HostEvent)>|
              -> Rc<dyn EditorHandle> {
            match window.upgrade() {
                Some(ui) => Rc::new(Bound(bind_block_editor(&ui, controller, on_host))),
                None => Rc::new(Detached),
            }
        },
    )
}

/// The content page kind of a route, for the shell's `content-kind` callback: "home", "page", "collections", "media",
/// "reviews", or "" when the route is not a content page.
pub fn kind(route: &str) -> &'static str {
    match routes::parse(route) {
        Some(Route::ScopeHome(_)) => "home",
        Some(Route::Page { .. }) => "page",
        Some(Route::Collections) => "collections",
        Some(Route::Media) => "media",
        Some(Route::Reviews) => "reviews",
        None => "",
    }
}

/// What the content pages need from the shell: the page data slot and the web views of the editor's web-only blocks.
pub struct Context {
    pub data: DataCell,
    pub web: Rc<RefCell<WebHost>>,
}

/// The projects as scope picker rows: (id, name). A project's scope id is its key (UNSURE: the server's scope id is the
/// project's key here, as the `/p/:key` routes use it).
fn project_pairs() -> Vec<(String, String)> {
    projects::list()
        .into_iter()
        .map(|p| (p.key, p.name))
        .collect()
}

/// The scopes the reviews page searches: Company, then each project the person is in.
fn review_scopes() -> Vec<(Scope, String)> {
    let mut scopes = vec![(Scope::Company, "Company".to_string())];
    for project in projects::list() {
        scopes.push((
            Scope::Project(project.key),
            format!("Project: {}", project.name),
        ));
    }
    scopes
}

/// Wires the content page the route names onto the window, and loads it. The shell calls it when a content page reports
/// `content-opened`. Routes that are not content pages do nothing.
pub fn open(ui: &AppWindow, cx: &Context, route: &str) {
    let Some(parsed) = routes::parse(route) else {
        return;
    };
    match parsed {
        Route::ScopeHome(scope) => {
            let view = home::wire(ui, cx.data.clone(), scope);
            // The move dialog's teams and departments come from the home's own load.
            view.set_projects(project_pairs());
            view.load();
        }
        Route::Page {
            scope,
            page: page_id,
            edit,
        } => {
            let web = cx.web.clone();
            let deps = PageDeps {
                binder: editor_binder(ui.as_weak()),
                on_web: Box::new(move |event| web.borrow_mut().on_editor_event(event)),
                name: ui.get_user_name().to_string(),
                // The caller's stable presence colour: the hash of their user id (`presence::colour_for`).
                color: presence::colour_hex(ui.get_user_id().as_str()),
            };
            let view = page::wire(ui, cx.data.clone(), scope, &page_id, edit, deps);
            view.load();
        }
        Route::Collections => collections::wire(ui, cx.data.clone()).load(),
        Route::Media => media::wire(ui, cx.data.clone(), None).load(),
        Route::Reviews => {
            let view = review::wire_reviews(ui, cx.data.clone());
            view.set_scopes(review_scopes());
            view.load();
        }
    }
}
