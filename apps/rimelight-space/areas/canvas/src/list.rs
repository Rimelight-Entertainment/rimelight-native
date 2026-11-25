//! Canvas lists (CV.6, CV.11): the personal canvases at `/home/canvases` and one project's canvases at `/p/{KEY}/canvases`. The
//! page's markup is `areas/canvas/ui/canvas-list.slint` and its `CanvasListState` global. The list is a Table: its rows come
//! from the loaded canvases in the chosen sort order, and the selection is one canvas, kept by id across a reload. A row
//! opens its canvas in the same page tab (through the window's `canvas-open-route`), and New creates a canvas in this list's
//! scope and opens it.
//!
//! Selection changes update only the `selected` flag of the rows that change, in place, on the Table's one rows model
//! (`VecModel`), so the Table keeps its row elements and a double-click still reaches its row. The rows are rebuilt only when
//! the list itself changes (load, sort, rename, archive, delete, move).
//!
//! The personal list also has Share (the people the canvas is shared with, and an Add person search) and Move to project. The
//! storage quota line shows the list's scope quota.
//!
//! The page data pattern (`space_page_kit::page_data`): a load bumps a generation, and a result for an older generation is
//! dropped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{Timer, Weak};
use space_client::canvases::CanvasScope;
use space_client::error::ClientError;
use space_client::people::DirectoryQuery;
use space_proto::canvases::{CanvasView, CreateCanvas, Quota, Share};
use space_proto::people::PersonSummary;

use crate::window::{CanvasListWindow, ListChoice, ListColumn, ListHeader, ListRow, ListSelection};
use rimelight_ui_kit::table::{clamp_width, sort_rows};
use rimelight_ui_kit::toast::{self, Tone};
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell};
use space_page_kit::picker::choice;

/// The columns of the list, in order: Name, Last updated, Created and Access.
const NAME: usize = 0;
const UPDATED: usize = 1;
const CREATED: usize = 2;
const ACCESS: usize = 3;

const COLUMN_TITLES: [&str; 4] = ["Name", "Last updated", "Created", "Access"];
/// The width of each column when the list opens, in logical pixels.
const COLUMN_WIDTHS: [f32; 4] = [360.0, 180.0, 180.0, 120.0];
/// How long the Add person field waits after the last keystroke before it searches.
const SEARCH_DELAY: Duration = Duration::from_millis(300);
/// How many people the Add person search shows.
const SEARCH_LIMIT: u32 = 8;
/// The role a person added to a canvas gets.
const DEFAULT_ROLE: &str = "Viewer";
/// Bytes in a gigabyte, as a float for the GB text.
const GIB: f64 = 1_073_741_824.0;

/// The scope a canvas list route shows: `/home/canvases` is personal, `/p/{KEY}/canvases` is one project's canvases. The
/// project key is 2 to 10 ASCII letters or digits. `None` for any other route.
pub fn list_scope_of(route: &str) -> Option<CanvasScope> {
    if route == "/home/canvases" {
        return Some(CanvasScope::Personal);
    }
    let key = route.strip_prefix("/p/")?.strip_suffix("/canvases")?;
    ((2..=10).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| CanvasScope::Project(key.to_string()))
}

/// The route of a canvas: `/home/canvases/{id}` for a personal canvas, `/p/{KEY}/canvases/{id}` for a project one.
pub fn route_of(view: &CanvasView) -> String {
    if view.home == "personal" {
        format!("/home/canvases/{}", view.id)
    } else {
        format!(
            "/p/{}/canvases/{}",
            view.project_key.as_deref().unwrap_or(""),
            view.id
        )
    }
}

/// The route of canvas `id` in `scope`.
fn route_in(scope: &CanvasScope, id: &str) -> String {
    match scope {
        CanvasScope::Project(key) => format!("/p/{key}/canvases/{id}"),
        CanvasScope::Personal | CanvasScope::Shared => format!("/home/canvases/{id}"),
    }
}

/// The title a dialog typed for a new canvas, trimmed. Empty means no title: the server picks one.
fn named_title(raw: &str) -> Option<String> {
    let title = raw.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// The time a canvas was updated, as the list shows it: `YYYY-MM-DD HH:MM` from the ISO timestamp.
fn updated_text(iso: &str) -> String {
    iso.chars().take(16).collect::<String>().replace('T', " ")
}

/// The Access column's label for a canvas's `my_access`.
fn access_label(access: &str) -> &'static str {
    match access {
        "owner" | "manage" => "Can manage",
        "edit" => "Can edit",
        _ => "View only",
    }
}

/// Whether the caller owns or manages a personal canvas: the ones Share and Move to project apply to.
fn personal_and_managed(canvas: &CanvasView) -> bool {
    canvas.home == "personal" && matches!(canvas.my_access.as_str(), "owner" | "manage")
}

/// The text a canvas sorts by in `column`: its title, its created or updated time (ISO text, which sorts by time), or its
/// access label.
fn sort_text(canvas: &CanvasView, column: usize) -> String {
    match column {
        NAME => canvas.title.clone(),
        CREATED => canvas.created_at.clone(),
        ACCESS => access_label(&canvas.my_access).to_string(),
        _ => canvas.updated_at.clone(),
    }
}

/// The canvases in display order: sorted by the chosen column and direction.
fn ordered(s: &Shared) -> Vec<CanvasView> {
    let column = s.sort_column;
    sort_rows(
        &s.canvases,
        |canvas: &CanvasView| sort_text(canvas, column),
        s.sort_descending,
    )
}

/// The place of the selected canvas in `rows`, if it is in the list.
fn position_of(s: &Shared, rows: &[CanvasView]) -> Option<usize> {
    s.selected_id.as_ref().and_then(|id| {
        rows.iter()
            .position(|canvas| canvas.id.as_str() == id.as_str())
    })
}

/// The column a Table index names, if it is one of the list's columns.
fn column_index(column: i32) -> Option<usize> {
    usize::try_from(column)
        .ok()
        .filter(|&c| c < COLUMN_TITLES.len())
}

/// The index a dialog's list names, if it is a valid index.
fn index_of(index: i32) -> Option<usize> {
    usize::try_from(index).ok()
}

/// The Table's columns: their titles and widths, all sortable.
fn columns_model(widths: [f32; 4]) -> Vec<ListColumn> {
    COLUMN_TITLES
        .iter()
        .zip(widths)
        .map(|(title, width)| ListColumn {
            title: (*title).to_string(),
            width,
            sortable: true,
        })
        .collect()
}

/// A byte count in GB, with at most two decimals and no trailing zeros: `2`, `0.5`, `1.25`.
fn gb_text(bytes: u64) -> String {
    let gb = format!("{:.2}", bytes as f64 / GIB);
    gb.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A byte count as the quota line shows it: GB, MB or KB.
fn size_text(bytes: u64) -> String {
    if bytes >= 1_u64 << 30 {
        format!("{} GB", gb_text(bytes))
    } else if bytes >= 1_u64 << 20 {
        format!("{} MB", bytes >> 20)
    } else {
        format!("{} KB", bytes >> 10)
    }
}

/// The share of a quota in use, 0 to 1. A zero limit is 0.
fn ratio(used: u64, limit: u64) -> f32 {
    if limit == 0 {
        0.0
    } else {
        (used as f64 / limit as f64).clamp(0.0, 1.0) as f32
    }
}

/// A quota as the page keeps it: what the quota line needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct QuotaView {
    usage_bytes: u64,
    limit_bytes: u64,
}

impl From<Quota> for QuotaView {
    fn from(quota: Quota) -> Self {
        Self {
            usage_bytes: quota.usage_bytes,
            limit_bytes: quota.limit_bytes,
        }
    }
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    /// The scope of the list on the page, from its route.
    scope: Option<CanvasScope>,
    /// True when the list shows the archived canvases instead of the active ones.
    archived: bool,
    /// True once a load has succeeded.
    loaded: bool,
    /// The text of the last failed load. `None` when there is none.
    error: Option<String>,
    canvases: Vec<CanvasView>,
    /// The column the list is sorted by, and whether the order is descending.
    sort_column: usize,
    sort_descending: bool,
    /// The width of each column, as the person has resized it.
    widths: [f32; 4],
    /// The id of the selected canvas. Cleared when the list no longer has it.
    selected_id: Option<String>,
    /// The quota of the list's scope, when it has loaded.
    quota: Option<QuotaView>,
    /// The canvas the Share dialog is for, and who it is shared with.
    share_id: Option<String>,
    shares: Vec<Share>,
    /// The people the Add person search found, and the search's number: a result for an older search is dropped.
    share_results: Vec<PersonSummary>,
    search_generation: u64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            generation: 0,
            scope: None,
            archived: false,
            loaded: false,
            error: None,
            canvases: Vec::new(),
            sort_column: UPDATED,
            sort_descending: true,
            widths: COLUMN_WIDTHS,
            selected_id: None,
            quota: None,
            share_id: None,
            shares: Vec::new(),
            share_results: Vec::new(),
            search_generation: 0,
        }
    }
}

impl Shared {
    /// The Table's rows: one per canvas, in display order, with the selection flag set on the selected one.
    fn table_rows(&self, rows: &[CanvasView]) -> Vec<ListRow> {
        rows.iter()
            .map(|canvas| ListRow {
                cells: vec![
                    canvas.title.clone(),
                    updated_text(&canvas.updated_at),
                    updated_text(&canvas.created_at),
                    access_label(&canvas.my_access).to_string(),
                ],
                selected: self.selected_id.as_deref() == Some(canvas.id.as_str()),
            })
            .collect()
    }
}

/// The canvas lists: the state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct CanvasList<W: CanvasListWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: CanvasListWindow> Clone for CanvasList<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the `CanvasListState` callbacks. `data` is the slot the session driver fills; until it is set, a load does nothing
/// and the page stays in its loading state.
pub fn wire<W: CanvasListWindow>(ui: &W, data: DataCell) -> CanvasList<W> {
    let page = CanvasList {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };
    ui.init_models(vec![choice("Viewer", "Viewer"), choice("Editor", "Editor")]);

    let target = page.clone();
    ui.on_opened(move |route| target.opened(route.as_str()));

    let target = page.clone();
    ui.on_retry(move || target.reload());

    let target = page.clone();
    ui.on_create(move || target.create(None));

    let target = page.clone();
    ui.on_create_named(move |title| target.create(named_title(title.as_str())));

    let target = page.clone();
    ui.on_toggle_archived(move || target.toggle_archived());

    let target = page.clone();
    ui.on_sort_by(move |column| target.sort_by(column));

    let target = page.clone();
    ui.on_pick(move |index| target.pick(index));

    let target = page.clone();
    ui.on_open_row(move |index| target.open_row(index));

    let target = page.clone();
    ui.on_resize_column(move |column, width| target.resize_column(column, width));

    let target = page.clone();
    ui.on_row_action(move |id, action| target.row_action(id.as_str(), action.as_str()));

    let target = page.clone();
    ui.on_rename_confirmed(move |id, title| target.rename(id.as_str(), title.as_str()));

    let target = page.clone();
    ui.on_delete_confirmed(move |id| target.delete(id.as_str()));

    let target = page.clone();
    ui.on_share_query_changed(move |query| target.search_people(query.as_str()));

    let target = page.clone();
    ui.on_share_add(move |index| target.share_add(index));

    let target = page.clone();
    ui.on_share_role_changed(move |index, role| target.share_role(index, role.as_str()));

    let target = page.clone();
    ui.on_share_remove(move |index| target.share_remove(index));

    let target = page.clone();
    ui.on_move_confirmed(move |id, key| target.move_to(id.as_str(), key.as_str()));

    page
}

impl<W: CanvasListWindow> CanvasList<W> {
    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// Delivers a load's result, as the worker would. For tests: no source is set, so nothing is fetched.
    pub fn deliver(&self, generation: u64, result: Result<Vec<CanvasView>, ClientError>) {
        if let Some(ui) = self.ui.upgrade() {
            apply_load(&ui, &self.shared, generation, result);
        }
    }

    /// The page opened on `route`: its scope is read from the route and its canvases are loaded.
    fn opened(&self, route: &str) {
        let Some(scope) = list_scope_of(route) else {
            return;
        };
        lock(&self.shared).scope = Some(scope);
        self.reload();
    }

    /// Loads the canvases of the page's scope, active or archived as the page shows, and the scope's quota. Every load starts
    /// from the loading state.
    fn reload(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let (generation, scope, archived) = {
            let mut s = lock(&self.shared);
            let Some(scope) = s.scope.clone() else {
                return;
            };
            s.generation += 1;
            s.loaded = false;
            s.error = None;
            show(&ui, &mut s);
            (s.generation, scope, s.archived)
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.refresh_quota(&scope);
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.canvases(&token, &scope, archived).await },
            move |ui, result| apply_load(ui, &shared, generation, result),
        );
    }

    /// Loads the quota of the list's scope and draws the quota line. A failure hides the line.
    fn refresh_quota(&self, scope: &CanvasScope) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let project = match scope {
            CanvasScope::Project(key) => Some(key.clone()),
            CanvasScope::Personal | CanvasScope::Shared => None,
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                match project {
                    Some(key) => api.project_canvas_quota(&token, &key).await,
                    None => api.my_canvas_quota(&token).await,
                }
            },
            move |ui, result| {
                let mut s = lock(&shared);
                match result {
                    Ok(quota) => s.quota = Some(QuotaView::from(quota)),
                    Err(_) => s.quota = None,
                }
                draw_quota(ui, &s);
            },
        );
    }

    /// Flips between the active and the archived canvases, and loads the other list.
    fn toggle_archived(&self) {
        {
            let mut s = lock(&self.shared);
            s.archived = !s.archived;
        }
        self.reload();
    }

    /// Applies `edit` to the request state, then draws the page again.
    fn redraw(&self, edit: impl FnOnce(&mut Shared)) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut s = lock(&self.shared);
        edit(&mut s);
        show(&ui, &mut s);
    }

    /// Applies `edit` to the share dialog's state, then draws the dialog again.
    fn redraw_share(&self, edit: impl FnOnce(&mut Shared)) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut s = lock(&self.shared);
        edit(&mut s);
        draw_share(&ui, &s);
    }

    /// Sorts the list by `column` (a Table column index). The same column flips the direction. A new column sorts A to Z
    /// for Name and Access, and newest first for the two dates.
    fn sort_by(&self, column: i32) {
        let Some(column) = column_index(column) else {
            return;
        };
        self.redraw(|s| {
            if s.sort_column == column {
                s.sort_descending = !s.sort_descending;
            } else {
                s.sort_column = column;
                s.sort_descending = column == UPDATED || column == CREATED;
            }
        });
    }

    /// The id of the canvas at row `index` in display order, if there is one.
    fn id_at(&self, index: i32) -> Option<String> {
        let index = usize::try_from(index).ok()?;
        let s = lock(&self.shared);
        let rows = ordered(&s);
        rows.get(index).map(|canvas| canvas.id.as_str().to_string())
    }

    /// Selects only the canvas at row `index`. Only the selected flags of the rows change, in place, so the Table's row
    /// elements survive and a double-click reaches its row.
    fn pick(&self, index: i32) {
        let Some(id) = self.id_at(index) else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut s = lock(&self.shared);
        s.selected_id = Some(id);
        mark_selection(&ui, &s);
    }

    /// Opens the canvas at row `index`.
    fn open_row(&self, index: i32) {
        if let Some(id) = self.id_at(index) {
            self.open(&id);
        }
    }

    /// Sets the width of a column after a drag. The width is clamped to the Table's minimum.
    fn resize_column(&self, column: i32, width: f32) {
        let Some(column) = column_index(column) else {
            return;
        };
        self.redraw(|s| s.widths[column] = clamp_width(width));
    }

    /// The title of canvas `id` on the page, if the list has it.
    fn title_of(&self, id: &str) -> Option<String> {
        lock(&self.shared)
            .canvases
            .iter()
            .find(|canvas| canvas.id.as_str() == id)
            .map(|canvas| canvas.title.clone())
    }

    /// A row action: Open opens the canvas, Rename, Share, Move and Delete open their dialogs, and Archive and Unarchive act at
    /// once.
    fn row_action(&self, id: &str, action: &str) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        match action {
            "open" => self.open(id),
            "rename" => {
                let Some(title) = self.title_of(id) else {
                    return;
                };
                ui.open_rename(id, &title);
            }
            "delete" => {
                let Some(title) = self.title_of(id) else {
                    return;
                };
                ui.open_delete(id, &title);
            }
            "share" => self.open_share(id),
            "move" => self.open_move(id),
            "archive" => self.set_archived(id, true),
            "unarchive" => self.set_archived(id, false),
            _ => {}
        }
    }

    /// Whether canvas `id` is a personal canvas the caller manages: the only kind Share and Move apply to.
    fn is_personal_and_managed(&self, id: &str) -> bool {
        lock(&self.shared)
            .canvases
            .iter()
            .any(|canvas| canvas.id.as_str() == id && personal_and_managed(canvas))
    }

    /// Opens the Share dialog for canvas `id` and loads who it is shared with.
    fn open_share(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !self.is_personal_and_managed(id) {
            return;
        }
        let title = self.title_of(id).unwrap_or_default();
        {
            let mut s = lock(&self.shared);
            s.share_id = Some(id.to_string());
            s.shares.clear();
            s.share_results.clear();
            s.search_generation += 1;
            draw_share(&ui, &s);
        }
        ui.open_share(id, &title);

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let work_id = id.to_string();
        let done_id = id.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.canvas_shares(&token, &work_id).await },
            move |ui, result| match result {
                Ok(shares) => {
                    let mut s = lock(&shared);
                    if s.share_id.as_deref() != Some(done_id.as_str()) {
                        return;
                    }
                    s.shares = shares;
                    draw_share(ui, &s);
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Searches the people directory for the Add person field. The search waits [`SEARCH_DELAY`] after the last keystroke;
    /// an empty field clears the results.
    fn search_people(&self, query: &str) {
        let query = query.trim().to_string();
        let generation = {
            let mut s = lock(&self.shared);
            s.search_generation += 1;
            s.search_generation
        };
        if query.is_empty() {
            self.redraw_share(|s| s.share_results.clear());
            return;
        }
        let target = self.clone();
        Timer::single_shot(SEARCH_DELAY, move || target.run_search(generation, query));
    }

    /// Runs the people search of `generation`, unless a newer keystroke has replaced it.
    fn run_search(&self, generation: u64, query: String) {
        if lock(&self.shared).search_generation != generation {
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let filter = DirectoryQuery {
            q: Some(query),
            limit: Some(SEARCH_LIMIT),
            ..Default::default()
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.directory(&token, &filter).await },
            move |ui, result| {
                let mut s = lock(&shared);
                if s.search_generation != generation {
                    return;
                }
                match result {
                    Ok(page) => s.share_results = page.items,
                    Err(error) => {
                        s.share_results.clear();
                        toast::announce(Tone::Danger, page_data::error_text(&error));
                    }
                }
                draw_share(ui, &s);
            },
        );
    }

    /// Shares the open canvas with the person at `index` of the search results, as a Viewer.
    fn share_add(&self, index: i32) {
        let Some(index) = index_of(index) else {
            return;
        };
        let (canvas_id, person_id) = {
            let s = lock(&self.shared);
            let Some(person) = s.share_results.get(index) else {
                return;
            };
            let Some(canvas_id) = s.share_id.clone() else {
                return;
            };
            (canvas_id, person.id.clone())
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let check_id = canvas_id.clone();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.set_canvas_share(&token, &canvas_id, &person_id, DEFAULT_ROLE)
                    .await
            },
            move |ui, result| match result {
                Ok(share) => {
                    let mut s = lock(&shared);
                    if s.share_id.as_deref() != Some(check_id.as_str()) {
                        return;
                    }
                    toast::announce(
                        Tone::Success,
                        format!("Shared with {} as {}.", share.name, share.role),
                    );
                    s.shares
                        .retain(|existing| existing.user_id != share.user_id);
                    s.shares.push(share);
                    s.share_results.clear();
                    draw_share(ui, &s);
                    ui.set_share_query("");
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Changes the role of the share at `index` of the Share dialog's list.
    fn share_role(&self, index: i32, role: &str) {
        let Some(index) = index_of(index) else {
            return;
        };
        let (canvas_id, user_id) = {
            let s = lock(&self.shared);
            let Some(share) = s.shares.get(index) else {
                return;
            };
            let Some(canvas_id) = s.share_id.clone() else {
                return;
            };
            (canvas_id, share.user_id.clone())
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let check_id = canvas_id.clone();
        let role = role.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.set_canvas_share(&token, &canvas_id, &user_id, &role)
                    .await
            },
            move |ui, result| {
                let mut s = lock(&shared);
                if s.share_id.as_deref() != Some(check_id.as_str()) {
                    return;
                }
                match result {
                    Ok(share) => {
                        if let Some(existing) =
                            s.shares.iter_mut().find(|e| e.user_id == share.user_id)
                        {
                            *existing = share;
                        }
                    }
                    Err(error) => {
                        toast::announce(Tone::Danger, page_data::error_text(&error));
                    }
                }
                // Also on a failure: the list is drawn again, so the role select shows the stored role.
                draw_share(ui, &s);
            },
        );
    }

    /// Takes the person at `index` of the Share dialog's list off the canvas.
    fn share_remove(&self, index: i32) {
        let Some(index) = index_of(index) else {
            return;
        };
        let (canvas_id, user_id, name) = {
            let s = lock(&self.shared);
            let Some(share) = s.shares.get(index) else {
                return;
            };
            let Some(canvas_id) = s.share_id.clone() else {
                return;
            };
            (canvas_id, share.user_id.clone(), share.name.clone())
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let check_id = canvas_id.clone();
        let removed_id = user_id.clone();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.remove_canvas_share(&token, &canvas_id, &user_id).await },
            move |ui, result| match result {
                Ok(()) => {
                    let mut s = lock(&shared);
                    if s.share_id.as_deref() != Some(check_id.as_str()) {
                        return;
                    }
                    s.shares.retain(|share| share.user_id != removed_id);
                    draw_share(ui, &s);
                    toast::announce(Tone::Success, format!("Removed {name}."));
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Opens the Move to project dialog for canvas `id`, and fills its project list with the projects the caller is an admin
    /// or a member of.
    fn open_move(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if !self.is_personal_and_managed(id) {
            return;
        }
        let Some(title) = self.title_of(id) else {
            return;
        };
        ui.open_move(id, &title);

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        source.fetch(
            &self.ui,
            move |api, token| async move { api.projects(&token).await },
            move |ui, result| match result {
                Ok(projects) => {
                    let options: Vec<ListChoice> = projects
                        .iter()
                        .filter(|p| matches!(p.my_role.as_deref(), Some("admin" | "member")))
                        .map(|p| choice(&p.key, &p.name))
                        .collect();
                    ui.set_move_options(options);
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Moves canvas `id` into the project `key`. On success the canvas leaves this personal list. A failure (a 413 when the
    /// project is over its quota, for one) is a toast.
    fn move_to(&self, id: &str, key: &str) {
        if key.is_empty() {
            toast::announce(Tone::Danger, "Pick a project to move the canvas to.");
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let work_id = id.to_string();
        let work_key = key.to_string();
        let done_id = id.to_string();
        let done_key = key.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.move_canvas(&token, &work_id, &work_key).await },
            move |ui, result| match result {
                Ok(_) => {
                    let mut s = lock(&shared);
                    s.canvases.retain(|canvas| canvas.id != done_id);
                    show(ui, &mut s);
                    toast::announce(Tone::Success, format!("Moved to {done_key}."));
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Archives (`archive`) or restores canvas `id`. On success the row leaves this list. A failure is a toast.
    fn set_archived(&self, id: &str, archive: bool) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let work_id = id.to_string();
        let done_id = id.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                if archive {
                    api.archive_canvas(&token, &work_id).await
                } else {
                    api.unarchive_canvas(&token, &work_id).await
                }
            },
            move |ui, result| match result {
                Ok(_) => {
                    let mut s = lock(&shared);
                    s.canvases.retain(|canvas| canvas.id != done_id);
                    show(ui, &mut s);
                    let text = if archive {
                        "Canvas archived."
                    } else {
                        "Canvas restored."
                    };
                    toast::announce(Tone::Success, text);
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Renames canvas `id` to the trimmed `raw` title. An empty title does nothing. A failure is a toast.
    fn rename(&self, id: &str, raw: &str) {
        let Some(title) = named_title(raw) else {
            return;
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let work_id = id.to_string();
        let done_id = id.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.rename_canvas(&token, &work_id, &title).await },
            move |ui, result| match result {
                Ok(view) => {
                    let mut s = lock(&shared);
                    if let Some(canvas) = s.canvases.iter_mut().find(|c| c.id == done_id) {
                        canvas.title = view.title;
                    }
                    show(ui, &mut s);
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Permanently deletes canvas `id` and removes its row. A failure is a toast.
    fn delete(&self, id: &str) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let work_id = id.to_string();
        let done_id = id.to_string();
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.delete_canvas(&token, &work_id).await },
            move |ui, result| match result {
                Ok(()) => {
                    let mut s = lock(&shared);
                    s.canvases.retain(|canvas| canvas.id != done_id);
                    show(ui, &mut s);
                    toast::announce(Tone::Success, "Canvas deleted.");
                }
                Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
            },
        );
    }

    /// Creates a canvas in the page's scope and opens it. `title` names it; `None` lets the server pick. A failure is a toast.
    fn create(&self, title: Option<String>) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let Some(scope) = lock(&self.shared).scope.clone() else {
            return;
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let project_key = match &scope {
            CanvasScope::Project(key) => Some(key.clone()),
            CanvasScope::Personal | CanvasScope::Shared => None,
        };
        ui.set_creating(true);
        let body = CreateCanvas { title, project_key };
        source.fetch(
            &self.ui,
            move |api, token| async move { api.create_canvas(&token, &body).await },
            move |ui, result| {
                ui.set_creating(false);
                match result {
                    Ok(view) => ui.canvas_open_route(&route_of(&view)),
                    Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
                }
            },
        );
    }

    /// Opens canvas `id` of the page's scope in the same page tab.
    fn open(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let Some(scope) = lock(&self.shared).scope.clone() else {
            return;
        };
        ui.canvas_open_route(&route_in(&scope, id));
    }
}

/// Keeps a load's result if it is for the latest load, then shows the list or the error.
fn apply_load<W: CanvasListWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Vec<CanvasView>, ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    match result {
        Ok(canvases) => {
            s.canvases = canvases;
            s.loaded = true;
            s.error = None;
        }
        Err(error) => {
            s.loaded = false;
            s.canvases.clear();
            s.error = Some(page_data::error_text(&error));
        }
    }
    show(ui, &mut s);
}

/// Draws the whole page from the request state: the load state, the columns, the rows (rebuilt in the one rows model) and the
/// selection. A selection whose canvas is no longer in the list is cleared. Used when the list itself changes.
fn show<W: CanvasListWindow>(ui: &W, s: &mut Shared) {
    let state = match (&s.error, s.loaded) {
        (Some(_), _) => LoadState::Error,
        (None, true) if s.canvases.is_empty() => LoadState::Empty,
        (None, true) => LoadState::Loaded,
        (None, false) => LoadState::Loading,
    };
    let rows = ordered(s);
    let position = position_of(s, &rows);
    if position.is_none() {
        s.selected_id = None;
    }

    ui.show_header(ListHeader {
        state,
        error: s.error.clone().unwrap_or_default(),
        can_create: s.scope.is_some(),
        show_archived: s.archived,
        sort_column: s.sort_column as i32,
        sort_descending: s.sort_descending,
        columns: columns_model(s.widths),
    });
    ui.set_table_rows(s.table_rows(&rows));
    draw_selection(ui, s, &rows, position);
}

/// Draws a selection change in place: the `selected` flag of each row whose state changed, on the same rows model, then the
/// selected canvas's properties. The rows are not rebuilt, so the Table's row elements survive.
fn mark_selection<W: CanvasListWindow>(ui: &W, s: &Shared) {
    let rows = ordered(s);
    let position = position_of(s, &rows);
    for (i, canvas) in rows.iter().enumerate() {
        let selected = s.selected_id.as_deref() == Some(canvas.id.as_str());
        if ui
            .table_row_selected(i)
            .is_some_and(|current| current != selected)
        {
            ui.set_table_row_selected(i, selected);
        }
    }
    draw_selection(ui, s, &rows, position);
}

/// Draws the selected canvas's properties: its place, id, and what the person may do with it.
fn draw_selection<W: CanvasListWindow>(
    ui: &W,
    s: &Shared,
    rows: &[CanvasView],
    position: Option<usize>,
) {
    let selected = position.map(|i| &rows[i]);
    ui.show_selection(ListSelection {
        index: position.map_or(-1, |i| i as i32),
        id: s.selected_id.clone().unwrap_or_default(),
        can_manage: selected.is_some_and(|canvas| canvas.my_access == "manage"),
        archived: selected.is_some_and(|canvas| canvas.archived),
        can_share: selected.is_some_and(personal_and_managed),
        can_move: selected.is_some_and(personal_and_managed),
    });
}

/// Draws the quota line: its text and bar.
fn draw_quota<W: CanvasListWindow>(ui: &W, s: &Shared) {
    match &s.quota {
        Some(quota) => ui.show_quota(Some((
            format!(
                "Storage: {} of {}",
                size_text(quota.usage_bytes),
                size_text(quota.limit_bytes)
            ),
            ratio(quota.usage_bytes, quota.limit_bytes),
        ))),
        None => ui.show_quota(None),
    }
}

/// Draws the Share dialog: the shares (name, email, role) and the people the search found (name, title).
fn draw_share<W: CanvasListWindow>(ui: &W, s: &Shared) {
    let shares: Vec<Vec<String>> = s
        .shares
        .iter()
        .map(|share| vec![share.name.clone(), share.email.clone(), share.role.clone()])
        .collect();
    ui.show_share_rows(shares);
    let results: Vec<Vec<String>> = s
        .share_results
        .iter()
        .map(|person| {
            vec![
                person.display_name.clone(),
                person.title.as_deref().unwrap_or("").to_string(),
            ]
        })
        .collect();
    ui.show_share_results(results);
    let message = if !ui.share_query().is_empty() && s.share_results.is_empty() {
        "No people found."
    } else {
        ""
    };
    ui.set_share_message(message);
}
