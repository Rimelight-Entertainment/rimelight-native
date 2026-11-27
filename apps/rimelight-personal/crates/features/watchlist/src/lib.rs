//! The Watchlist page controller (ported from `crates/app/src/watchlist.rs`).
//!
//! Plain Rust with no Slint. The controller holds the page state (the list, the section and archive filter, the search,
//! the category and form dialogs), calls the watchlist endpoints through [`WatchlistApi`], and reports each state change
//! as a [`WatchlistView`]. The Slint glue in `apps/desktop/src/pages/watchlist.rs` turns those views into the models of
//! `ui/watchlist.slint`.
//!
//! Threading: the API calls return at once and answer later on another thread (ehttp's worker). The controller locks
//! its state only around reads and writes, never across an API call, and `notify` runs after the lock is released.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rimelight_personal_core::watchlist::{
    Category, Item, ItemPatch, Row, Section, Status, WatchlistData, details, rows, to_csv,
};
use rimelight_personal_core::{ApiClient, ApiError};
use rimelight_ui_kit::list::ListModel;
use rimelight_ui_kit::table::SelectionMode;

/// The add and edit form, as text. Re-exported so the page glue needs no direct `rimelight_personal_core` dependency.
pub use rimelight_personal_core::watchlist::Draft;

/// The page's Slint markup, for the shell's build script (`apps/desktop/build.rs` adds it as `@rimelight_personal_watchlist`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The answer of every call that changes the watchlist: the whole list, as the server returns it.
pub type Reply = Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait WatchlistApi: Send + Sync {
    fn load(&self, on_done: Reply);
    fn add_category(&self, section: Section, name: &str, on_done: Reply);
    fn delete_category(&self, section: Section, cat_id: &str, on_done: Reply);
    fn add_item(&self, section: Section, cat_id: &str, item: &ItemPatch, on_done: Reply);
    fn update_item(
        &self,
        section: Section,
        cat_id: &str,
        item_id: &str,
        item: &ItemPatch,
        on_done: Reply,
    );
    fn delete_item(&self, section: Section, cat_id: &str, item_id: &str, on_done: Reply);
    fn toggle_completed(&self, section: Section, cat_id: &str, item_id: &str, on_done: Reply);
}

impl WatchlistApi for ApiClient {
    fn load(&self, on_done: Reply) {
        ApiClient::watchlist(self, on_done);
    }

    fn add_category(&self, section: Section, name: &str, on_done: Reply) {
        ApiClient::watchlist_add_category(self, section, name, on_done);
    }

    fn delete_category(&self, section: Section, cat_id: &str, on_done: Reply) {
        ApiClient::watchlist_delete_category(self, section, cat_id, on_done);
    }

    fn add_item(&self, section: Section, cat_id: &str, item: &ItemPatch, on_done: Reply) {
        ApiClient::watchlist_add_item(self, section, cat_id, item, on_done);
    }

    fn update_item(
        &self,
        section: Section,
        cat_id: &str,
        item_id: &str,
        item: &ItemPatch,
        on_done: Reply,
    ) {
        ApiClient::watchlist_update_item(self, section, cat_id, item_id, item, on_done);
    }

    fn delete_item(&self, section: Section, cat_id: &str, item_id: &str, on_done: Reply) {
        ApiClient::watchlist_delete_item(self, section, cat_id, item_id, on_done);
    }

    fn toggle_completed(&self, section: Section, cat_id: &str, item_id: &str, on_done: Reply) {
        ApiClient::watchlist_toggle_completed(self, section, cat_id, item_id, on_done);
    }
}

/// The live API for `api_base`, for example `https://example.com/api/personal`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn WatchlistApi> {
    Arc::new(ApiClient::new(api_base))
}

// ---------------------------------------------------------------------------
// Menus and keys
// ---------------------------------------------------------------------------

/// One entry of a Slint choice menu: the value the page sends back, and the text it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

fn choice(value: &str, label: &str) -> Choice {
    Choice {
        value: value.to_owned(),
        label: label.to_owned(),
    }
}

/// The Media Type menu of the add form (Solid's singular nouns).
pub fn section_choices() -> Vec<Choice> {
    Section::ALL
        .into_iter()
        .map(|section| choice(section.key(), section.noun()))
        .collect()
}

/// The Status menu of each table row, with the table's labels.
pub fn table_status_choices() -> Vec<Choice> {
    Status::ALL
        .into_iter()
        .map(|status| choice(status_key(status), status.label()))
        .collect()
}

/// The Status menu of the add and edit form, with Solid's descriptions.
pub fn form_status_choices() -> Vec<Choice> {
    Status::ALL
        .into_iter()
        .map(|status| choice(status_key(status), form_status_label(status)))
        .collect()
}

/// The key a menu uses for a status. The server keeps the same names (`in-progress`).
pub fn status_key(status: Status) -> &'static str {
    match status {
        Status::Planned => "planned",
        Status::InProgress => "in-progress",
        Status::Completed => "completed",
        Status::Dropped => "dropped",
    }
}

/// The status a menu key stands for, or `None` for an unknown key.
pub fn status_from_key(key: &str) -> Option<Status> {
    Status::ALL
        .into_iter()
        .find(|status| status_key(*status) == key)
}

/// The section a menu key stands for (`movies`, `series`, `books`), or `None` for an unknown key.
pub fn section_from_key(key: &str) -> Option<Section> {
    Section::ALL
        .into_iter()
        .find(|section| section.key() == key)
}

/// Solid's form status options, with the modal's descriptions.
fn form_status_label(status: Status) -> &'static str {
    match status {
        Status::Planned => "Planned (To Watch/Read)",
        Status::InProgress => "In Progress (Active)",
        Status::Completed => "Completed",
        Status::Dropped => "Dropped",
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// The table's columns, in the table's order. The title always shows; the others can be hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Title,
    Meta,
    Category,
    Genre,
    Rating,
    Status,
    Details,
    Notes,
}

impl Column {
    pub const ALL: [Column; 8] = [
        Column::Title,
        Column::Meta,
        Column::Category,
        Column::Genre,
        Column::Rating,
        Column::Status,
        Column::Details,
        Column::Notes,
    ];

    /// The column's header text.
    pub fn label(self) -> &'static str {
        match self {
            Column::Title => "Title",
            Column::Meta => "Creator",
            Column::Category => "Category",
            Column::Genre => "Genre",
            Column::Rating => "Rating",
            Column::Status => "Status",
            Column::Details => "Details",
            Column::Notes => "Notes",
        }
    }
}

/// One column's header state: whether it shows, and its sort order when the table is sorted by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnView {
    pub column: Column,
    pub label: String,
    pub visible: bool,
    /// `None` when the table is not sorted by this column; `Some(descending)` when it is.
    pub sorted: Option<bool>,
}

/// The CSV file name for a section, as the save dialog suggests it.
pub fn csv_file_name(section: Section) -> String {
    format!("watchlist-{}.csv", section.key())
}

/// The save dialog's file type: CSV only.
const CSV_FILTER: &[(&str, &[&str])] = &[("CSV", &["csv"])];

/// One table row, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub cat_id: String,
    pub item_id: String,
    pub title: String,
    pub completed: bool,
    /// Creator, year and recommender, as the grey line under the title. Empty when there are none.
    pub meta: String,
    /// The notes line under the meta. Empty when there are none.
    pub notes: String,
    pub category: String,
    /// The genre, or an em dash.
    pub genre: String,
    /// `★ 8.5`, or an em dash when the rating is unset or zero.
    pub rating: String,
    pub status: Status,
    /// The Details column: duration, seasons and episodes, or pages.
    pub details: String,
    /// Whether the row is selected (the list template's selection).
    pub selected: bool,
    /// The cells the table shows for this row, one per visible column, in the table's order.
    pub cells: Vec<String>,
}

/// The line the page shows when the table has no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyView {
    pub title: String,
    pub text: String,
}

/// One category, as the Categories dialog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    pub id: String,
    pub label: String,
    pub items: usize,
}

/// The add or edit form, as the page shows it. The field text is not here: the page holds it, and the glue reads it when
/// the form is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormView {
    pub editing: bool,
    pub title: String,
    pub description: String,
    pub section: Section,
    /// The category id the form saves into (Add), or the item's own category (Edit).
    pub category: String,
    pub categories: Vec<Choice>,
    pub busy: bool,
    pub no_category_hint: bool,
    /// Whether Save is enabled. The page also needs a non-blank title for that.
    pub can_save: bool,
    pub save_label: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistView {
    pub section: Section,
    pub archived: bool,
    pub heading: String,
    pub count_label: String,
    pub rows: Vec<RowView>,
    pub empty: Option<EmptyView>,
    /// The message of the last failed call, until the user dismisses it.
    pub error: Option<String>,
    pub categories_label: String,
    pub category_open: bool,
    pub category_title: String,
    pub categories: Vec<CategoryView>,
    /// The delete-category confirm is showing.
    pub confirm_category: bool,
    pub form: Option<FormView>,
    /// The table's columns with their visibility and sort state, in the table's order.
    pub columns: Vec<ColumnView>,
    /// The sorted column's index among the visible columns (what the list template's sort reports), or `None`.
    pub sort_column: Option<usize>,
    pub sort_descending: bool,
    /// How many rows are selected (the bulk bar shows it).
    pub selected_count: usize,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq)]
enum FormMode {
    Add,
    Edit { cat_id: String, item_id: String },
}

struct Form {
    mode: FormMode,
    section: Section,
    /// The category an add saves into. Edit keeps the item's own category.
    category: String,
    busy: bool,
}

struct State {
    data: WatchlistData,
    loaded: bool,
    error: Option<String>,
    section: Section,
    archived: bool,
    category_open: bool,
    /// Category id waiting for the delete confirm.
    confirm_category: Option<String>,
    form: Option<Form>,
    /// The filter, the sort and the selection of the table (the shared list rules).
    list: ListModel,
    /// Columns the person has hidden.
    hidden: Vec<Column>,
}

impl State {
    fn new() -> Self {
        State {
            data: WatchlistData::default(),
            loaded: false,
            error: None,
            section: Section::Movies,
            archived: false,
            category_open: false,
            confirm_category: None,
            form: None,
            list: ListModel::new(SelectionMode::Multi),
            hidden: Vec::new(),
        }
    }

    fn view(&self) -> WatchlistView {
        let key = self.section.key();
        let all = rows(&self.data, self.section, self.archived);
        let cells: Vec<Vec<String>> = all.iter().map(|row| cells_of(row, self.section)).collect();
        let order = self.list.order(&cells);
        let shown = self.shown_cells();
        let visible: Vec<RowView> = order
            .iter()
            .map(|&index| {
                let mut view = row_view(&all[index], self.section);
                view.selected = self.list.is_selected(index);
                view.cells = shown
                    .iter()
                    .map(|&cell| cells[index][cell].clone())
                    .collect();
                view
            })
            .collect();
        let empty = if !visible.is_empty() {
            None
        } else if !self.loaded {
            Some(EmptyView {
                title: "Loading watchlist...".to_owned(),
                text: String::new(),
            })
        } else if self.list.filter().trim().is_empty() {
            Some(EmptyView {
                title: if self.archived {
                    "Archive is empty"
                } else {
                    "No entries yet"
                }
                .to_owned(),
                text: "Add a movie, series, or book, or mark an item complete to archive it."
                    .to_owned(),
            })
        } else {
            Some(EmptyView {
                title: "No matching entries.".to_owned(),
                text: String::new(),
            })
        };
        let categories = self.data.section(self.section);
        WatchlistView {
            section: self.section,
            archived: self.archived,
            heading: if self.archived {
                format!("Archived {}", capitalized(key))
            } else {
                format!("{} Library Matrix", capitalized(key))
            },
            count_label: format!("({} entries)", visible.len()),
            rows: visible,
            empty,
            error: self.error.clone(),
            categories_label: format!("Categories ({})", categories.len()),
            category_open: self.category_open,
            category_title: format!("Manage {key} Categories"),
            categories: categories
                .iter()
                .map(|category| CategoryView {
                    id: category.id.clone(),
                    label: category_label(category),
                    items: category.items.len(),
                })
                .collect(),
            confirm_category: self.confirm_category.is_some(),
            form: self.form.as_ref().map(|form| form_view(form, &self.data)),
            sort_column: self
                .list
                .sort()
                .column
                .and_then(|cell| shown.iter().position(|&shown_cell| shown_cell == cell)),
            sort_descending: self.list.sort().descending,
            selected_count: self.list.selected_rows().len(),
            columns: Column::ALL
                .into_iter()
                .map(|column| ColumnView {
                    column,
                    label: column.label().to_owned(),
                    visible: !self.hidden.contains(&column),
                    sorted: match self.list.sort().column {
                        Some(cell) if Column::ALL[cell] == column => {
                            Some(self.list.sort().descending)
                        }
                        _ => None,
                    },
                })
                .collect(),
        }
    }
}

/// One row's eight cells, in [`Column::ALL`] order. The list rules filter and sort on these text cells.
fn cells_of(row: &Row<'_>, section: Section) -> Vec<String> {
    let view = row_view(row, section);
    vec![
        view.title,
        view.meta,
        view.category,
        view.genre,
        view.rating,
        view.status.label().to_owned(),
        view.details,
        view.notes,
    ]
}

/// The data rows in display order: the section's rows (live or archived), filtered and sorted by `list`. The cells of
/// each row are the same text the table shows, so a rating sorts as text (`★ 10` before `★ 9`).
fn display_rows<'a>(
    data: &'a WatchlistData,
    section: Section,
    archived: bool,
    list: &ListModel,
) -> Vec<Row<'a>> {
    let all = rows(data, section, archived);
    let cells: Vec<Vec<String>> = all.iter().map(|row| cells_of(row, section)).collect();
    let order = list.order(&cells);
    let mut slots: Vec<Option<Row<'a>>> = all.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|index| slots[index].take())
        .collect()
}

impl State {
    /// The cell indexes of the visible columns, in the table's order.
    fn shown_cells(&self) -> Vec<usize> {
        (0..Column::ALL.len())
            .filter(|&cell| !self.hidden.contains(&Column::ALL[cell]))
            .collect()
    }
}

fn row_view(row: &Row<'_>, section: Section) -> RowView {
    let item = row.item;
    let creator = non_empty(item.director.as_deref()).or(non_empty(item.author.as_deref()));
    let mut meta = Vec::new();
    if let Some(name) = creator {
        meta.push(name.to_owned());
    }
    if let Some(year) = item.year {
        meta.push(format!("• {year}"));
    }
    if let Some(by) = non_empty(item.recommended_by.as_deref()) {
        meta.push(format!("• via {by}"));
    }
    RowView {
        cat_id: row.cat_id.to_owned(),
        item_id: item.id.clone(),
        title: item.title.clone(),
        completed: item.completed,
        meta: meta.join(" "),
        notes: non_empty(item.notes.as_deref())
            .unwrap_or_default()
            .to_owned(),
        category: row.cat_name.to_owned(),
        genre: non_empty(item.genre.as_deref()).unwrap_or("—").to_owned(),
        rating: match item.rating {
            Some(rating) if rating != 0.0 => format!("★ {rating}"),
            _ => "—".to_owned(),
        },
        status: item.status,
        details: details(section, item),
        selected: false,
        cells: Vec::new(),
    }
}

fn form_view(form: &Form, data: &WatchlistData) -> FormView {
    let editing = matches!(form.mode, FormMode::Edit { .. });
    let (title, description) = if editing {
        (
            "Edit Media Details",
            "Configure media title, creator, rating and metadata",
        )
    } else {
        (
            "New Watchlist Item",
            "Add a movie, TV series, or book with full details",
        )
    };
    let save_label = match (editing, form.busy) {
        (true, _) => "Save Changes",
        (false, true) => "Adding...",
        (false, false) => "Add Entry",
    };
    FormView {
        editing,
        title: title.to_owned(),
        description: description.to_owned(),
        section: form.section,
        category: form.category.clone(),
        categories: data
            .section(form.section)
            .iter()
            .map(|category| choice(&category.id, &category_label(category)))
            .collect(),
        busy: form.busy,
        no_category_hint: !editing && form.category.is_empty(),
        can_save: !form.busy && (editing || !form.category.is_empty()),
        save_label: save_label.to_owned(),
    }
}

/// The page's state holder. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct WatchlistController {
    shared: Arc<Shared>,
}

type Notify = Arc<dyn Fn(WatchlistView) + Send + Sync>;

struct Shared {
    api: Arc<dyn WatchlistApi>,
    state: Mutex<State>,
    notify: Notify,
}

impl WatchlistController {
    /// `notify` receives the new view after every state change, possibly on a worker thread. Nothing is fetched until
    /// [`WatchlistController::activate`] or [`WatchlistController::refresh`].
    pub fn new(
        api: Arc<dyn WatchlistApi>,
        notify: impl Fn(WatchlistView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::new(State::new()),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> WatchlistView {
        self.lock().view()
    }

    /// The page opened on `route`: the section comes from the route (`/watchlist/series` is TV series), and the list is
    /// fetched again.
    pub fn activate(&self, route: &str) {
        self.update(|s| s.section = Section::from_route(route));
        self.refresh();
    }

    /// Fetches the whole watchlist again.
    pub fn refresh(&self) {
        let me = self.clone();
        self.shared
            .api
            .load(Box::new(move |result| me.finish(result, false)));
    }

    /// Switches between the live list and the Archive Bin (the completed items).
    pub fn toggle_archived(&self) {
        self.update(|s| s.archived = !s.archived);
    }

    /// The table search text. Matches are case-insensitive, over the title, creator, genre, notes and more.
    pub fn set_search(&self, query: &str) {
        self.update(|s| s.list.set_filter(query));
    }

    /// A click or Enter on a visible column header. `column` is the index among the visible columns, which is what the
    /// list template reports. The same column flips its direction; a new one sorts ascending.
    pub fn sort_requested(&self, column: usize) {
        self.update(|s| {
            if let Some(&cell) = s.shown_cells().get(column) {
                s.list.request_sort(cell);
            }
        });
    }

    /// A pick on the row at `display_index` (its position on screen). `shift` and `ctrl` are the modifier keys.
    pub fn pick(&self, display_index: usize, shift: bool, ctrl: bool) {
        self.update(|s| {
            let cells: Vec<Vec<String>> = rows(&s.data, s.section, s.archived)
                .iter()
                .map(|row| cells_of(row, s.section))
                .collect();
            s.list.pick(&cells, display_index, shift, ctrl);
        });
    }

    /// The (category id, item id, completed) of each selected row, in data order.
    fn selected_items(&self) -> Vec<(String, String, bool)> {
        let s = self.lock();
        let all = rows(&s.data, s.section, s.archived);
        s.list
            .selected_rows()
            .into_iter()
            .filter_map(|index| all.get(index))
            .map(|row| {
                (
                    row.cat_id.to_owned(),
                    row.item.id.clone(),
                    row.item.completed,
                )
            })
            .collect()
    }

    /// Marks every selected row completed (the bulk bar's action). Rows already completed are left as they are.
    pub fn complete_selected(&self) {
        for (cat_id, item_id, completed) in self.selected_items() {
            if !completed {
                self.toggle_completed(&cat_id, &item_id);
            }
        }
        self.update(|s| s.list.clear_selection());
    }

    /// Deletes every selected row (the bulk bar's action). Solid deletes without a confirm.
    pub fn delete_selected(&self) {
        for (cat_id, item_id, _) in self.selected_items() {
            self.delete_item(&cat_id, &item_id);
        }
        self.update(|s| s.list.clear_selection());
    }

    /// The one selected row as (category id, item id), for the Edit action. `None` unless exactly one row is selected.
    pub fn selected_item(&self) -> Option<(String, String)> {
        match self.selected_items().as_slice() {
            [(cat_id, item_id, _)] => Some((cat_id.clone(), item_id.clone())),
            _ => None,
        }
    }

    /// Flips the completed flag of an item, then the server's answer replaces the list. The flip shows at once.
    pub fn toggle_completed(&self, cat_id: &str, item_id: &str) {
        let section = self.update(|s| {
            let section = s.section;
            if let Some(item) = item_mut(&mut s.data, section, cat_id, item_id) {
                item.toggle_completed();
            }
            section
        });
        let me = self.clone();
        self.shared.api.toggle_completed(
            section,
            cat_id,
            item_id,
            Box::new(move |result| me.finish(result, false)),
        );
    }

    /// Sets the status of an item. The patch carries only the status and the completed flag, as Solid's select does.
    pub fn set_status(&self, cat_id: &str, item_id: &str, status: Status) {
        let section = self.section();
        let me = self.clone();
        self.shared.api.update_item(
            section,
            cat_id,
            item_id,
            &ItemPatch::status_only(status),
            Box::new(move |result| me.finish(result, false)),
        );
    }

    /// Deletes an item. Solid deletes without a confirm.
    pub fn delete_item(&self, cat_id: &str, item_id: &str) {
        let section = self.section();
        let me = self.clone();
        self.shared.api.delete_item(
            section,
            cat_id,
            item_id,
            Box::new(move |result| me.finish(result, false)),
        );
    }

    /// Opens the Manage Categories dialog.
    pub fn open_categories(&self) {
        self.update(|s| s.category_open = true);
    }

    /// Closes the Manage Categories dialog.
    pub fn close_categories(&self) {
        self.update(|s| s.category_open = false);
    }

    /// Adds a category named `name` (trimmed) to the current section. Returns `false`, and calls nothing, when the name is
    /// blank.
    pub fn add_category(&self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let section = self.section();
        let me = self.clone();
        self.shared.api.add_category(
            section,
            name,
            Box::new(move |result| me.finish(result, false)),
        );
        true
    }

    /// Asks to delete a category. The dialog then waits for [`WatchlistController::answer_delete_category`].
    pub fn request_delete_category(&self, cat_id: &str) {
        self.update(|s| s.confirm_category = Some(cat_id.to_owned()));
    }

    /// The answer to the delete-category confirm. Only a yes calls the API.
    pub fn answer_delete_category(&self, confirmed: bool) {
        let target = self.update(|s| s.confirm_category.take());
        if let (true, Some(cat_id)) = (confirmed, target) {
            let section = self.section();
            let me = self.clone();
            self.shared.api.delete_category(
                section,
                &cat_id,
                Box::new(move |result| me.finish(result, false)),
            );
        }
    }

    /// Opens the add form. Returns the draft the form starts with (this year, rating 8.5, one season); the page writes it
    /// into its fields. The category is the section's first one.
    pub fn open_add(&self, year: i64) -> Draft {
        self.update(|s| {
            let section = s.section;
            s.form = Some(Form {
                mode: FormMode::Add,
                section,
                category: first_category(&s.data, section),
                busy: false,
            });
            Draft::new_entry(year)
        })
    }

    /// Opens the edit form for an item. Returns the draft to show, or `None` when the item is not in the list.
    pub fn open_edit(&self, cat_id: &str, item_id: &str) -> Option<Draft> {
        self.update(|s| {
            let section = s.section;
            let draft = s
                .data
                .category(section, cat_id)?
                .items
                .iter()
                .find(|item| item.id == item_id)
                .map(Draft::from_item)?;
            s.form = Some(Form {
                mode: FormMode::Edit {
                    cat_id: cat_id.to_owned(),
                    item_id: item_id.to_owned(),
                },
                section,
                category: cat_id.to_owned(),
                busy: false,
            });
            Some(draft)
        })
    }

    /// The Media Type choice of an add form. The category resets to the new section's first one.
    pub fn set_form_section(&self, section: Section) {
        self.update(|s| {
            if let Some(form) = s.form.as_mut() {
                form.section = section;
                form.category = first_category(&s.data, section);
            }
        });
    }

    /// The Category choice of an add form.
    pub fn set_form_category(&self, cat_id: &str) {
        self.update(|s| {
            if let Some(form) = s.form.as_mut() {
                form.category = cat_id.to_owned();
            }
        });
    }

    /// Saves the form: adds the item, or updates the one being edited. Returns `false`, and calls nothing, when there is no
    /// form, a save is already running, the title is blank, or an add has no category. On success the form closes.
    pub fn submit(&self, draft: &Draft) -> bool {
        // `origin` is the item's own category (Edit only); `target` is the category picked in the form.
        let (section, origin, item_id, target, patch) = {
            let mut state = self.lock();
            let Some(form) = state.form.as_mut() else {
                return false;
            };
            if form.busy {
                return false;
            }
            let (origin, item_id) = match &form.mode {
                FormMode::Add => (String::new(), None),
                FormMode::Edit { cat_id, item_id } => (cat_id.clone(), Some(item_id.clone())),
            };
            let target = form.category.clone();
            if target.is_empty() {
                return false;
            }
            let Some(patch) = draft.to_patch(form.section) else {
                return false;
            };
            form.busy = true;
            (form.section, origin, item_id, target, patch)
        };
        self.notify_now();
        let me = self.clone();
        match item_id {
            None => self.shared.api.add_item(
                section,
                &target,
                &patch,
                Box::new(move |result| me.finish(result, true)),
            ),
            Some(item_id) if origin == target => self.shared.api.update_item(
                section,
                &origin,
                &item_id,
                &patch,
                Box::new(move |result| me.finish(result, true)),
            ),
            Some(item_id) => {
                // The picked category differs from the item's: add the item to it, then remove the old copy. The
                // server's list answer to the second call is the one the page shows.
                let api = self.shared.api.clone();
                self.shared.api.add_item(
                    section,
                    &target,
                    &patch,
                    Box::new(move |result| match result {
                        Ok(_) => api.delete_item(
                            section,
                            &origin,
                            &item_id,
                            Box::new(move |result| me.finish(result, true)),
                        ),
                        Err(error) => me.finish(Err(error), true),
                    }),
                );
            }
        }
        true
    }

    /// Shows or hides a column. The title always shows, so it cannot be hidden.
    pub fn toggle_column(&self, column: Column) {
        self.update(|s| {
            if column == Column::Title {
                return;
            }
            match s.hidden.iter().position(|hidden| *hidden == column) {
                Some(index) => {
                    s.hidden.remove(index);
                }
                None => s.hidden.push(column),
            }
        });
    }

    /// Writes the visible rows to a CSV file the person picks. Returns the path written, or `None` when they cancel
    /// the dialog. A write failure is returned as the message to show.
    pub fn save_csv(&self) -> Result<Option<PathBuf>, String> {
        let (name, text) = {
            let s = self.lock();
            let list = display_rows(&s.data, s.section, s.archived, &s.list);
            (csv_file_name(s.section), to_csv(s.section, &list))
        };
        let Some(path) = rimelight_platform::save_file_dialog(&name, CSV_FILTER) else {
            return Ok(None);
        };
        std::fs::write(&path, text)
            .map_err(|error| format!("Could not save {}: {error}", path.display()))?;
        Ok(Some(path))
    }

    /// Closes the add or edit form without saving.
    pub fn close_form(&self) {
        self.update(|s| s.form = None);
    }

    /// The CSV of the visible rows (the section, the archive filter and the search). The page copies it to the
    /// clipboard, as the egui page's Export CSV did.
    pub fn export_csv(&self) -> String {
        let s = self.lock();
        let visible = display_rows(&s.data, s.section, s.archived, &s.list);
        to_csv(s.section, &visible)
    }

    /// Shows `message` in the error banner, for a failure the page finds itself (the clipboard refused the copy).
    pub fn report_error(&self, message: &str) {
        self.update(|s| s.error = Some(message.to_owned()));
    }

    /// Clears the error message.
    pub fn dismiss_error(&self) {
        self.update(|s| s.error = None);
    }

    fn section(&self) -> Section {
        self.lock().section
    }

    /// Applies a list answer. A save also closes the form, as Solid's modal does; a failure ends the save and shows the
    /// message.
    fn finish(&self, result: Result<WatchlistData, ApiError>, saved: bool) {
        self.update(|s| match result {
            Ok(data) => {
                s.data = data;
                s.loaded = true;
                s.error = None;
                if saved {
                    s.form = None;
                }
            }
            Err(error) => {
                if let Some(form) = s.form.as_mut() {
                    form.busy = false;
                }
                s.error = Some(format!("Watchlist request failed: {error}"));
            }
        });
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in one callback must not make every later call panic too.
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn notify_now(&self) {
        let view = self.view();
        (self.shared.notify)(view);
    }

    /// Applies `change` under the lock, then notifies with the new view (the lock is released first). Returns what
    /// `change` returned.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> T {
        let (value, view) = {
            let mut state = self.lock();
            let value = change(&mut state);
            (value, state.view())
        };
        (self.shared.notify)(view);
        value
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

/// `movies` to `Movies`, as the CSS `capitalize` in Solid's headings shows it.
fn capitalized(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn category_label(category: &Category) -> String {
    if category.name.is_empty() {
        "Untitled".to_owned()
    } else {
        category.name.clone()
    }
}

fn first_category(data: &WatchlistData, section: Section) -> String {
    data.section(section)
        .first()
        .map(|category| category.id.clone())
        .unwrap_or_default()
}

fn item_mut<'a>(
    data: &'a mut WatchlistData,
    section: Section,
    cat_id: &str,
    item_id: &str,
) -> Option<&'a mut Item> {
    let categories: &mut Vec<Category> = match section {
        Section::Movies => &mut data.movies,
        Section::Series => &mut data.series,
        Section::Books => &mut data.books,
    };
    categories
        .iter_mut()
        .find(|category| category.id == cat_id)?
        .items
        .iter_mut()
        .find(|item| item.id == item_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each call and answers at once from one fixed list, or with a network error when `fail` is set.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        fail: Mutex<bool>,
        /// Answers with Heat still open, so both movies show in the live list (sort and pick tests).
        both_open: Mutex<bool>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn answer(&self) -> Result<WatchlistData, ApiError> {
            if *self.fail.lock().unwrap() {
                Err(ApiError::Transport("down".to_owned()))
            } else {
                let mut data = sample();
                if *self.both_open.lock().unwrap() {
                    data.movies[0].items[1].completed = false;
                }
                Ok(data)
            }
        }
    }

    impl WatchlistApi for Arc<Fake> {
        fn load(&self, on_done: Reply) {
            self.record("load".to_owned());
            on_done(self.answer());
        }

        fn add_category(&self, _section: Section, name: &str, on_done: Reply) {
            self.record(format!("add-category:{name}"));
            on_done(self.answer());
        }

        fn delete_category(&self, _section: Section, cat_id: &str, on_done: Reply) {
            self.record(format!("delete-category:{cat_id}"));
            on_done(self.answer());
        }

        fn add_item(&self, _section: Section, cat_id: &str, item: &ItemPatch, on_done: Reply) {
            self.record(format!("add:{cat_id}:{item:?}"));
            on_done(self.answer());
        }

        fn update_item(
            &self,
            _section: Section,
            _cat_id: &str,
            item_id: &str,
            item: &ItemPatch,
            on_done: Reply,
        ) {
            self.record(format!("update:{item_id}:{item:?}"));
            on_done(self.answer());
        }

        fn delete_item(&self, _section: Section, _cat_id: &str, item_id: &str, on_done: Reply) {
            self.record(format!("delete-item:{item_id}"));
            on_done(self.answer());
        }

        fn toggle_completed(
            &self,
            _section: Section,
            _cat_id: &str,
            item_id: &str,
            on_done: Reply,
        ) {
            self.record(format!("toggle:{item_id}"));
            on_done(self.answer());
        }
    }

    fn item(id: &str, title: &str, completed: bool) -> Item {
        Item {
            id: id.to_owned(),
            title: title.to_owned(),
            completed,
            status: if completed {
                Status::Completed
            } else {
                Status::Planned
            },
            ..Item::default()
        }
    }

    fn sample() -> WatchlistData {
        let mut dune = item("item-1", "Dune", false);
        dune.director = Some("Villeneuve".to_owned());
        dune.year = Some(2021);
        dune.rating = Some(8.5);
        dune.genre = Some("Sci-Fi".to_owned());
        dune.duration = Some("2h 35m".to_owned());
        let mut heat = item("item-2", "Heat", true);
        heat.director = Some("Mann".to_owned());
        heat.year = Some(1995);
        let mut severance = item("item-3", "Severance", false);
        severance.seasons = Some(2);
        severance.episodes = Some(19);
        WatchlistData {
            movies: vec![Category {
                id: "cat-a".to_owned(),
                name: "Sci-Fi".to_owned(),
                items: vec![dune, heat],
            }],
            series: vec![Category {
                id: "cat-s".to_owned(),
                name: "Drama".to_owned(),
                items: vec![severance],
            }],
            books: Vec::new(),
        }
    }

    fn controller(fake: &Arc<Fake>) -> (WatchlistController, Arc<Mutex<Vec<WatchlistView>>>) {
        let seen: Arc<Mutex<Vec<WatchlistView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn WatchlistApi> = Arc::new(fake.clone());
        let c = WatchlistController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn titles(view: &WatchlistView) -> Vec<&str> {
        view.rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[test]
    fn activate_loads_the_section_of_the_route() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist/series");

        assert_eq!(fake.calls(), ["load"]);
        let view = c.view();
        assert_eq!(view.section, Section::Series);
        assert_eq!(titles(&view), ["Severance"]);
        assert_eq!(view.heading, "Series Library Matrix");
        assert_eq!(view.count_label, "(1 entries)");
        assert_eq!(view.empty, None);
    }

    #[test]
    fn archive_bin_shows_only_completed_items() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        assert_eq!(titles(&c.view()), ["Dune"]);

        c.toggle_archived();
        let view = c.view();
        assert!(view.archived);
        assert_eq!(titles(&view), ["Heat"]);
        assert_eq!(view.heading, "Archived Movies");
    }

    #[test]
    fn search_is_case_insensitive_and_names_the_empty_result() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        c.set_search("  DUNE ");
        assert_eq!(titles(&c.view()), ["Dune"]);

        c.set_search("zzz");
        let view = c.view();
        assert!(view.rows.is_empty());
        assert_eq!(view.empty.unwrap().title, "No matching entries.");
    }

    #[test]
    fn toggle_flips_at_once_then_the_answer_replaces_the_list() {
        let fake = Arc::default();
        let (c, seen) = controller(&fake);
        c.activate("/watchlist");

        c.toggle_completed("cat-a", "item-1");
        assert_eq!(fake.calls().last().unwrap(), "toggle:item-1");
        // Dune is completed now, so it has left the live list before the answer arrives.
        let flipped = seen
            .lock()
            .unwrap()
            .iter()
            .any(|view| !view.archived && view.rows.iter().all(|row| row.title != "Dune"));
        assert!(flipped, "the flip is shown before the answer");
        // The fake's answer is the original list, so Dune is open again.
        assert!(!c.view().rows[0].completed);
    }

    #[test]
    fn status_change_sends_the_status_and_completed_flag_only() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        c.set_status("cat-a", "item-1", Status::Completed);
        let call = fake.calls().pop().unwrap();
        assert!(call.starts_with("update:item-1:"));
        assert!(call.contains("status: Some(Completed)"));
        assert!(call.contains("completed: Some(true)"));
        assert!(call.contains("title: None"));
    }

    #[test]
    fn blank_title_is_refused_and_a_valid_add_saves_then_closes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        let draft = c.open_add(2024);
        assert_eq!(draft.year, "2024");
        assert_eq!(draft.rating, "8.5");
        assert_eq!(draft.seasons, "1");
        let form = c.view().form.unwrap();
        assert_eq!(form.category, "cat-a");
        assert!(!form.editing);

        let blank = Draft {
            title: "   ".to_owned(),
            ..draft.clone()
        };
        assert!(!c.submit(&blank));
        assert_eq!(fake.calls(), ["load"]);

        let arrival = Draft {
            title: "Arrival".to_owned(),
            ..draft
        };
        assert!(c.submit(&arrival));
        let call = fake.calls().pop().unwrap();
        assert!(call.starts_with("add:cat-a:"));
        assert!(call.contains("title: Some(\"Arrival\")"));
        assert_eq!(c.view().form, None, "a saved form closes");
    }

    #[test]
    fn failed_save_keeps_the_form_open_with_the_message() {
        let fake: Arc<Fake> = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        let arrival = Draft {
            title: "Arrival".to_owned(),
            ..c.open_add(2024)
        };
        *fake.fail.lock().unwrap() = true;

        assert!(c.submit(&arrival));
        let view = c.view();
        let form = view.form.expect("the form stays open");
        assert!(!form.busy, "the save ends with the failure");
        assert_eq!(
            view.error.as_deref(),
            Some("Watchlist request failed: network error: down")
        );
    }

    #[test]
    fn edit_prefills_the_form_and_sends_an_update() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        let draft = c.open_edit("cat-a", "item-1").expect("Dune is in the list");
        assert_eq!(draft.title, "Dune");
        assert_eq!(draft.creator, "Villeneuve");
        assert_eq!(draft.year, "2021");
        assert!(c.view().form.unwrap().editing);

        let renamed = Draft {
            title: "Dune Part Two".to_owned(),
            ..draft
        };
        assert!(c.submit(&renamed));
        let call = fake.calls().pop().unwrap();
        assert!(call.starts_with("update:item-1:"));
        assert!(call.contains("title: Some(\"Dune Part Two\")"));
        assert_eq!(c.view().form, None);
    }

    #[test]
    fn switching_the_form_section_resets_its_category() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        c.open_add(2024);

        c.set_form_section(Section::Series);
        let form = c.view().form.unwrap();
        assert_eq!(form.section, Section::Series);
        assert_eq!(form.category, "cat-s");
    }

    #[test]
    fn category_add_refuses_blank_and_delete_waits_for_a_yes() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        assert!(!c.add_category("   "));
        assert!(c.add_category("  Favorites "));
        assert_eq!(fake.calls().pop().unwrap(), "add-category:Favorites");

        c.request_delete_category("cat-a");
        assert!(c.view().confirm_category);
        c.answer_delete_category(false);
        assert!(!c.view().confirm_category);
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| call.starts_with("delete-category"))
        );

        c.request_delete_category("cat-a");
        c.answer_delete_category(true);
        assert_eq!(fake.calls().pop().unwrap(), "delete-category:cat-a");
    }

    #[test]
    fn csv_export_has_the_header_and_the_visible_rows() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        let csv = c.export_csv();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some(
                "Done,Title,Creator,Category,Genre,Year,Rating,Status,Details,Recommended By,Notes"
            )
        );
        assert_eq!(
            lines.next(),
            Some("No,Dune,Villeneuve,Sci-Fi,Sci-Fi,2021,8.5,Planned,2h 35m,,")
        );
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn sort_orders_rows_and_a_second_press_flips_the_order() {
        let fake: Arc<Fake> = Arc::default();
        *fake.both_open.lock().unwrap() = true;
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        assert_eq!(
            titles(&c.view()),
            vec!["Dune", "Heat"],
            "unsorted keeps the list order"
        );

        c.sort_requested(0);
        assert_eq!(titles(&c.view()), vec!["Dune", "Heat"]);
        assert_eq!(c.view().columns[0].sorted, Some(false));

        c.sort_requested(0);
        assert_eq!(titles(&c.view()), vec!["Heat", "Dune"]);
        assert_eq!(c.view().columns[0].sorted, Some(true));

        c.sort_requested(4);
        assert_eq!(
            titles(&c.view()),
            vec!["Dune", "Heat"],
            "a rated cell is a number and sorts before the blank text of an unrated one"
        );
        let sorted: Vec<Option<bool>> = c
            .view()
            .columns
            .iter()
            .map(|column| column.sorted)
            .collect();
        assert_eq!(sorted[4], Some(false));
        assert_eq!(sorted[0], None);
    }

    #[test]
    fn columns_toggle_and_the_title_stays() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        assert!(c.view().columns.iter().all(|column| column.visible));

        c.toggle_column(Column::Notes);
        c.toggle_column(Column::Title);
        let view = c.view();
        let notes = view
            .columns
            .iter()
            .find(|column| column.column == Column::Notes)
            .unwrap();
        let title = view
            .columns
            .iter()
            .find(|column| column.column == Column::Title)
            .unwrap();
        assert!(!notes.visible);
        assert!(title.visible, "the title cannot be hidden");

        c.toggle_column(Column::Notes);
        assert!(
            c.view()
                .columns
                .iter()
                .find(|column| column.column == Column::Notes)
                .unwrap()
                .visible
        );
    }

    #[test]
    fn edit_into_another_category_adds_there_then_removes_the_old_copy() {
        let fake = Arc::default();
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");
        let draft = c.open_edit("cat-a", "item-1").expect("Dune is in the list");
        c.set_form_category("cat-b");
        assert!(c.submit(&draft));
        let calls = fake.calls();
        assert!(
            calls[calls.len() - 2].starts_with("add:cat-b:"),
            "adds to the picked category"
        );
        assert_eq!(
            calls.last().unwrap(),
            "delete-item:item-1",
            "then removes the old copy"
        );
        assert_eq!(c.view().form, None);
    }

    #[test]
    fn picking_then_the_bulk_actions_act_on_the_selected_rows() {
        let fake: Arc<Fake> = Arc::default();
        *fake.both_open.lock().unwrap() = true;
        let (c, _seen) = controller(&fake);
        c.activate("/watchlist");

        // Unsorted, Dune (not completed) is the first row shown and Heat (completed) the second.
        c.pick(0, false, false);
        let view = c.view();
        assert_eq!(view.selected_count, 1);
        assert!(view.rows[0].selected && !view.rows[1].selected);
        assert_eq!(
            c.selected_item(),
            Some(("cat-a".to_owned(), "item-1".to_owned()))
        );

        c.complete_selected();
        assert!(
            fake.calls().iter().any(|call| call == "toggle:item-1"),
            "the selected row is marked completed"
        );
        assert_eq!(
            c.view().selected_count,
            0,
            "the bulk action clears the selection"
        );

        c.pick(1, false, false);
        c.delete_selected();
        assert!(fake.calls().iter().any(|call| call == "delete-item:item-2"));
    }

    #[test]
    fn csv_file_is_named_after_the_section() {
        assert_eq!(csv_file_name(Section::Books), "watchlist-books.csv");
        assert_eq!(csv_file_name(Section::Movies), "watchlist-movies.csv");
    }
}
