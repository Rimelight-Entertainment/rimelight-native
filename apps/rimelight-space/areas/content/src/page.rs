//! One page (CMS.6): the published page read-only with the shared block editor and the comments panel, and edit mode on the
//! caller's draft (the editor bound to an `EditorDoc` loaded from the draft room, presence, invite, publish, discard, catch up).
//!
//! Routes: `/p/:key/docs/:page`, `/company/knowledge/:page`, `/company/teams/:id/docs/:page`, `/company/departments/:id/docs/:page`,
//! with `?mode=edit` to start in edit mode.
//!
//! Threads. Everything that touches the editor (the Rc-based controller and the Slint page) runs on the UI thread. Worker results
//! and room events arrive from other threads into an inbox; a UI-thread timer drains it (`pump`), so no closure that crosses
//! a thread captures this page's state.
//!
//! The editor is a Slint component the app owns. The app passes an [`EditorBinder`] that builds the binding (the macro
//! `rimelight_editor::wire_block_editor!` in the app crate) and returns it as an [`EditorHandle`]. Web-only blocks report their
//! boxes as `HostEvent::WebGeometry`; the page forwards those to `PageDeps::on_web`, which the app points at its
//! `rimelight_platform::webview::WebViews`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rimelight_content::editor::EditorDoc;
use rimelight_content::import_page;
use rimelight_content::sync::{Frame, Received, SyncSession};
use rimelight_editor::controller::{
    CommentRequest, CommentView as EditorComment, ControllerEffect, MentionChoice,
};
use rimelight_editor::{EditorController, HostEvent, RemoteCursor};
// RemoteCursor is the same type as rimelight_content::sync::RemoteCursor (re-exported by rimelight_editor).
use serde_json::{Map, Value, json};
use slint::{ComponentHandle, Timer, TimerMode, Weak};
use space_client::api::ApiClient;
use space_client::content::{
    CommentChange, CommentView, ContentCollection, DraftView, MentionResult, MovePage, PageDetail,
    PatchPage, PublishResult, Visibility,
};
use space_client::content_live::{
    ContentEventSink, ContentLiveEvent, ContentLiveSession, ContentRoom, RoomEvent,
};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{DataCell, DataSource, error_text};
use space_page_kit::projects;

use crate::routes::home_route;
use crate::scope::{
    Scope, ScopeChoice, can_edit, can_manage, move_targets, parse_access, scope_choices,
};
use crate::view::{
    FALLBACK_LOCALE, localized, short_time, status_label, valid_slug, visibility_label,
};

/// How often the UI thread drains the inbox.
const PUMP_EVERY: Duration = Duration::from_millis(30);

/// The window side of one page. `wire_content!` implements it for the generated window.
pub trait PageWindow: ComponentHandle + 'static {
    fn user_id(&self) -> String;
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_header(&self, header: PageHeader);
    /// Read-only (false) or edit mode (true). Edit mode shows the editor; read mode shows the published page in it.
    fn show_mode(&self, edit: bool);
    /// The connection line over the editor: empty when connected, else the reason ("Reconnecting in 3 s", "Lost access").
    fn show_connection(&self, text: &str);
    fn show_notice(&self, text: &str);
    fn show_catch_up(&self, on: bool);
    fn copy_text(&self, text: &str);
    fn navigate(&self, route: &str);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_edit(&self, f: impl Fn() + 'static);
    fn on_discard(&self, f: impl Fn() + 'static);
    fn on_publish(&self, f: impl Fn(String) + 'static);
    fn on_invite(&self, f: impl Fn(String) + 'static);
    fn on_catch_up(&self, f: impl Fn() + 'static);
    fn on_share(&self, f: impl Fn() + 'static);
    fn on_leave(&self, f: impl Fn() + 'static);
    /// The page panel: the slug, public or internal, and the collection properties (one row per field, in field order).
    fn show_panel(&self, slug: &str, public: bool, properties: Vec<PropertyRow>);
    /// The Move choices: open with their labels (`move_to` takes an index into them), or closed.
    fn show_move(&self, open: bool, labels: Vec<String>);
    fn on_slug_changed(&self, f: impl Fn(String) + 'static);
    fn on_visibility_changed(&self, f: impl Fn(bool) + 'static);
    fn on_move_requested(&self, f: impl Fn() + 'static);
    fn on_move_chosen(&self, f: impl Fn(i32) + 'static);
    fn on_delete_requested(&self, f: impl Fn() + 'static);
    fn on_property_changed(&self, f: impl Fn(String, String) + 'static);
}

/// The editor page as the page drives it. The app's binding implements it.
pub trait EditorHandle {
    /// Redraws the rows from the controller (after a co-editor's change).
    fn refresh(&self);
    /// The room's remote cursors and presence (redraws the carets, colour bars and presence strip).
    fn set_remote_cursors(&self, cursors: Vec<RemoteCursor>);
}

/// Builds the editor binding for the controller. `on_host` receives every effect and web-only block geometry. Keep the returned
/// handle alive while the page is in edit mode.
pub type EditorBinder =
    Box<dyn Fn(Rc<RefCell<EditorController>>, Box<dyn Fn(HostEvent)>) -> Rc<dyn EditorHandle>>;

/// What a page needs from the app, besides its window.
pub struct PageDeps {
    pub binder: EditorBinder,
    /// Web-only block geometry (`HostEvent::WebGeometry`), for the app's `WebViews`.
    pub on_web: Box<dyn Fn(HostEvent)>,
    /// The caller's name and presence colour (hex). Used for presence in the draft.
    pub name: String,
    pub color: String,
}

/// The page header as the window draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageHeader {
    pub title: String,
    pub collection: String,
    pub scope_label: String,
    pub visibility: &'static str,
    pub status: &'static str,
    pub can_edit: bool,
    pub can_manage: bool,
    pub has_draft: bool,
}

/// One comment in the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentRow {
    pub id: String,
    pub author: String,
    pub body: String,
    pub when: String,
    pub resolved: bool,
}

/// One person in the presence strip.
#[derive(Debug, Clone, PartialEq)]
pub struct PresenceChip {
    pub initials: String,
    pub name: String,
    pub colour: (u8, u8, u8),
}

/// One collection property in the page panel: the field's key, label and kind, the page's value of it as text, and the
/// choices (value, label) of a select.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRow {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub value: String,
    pub options: Vec<(String, String)>,
}

/// What worker results and room events hand to the UI thread.
enum Inbox {
    Loaded {
        detail: PageDetail,
        collections: Vec<ContentCollection>,
        comments: Vec<CommentView>,
        drafts: Vec<DraftView>,
    },
    LoadFailed(String),
    Comments(Vec<CommentView>),
    Mentions(Vec<MentionResult>),
    DraftOpened(DraftView),
    Published(PublishResult),
    Discarded,
    DraftChanged(DraftView),
    Live(ContentLiveEvent),
    Notice(String),
    /// A change went through: reload the page.
    Reload,
    /// The Move targets, in the order the panel lists them.
    MoveChoices(Vec<ScopeChoice>),
}

/// The page's state. Only the UI thread reads and writes it.
#[derive(Default)]
struct State {
    detail: Option<PageDetail>,
    collections: Vec<ContentCollection>,
    comments: Vec<CommentView>,
    /// The caller's open draft, when in edit mode.
    draft: Option<DraftView>,
    edit: bool,
    catch_up: bool,
    controller: Option<Rc<RefCell<EditorController>>>,
    sync: Option<SyncSession>,
    editor: Option<Rc<dyn EditorHandle>>,
    session: Option<ContentLiveSession>,
    /// The page's main room, for its `published` event (the main line moved).
    main_session: Option<ContentLiveSession>,
    /// The read-only editor of the published page (read mode), and its handle.
    view_controller: Option<Rc<RefCell<EditorController>>>,
    view_editor: Option<Rc<dyn EditorHandle>>,
    /// The Move targets the panel offers, by index (see `Inbox::MoveChoices`).
    move_choices: Vec<ScopeChoice>,
}

struct Inner<W: PageWindow> {
    ui: Weak<W>,
    data: DataCell,
    scope: Scope,
    page_id: String,
    /// Opens the draft on the first load when the route says `?mode=edit`; cleared after that.
    autostart: Cell<bool>,
    deps: PageDeps,
    locale: RefCell<String>,
    inbox: Arc<Mutex<VecDeque<Inbox>>>,
    state: RefCell<State>,
    timer: RefCell<Option<Timer>>,
}

/// The page: its state and window. Clones share the state; the window's callbacks hold clones.
pub struct Page<W: PageWindow> {
    inner: Rc<Inner<W>>,
}

impl<W: PageWindow> Clone for Page<W> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui` for the page `page_id` in `scope`. `edit` starts in edit mode (`?mode=edit`).
pub fn wire<W: PageWindow>(
    ui: &W,
    data: DataCell,
    scope: Scope,
    page_id: &str,
    edit: bool,
    deps: PageDeps,
) -> Page<W> {
    let inner = Rc::new(Inner {
        ui: ui.as_weak(),
        data,
        scope,
        page_id: page_id.to_string(),
        autostart: Cell::new(edit),
        deps,
        locale: RefCell::new(FALLBACK_LOCALE.to_string()),
        inbox: Arc::new(Mutex::new(VecDeque::new())),
        state: RefCell::new(State::default()),
        timer: RefCell::new(None),
    });
    let page = Page { inner };

    let p = page.clone();
    ui.on_retry(move || p.load());
    let p = page.clone();
    ui.on_edit(move || p.start_edit());
    let p = page.clone();
    ui.on_discard(move || p.discard());
    let p = page.clone();
    ui.on_publish(move |summary| p.publish(&summary));
    let p = page.clone();
    ui.on_invite(move |user| p.invite(&user));
    let p = page.clone();
    ui.on_catch_up(move || p.catch_up());
    let p = page.clone();
    ui.on_share(move || p.share());
    let p = page.clone();
    ui.on_leave(move || p.leave());
    let p = page.clone();
    ui.on_slug_changed(move |slug| p.change_slug(&slug));
    let p = page.clone();
    ui.on_visibility_changed(move |public| p.change_visibility(public));
    let p = page.clone();
    ui.on_move_requested(move || p.open_move());
    let p = page.clone();
    ui.on_move_chosen(move |index| p.move_to(index));
    let p = page.clone();
    ui.on_delete_requested(move || p.delete());
    let p = page.clone();
    ui.on_property_changed(move |key, text| p.property_changed(&key, &text));

    // The pump: drains the inbox on the UI thread. The timer holds the page weakly, so the page can go away with its window.
    let weak: RcWeak<Inner<W>> = Rc::downgrade(&page.inner);
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, PUMP_EVERY, move || {
        if let Some(inner) = weak.upgrade() {
            Page { inner }.pump();
        }
    });
    *page.inner.timer.borrow_mut() = Some(timer);

    page
}

impl<W: PageWindow> Page<W> {
    /// The page's route (for copy link and navigation).
    fn route(&self) -> String {
        crate::routes::page_route(&self.inner.scope, &self.inner.page_id, false)
    }

    /// Loads the page (its published content), its collections, its comments and the caller's drafts; then opens the draft
    /// when the page was opened in edit mode.
    pub fn load(&self) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        self.close_session();
        {
            let mut s = self.inner.state.borrow_mut();
            s.detail = None;
            s.edit = false;
        }
        ui.show_state(LoadState::Loading);
        let id = self.inner.page_id.clone();
        let inbox = self.inner.inbox.clone();
        let me = ui.user_id();
        self.run(
            move |api, token| async move {
                let detail = api.content_page(&token, &id).await?;
                let collections = api.content_collections(&token).await?;
                let comments = api.content_comments(&token, &id).await?;
                let drafts = api.content_drafts(&token, &id).await?;
                let drafts: Vec<DraftView> = drafts
                    .into_iter()
                    .filter(|d| d.author_id == me || d.members.contains(&me))
                    .collect();
                Ok((detail, collections, comments, drafts))
            },
            move |_ui, result| {
                let item = match result {
                    Ok((detail, collections, comments, drafts)) => Inbox::Loaded {
                        detail,
                        collections,
                        comments,
                        drafts,
                    },
                    Err(e) => Inbox::LoadFailed(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Edit: opens (or reuses) the caller's draft, then the editor connects to its room when the pump sees the draft.
    pub fn start_edit(&self) {
        if self.inner.state.borrow().edit {
            return;
        }
        let id = self.inner.page_id.clone();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move { api.open_content_draft(&token, &id).await },
            move |_ui, result| {
                let item = match result {
                    Ok(draft) => Inbox::DraftOpened(draft),
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Publish with a summary. Approval rules may send the draft to review ("Sent for review").
    pub fn publish(&self, summary: &str) {
        let Some(draft) = self.inner.state.borrow().draft.clone() else {
            return;
        };
        let summary = summary.trim().to_string();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move {
                let summary = (!summary.is_empty()).then_some(summary);
                api.publish_content_draft(&token, &draft.id, summary.as_deref())
                    .await
            },
            move |_ui, result| {
                let item = match result {
                    Ok(published) => Inbox::Published(published),
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Discard the caller's draft (its changes are dropped; the page stays as published).
    pub fn discard(&self) {
        let Some(draft) = self.inner.state.borrow().draft.clone() else {
            return;
        };
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move { api.discard_content_draft(&token, &draft.id).await },
            move |_ui, result| {
                let item = match result {
                    Ok(_) => Inbox::Discarded,
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Invites a person to the caller's draft (manage on the page; the people picker is the app's).
    pub fn invite(&self, user_id: &str) {
        let Some(draft) = self.inner.state.borrow().draft.clone() else {
            return;
        };
        let user = user_id.to_string();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move {
                api.add_content_draft_member(&token, &draft.id, &user).await
            },
            move |_ui, result| {
                let item = match result {
                    Ok(updated) => Inbox::DraftChanged(updated),
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// "Catch up": the main line moved. Reconnects the draft so its room sends the current state again.
    // UNSURE: the client has no "merge main into draft" call. Reconnecting is the only refresh that exists here; the server
    // is assumed to bring the draft up to date on connect (CMS.5 design section 5 says it does).
    pub fn catch_up(&self) {
        self.inner.state.borrow_mut().catch_up = false;
        if let Some(ui) = self.inner.ui.upgrade() {
            ui.show_catch_up(false);
        }
        let draft = self.inner.state.borrow().draft.clone();
        if let Some(draft) = draft {
            self.close_draft_session();
            self.connect_draft(&draft);
        }
    }

    /// Share. A Public page copies its full public address: the API base the client uses, then the page's route. An Internal
    /// page (or one not loaded yet) copies the in-app route, and says it opens in Space.
    pub fn share(&self) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        let route = self.route();
        let public = self
            .inner
            .state
            .borrow()
            .detail
            .as_ref()
            .is_some_and(|d| matches!(d.page.visibility, Visibility::Public));
        if !public {
            ui.copy_text(&route);
            ui.show_notice("Internal page: the link opens in Space");
            return;
        }
        let base = self
            .inner
            .data
            .borrow()
            .as_ref()
            .map(|s| s.api().base_url().to_string());
        match base {
            Some(base) => {
                ui.copy_text(&crate::routes::public_address(&base, &route));
                ui.show_notice("Link copied.");
            }
            // UNSURE: with no session there is no API base to build the address from; nothing is copied.
            None => ui.show_notice("Could not build the public address: no session."),
        }
    }

    /// Leaves edit mode: closes the draft room and shows the published page again.
    pub fn leave(&self) {
        self.close_draft_session();
        self.inner.state.borrow_mut().draft = None;
        self.load();
    }

    /// Changes the page's address. An address that is not valid is refused here; the reload shows the new one.
    pub fn change_slug(&self, slug: &str) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        let slug = slug.trim().to_string();
        if !valid_slug(&slug) {
            ui.show_notice("The address may use lowercase letters, digits and single hyphens.");
            return;
        }
        self.patch(PatchPage {
            visibility: None,
            slug: Some(slug),
        });
    }

    /// Sets the page public or internal (the server checks the access).
    pub fn change_visibility(&self, public: bool) {
        self.patch(PatchPage {
            visibility: Some(if public {
                Visibility::Public
            } else {
                Visibility::Internal
            }),
            slug: None,
        });
    }

    /// Sends a page patch, then reloads the page.
    fn patch(&self, body: PatchPage) {
        let id = self.inner.page_id.clone();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move { api.update_content_page(&token, &id, &body).await },
            move |_ui, result| {
                let item = match result {
                    Ok(_) => Inbox::Reload,
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Opens the Move choices: the scopes other than the page's own, loaded as the scope home loads them (projects, teams,
    /// departments). The pump shows them (`Inbox::MoveChoices`). Manage only.
    pub fn open_move(&self) {
        let (current, manage) = {
            let s = self.inner.state.borrow();
            let Some(detail) = s.detail.as_ref() else {
                return;
            };
            (
                Scope::from_page(detail.page.scope_kind, detail.page.scope_id.as_deref()),
                can_manage(parse_access(&detail.page.my_access)),
            )
        };
        if !manage {
            return;
        }
        let projects: Vec<(String, String)> = projects::list()
            .into_iter()
            .map(|p| (p.key, p.name))
            .collect();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move {
                // UNSURE: as in the scope home, a failed teams or departments list leaves those targets out.
                let teams = api.teams(&token).await.unwrap_or_default();
                let departments = api.departments(&token).await.unwrap_or_default();
                Ok::<_, ClientError>((teams, departments))
            },
            move |_ui, result| {
                let Ok((teams, departments)) = result else {
                    return;
                };
                let teams: Vec<(String, String)> =
                    teams.into_iter().map(|t| (t.id, t.name)).collect();
                let departments: Vec<(String, String)> =
                    departments.into_iter().map(|d| (d.id, d.name)).collect();
                let choices = scope_choices(&projects, &teams, &departments);
                let targets = move_targets(&current, &choices);
                lock(&inbox).push_back(Inbox::MoveChoices(targets));
            },
        );
    }

    /// Moves the page to the Move choice at `index`, then closes the choices and reloads the page.
    pub fn move_to(&self, index: i32) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        let target = {
            let mut s = self.inner.state.borrow_mut();
            let target = usize::try_from(index)
                .ok()
                .and_then(|i| s.move_choices.get(i))
                .map(|c| c.scope.clone());
            s.move_choices.clear();
            target
        };
        ui.show_move(false, Vec::new());
        let Some(target) = target else {
            return;
        };
        let body = MovePage {
            scope_kind: target.to_page_scope().kind,
            scope_id: target.id().map(str::to_string),
        };
        let id = self.inner.page_id.clone();
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move { api.move_content_page(&token, &id, &body).await },
            move |_ui, result| {
                let item = match result {
                    Ok(_) => Inbox::Reload,
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Deletes the page (a soft delete), then opens the home of the scope the page is in. Manage only.
    pub fn delete(&self) {
        let home = {
            let s = self.inner.state.borrow();
            let Some(detail) = s.detail.as_ref() else {
                return;
            };
            if !can_manage(parse_access(&detail.page.my_access)) {
                return;
            }
            home_route(&Scope::from_page(
                detail.page.scope_kind,
                detail.page.scope_id.as_deref(),
            ))
        };
        let id = self.inner.page_id.clone();
        self.run(
            move |api, token| async move { api.delete_content_page(&token, &id).await },
            move |ui, result| match result {
                Ok(_) => ui.navigate(&home),
                Err(e) => ui.show_notice(&error_text(&e)),
            },
        );
    }

    /// A collection property changed in the panel (edit mode only). The text is converted by the field's kind and given to
    /// the draft's controller; the effects go out like any other controller's (the sync update, the editor's refresh).
    // UNSURE: the panel is not redrawn after this edit, since a redraw would replace the rows under the typing caret. The
    // panel shows the text as typed; a co-editor's change redraws it (`on_frame`).
    pub fn property_changed(&self, key: &str, text: &str) {
        let controller = self.inner.state.borrow().controller.clone();
        let Some(controller) = controller else {
            return;
        };
        let kind = kind_for_key(&field_defs(&self.inner.state.borrow()), key);
        let Some(value) = property_value(&kind, text) else {
            return;
        };
        let effects = controller.borrow_mut().property_changed(key, value);
        self.handle_effects(effects);
        self.refresh_editor();
    }

    /// The page panel: the slug, public or internal, and the collection properties. The draft's values in edit mode, else
    /// the published page's (the read-only editor's).
    fn show_panel(&self) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        let s = self.inner.state.borrow();
        let Some(detail) = s.detail.as_ref() else {
            return;
        };
        let controller = if s.edit {
            s.controller.clone()
        } else {
            s.view_controller.clone()
        };
        let values = controller
            .map(|c| c.borrow().properties())
            .unwrap_or_default();
        let locale = self.inner.locale.borrow().clone();
        let rows = property_rows(&field_defs(&s), &values, &locale);
        ui.show_panel(
            &detail.page.slug,
            matches!(detail.page.visibility, Visibility::Public),
            rows,
        );
    }

    /// Binds the read-only editor to the published page (read mode): the page's content in the same editor, which the markup
    /// sets read-only while edit mode is off. Edit mode replaces it with the draft's editor (`start_editor`).
    fn bind_view(&self) {
        let published = {
            let s = self.inner.state.borrow();
            let Some(detail) = s.detail.as_ref() else {
                return;
            };
            published_page(detail)
        };
        let loro = match published
            .map_err(|e| e.to_string())
            .and_then(|page| import_page(&page).map_err(|e| e.to_string()))
        {
            Ok(loro) => loro,
            Err(text) => {
                if let Some(ui) = self.inner.ui.upgrade() {
                    ui.show_notice(&format!("Could not show the page: {text}"));
                }
                return;
            }
        };
        let doc = EditorDoc::from_doc(loro);
        let locale = pick_locale(&doc);
        *self.inner.locale.borrow_mut() = locale.clone();
        let controller = Rc::new(RefCell::new(EditorController::new(doc, &locale)));
        let _ = controller.borrow_mut().set_web_base(self.web_base());
        let page = self.clone();
        let on_host: Box<dyn Fn(HostEvent)> = Box::new(move |event| page.on_host(event));
        let editor = (self.inner.deps.binder)(controller.clone(), on_host);
        editor.refresh();
        {
            let mut s = self.inner.state.borrow_mut();
            s.view_controller = Some(controller);
            s.view_editor = Some(editor);
        }
        self.sync_comments();
    }

    /// The API base the editor's web blocks load from (see `start_editor`).
    fn web_base(&self) -> Option<String> {
        self.inner
            .data
            .borrow()
            .as_ref()
            .map(|s| s.api().base_url().to_string())
    }

    /// Runs `work` on the session's data, then `done` on the UI thread. With no session source, nothing happens.
    fn run<T, F, Fut, D>(&self, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(Arc<ApiClient>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, ClientError>>,
        D: FnOnce(&W, Result<T, ClientError>) + Send + 'static,
    {
        let source: Option<Arc<DataSource>> = self.inner.data.borrow().clone();
        if let Some(source) = source {
            source.fetch(&self.inner.ui, work, done);
        }
    }

    /// Drains the inbox. Called every [`PUMP_EVERY`] on the UI thread.
    fn pump(&self) {
        let items: Vec<Inbox> = lock(&self.inner.inbox).drain(..).collect();
        for item in items {
            match item {
                Inbox::Loaded {
                    detail,
                    collections,
                    comments,
                    drafts,
                } => {
                    self.inner.state.borrow_mut().collections = collections;
                    self.inner.state.borrow_mut().comments = comments;
                    let mine = drafts.into_iter().next();
                    self.inner.state.borrow_mut().detail = Some(detail);
                    if !self.inner.state.borrow().edit {
                        self.bind_view();
                    }
                    self.show_page();
                    self.sync_comments();
                    if let Some(draft) = mine {
                        self.inner.state.borrow_mut().draft = Some(draft);
                    }
                    if self.inner.autostart.replace(false) && !self.inner.state.borrow().edit {
                        self.start_edit();
                    }
                    self.open_main_room();
                }
                Inbox::LoadFailed(text) => {
                    if let Some(ui) = self.inner.ui.upgrade() {
                        ui.show_state(LoadState::Error);
                        ui.show_error(&text);
                    }
                }
                Inbox::Comments(rows) => {
                    self.inner.state.borrow_mut().comments = rows;
                    self.sync_comments();
                }
                Inbox::DraftOpened(draft) => {
                    self.inner.state.borrow_mut().draft = Some(draft.clone());
                    self.connect_draft(&draft);
                }
                Inbox::Published(result) => {
                    let text = if result.status == "in_review" {
                        "Sent for review".to_string()
                    } else {
                        "Published".to_string()
                    };
                    self.after_draft_closed(&text);
                }
                Inbox::Discarded => self.after_draft_closed("Draft discarded"),
                Inbox::DraftChanged(draft) => {
                    self.inner.state.borrow_mut().draft = Some(draft);
                    self.show_page();
                }
                Inbox::Notice(text) => {
                    if let Some(ui) = self.inner.ui.upgrade() {
                        ui.show_notice(&text);
                    }
                }
                Inbox::Reload => self.load(),
                Inbox::MoveChoices(choices) => {
                    let labels: Vec<String> = choices.iter().map(|c| c.label.clone()).collect();
                    self.inner.state.borrow_mut().move_choices = choices;
                    if let Some(ui) = self.inner.ui.upgrade() {
                        ui.show_move(true, labels);
                    }
                }
                Inbox::Live(event) => self.on_live(event),
                Inbox::Mentions(hits) => {
                    let choices: Vec<MentionChoice> = hits
                        .into_iter()
                        .map(|h| MentionChoice {
                            entry_id: h.entry_id,
                            collection: h.collection,
                            slug: h.slug,
                            title: localized(&h.title, FALLBACK_LOCALE),
                        })
                        .collect();
                    let controller = self.inner.state.borrow().controller.clone();
                    if let Some(controller) = controller {
                        let effects = controller.borrow_mut().set_mention_results(choices);
                        self.handle_effects(effects);
                    }
                }
            }
        }
    }

    /// Runs a mention search for the editor's `@` picker. The answer comes back through the inbox (see `Inbox::Mentions`).
    fn search_mentions(&self, query: String) {
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move { api.content_mentions(&token, &query, Some(8)).await },
            move |_ui, result| {
                let hits = match result {
                    Ok(hits) => hits,
                    Err(e) => {
                        lock(&inbox).push_back(Inbox::Notice(error_text(&e)));
                        Vec::new()
                    }
                };
                lock(&inbox).push_back(Inbox::Mentions(hits));
            },
        );
    }

    /// Sends one comment change from the editor (submit, resolve or reopen) on the draft when the page is in edit mode, then
    /// reloads the page's comments. The comments come back through the inbox.
    fn send_comment(&self, request: CommentRequest) {
        let page_id = self.inner.page_id.clone();
        let draft_id = self
            .inner
            .state
            .borrow()
            .draft
            .as_ref()
            .map(|d| d.id.clone());
        let inbox = self.inner.inbox.clone();
        self.run(
            move |api, token| async move {
                let change = match &request {
                    CommentRequest::Submit {
                        block_id,
                        parent_id,
                        body,
                    } => CommentChange::Submit {
                        block_id,
                        parent_id: parent_id.as_deref(),
                        body,
                    },
                    CommentRequest::Resolve { id } => CommentChange::Resolve { id },
                    CommentRequest::Reopen { id } => CommentChange::Reopen { id },
                };
                api.apply_comment_request(&token, &page_id, draft_id.as_deref(), change)
                    .await?;
                api.content_comments(&token, &page_id).await
            },
            move |_ui, result| {
                let item = match result {
                    Ok(rows) => Inbox::Comments(rows),
                    Err(e) => Inbox::Notice(error_text(&e)),
                };
                lock(&inbox).push_back(item);
            },
        );
    }

    /// Connects the editor to the draft's room (edit mode). The first frame is the snapshot the editor starts from.
    fn connect_draft(&self, draft: &DraftView) {
        let Some(source) = self.inner.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else { return };
        let inbox = self.inner.inbox.clone();
        let sink: ContentEventSink = space_client::content_live::content_sink(move |event| {
            lock(&inbox).push_back(Inbox::Live(event));
        });
        let session = ContentLiveSession::connect(
            source.api(),
            token,
            ContentRoom::Draft(draft.id.clone()),
            sink,
        );
        let mut s = self.inner.state.borrow_mut();
        s.session = Some(session);
        s.edit = true;
        drop(s);
        if let Some(ui) = self.inner.ui.upgrade() {
            ui.show_mode(true);
            ui.show_connection("Connecting...");
        }
        self.show_page();
    }

    /// Watches the page's main room for its `published` event (the main line moved; catch up).
    fn open_main_room(&self) {
        if self.inner.state.borrow().main_session.is_some() {
            return;
        }
        let Some(source) = self.inner.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else { return };
        let inbox = self.inner.inbox.clone();
        let sink: ContentEventSink = space_client::content_live::content_sink(move |event| {
            lock(&inbox).push_back(Inbox::Live(event));
        });
        let session = ContentLiveSession::connect(
            source.api(),
            token,
            ContentRoom::Page(self.inner.page_id.clone()),
            sink,
        );
        self.inner.state.borrow_mut().main_session = Some(session);
    }

    /// Handles a live event: a frame goes to the sync session, an event to the page.
    fn on_live(&self, event: ContentLiveEvent) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        match event {
            ContentLiveEvent::Connected => ui.show_connection(""),
            ContentLiveEvent::Disconnected { reason, retry_in } => {
                ui.show_connection(&format!(
                    "{reason}, reconnecting in {} s",
                    retry_in.as_secs().max(1)
                ));
            }
            ContentLiveEvent::Ended { reason } => {
                let text = match reason.as_str() {
                    "draft_closed" => "This draft is closed.".to_string(),
                    "forbidden" => "You no longer have access to this draft.".to_string(),
                    "not_found" => "This draft was not found.".to_string(),
                    "unauthorized" => "Sign in again to keep editing.".to_string(),
                    _ => format!("Editing stopped: {reason}"),
                };
                // A main-room end is not the draft's: only the draft's session stops the editor.
                self.inner.state.borrow_mut().session = None;
                if self.inner.state.borrow().edit {
                    self.inner.state.borrow_mut().edit = false;
                    ui.show_mode(false);
                    ui.show_notice(&text);
                }
            }
            ContentLiveEvent::Event(RoomEvent::Published) => {
                self.inner.state.borrow_mut().catch_up = true;
                ui.show_catch_up(true);
            }
            ContentLiveEvent::Event(RoomEvent::Comments) => {
                let id = self.inner.page_id.clone();
                let inbox = self.inner.inbox.clone();
                self.run(
                    move |api, token| async move { api.content_comments(&token, &id).await },
                    move |_ui, result| {
                        let item = match result {
                            Ok(comments) => Inbox::Comments(comments),
                            Err(e) => Inbox::Notice(error_text(&e)),
                        };
                        lock(&inbox).push_back(item);
                    },
                );
            }
            ContentLiveEvent::Event(RoomEvent::Error(text)) => ui.show_notice(&text),
            ContentLiveEvent::Event(RoomEvent::Readonly) => {
                ui.show_notice("This view is read-only; your change was not sent.");
            }
            ContentLiveEvent::Event(RoomEvent::Other(_)) => {}
            ContentLiveEvent::Frame(frame) => self.on_frame(frame),
        }
    }

    /// A snapshot starts the editor; an update or presence frame goes into the sync session.
    fn on_frame(&self, frame: Frame) {
        let snapshot = matches!(frame, Frame::Snapshot(_));
        if snapshot && self.inner.state.borrow().controller.is_none() {
            let Frame::Snapshot(bytes) = frame else {
                return;
            };
            self.start_editor(&bytes);
            return;
        }
        let (controller, sync) = {
            let s = self.inner.state.borrow();
            (s.controller.clone(), s.sync.is_some())
        };
        let (Some(controller), true) = (controller, sync) else {
            return;
        };
        let changed = {
            let doc = controller.borrow();
            let mut s = self.inner.state.borrow_mut();
            let Some(sync) = s.sync.as_mut() else { return };

            sync.receive(doc.doc(), frame)
        };
        match changed {
            Received::DocChanged => {
                let effects = controller.borrow_mut().refresh();
                self.handle_effects(effects);
                self.refresh_editor();
                self.show_panel();
            }
            Received::Presence => self.update_presence(),
            _ => {}
        }
    }

    /// Builds the document and the editor from the snapshot.
    fn start_editor(&self, bytes: &[u8]) {
        let doc = match EditorDoc::from_snapshot(bytes) {
            Ok(doc) => doc,
            Err(e) => {
                if let Some(ui) = self.inner.ui.upgrade() {
                    ui.show_notice(&format!("Could not open the draft: {e}"));
                }
                return;
            }
        };
        let locale = pick_locale(&doc);
        *self.inner.locale.borrow_mut() = locale.clone();
        let controller = Rc::new(RefCell::new(EditorController::new(doc, &locale)));
        // Web-only blocks (live preview, API docs) load their embed route from this base. UNSURE: the API base is used as the
        // website base; the embed route is `{base}/embed/block` (rimelight_editor `embed_url`).
        let _ = controller.borrow_mut().set_web_base(self.web_base());
        let page = self.clone();
        let on_host: Box<dyn Fn(HostEvent)> = Box::new(move |event| page.on_host(event));
        let editor = (self.inner.deps.binder)(controller.clone(), on_host);
        let user = self
            .inner
            .ui
            .upgrade()
            .map(|ui| ui.user_id())
            .unwrap_or_default();
        let sync = SyncSession::new(&user, &self.inner.deps.name, &self.inner.deps.color);
        {
            let mut s = self.inner.state.borrow_mut();
            s.controller = Some(controller.clone());
            s.editor = Some(editor);
            s.sync = Some(sync);
            // The draft's editor replaces the read-only one on the window.
            s.view_controller = None;
            s.view_editor = None;
        }
        if let Some(ui) = self.inner.ui.upgrade() {
            ui.show_connection("");
        }
        self.sync_comments();
        self.refresh_editor();
        self.show_panel();
    }

    /// Host events from the editor: sync, menus, the dialogs the app owns, and web-only geometry.
    fn on_host(&self, event: HostEvent) {
        match event {
            HostEvent::Effect(effect) => self.handle_effect(effect),
            web @ HostEvent::WebGeometry { .. } => (self.inner.deps.on_web)(web),
        }
    }

    fn handle_effects(&self, effects: Vec<ControllerEffect>) {
        for effect in effects {
            self.handle_effect(effect);
        }
    }

    fn handle_effect(&self, effect: ControllerEffect) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        match effect {
            ControllerEffect::SyncNeeded => self.send_outgoing(),
            ControllerEffect::Error(text) => ui.show_notice(&text),
            ControllerEffect::Done => {}
            ControllerEffect::MentionSearch { query } => self.search_mentions(query),
            // The editor's comment panel asks for its threads: they are the page's comments already loaded here.
            ControllerEffect::CommentRequested { .. } => self.sync_comments(),
            ControllerEffect::Comment { request } => self.send_comment(request),
            ControllerEffect::LocaleChanged { locale } => {
                *self.inner.locale.borrow_mut() = locale;
            }
            ControllerEffect::RowsChanged
            | ControllerEffect::Focus { .. }
            | ControllerEffect::MenusChanged => {
                self.refresh_editor();
            }
        }
    }

    /// The page's comments as the editor takes them (its panel and the per-block badges).
    fn editor_comments(&self) -> Vec<EditorComment> {
        self.inner
            .state
            .borrow()
            .comments
            .iter()
            .map(|c| EditorComment {
                id: c.id.clone(),
                block_id: c.block_id.clone().unwrap_or_default(),
                parent_id: c.parent_id.clone(),
                author: c.author_name.clone(),
                body: c.body.clone(),
                created_at: short_time(&c.created_at),
                resolved: c.resolved,
            })
            .collect()
    }

    /// Hands the page's comments to the editors: the draft's (when open) and the read-only one's.
    fn sync_comments(&self) {
        let controllers: Vec<Rc<RefCell<EditorController>>> = {
            let s = self.inner.state.borrow();
            [s.controller.clone(), s.view_controller.clone()]
                .into_iter()
                .flatten()
                .collect()
        };
        if controllers.is_empty() {
            return;
        }
        for controller in controllers {
            let effects = controller.borrow_mut().set_comments(self.editor_comments());
            self.handle_effects(effects);
        }
        self.refresh_editor();
    }

    /// Sends this side's changes to the room (updates and presence).
    fn send_outgoing(&self) {
        let Some(controller) = self.inner.state.borrow().controller.clone() else {
            return;
        };
        let frames = {
            let doc = controller.borrow();
            let mut s = self.inner.state.borrow_mut();

            match s.sync.as_mut() {
                Some(sync) => sync.outgoing_at(doc.doc(), now_ms()),
                None => return,
            }
        };
        let s = self.inner.state.borrow();
        let Some(live) = s.session.as_ref() else {
            return;
        };
        for frame in frames {
            match frame {
                Frame::Update(bytes) => live.send_update(bytes),
                Frame::Presence(bytes) => live.send_presence(bytes),
                _ => {}
            }
        }
    }

    /// Presence: the other people in the draft go to the editor's presence strip and carets.
    fn update_presence(&self) {
        let (cursors, editor) = {
            let s = self.inner.state.borrow();
            (
                s.sync
                    .as_ref()
                    .map(|sync| sync.remote_cursors_at(now_ms()))
                    .unwrap_or_default(),
                s.editor.clone(),
            )
        };
        if let Some(editor) = editor {
            editor.set_remote_cursors(cursors);
        }
    }

    fn refresh_editor(&self) {
        let editor = self.inner.state.borrow().editor.clone();
        if let Some(editor) = editor {
            editor.refresh();
        }
    }

    /// The draft is done (published, in review or discarded): close the editor and show the page again.
    fn after_draft_closed(&self, text: &str) {
        self.close_draft_session();
        {
            let mut s = self.inner.state.borrow_mut();
            s.draft = None;
            s.edit = false;
        }
        if let Some(ui) = self.inner.ui.upgrade() {
            ui.show_mode(false);
            ui.show_notice(text);
        }
        self.load();
    }

    /// Closes the editor's room and drops the editor (the page stays loaded).
    fn close_draft_session(&self) {
        let mut s = self.inner.state.borrow_mut();
        if let Some(session) = s.session.take() {
            session.close();
        }
        s.controller = None;
        s.editor = None;
        s.sync = None;
    }

    /// Drops both rooms (on load and leave).
    fn close_session(&self) {
        self.close_draft_session();
        let mut s = self.inner.state.borrow_mut();
        if let Some(main) = s.main_session.take() {
            main.close();
        }
    }

    /// The header, the published content and the mode, drawn from the state.
    fn show_page(&self) {
        let Some(ui) = self.inner.ui.upgrade() else {
            return;
        };
        let s = self.inner.state.borrow();
        let Some(detail) = s.detail.as_ref() else {
            return;
        };
        let locale = self.inner.locale.borrow().clone();
        let access = parse_access(&detail.page.my_access);
        let collection = s
            .collections
            .iter()
            .find(|c| c.slug == detail.page.page_type)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| detail.page.page_type.clone());
        let scope = Scope::from_page(detail.page.scope_kind, detail.page.scope_id.as_deref());
        ui.show_header(PageHeader {
            title: localized(&detail.page.title, &locale),
            collection,
            scope_label: crate::home::scope_label(&scope),
            visibility: visibility_label(detail.page.visibility),
            status: status_label(&detail.page.status),
            can_edit: can_edit(access),
            can_manage: can_manage(access),
            has_draft: s.draft.is_some(),
        });
        self.show_panel();
        ui.show_state(LoadState::Loaded);
        ui.show_mode(s.edit);
    }
}

/// Milliseconds since the Unix epoch, for presence timestamps (the sync session takes the time as a parameter on every target).
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
}

/// The locale the editor opens in: the fallback when the document has it, else its first locale.
fn pick_locale(doc: &EditorDoc) -> String {
    let locales = doc.locales();
    locales
        .iter()
        .find(|l| *l == FALLBACK_LOCALE)
        .or_else(|| locales.first())
        .cloned()
        .unwrap_or_else(|| FALLBACK_LOCALE.to_string())
}

/// The published page as `rimelight_content::import_page` reads it: its meta (slug, type, title, description) and its content.
fn published_page(detail: &PageDetail) -> Result<Value, serde_json::Error> {
    let mut meta = Map::new();
    meta.insert("slug".to_string(), Value::String(detail.page.slug.clone()));
    meta.insert(
        "type".to_string(),
        Value::String(detail.page.page_type.clone()),
    );
    meta.insert("title".to_string(), detail.page.title.clone());
    if let Some(description) = &detail.page.description {
        meta.insert("description".to_string(), description.clone());
    }
    let content = serde_json::to_value(&detail.content)?;
    Ok(json!({ "meta": Value::Object(meta), "content": content }))
}

/// The collection's field definitions for the loaded page (empty until the page loads).
fn field_defs(s: &State) -> Vec<Value> {
    let Some(detail) = s.detail.as_ref() else {
        return Vec::new();
    };
    s.collections
        .iter()
        .find(|c| c.slug == detail.page.page_type)
        .map(|c| c.fields.clone())
        .unwrap_or_default()
}

/// A field definition's kind (its `type`; "text" when it has none).
fn kind_of(field: &Value) -> String {
    field
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .to_string()
}

/// The kind of the field called `key` ("text" when there is no such field).
fn kind_for_key(fields: &[Value], key: &str) -> String {
    fields
        .iter()
        .find(|f| f.get("name").and_then(Value::as_str) == Some(key))
        .map(kind_of)
        .unwrap_or_else(|| "text".to_string())
}

/// The panel's rows: one per field of the collection, in field order, with the page's value of it as text.
pub fn property_rows(
    fields: &[Value],
    values: &Map<String, Value>,
    locale: &str,
) -> Vec<PropertyRow> {
    fields
        .iter()
        .filter_map(|field| {
            let key = field.get("name")?.as_str()?.to_string();
            let label = field
                .get("label")
                .map(|l| localized(l, locale))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| key.clone());
            let options = field
                .get("options")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|o| option_pair(o, locale))
                        .collect()
                })
                .unwrap_or_default();
            Some(PropertyRow {
                kind: kind_of(field),
                value: values.get(&key).map(value_text).unwrap_or_default(),
                key,
                label,
                options,
            })
        })
        .collect()
}

/// One choice of a select: `{value, label}` (the label localized), or a plain string that is both.
fn option_pair(option: &Value, locale: &str) -> Option<(String, String)> {
    match option {
        Value::Object(map) => {
            let value = value_text(map.get("value")?);
            let label = map
                .get("label")
                .map(|l| localized(l, locale))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| value.clone());
            Some((value, label))
        }
        Value::Null => None,
        other => {
            let text = value_text(other);
            Some((text.clone(), text))
        }
    }
}

/// A property's value as the panel's text: strings as they are, numbers and booleans as they print, lists of strings joined
/// with ", " (tags), null as empty, anything else as compact JSON.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(items) if items.iter().all(Value::is_string) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

/// The JSON value of a panel edit, by the field's kind: a number, a boolean ("true"), tags (comma-separated, trimmed, empty
/// ones dropped), parsed JSON, or the text. `None` when the text cannot be that kind (a number that does not parse, JSON that
/// is not valid); the edit is then not sent. An empty number is null.
pub fn property_value(kind: &str, text: &str) -> Option<Value> {
    match kind {
        "number" => {
            let text = text.trim();
            if text.is_empty() {
                return Some(Value::Null);
            }
            text.parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
        }
        "boolean" => Some(Value::Bool(text == "true")),
        "tags" => Some(Value::Array(
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect(),
        )),
        "json" => serde_json::from_str::<Value>(text).ok(),
        _ => Some(Value::String(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_client::content::Visibility;

    #[test]
    fn visibility_wording_is_fixed() {
        assert_eq!(visibility_label(Visibility::Internal), "Internal");
    }

    #[test]
    fn panel_rows_follow_the_fields_and_read_the_values() {
        let fields = vec![
            json!({"name": "era", "type": "select", "label": {"en": "Era"},
                   "options": [{"value": "a", "label": "Ancient"}, "b"]}),
            json!({"name": "tags", "type": "tags"}),
            json!({"name": "count", "type": "number", "label": "Count"}),
            json!({"name": "extra", "type": "json"}),
        ];
        let values = json!({"era": "a", "tags": ["x", "y"], "count": 3.5, "extra": {"k": 1}});
        let rows = property_rows(&fields, values.as_object().unwrap(), "en");
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            PropertyRow {
                key: "era".into(),
                label: "Era".into(),
                kind: "select".into(),
                value: "a".into(),
                options: vec![("a".into(), "Ancient".into()), ("b".into(), "b".into())],
            }
        );
        assert_eq!(rows[1].label, "tags");
        assert_eq!(rows[1].value, "x, y");
        assert_eq!(rows[2].value, "3.5");
        assert_eq!(rows[3].value, r#"{"k":1}"#);
    }

    #[test]
    fn panel_values_are_empty_for_null_and_missing() {
        let fields = vec![
            json!({"name": "note", "type": "text"}),
            json!({"name": "flag", "type": "boolean"}),
        ];
        let values = json!({"flag": true, "note": null});
        let rows = property_rows(&fields, values.as_object().unwrap(), "en");
        assert_eq!(rows[0].value, "");
        assert_eq!(rows[1].value, "true");
        let none = property_rows(&fields, &Map::new(), "en");
        assert_eq!(none[1].value, "");
    }

    #[test]
    fn panel_edits_convert_by_kind() {
        assert_eq!(property_value("number", " 2.5 "), Some(json!(2.5)));
        assert_eq!(property_value("number", "abc"), None);
        assert_eq!(property_value("boolean", "true"), Some(json!(true)));
        assert_eq!(property_value("boolean", "no"), Some(json!(false)));
        assert_eq!(property_value("tags", "a, , b ,"), Some(json!(["a", "b"])));
        assert_eq!(property_value("json", r#"{"x":1}"#), Some(json!({"x": 1})));
        assert_eq!(property_value("json", "{nope"), None);
        assert_eq!(property_value("select", "a"), Some(json!("a")));
    }
}
