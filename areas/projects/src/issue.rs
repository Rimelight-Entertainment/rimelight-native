//! Projects > Issue (S1.3, part 3): one issue at `/p/:key/issues/:id` (the id is the issue key, for example `EMB-12`). The header
//! has one edit button, which opens the edit modal (the New issue dialog in edit mode, src/pages/projects/new_issue.rs) for
//! the whole issue; the parent and subtasks open, a subtask is created under this issue, links are added and removed,
//! comments are added, edited and deleted, the activity log is listed, and a project admin can delete the issue after a
//! confirm step.
//!
//! Who may do what (decision 0031): a member or an admin edits; a viewer, and a person with no role (a global admin who is not
//! a member), sees the page read-only. A project admin also deletes issues and changes any comment; a member changes only
//! their own comments. Every write is checked against the role before it is recorded or sent, so a control that is hidden
//! cannot be used by a callback either.
//!
//! The page data pattern (`space_page_kit::page_data`). The page's markup is `ui/issue.slint`. Every write is kept as a
//! [`Request`] in `Shared::last`, so a test can check what would be sent without a network.
//!
//! Plain Rust. The window side is [`IssueWindow`], which `wire_projects!` implements for the generated window.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use slint::{Image, Weak};
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_proto::projects::{
    ActivityEntry, Area, Attachment, COMMENT_MAX, Comment, CreateIssue, CreateLink, CreateWorkItem,
    ISSUE_TYPES, Issue, IssueLink, LINK_KINDS, PRIORITIES, Project, SUMMARY_MAX, Sprint, WorkItem,
};

use super::issues::{ListKeys, SharedKeys, can_create, issue_route, list_route, relative_time};
use super::new_issue::{NewIssue, NewIssueWindow};
use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataCell, DataSource, Pictures};
use space_page_kit::person::initials;
use space_page_kit::picker::{Choice, choice};

/// The project key and the issue key of an issue route (`/p/<key>/issues/<issue>`), or `None` for any other route.
pub fn ids_of(route: &str) -> Option<(&str, &str)> {
    let rest = route.strip_prefix("/p/")?;
    let (project, tail) = rest.split_once('/')?;
    let (word, issue) = tail.split_once('/')?;
    let plain = |text: &str| {
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    (plain(project) && word == "issues" && plain(issue)).then_some((project, issue))
}

/// What one load reads: the issue, its project (the caller's role, the workflow and the members), the project's sprints,
/// and the issue's comments and activity.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueData {
    pub issue: Issue,
    pub project: Project,
    pub sprints: Vec<Sprint>,
    pub comments: Vec<Comment>,
    pub activity: Vec<ActivityEntry>,
    /// The spent-time entries, newest first as the server lists them (S1.5b).
    pub work_items: Vec<WorkItem>,
    /// The project's area catalogue, for the colour of each area on the issue. Empty when it could not be read.
    pub areas: Vec<Area>,
}

/// A write the page sends. Kept in `Shared::last` before it is sent, so a test can read it.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// `POST /v1/projects/{key}/issues` with this issue as the parent.
    Subtask(CreateIssue),
    /// `POST /v1/issues/{key}/links`.
    AddLink(CreateLink),
    /// `DELETE /v1/issues/{key}/links/{id}`.
    RemoveLink(String),
    /// `POST /v1/issues/{key}/comments`.
    AddComment(String),
    /// `PATCH /v1/issues/{key}/comments/{id}`.
    EditComment { id: String, body: String },
    /// `DELETE /v1/issues/{key}/comments/{id}`.
    DeleteComment(String),
    /// `DELETE /v1/issues/{key}`.
    Delete,
    /// `PUT` (true) or `DELETE` (false) `/v1/issues/{key}/vote`. Any person who can see the issue may vote.
    Vote(bool),
    /// `PUT` (true) or `DELETE` (false) `/v1/issues/{key}/star`. Any person who can see the issue may star it.
    Star(bool),
    /// `POST /v1/issues/{key}/work-items`: spent time as the caller.
    AddWorkItem(CreateWorkItem),
    /// `DELETE /v1/issues/{key}/work-items/{id}`.
    DeleteWorkItem(String),
    /// Chooses files with the native dialog, then `POST /v1/issues/{key}/attachments` for each. Cancelling sends nothing.
    Attach,
    /// `DELETE /v1/issues/{key}/attachments/{id}`.
    DeleteAttachment(String),
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// The keys of the issue list, for the previous and next arrows (shared with the list page).
    list: Option<SharedKeys>,
    /// Bumped by every load and every write. A result for an older number is dropped.
    generation: u64,
    /// The route on show, and its project and issue keys.
    route: String,
    project: String,
    key: String,
    /// The loaded issue, or `None` while loading or after an error.
    data: Option<IssueData>,
    error: Option<String>,
    /// True while a write is on its way.
    busy: bool,
    /// The message of the last refused or failed write.
    action_error: String,
    /// The comment being edited, by id.
    comment_editing: Option<String>,
    /// True after "Delete issue": the confirm step is showing.
    confirm_delete: bool,
    /// The last write the page started, for tests and for the delete check.
    last: Option<Request>,
    /// The decoded pictures of the image attachments, by attachment id, and the ids on their way.
    pictures: Pictures,
    /// The attachment ids whose bytes did not decode as a picture (or could not be read): not asked for again on this page.
    failed: HashSet<String>,
}

/// An area chip: the area's name on its colour, with the ink (black or white) that reads on that colour (RGB bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaChipView {
    pub name: String,
    pub color: (u8, u8, u8),
    pub ink: (u8, u8, u8),
}

/// One attachment as the grid shows it. `thumbnail` is the decoded picture of an image attachment, else `None`.
#[derive(Clone)]
pub struct AttachmentTileView {
    pub name: String,
    pub size: String,
    pub is_image: bool,
    pub thumbnail: Option<Image>,
    pub changeable: bool,
}

/// One picture in the image viewer.
#[derive(Clone)]
pub struct ImageView {
    pub image: Image,
    pub title: String,
}

/// The header fields as the page draws them: the values, and the choices of each select. Empty strings mean "not set".
#[derive(Clone)]
pub struct IssueView {
    pub key: String,
    pub project_name: String,
    pub position: String,
    pub has_prev: bool,
    pub has_next: bool,
    pub votes: i32,
    pub voted: bool,
    pub starred: bool,
    pub spent: String,
    pub summary: String,
    pub issue_type: String,
    pub state_id: String,
    pub state_name: String,
    pub priority: String,
    pub assignee_id: String,
    pub assignee_name: String,
    pub sprint_id: String,
    pub sprint_name: String,
    pub estimate: String,
    pub due_date: String,
    pub tags: Vec<String>,
    pub area_chips: Vec<AreaChipView>,
    pub reporter: String,
    pub created: String,
    pub updated: String,
    pub updated_by: String,
    pub description: String,
    pub has_parent: bool,
    pub parent_key: String,
    pub parent_summary: String,
    pub attachment_tiles: Vec<AttachmentTileView>,
    pub attachment_images: Vec<ImageView>,
    pub attachment_image_index: Vec<i32>,
    pub state_options: Vec<Choice>,
    pub type_options: Vec<Choice>,
    pub priority_options: Vec<Choice>,
    pub assignee_options: Vec<Choice>,
    pub sprint_options: Vec<Choice>,
}

/// An issue in a list (a subtask).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRefView {
    pub key: String,
    pub summary: String,
    pub state: String,
}

/// A link on the issue. `heading` is the group's label (the first row of each group only; empty otherwise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    pub id: String,
    pub heading: String,
    pub key: String,
    pub summary: String,
    pub state: String,
}

/// A comment. `changeable` is true when the caller may edit and delete it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentView {
    pub id: String,
    pub initials: String,
    pub author: String,
    pub when: String,
    pub body: String,
    pub changeable: bool,
    pub editing: bool,
}

/// A spent-time entry. `changeable`: the caller's own, or a project admin's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkView {
    pub id: String,
    pub author: String,
    pub minutes: String,
    pub spent_on: String,
    pub note: String,
    pub changeable: bool,
}

/// An attachment row. `changeable` is true for a member or an admin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentView {
    pub id: String,
    pub name: String,
    pub size: String,
    pub changeable: bool,
}

/// Everything the page shows once the issue is loaded.
#[derive(Clone)]
pub struct Body {
    pub view: IssueView,
    pub work: Vec<WorkView>,
    pub files: Vec<AttachmentView>,
    pub subtasks: Vec<IssueRefView>,
    pub links: Vec<LinkView>,
    pub comments: Vec<CommentView>,
    pub activity: Vec<String>,
}

/// The window side of the issue page. `wire_projects!` implements it for the generated window. It also needs the New issue
/// dialog's window side, because the edit button opens that dialog. The method names differ from [`NewIssueWindow`]'s.
pub trait IssueWindow: NewIssueWindow {
    /// The signed-in person's id.
    fn issue_user_id(&self) -> String;
    fn show_issue_state(&self, state: LoadState);
    fn show_issue_error(&self, text: &str);
    fn show_saving(&self, on: bool);
    fn show_action_error(&self, text: &str);
    fn show_confirm_delete(&self, on: bool);
    /// What the caller may do: edit, moderate comments, delete the issue.
    fn show_access(&self, can_edit: bool, can_moderate: bool, can_delete: bool);
    /// The loaded issue's whole view.
    fn show_body(&self, body: Body);
    /// The view of no issue (loading or after an error): default header, empty lists.
    fn clear_body(&self);
    /// The text of the comment edit box.
    fn show_comment_draft(&self, text: &str);
    /// Opens the page at `route` (the app shell's issues-open).
    fn open_route(&self, route: String);
    /// Puts `link` on the clipboard.
    fn copy_link(&self, link: &str);
    fn on_is_route(&self, f: impl Fn(String) -> bool + 'static);
    fn on_vote(&self, f: impl Fn(bool) + 'static);
    fn on_star(&self, f: impl Fn(bool) + 'static);
    fn on_prev(&self, f: impl Fn() + 'static);
    fn on_next(&self, f: impl Fn() + 'static);
    fn on_copy_key(&self, f: impl Fn(String) + 'static);
    fn on_work_add(&self, f: impl Fn(String, String, String) + 'static);
    fn on_work_delete(&self, f: impl Fn(String) + 'static);
    fn on_attach_files(&self, f: impl Fn() + 'static);
    fn on_attachment_delete(&self, f: impl Fn(String) + 'static);
    fn on_attachment_download(&self, f: impl Fn(i32) + 'static);
    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_back(&self, f: impl Fn() + 'static);
    fn on_open_issue(&self, f: impl Fn(String) + 'static);
    fn on_edit_open(&self, f: impl Fn() + 'static);
    fn on_subtask_add(&self, f: impl Fn(String) + 'static);
    fn on_link_add(&self, f: impl Fn(String, String) + 'static);
    fn on_link_remove(&self, f: impl Fn(String) + 'static);
    fn on_comment_add(&self, f: impl Fn(String) + 'static);
    fn on_comment_edit_start(&self, f: impl Fn(String) + 'static);
    fn on_comment_edit_cancel(&self, f: impl Fn() + 'static);
    fn on_comment_edit_save(&self, f: impl Fn(String, String) + 'static);
    fn on_comment_delete(&self, f: impl Fn(String) + 'static);
    fn on_delete_ask(&self, f: impl Fn() + 'static);
    fn on_delete_cancel(&self, f: impl Fn() + 'static);
    fn on_delete_confirm(&self, f: impl Fn() + 'static);
}

/// The issue page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct IssueDetail<W: IssueWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: IssueWindow> Clone for IssueDetail<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, requests do nothing and the
/// page stays in its loading state. `list` is the issue list's keys, for the previous and next arrows.
pub fn wire<W: IssueWindow>(
    ui: &W,
    data: DataCell,
    list: SharedKeys,
    dialog: NewIssue<W>,
) -> IssueDetail<W> {
    let page = IssueDetail {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared {
            list: Some(list),
            ..Shared::default()
        })),
    };

    ui.on_is_route(|route| ids_of(route.as_str()).is_some());

    let p = page.clone();
    ui.on_vote(move |on| p.mutate(Request::Vote(on)));

    let p = page.clone();
    ui.on_star(move |on| p.mutate(Request::Star(on)));

    let p = page.clone();
    ui.on_prev(move || p.step(-1));

    let p = page.clone();
    ui.on_next(move || p.step(1));

    let weak = ui.as_weak();
    ui.on_copy_key(move |key| {
        if let Some(ui) = weak.upgrade() {
            ui.copy_link(key.as_str());
        }
    });

    let p = page.clone();
    ui.on_work_add(move |minutes, date, note| p.add_work(&minutes, &date, &note));

    let p = page.clone();
    ui.on_work_delete(move |id| p.mutate(Request::DeleteWorkItem(id)));

    let p = page.clone();
    ui.on_attach_files(move || p.mutate(Request::Attach));

    let p = page.clone();
    ui.on_attachment_delete(move |id| p.mutate(Request::DeleteAttachment(id)));

    let p = page.clone();
    ui.on_attachment_download(move |index| p.download(index));

    let p = page.clone();
    ui.on_opened(move |route| p.open(route.as_str(), false));

    let p = page.clone();
    ui.on_retry(move || {
        let route = lock(&p.shared).route.clone();
        p.open(&route, true);
    });

    let p = page.clone();
    ui.on_back(move || {
        if let Some(ui) = p.ui.upgrade() {
            let project = lock(&p.shared).project.clone();
            ui.open_route(list_route(&project));
        }
    });

    let p = page.clone();
    ui.on_open_issue(move |key| {
        if let Some(ui) = p.ui.upgrade() {
            let project = lock(&p.shared).project.clone();
            ui.open_route(issue_route(&project, key.as_str()));
        }
    });

    // The one edit button: the edit modal (the New issue dialog in edit mode) changes the whole issue.
    let p = page.clone();
    ui.on_edit_open(move || p.edit_open(&dialog));

    let p = page.clone();
    ui.on_subtask_add(move |summary| p.add_subtask(summary.as_str()));

    let p = page.clone();
    ui.on_link_add(move |target, kind| p.add_link(target.as_str(), kind.as_str()));

    let p = page.clone();
    ui.on_link_remove(move |id| p.mutate(Request::RemoveLink(id)));

    let p = page.clone();
    ui.on_comment_add(move |body| p.add_comment(body.as_str()));

    let p = page.clone();
    ui.on_comment_edit_start(move |id| p.comment_edit_start(id.as_str()));

    let p = page.clone();
    ui.on_comment_edit_cancel(move || {
        let mut s = lock(&p.shared);
        s.comment_editing = None;
        if let Some(ui) = p.ui.upgrade() {
            show(&ui, &s);
        }
    });

    let p = page.clone();
    ui.on_comment_edit_save(move |id, body| p.edit_comment(id.as_str(), body.as_str()));

    let p = page.clone();
    ui.on_comment_delete(move |id| {
        p.mutate(Request::DeleteComment(id));
    });

    let p = page.clone();
    ui.on_delete_ask(move || {
        let mut s = lock(&p.shared);
        if !allowed_delete(&s) {
            return;
        }
        s.confirm_delete = true;
        if let Some(ui) = p.ui.upgrade() {
            show(&ui, &s);
        }
    });

    let p = page.clone();
    ui.on_delete_cancel(move || {
        let mut s = lock(&p.shared);
        s.confirm_delete = false;
        if let Some(ui) = p.ui.upgrade() {
            show(&ui, &s);
        }
    });

    let p = page.clone();
    ui.on_delete_confirm(move || {
        // Only after "Delete issue" showed the confirm step, and only while that step is up.
        let confirming = lock(&p.shared).confirm_delete;
        if confirming {
            p.mutate(Request::Delete);
        }
    });

    page
}

impl<W: IssueWindow> IssueDetail<W> {
    /// The generation of the latest load or write.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// The last write the page started (the request it would send), or `None`.
    pub fn last_request(&self) -> Option<Request> {
        lock(&self.shared).last.clone()
    }

    /// Delivers a load's result for the load with `generation`, as the worker would. For tests: no source is set, so nothing is
    /// fetched.
    pub fn deliver_load(&self, generation: u64, result: Result<IssueData, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver_load(&ui, &self.shared, generation, result);
    }

    /// Delivers a write's result, as the worker would, for the write that is on its way. For tests. A successful delete goes to
    /// the list, as it does on the window.
    pub fn deliver_action(&self, result: Result<(), ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = self.generation();
        finish(&ui, &self.shared, None, generation, result);
    }

    /// Starts a load of the issue at `route` (ignored when it is not an issue route). A load for the route already on show is not
    /// repeated, unless `force` (Try again).
    fn open(&self, route: &str, force: bool) {
        let Some((project, key)) = ids_of(route).map(|(p, k)| (p.to_owned(), k.to_owned())) else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            if !force && s.route == route && s.error.is_none() {
                // Shown again: nothing is loaded, but the place in the list may have changed since.
                show(&ui, &s);
                return;
            }
            s.generation += 1;
            s.route = route.to_owned();
            s.project = project.clone();
            s.key = key.clone();
            s.data = None;
            s.error = None;
            s.busy = false;
            s.action_error.clear();
            s.comment_editing = None;
            s.confirm_delete = false;
            show(&ui, &s);
            s.generation
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        fetch_load(&self.ui, &source, &self.shared, generation, project, key);
    }

    /// Opens the edit modal for the loaded issue, when the caller may edit it. Nothing is sent until the modal's Save.
    fn edit_open(&self, dialog: &NewIssue<W>) {
        let Some(data) = lock(&self.shared).data.clone() else {
            return;
        };
        if !can_create(data.project.my_role.as_deref()) || lock(&self.shared).busy {
            return;
        }
        dialog.open_edit(&data.issue, &data.project, &data.sprints);
    }

    /// Creates a subtask under this issue.
    fn add_subtask(&self, summary: &str) {
        let parent = match lock(&self.shared).data.as_ref() {
            Some(data) => data.issue.id.clone(),
            None => return,
        };
        match subtask_body(&parent, summary) {
            Ok(body) => self.mutate(Request::Subtask(body)),
            Err(message) => self.refuse(message),
        }
    }

    /// Adds a link from this issue to `target` of `kind`.
    fn add_link(&self, target: &str, kind: &str) {
        match link_body(target, kind) {
            Ok(body) => self.mutate(Request::AddLink(body)),
            Err(message) => self.refuse(message),
        }
    }

    /// Adds a comment as the caller.
    fn add_comment(&self, body: &str) {
        match comment_text(body) {
            Ok(text) => self.mutate(Request::AddComment(text)),
            Err(message) => self.refuse(message),
        }
    }

    /// Records spent time as the caller, from the form's minutes, date and note.
    fn add_work(&self, minutes: &str, spent_on: &str, note: &str) {
        match work_item_body(minutes, spent_on, note) {
            Ok(body) => self.mutate(Request::AddWorkItem(body)),
            Err(message) => self.refuse(message),
        }
    }

    /// Opens the issue before (`-1`) or after (`1`) this one in the list on show. Does nothing when the list is for another
    /// project, or this issue is not in it.
    fn step(&self, delta: isize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (project, key, list) = {
            let s = lock(&self.shared);
            (s.project.clone(), s.key.clone(), s.list.clone())
        };
        let Some(list) = list else { return };
        let target = {
            let keys = lock_keys(&list);
            if keys.project != project {
                return;
            }
            neighbour(&keys.keys, &key, delta)
        };
        if let Some(target) = target {
            ui.open_route(issue_route(&project, &target));
        }
    }

    /// Starts editing a comment the caller may change. Its text is copied to the edit draft.
    fn comment_edit_start(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let Some(data) = s.data.as_ref() else { return };
        let me = ui.issue_user_id();
        let moderate = is_admin(data);
        let Some(comment) = data.comments.iter().find(|c| c.id == id) else {
            return;
        };
        if !can_create(data.project.my_role.as_deref()) || !(comment.author.id == me || moderate) {
            return;
        }
        ui.show_comment_draft(comment.body.as_str());
        s.comment_editing = Some(id.to_owned());
        s.action_error.clear();
        show(&ui, &s);
    }

    /// Saves an edited comment.
    fn edit_comment(&self, id: &str, body: &str) {
        match comment_text(body) {
            Ok(text) => self.mutate(Request::EditComment {
                id: id.to_owned(),
                body: text,
            }),
            Err(message) => self.refuse(message),
        }
    }

    /// Shows a message under the page, without sending anything.
    fn refuse(&self, message: String) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        s.action_error = message;
        show(&ui, &s);
    }

    /// Starts a write: records it, marks the page busy, and sends it on a worker. A write the caller's role does not allow is
    /// dropped without a request. The result comes back to [`finish`].
    fn mutate(&self, request: Request) {
        let Some(ui) = self.ui.upgrade() else { return };
        let me = ui.issue_user_id();
        let (generation, project, key) = {
            let mut s = lock(&self.shared);
            if s.busy || !allowed(&s, &request, &me) {
                return;
            }
            s.generation += 1;
            s.busy = true;
            s.action_error.clear();
            s.last = Some(request.clone());
            show(&ui, &s);
            (s.generation, s.project.clone(), s.key.clone())
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let refresh = source.clone();
        let (send_project, send_key) = (project, key);
        source.fetch(
            &self.ui,
            move |api, token| async move { send(&api, &token, &send_project, &send_key, request).await },
            move |ui, result| finish(ui, &shared, Some(&refresh), generation, result),
        );
    }

    /// Fetches the attachment at `index` and saves it through the native save dialog. Not a write: the issue is not loaded
    /// again. A failure, or a save that did not write, is shown under the page if the same issue is still on show.
    fn download(&self, index: i32) {
        if self.ui.upgrade().is_none() {
            return;
        }
        let (target, key) = {
            let s = lock(&self.shared);
            let Some(data) = &s.data else { return };
            let Some(target) = download_target(&data.issue.attachments, index) else {
                return;
            };
            (target, s.key.clone())
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let (id, name) = target;
        let done_key = key.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let bytes = api.attachment_bytes(&token, &key, &id).await?;
                save_attachment(&name, &bytes).await
            },
            move |ui, result| {
                let mut s = lock(&shared);
                if s.key != done_key {
                    return;
                }
                let message = match result {
                    Ok(None) => return,
                    Ok(Some(message)) => message,
                    Err(error) => page_data::error_text(&error),
                };
                s.action_error = message;
                show(ui, &s);
            },
        );
    }
}

/// The id and file name of the attachment at `index` (the grid's index into the issue's files), or `None` when there is none.
fn download_target(files: &[Attachment], index: i32) -> Option<(String, String)> {
    let index = usize::try_from(index).ok()?;
    files
        .get(index)
        .map(|file| (file.id.clone(), file.file_name.clone()))
}

/// Whether the caller's role lets them delete the issue: a project admin.
fn allowed_delete(s: &Shared) -> bool {
    s.data.as_ref().is_some_and(is_admin)
}

/// Whether the caller may send `request`. Every write needs a member or an admin; delete needs an admin; a comment edit or
/// delete needs the caller's own comment, or an admin.
fn allowed(s: &Shared, request: &Request, me: &str) -> bool {
    let Some(data) = s.data.as_ref() else {
        return false;
    };
    // A vote or a star needs only the right to see the issue, which every project role (and a global admin) has.
    if matches!(request, Request::Vote(_) | Request::Star(_)) {
        return true;
    }
    if !can_create(data.project.my_role.as_deref()) {
        return false;
    }
    let mine_or_admin = |id: &str| {
        is_admin(data)
            || data
                .comments
                .iter()
                .any(|c| c.id == id && c.author.id == me)
    };
    match request {
        Request::Delete => is_admin(data),
        Request::EditComment { id, .. } | Request::DeleteComment(id) => mine_or_admin(id),
        Request::DeleteWorkItem(id) => {
            is_admin(data)
                || data
                    .work_items
                    .iter()
                    .any(|w| w.id == *id && w.author.id == me)
        }
        _ => true,
    }
}

/// Whether the caller is a project admin (a role of `admin` on this project). A global admin who is not a member is not.
fn is_admin(data: &IssueData) -> bool {
    data.project.my_role.as_deref() == Some("admin")
}

/// The body of a subtask: its summary, with the issue as its parent.
pub fn subtask_body(parent_id: &str, summary: &str) -> Result<CreateIssue, String> {
    let summary = summary.trim();
    if summary.is_empty() {
        return Err("A subtask needs a title.".to_owned());
    }
    if summary.chars().count() > SUMMARY_MAX {
        return Err(format!(
            "The title is longer than {SUMMARY_MAX} characters."
        ));
    }
    Ok(CreateIssue {
        summary: summary.to_owned(),
        parent_id: Some(parent_id.to_owned()),
        ..CreateIssue::default()
    })
}

/// The body of a link: the target's key (for example `EMB-12`, upper-cased) and a link kind from the server's list.
pub fn link_body(target: &str, kind: &str) -> Result<CreateLink, String> {
    let target = target.trim().to_ascii_uppercase();
    let valid = target.split_once('-').is_some_and(|(project, number)| {
        !project.is_empty()
            && !number.is_empty()
            && project.bytes().all(|b| b.is_ascii_alphanumeric())
            && number.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    if !valid {
        return Err("Enter an issue key, for example EMB-12.".to_owned());
    }
    if !LINK_KINDS.contains(&kind) {
        return Err("Choose a kind of link.".to_owned());
    }
    Ok(CreateLink {
        target,
        kind: kind.to_owned(),
    })
}

/// The text of a comment: trimmed, not empty, and within the limit.
pub fn comment_text(body: &str) -> Result<String, String> {
    let text = body.trim();
    if text.is_empty() {
        return Err("A comment cannot be empty.".to_owned());
    }
    if text.chars().count() > COMMENT_MAX {
        return Err(format!(
            "A comment is longer than {COMMENT_MAX} characters."
        ));
    }
    Ok(text.to_owned())
}

/// The body of a spent-time entry: whole minutes (1 to 100 000), a date `YYYY-MM-DD` or empty (today, as the server reads it),
/// and an optional note.
pub fn work_item_body(minutes: &str, spent_on: &str, note: &str) -> Result<CreateWorkItem, String> {
    let minutes: u32 = minutes
        .trim()
        .parse()
        .map_err(|_| "Spent time is a whole number of minutes.".to_owned())?;
    if !(1..=100_000).contains(&minutes) {
        return Err("Spent time is between 1 and 100000 minutes.".to_owned());
    }
    let spent_on = spent_on.trim();
    if !spent_on.is_empty() && !is_date(spent_on) {
        return Err("The date is YYYY-MM-DD.".to_owned());
    }
    let note = note.trim();
    Ok(CreateWorkItem {
        minutes,
        spent_on: (!spent_on.is_empty()).then(|| spent_on.to_owned()),
        note: (!note.is_empty()).then(|| note.to_owned()),
    })
}

/// `YYYY-MM-DD` by shape. The server checks the calendar.
pub fn is_date(text: &str) -> bool {
    text.len() == 10
        && text.bytes().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Spent time as `2h 30m`, `2h`, `45m` or `0m`.
pub fn minutes_text(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// A file size as `512 B`, `12 KB` or `1.5 MB`.
pub fn size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}

/// The description as the page shows it. The page has no rich text, so the Markdown is read as plain text: a heading loses
/// its `#`, a list item becomes a bullet, and `**bold**` and `` `code` `` lose their marks.
pub fn plain_description(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let body = if trimmed.starts_with('#') {
                trimmed.trim_start_matches('#').trim_start().to_owned()
            } else if let Some(rest) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                format!("\u{2022} {rest}")
            } else {
                trimmed.to_owned()
            };
            format!("{indent}{}", body.replace("**", "").replace('`', ""))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The place of `key` in the list `keys`: its 0-based index and the list's length, or `None` when it is not listed.
pub fn place_of(keys: &[String], key: &str) -> Option<(usize, usize)> {
    Some((keys.iter().position(|k| k == key)?, keys.len()))
}

/// The key before (`delta` -1) or after (`delta` 1) `key` in `keys`, or `None` at either end or when `key` is not listed.
pub fn neighbour(keys: &[String], key: &str, delta: isize) -> Option<String> {
    let index = keys.iter().position(|k| k == key)?;
    keys.get(index.checked_add_signed(delta)?).cloned()
}

/// The media type of an attachment by its file name. A type the page does not name is sent as a generic binary.
pub fn attachment_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// The people, states and sprints of a load by id, for the activity line (an id is shown as its name).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Names {
    pub states: Vec<(String, String)>,
    pub people: Vec<(String, String)>,
    pub sprints: Vec<(String, String)>,
}

impl Names {
    /// The names of one loaded issue.
    pub fn of(data: &IssueData) -> Self {
        let mut people: Vec<(String, String)> = data
            .project
            .members
            .iter()
            .map(|m| (m.user.id.clone(), m.user.display_name.clone()))
            .collect();
        people.push((
            data.issue.reporter.id.clone(),
            data.issue.reporter.display_name.clone(),
        ));
        if let Some(assignee) = &data.issue.assignee {
            people.push((assignee.id.clone(), assignee.display_name.clone()));
        }
        Self {
            states: data
                .project
                .workflow
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect(),
            people,
            sprints: data
                .sprints
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect(),
        }
    }

    fn lookup(list: &[(String, String)], id: &str) -> String {
        list.iter()
            .find(|(known, _)| known == id)
            .map_or_else(|| id.to_owned(), |(_, name)| name.clone())
    }
}

/// One activity line: `Ana Silva changed State from Open to In Progress · 2 h ago`. Ids are shown as names where the names are
/// known, and a system event has no actor.
pub fn activity_line(entry: &ActivityEntry, names: &Names, now: i64) -> String {
    let actor = entry
        .actor
        .as_ref()
        .map_or("System", |user| user.display_name.as_str());
    let what = match entry.kind.as_str() {
        "created" => "created the issue".to_owned(),
        "comment_added" => "commented".to_owned(),
        "link_added" => "added a link".to_owned(),
        "link_removed" => "removed a link".to_owned(),
        "field_changed" => field_change(entry, names),
        other => format!("changed the issue ({other})"),
    };
    format!("{actor} {what} · {}", relative_time(entry.created_at, now))
}

/// The words for a field change. A comment's and a description's text is not shown, only that it changed.
fn field_change(entry: &ActivityEntry, names: &Names) -> String {
    let field = entry.field.as_deref().unwrap_or_default();
    match field {
        "comment" => "edited a comment".to_owned(),
        "description" => "changed the description".to_owned(),
        _ => format!(
            "changed {} from {} to {}",
            field_label(field),
            value_text(field, entry.old_value.as_ref(), names),
            value_text(field, entry.new_value.as_ref(), names),
        ),
    }
}

/// The label of a field in an activity line.
fn field_label(field: &str) -> &str {
    match field {
        "summary" => "Title",
        "type" => "Type",
        "priority" => "Priority",
        "state_id" => "State",
        "assignee_id" => "Assignee",
        "parent_id" => "Parent",
        "sprint_id" => "Sprint",
        "estimate_points" => "Estimate",
        "due_date" => "Due date",
        "tags" => "Tags",
        other => other,
    }
}

/// The text of one old or new value of a field. Absent or `null` is "none" (or the field's own word for nothing).
fn value_text(field: &str, value: Option<&serde_json::Value>, names: &Names) -> String {
    match value {
        None | Some(serde_json::Value::Null) => match field {
            "assignee_id" => "Unassigned",
            "sprint_id" => "no sprint",
            "parent_id" => "no parent",
            _ => "none",
        }
        .to_owned(),
        Some(serde_json::Value::String(id)) => match field {
            "state_id" => Names::lookup(&names.states, id),
            "assignee_id" => Names::lookup(&names.people, id),
            "sprint_id" => Names::lookup(&names.sprints, id),
            _ => id.clone(),
        },
        Some(serde_json::Value::Array(items)) if items.is_empty() => "none".to_owned(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map_or_else(|| item.to_string(), str::to_owned)
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    }
}

/// The link rows, grouped by their label: the first row of each group carries the label as its heading. The server's order is
/// kept inside a group.
pub fn link_rows(links: &[IssueLink]) -> Vec<LinkView> {
    let mut sorted: Vec<&IssueLink> = links.iter().collect();
    sorted.sort_by(|a, b| a.label.cmp(&b.label));
    let mut last: Option<&str> = None;
    sorted
        .into_iter()
        .map(|link| {
            let heading = if last == Some(link.label.as_str()) {
                String::new()
            } else {
                link.label.clone()
            };
            last = Some(link.label.as_str());
            LinkView {
                id: link.id.clone(),
                heading,
                key: link.issue.key.clone(),
                summary: link.issue.summary.clone(),
                state: link.issue.state.name.clone(),
            }
        })
        .collect()
}

/// The request-free part of the window's state, from the request state. Runs on the UI thread.
fn show<W: IssueWindow>(ui: &W, s: &Shared) {
    let state = if s.data.is_some() {
        LoadState::Loaded
    } else if s.error.is_some() {
        LoadState::Error
    } else {
        LoadState::Loading
    };
    ui.show_issue_state(state);
    ui.show_issue_error(&s.error.clone().unwrap_or_default());
    ui.show_saving(s.busy);
    ui.show_action_error(&s.action_error);
    ui.show_confirm_delete(s.confirm_delete);
    let me = ui.issue_user_id();
    // The issue's place in the list on show, for the previous and next arrows. `None` when the list is for another project.
    let place = s.list.as_ref().and_then(|list| {
        let keys = lock_keys(list);
        (keys.project == s.project)
            .then(|| place_of(&keys.keys, &s.key))
            .flatten()
    });
    let Some(data) = &s.data else {
        ui.show_access(false, false, false);
        ui.clear_body();
        return;
    };
    let edit = can_create(data.project.my_role.as_deref());
    let moderate = is_admin(data);
    let now = chrono::Utc::now().timestamp();
    ui.show_access(edit, moderate, moderate);
    let mut detail = view(data, now, place);
    let (tiles, images, image_index) = attachment_tiles(&data.issue.attachments, s, edit);
    detail.attachment_tiles = tiles;
    detail.attachment_images = images;
    detail.attachment_image_index = image_index;
    let work: Vec<WorkView> = data
        .work_items
        .iter()
        .map(|entry| WorkView {
            id: entry.id.clone(),
            author: entry.author.display_name.clone(),
            minutes: minutes_text(entry.minutes),
            spent_on: entry.spent_on.clone(),
            note: entry.note.clone(),
            changeable: edit && (entry.author.id == me || moderate),
        })
        .collect();
    let files: Vec<AttachmentView> = data
        .issue
        .attachments
        .iter()
        .map(|file| AttachmentView {
            id: file.id.clone(),
            name: file.file_name.clone(),
            size: size_text(file.size_bytes),
            changeable: edit,
        })
        .collect();
    let subtasks: Vec<IssueRefView> = data
        .issue
        .subtasks
        .iter()
        .map(|sub| IssueRefView {
            key: sub.key.clone(),
            summary: sub.summary.clone(),
            state: sub.state.name.clone(),
        })
        .collect();
    let comments: Vec<CommentView> = data
        .comments
        .iter()
        .map(|comment| CommentView {
            id: comment.id.clone(),
            initials: initials(&comment.author.display_name),
            author: comment.author.display_name.clone(),
            when: comment_when(comment, now),
            body: comment.body.clone(),
            changeable: edit && (comment.author.id == me || moderate),
            editing: s.comment_editing.as_deref() == Some(comment.id.as_str()),
        })
        .collect();
    let names = Names::of(data);
    let activity: Vec<String> = data
        .activity
        .iter()
        .map(|entry| activity_line(entry, &names, now))
        .collect();
    ui.show_body(Body {
        view: detail,
        work,
        files,
        subtasks,
        links: link_rows(&data.issue.links),
        comments,
        activity,
    });
}

/// "2 h ago", with "edited" when the comment changed after it was written.
fn comment_when(comment: &Comment, now: i64) -> String {
    let when = relative_time(comment.created_at, now);
    if comment.updated_at > comment.created_at {
        format!("{when} · edited")
    } else {
        when
    }
}

/// The header fields as the page draws them: the values, and the choices of each select. The first choice of the assignee and
/// the sprint is "none" (an empty value).
fn view(data: &IssueData, now: i64, place: Option<(usize, usize)>) -> IssueView {
    let i = &data.issue;
    let spent = if i.spent_minutes == 0 {
        "None".to_owned()
    } else {
        minutes_text(i.spent_minutes)
    };
    let mut assignees = vec![choice("", "Unassigned")];
    assignees.extend(
        data.project
            .members
            .iter()
            .map(|m| choice(&m.user.id, &m.user.display_name)),
    );
    let mut sprints = vec![choice("", "No sprint")];
    sprints.extend(data.sprints.iter().map(|s| choice(&s.id, &s.name)));
    let states: Vec<Choice> = data
        .project
        .workflow
        .iter()
        .map(|s| choice(&s.id, &s.name))
        .collect();
    let parent = i.parent.as_ref();
    IssueView {
        key: i.key.clone(),
        project_name: data.project.name.clone(),
        position: place
            .map(|(index, total)| format!("{} of {total}", index + 1))
            .unwrap_or_default(),
        has_prev: place.is_some_and(|(index, _)| index > 0),
        has_next: place.is_some_and(|(index, total)| index + 1 < total),
        votes: i32::try_from(i.votes).unwrap_or(i32::MAX),
        voted: i.voted_by_me,
        starred: i.starred_by_me,
        spent,
        updated_by: i
            .updated_by
            .as_ref()
            .map(|user| user.display_name.as_str())
            .unwrap_or_default()
            .to_owned(),
        summary: i.summary.clone(),
        issue_type: i.issue_type.clone(),
        state_id: i.state.id.clone(),
        state_name: i.state.name.clone(),
        sprint_name: i
            .sprint
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("None")
            .to_owned(),
        priority: i.priority.clone(),
        assignee_id: i
            .assignee
            .as_ref()
            .map(|a| a.id.as_str())
            .unwrap_or_default()
            .to_owned(),
        assignee_name: i
            .assignee
            .as_ref()
            .map(|a| a.display_name.as_str())
            .unwrap_or("Unassigned")
            .to_owned(),
        sprint_id: i
            .sprint
            .as_ref()
            .map(|s| s.id.as_str())
            .unwrap_or_default()
            .to_owned(),
        estimate: i.estimate_points.map(|n| n.to_string()).unwrap_or_default(),
        due_date: i.due_date.clone().unwrap_or_default(),
        tags: i.tags.clone(),
        area_chips: area_chips(&i.tags, &data.areas),
        reporter: i.reporter.display_name.clone(),
        created: relative_time(i.created_at, now),
        updated: relative_time(i.updated_at, now),
        description: plain_description(&i.description),
        has_parent: parent.is_some(),
        parent_key: parent
            .map(|p| p.key.as_str())
            .unwrap_or_default()
            .to_owned(),
        parent_summary: parent
            .map(|p| p.summary.as_str())
            .unwrap_or_default()
            .to_owned(),
        attachment_tiles: Vec::new(),
        attachment_images: Vec::new(),
        attachment_image_index: Vec::new(),
        state_options: states,
        type_options: options(ISSUE_TYPES),
        priority_options: options(PRIORITIES),
        assignee_options: assignees,
        sprint_options: sprints,
    }
}

/// The attachment tiles, the viewer's images, and for each tile the index of its image (-1 for a file that is not an image). A
/// file is an image once its picture is cached: its type is an image and its bytes decoded.
fn attachment_tiles(
    files: &[Attachment],
    s: &Shared,
    changeable: bool,
) -> (Vec<AttachmentTileView>, Vec<ImageView>, Vec<i32>) {
    let mut tiles = Vec::with_capacity(files.len());
    let mut images = Vec::new();
    let mut image_index = Vec::with_capacity(files.len());
    for file in files {
        let name: String = file.file_name.clone();
        let size: String = size_text(file.size_bytes);
        match s.pictures.ready.get(&file.id).map(page_data::picture) {
            Some(thumbnail) => {
                image_index.push(i32::try_from(images.len()).unwrap_or(-1));
                images.push(ImageView {
                    image: thumbnail.clone(),
                    title: name.clone(),
                });
                tiles.push(AttachmentTileView {
                    name,
                    size,
                    is_image: true,
                    thumbnail: Some(thumbnail),
                    changeable,
                });
            }
            None => {
                image_index.push(-1);
                tiles.push(AttachmentTileView {
                    name,
                    size,
                    is_image: false,
                    thumbnail: None,
                    changeable,
                });
            }
        }
    }
    (tiles, images, image_index)
}

/// The issue's areas as chips: each name in its catalogue colour. An area missing from the catalogue (or with a bad colour)
/// gets the neutral grey. The ink is black or white, whichever reads on the colour.
fn area_chips(tags: &[String], areas: &[Area]) -> Vec<AreaChipView> {
    tags.iter()
        .map(|name| {
            let color = areas
                .iter()
                .find(|area| &area.name == name)
                .and_then(|area| parse_hex(&area.color))
                .unwrap_or((0x8A, 0x8F, 0x98));
            let luma = 0.299 * f32::from(color.0)
                + 0.587 * f32::from(color.1)
                + 0.114 * f32::from(color.2);
            let ink = if luma > 160.0 {
                (0x1A, 0x1A, 0x1A)
            } else {
                (0xFF, 0xFF, 0xFF)
            };
            AreaChipView {
                name: name.clone(),
                color,
                ink,
            }
        })
        .collect()
}

/// The colour of a `#RRGGBB` value as RGB bytes, or None when it is not one.
fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// The choices of a select: each stored value, labelled with the stored value.
fn options(values: &[&str]) -> Vec<Choice> {
    values.iter().map(|value| choice(value, value)).collect()
}

/// Loads the issue, its project, sprints, comments and activity on a worker, then delivers the result.
fn fetch_load<W: IssueWindow>(
    ui: &Weak<W>,
    source: &Arc<DataSource>,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    project: String,
    key: String,
) {
    let shared = shared.clone();
    let thumbnails = source.clone();
    source.fetch(
        ui,
        move |api, token| async move { load_all(&api, &token, &project, &key).await },
        move |ui, result| {
            let loaded = result.is_ok();
            deliver_load(ui, &shared, generation, result);
            if loaded {
                fetch_thumbnails(ui, &thumbnails, &shared, generation);
            }
        },
    );
}

/// Fetches the bytes of each image attachment that has no picture yet, one request each, and decodes them on the worker. A
/// picture that decodes is cached and drawn; one that does not is remembered in `Shared::failed`, so it is not asked for again.
fn fetch_thumbnails<W: IssueWindow>(
    ui: &W,
    source: &Arc<DataSource>,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
) {
    let (issue, wanted) = {
        let mut guard = lock(shared);
        let s = &mut *guard;
        if s.generation != generation {
            return;
        }
        let Some(data) = &s.data else {
            return;
        };
        let wanted: Vec<String> = data
            .issue
            .attachments
            .iter()
            .filter(|file| file.content_type.starts_with("image/"))
            .filter(|file| {
                !s.pictures.ready.contains_key(&file.id)
                    && !s.pictures.pending.contains(&file.id)
                    && !s.failed.contains(&file.id)
            })
            .map(|file| file.id.clone())
            .collect();
        for id in &wanted {
            s.pictures.pending.insert(id.clone());
        }
        (s.key.clone(), wanted)
    };
    for id in wanted {
        let (issue, done_id, shared) = (issue.clone(), id.clone(), shared.clone());
        source.fetch(
            &ui.as_weak(),
            move |api, token| async move {
                let bytes = api.attachment_bytes(&token, &issue, &id).await?;
                page_data::decode_picture(&bytes)
            },
            move |ui, result| {
                let mut guard = lock(&shared);
                guard.pictures.pending.remove(&done_id);
                match result {
                    Ok(buffer) => {
                        guard.pictures.ready.insert(done_id, buffer);
                    }
                    Err(_) => {
                        guard.failed.insert(done_id);
                    }
                }
                show(ui, &guard);
            },
        );
    }
}

/// Reads everything one issue page shows.
async fn load_all(
    api: &ApiClient,
    token: &str,
    project: &str,
    key: &str,
) -> Result<IssueData, ClientError> {
    let issue = api.issue(token, key).await?;
    let project = api.project(token, project).await?;
    let sprints = api.sprints(token, &project.key).await?;
    let comments = api.comments(token, key).await?;
    let activity = api.activity(token, key).await?;
    let work_items = api.work_items(token, key).await?;
    // The colours are cosmetic: a failed read shows the areas in the neutral colour rather than failing the page.
    let areas = api.areas(token).await.unwrap_or_default();
    Ok(IssueData {
        issue,
        project,
        sprints,
        comments,
        activity,
        work_items,
        areas,
    })
}

/// Sends one write. The reply is not kept: after a write the page loads the issue again.
async fn send(
    api: &ApiClient,
    token: &str,
    project: &str,
    key: &str,
    request: Request,
) -> Result<(), ClientError> {
    match request {
        Request::Subtask(body) => api.create_issue(token, project, &body).await.map(|_| ()),
        Request::AddLink(body) => api.add_link(token, key, &body).await.map(|_| ()),
        Request::RemoveLink(id) => api.remove_link(token, key, &id).await,
        Request::AddComment(body) => api.add_comment(token, key, &body).await.map(|_| ()),
        Request::EditComment { id, body } => {
            api.update_comment(token, key, &id, &body).await.map(|_| ())
        }
        Request::DeleteComment(id) => api.delete_comment(token, key, &id).await,
        Request::Delete => api.delete_issue(token, key).await,
        Request::Vote(on) => api.set_vote(token, key, on).await.map(|_| ()),
        Request::Star(on) => api.set_star(token, key, on).await.map(|_| ()),
        Request::AddWorkItem(body) => api.add_work_item(token, key, &body).await.map(|_| ()),
        Request::DeleteWorkItem(id) => api.delete_work_item(token, key, &id).await,
        Request::Attach => attach_files(api, token, key).await,
        Request::DeleteAttachment(id) => api.delete_attachment(token, key, &id).await,
    }
}

/// Asks where to save `bytes` as `name`, then writes them. `Ok(None)` when the file was saved or the dialog was cancelled;
/// `Ok(Some(message))` when the file could not be written.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn save_attachment(name: &str, bytes: &[u8]) -> Result<Option<String>, ClientError> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_title("Save attachment")
        .set_file_name(name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    Ok(handle
        .write(bytes)
        .await
        .err()
        .map(|error| format!("{name} could not be saved: {error}")))
}

/// The web build saves the bytes through the browser's download, so the save needs no dialog and always succeeds here
/// (a failure is reported as a message).
#[cfg(target_arch = "wasm32")]
async fn save_attachment(name: &str, bytes: &[u8]) -> Result<Option<String>, ClientError> {
    Ok(download_in_browser(name, bytes)
        .err()
        .map(|error| format!("{name} could not be saved: {error}")))
}

/// Starts a browser download of `bytes` as `name`: a Blob behind a temporary object URL, clicked through a hidden anchor
/// with the `download` attribute. The object URL is released a second later, after the browser has started the download.
#[cfg(target_arch = "wasm32")]
fn download_in_browser(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};
    let fail = |error: wasm_bindgen::JsValue| format!("{error:?}");
    let window = web_sys::window().ok_or_else(|| "no page".to_owned())?;
    let document = window.document().ok_or_else(|| "no page".to_owned())?;
    let body = document.body().ok_or_else(|| "no page body".to_owned())?;
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let options = BlobPropertyBag::new();
    options.set_type(attachment_type(name));
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(fail)?;
    let url = Url::create_object_url_with_blob(&blob).map_err(fail)?;
    let anchor: HtmlAnchorElement = document
        .create_element("a")
        .map_err(fail)?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|element| fail(element.into()))?;
    anchor.set_href(&url);
    anchor.set_download(name);
    body.append_child(&anchor).map_err(fail)?;
    anchor.click();
    body.remove_child(&anchor).map_err(fail)?;
    let release = wasm_bindgen::prelude::Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&url);
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(release.unchecked_ref(), 1000)
        .map_err(fail)?;
    Ok(())
}

/// Saves the Android hook: writes the bytes as the given name and returns the path written, or the reason it failed. The
/// Android app registers it with `set_android_saver` before the window opens.
#[cfg(target_os = "android")]
type AndroidSaver = Box<dyn Fn(&str, &[u8]) -> Result<String, String> + Send + Sync>;

#[cfg(target_os = "android")]
static ANDROID_SAVER: std::sync::OnceLock<AndroidSaver> = std::sync::OnceLock::new();

/// Registers the Android save for attachments (the Android app calls it once, with rimelight_platform's save). Only the first
/// registration counts.
#[cfg(target_os = "android")]
pub fn set_android_saver(
    save: impl Fn(&str, &[u8]) -> Result<String, String> + Send + Sync + 'static,
) {
    let _ = ANDROID_SAVER.set(Box::new(save));
}

/// Writes an attachment to the Android downloads folder through the registered save. The name is kept to one path part, so
/// a name from the server cannot reach another folder.
#[cfg(target_os = "android")]
async fn save_attachment(name: &str, bytes: &[u8]) -> Result<Option<String>, ClientError> {
    let Some(save) = ANDROID_SAVER.get() else {
        return Ok(Some(format!(
            "{name} could not be saved: saving is not set up"
        )));
    };
    let file_name: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    Ok(save(&file_name, bytes)
        .err()
        .map(|error| format!("{name} could not be saved: {error}")))
}

/// Chooses files with the native dialog and uploads each one. Runs on the worker, as the photo upload does. Cancelling uploads
/// nothing. A file over the limit is refused before the rest are sent.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn attach_files(api: &ApiClient, token: &str, key: &str) -> Result<(), ClientError> {
    use space_proto::projects::ATTACHMENT_MAX_BYTES;
    let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("Attach files")
        .pick_files()
        .await
    else {
        return Ok(());
    };
    for file in files {
        let name = file.file_name();
        let bytes = file.read().await;
        if bytes.len() > ATTACHMENT_MAX_BYTES {
            return Err(ClientError::Api {
                status: 413,
                error: "too_large".to_owned(),
                message: format!("{name} is larger than 10 MB."),
            });
        }
        api.upload_attachment(token, key, &name, attachment_type(&name), bytes)
            .await?;
    }
    Ok(())
}

/// The web and Android builds have no native file dialog here, so attaching does nothing.
#[cfg(not(all(not(target_arch = "wasm32"), not(target_os = "android"))))]
async fn attach_files(_api: &ApiClient, _token: &str, _key: &str) -> Result<(), ClientError> {
    Ok(())
}

/// Keeps a load's result if it is for the latest request, then shows the issue or the error.
fn deliver_load<W: IssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    result: Result<IssueData, ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    match result {
        Ok(data) => {
            s.data = Some(data);
            s.error = None;
        }
        Err(error) => s.error = Some(page_data::error_text(&error)),
    }
    show(ui, &s);
}

/// Ends a write. A success loads the issue again (or goes to the list after a delete). A refusal or a failure is shown under the
/// page. `source` is `None` in tests, where nothing is loaded again.
fn finish<W: IssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    source: Option<&Arc<DataSource>>,
    generation: u64,
    result: Result<(), ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    s.busy = false;
    s.confirm_delete = false;
    match result {
        Ok(()) => {
            if s.last == Some(Request::Delete) {
                let route = list_route(&s.project);
                drop(s);
                ui.open_route(route);
                return;
            }
            s.comment_editing = None;
            s.action_error.clear();
            s.generation += 1;
            let (next, project, key) = (s.generation, s.project.clone(), s.key.clone());
            show(ui, &s);
            drop(s);
            if let Some(source) = source {
                fetch_load(&ui.as_weak(), source, shared, next, project, key);
            }
        }
        Err(error) => {
            s.action_error = page_data::error_text(&error);
            show(ui, &s);
        }
    }
}

/// Locks the list's keys. A panic in another holder does not stop the page.
fn lock_keys(list: &SharedKeys) -> MutexGuard<'_, ListKeys> {
    list.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Locks the request state. A panic in another holder does not stop the page.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_proto::projects::{ActivityEntry, UserRef, WorkflowState};

    fn user(id: &str, name: &str) -> UserRef {
        UserRef {
            id: id.to_owned(),
            display_name: name.to_owned(),
        }
    }

    fn entry(
        kind: &str,
        field: Option<&str>,
        old: Option<&str>,
        new: Option<&str>,
    ) -> ActivityEntry {
        ActivityEntry {
            id: "a1".to_owned(),
            kind: kind.to_owned(),
            field: field.map(str::to_owned),
            old_value: old.map(|v| serde_json::Value::String(v.to_owned())),
            new_value: new.map(|v| serde_json::Value::String(v.to_owned())),
            actor: Some(user("u-ana", "Ana Silva")),
            created_at: 0,
        }
    }

    #[test]
    fn an_issue_route_gives_the_project_and_the_key() {
        assert_eq!(ids_of("/p/EMB/issues/EMB-12"), Some(("EMB", "EMB-12")));
        assert_eq!(ids_of("/p/EMB/issues"), None, "the list is not an issue");
        assert_eq!(ids_of("/p/EMB/issues/EMB-12/extra"), None);
        assert_eq!(ids_of("/p//issues/EMB-1"), None);
        assert_eq!(ids_of("/p/EMB/board/EMB-1"), None);
    }

    #[test]
    fn a_subtask_and_a_link_are_checked_before_they_are_sent() {
        let sub = subtask_body("issue-id", " Write the test ").expect("subtask");
        assert_eq!(sub.summary, "Write the test");
        assert_eq!(sub.parent_id.as_deref(), Some("issue-id"));
        assert!(subtask_body("issue-id", "").is_err());
        let link = link_body(" emb-12 ", "blocks").expect("link");
        assert_eq!(link.target, "EMB-12");
        assert_eq!(link.kind, "blocks");
        assert!(link_body("EMB12", "blocks").is_err(), "not a key");
        assert!(link_body("EMB-12", "clones").is_err(), "not a kind");
    }

    #[test]
    fn comments_are_trimmed_and_not_empty() {
        assert_eq!(comment_text("  Done. "), Ok("Done.".to_owned()));
        assert!(comment_text("   ").is_err());
    }

    #[test]
    fn activity_lines_read_as_sentences_with_names() {
        let names = Names {
            states: vec![
                ("s1".to_owned(), "Open".to_owned()),
                ("s2".to_owned(), "In Progress".to_owned()),
            ],
            people: vec![("u-ben".to_owned(), "Ben Brook".to_owned())],
            sprints: Vec::new(),
        };
        let mut state = entry("field_changed", Some("state_id"), Some("s1"), Some("s2"));
        state.created_at = 100;
        assert_eq!(
            activity_line(&state, &names, 100 + 2 * 3600),
            "Ana Silva changed State from Open to In Progress · 2 h ago",
        );
        let created = entry("created", None, None, None);
        assert_eq!(
            activity_line(&created, &names, 30),
            "Ana Silva created the issue · just now"
        );
        let assigned = ActivityEntry {
            actor: None,
            old_value: None,
            ..entry("field_changed", Some("assignee_id"), None, Some("u-ben"))
        };
        assert_eq!(
            activity_line(&assigned, &names, 30),
            "System changed Assignee from Unassigned to Ben Brook · just now",
            "a system event has no actor, and a null value is the field's word for nothing"
        );
        assert_eq!(
            activity_line(&entry("comment_added", None, None, None), &names, 30),
            "Ana Silva commented · just now"
        );
        assert_eq!(
            activity_line(
                &entry("field_changed", Some("comment"), None, None),
                &names,
                30
            ),
            "Ana Silva edited a comment · just now",
            "a comment's text is not shown in the log"
        );
    }

    #[test]
    fn spent_time_is_checked_and_shown_in_hours_and_minutes() {
        let entry = work_item_body(" 90 ", "2026-11-14", " reviewed ").expect("entry");
        assert_eq!(entry.minutes, 90);
        assert_eq!(entry.spent_on.as_deref(), Some("2026-11-14"));
        assert_eq!(entry.note.as_deref(), Some("reviewed"));
        let today = work_item_body("30", "", "").expect("today");
        assert_eq!(
            (today.spent_on, today.note),
            (None, None),
            "empty is today, no note"
        );
        assert!(work_item_body("0", "", "").is_err(), "at least one minute");
        assert!(work_item_body("1.5", "", "").is_err(), "whole minutes");
        assert!(
            work_item_body("30", "14/11/2026", "").is_err(),
            "YYYY-MM-DD"
        );
        assert_eq!(minutes_text(0), "0m");
        assert_eq!(minutes_text(45), "45m");
        assert_eq!(minutes_text(120), "2h");
        assert_eq!(minutes_text(150), "2h 30m");
    }

    #[test]
    fn the_description_reads_as_plain_text_with_bullets() {
        assert_eq!(
            plain_description("# Steps\n- open the menu\n  * press **Save**\nDone `now`"),
            "Steps\n\u{2022} open the menu\n  \u{2022} press Save\nDone now",
        );
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(12 * 1024), "12 KB");
        assert_eq!(size_text(3 * 1024 * 1024 / 2), "1.5 MB");
    }

    #[test]
    fn the_neighbours_follow_the_list_order() {
        let keys: Vec<String> = ["EMB-11", "EMB-12", "EMB-13"]
            .iter()
            .map(|k| k.to_string())
            .collect();
        assert_eq!(place_of(&keys, "EMB-12"), Some((1, 3)));
        assert_eq!(place_of(&keys, "EMB-99"), None, "not in the list");
        assert_eq!(neighbour(&keys, "EMB-12", -1).as_deref(), Some("EMB-11"));
        assert_eq!(neighbour(&keys, "EMB-12", 1).as_deref(), Some("EMB-13"));
        assert_eq!(
            neighbour(&keys, "EMB-11", -1),
            None,
            "the first has no previous"
        );
        assert_eq!(neighbour(&keys, "EMB-13", 1), None, "the last has no next");
    }

    #[test]
    fn links_are_grouped_by_their_label() {
        let link = |id: &str, label: &str, key: &str| IssueLink {
            id: id.to_owned(),
            kind: "blocks".to_owned(),
            direction: "outward".to_owned(),
            label: label.to_owned(),
            issue: space_proto::projects::IssueSummary {
                id: key.to_owned(),
                key: key.to_owned(),
                summary: "x".to_owned(),
                issue_type: "Task".to_owned(),
                priority: "Normal".to_owned(),
                state: WorkflowState {
                    id: "s1".to_owned(),
                    name: "Open".to_owned(),
                    category: "Unstarted".to_owned(),
                    position: 1,
                },
                assignee: None,
                parent_id: None,
                sprint_id: None,
                estimate_points: None,
                due_date: None,
                tags: Vec::new(),
                backlog_rank: "0".to_owned(),
                updated_at: 0,
            },
        };
        let links = vec![
            link("l1", "blocks", "EMB-1"),
            link("l2", "is blocked by", "EMB-2"),
            link("l3", "blocks", "EMB-3"),
        ];
        let rows = link_rows(&links);
        let headings: Vec<String> = rows.iter().map(|r| r.heading.to_string()).collect();
        assert_eq!(headings, vec!["blocks", "", "is blocked by"]);
        let keys: Vec<String> = rows.iter().map(|r| r.key.to_string()).collect();
        assert_eq!(
            keys,
            vec!["EMB-1", "EMB-3", "EMB-2"],
            "server order kept inside a group"
        );
    }

    #[test]
    fn download_target_finds_the_file_by_index() {
        let file = |id: &str, name: &str| Attachment {
            id: id.to_owned(),
            file_name: name.to_owned(),
            content_type: "application/pdf".to_owned(),
            size_bytes: 1,
            url: String::new(),
        };
        let files = [file("a1", "spec.pdf"), file("a2", "notes.txt")];
        assert_eq!(
            download_target(&files, 1),
            Some(("a2".to_owned(), "notes.txt".to_owned()))
        );
        assert_eq!(download_target(&files, 2), None);
        assert_eq!(download_target(&files, -1), None);
    }
}
