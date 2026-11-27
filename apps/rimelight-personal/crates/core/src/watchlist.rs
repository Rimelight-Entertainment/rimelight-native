//! Watchlist API client and pure logic. Mirrors `src/components/watchlist/WatchlistManager.tsx`
//! and `src/api/routes/watchlist.ts` in the previous personal backend, against `/api/watchlist`.

use std::fmt::Display;

use ehttp::{Method, Request};
use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, json_request};

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// The three watchlist sections. The server keys them `movies`, `series` and `books`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Movies,
    Series,
    Books,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Movies, Section::Series, Section::Books];

    /// The server key, also used in Solid's headings (`Movies Library Matrix`).
    pub fn key(self) -> &'static str {
        match self {
            Section::Movies => "movies",
            Section::Series => "series",
            Section::Books => "books",
        }
    }

    /// The tab label in the Solid sidebar.
    pub fn label(self) -> &'static str {
        match self {
            Section::Movies => "Movies",
            Section::Series => "TV Series",
            Section::Books => "Books",
        }
    }

    /// The singular noun the Solid add dialog uses in its title.
    pub fn noun(self) -> &'static str {
        match self {
            Section::Movies => "Movie",
            Section::Series => "TV Series",
            Section::Books => "Book",
        }
    }

    /// The section a `/watchlist/...` route shows. `/watchlist` itself shows movies, as Solid does.
    pub fn from_route(route: &str) -> Section {
        match route.trim_end_matches('/') {
            "/watchlist/series" => Section::Series,
            "/watchlist/books" => Section::Books,
            _ => Section::Movies,
        }
    }
}

/// Watch or read status. The server keys them `planned`, `in-progress`, `completed`, `dropped`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    Planned,
    InProgress,
    Completed,
    Dropped,
}

impl Status {
    pub const ALL: [Status; 4] = [
        Status::Planned,
        Status::InProgress,
        Status::Completed,
        Status::Dropped,
    ];

    /// The label Solid's status select shows in the table.
    pub fn label(self) -> &'static str {
        match self {
            Status::Planned => "Planned",
            Status::InProgress => "In Progress",
            Status::Completed => "Completed",
            Status::Dropped => "Dropped",
        }
    }
}

/// One media item. Field names follow the stored JSON (camelCase).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub director: Option<String>,
    pub author: Option<String>,
    pub genre: Option<String>,
    pub duration: Option<String>,
    pub year: Option<i64>,
    pub rating: Option<f64>,
    pub status: Status,
    pub notes: Option<String>,
    pub recommended_by: Option<String>,
    pub completed: bool,
    pub seasons: Option<u32>,
    pub episodes: Option<u32>,
    pub pages: Option<u32>,
}

impl Default for Item {
    fn default() -> Self {
        Item {
            id: String::new(),
            title: String::new(),
            description: None,
            director: None,
            author: None,
            genre: None,
            duration: None,
            year: None,
            rating: None,
            status: Status::Planned,
            notes: None,
            recommended_by: None,
            completed: false,
            seasons: None,
            episodes: None,
            pages: None,
        }
    }
}

impl Item {
    /// Solid's `handleToggleCompleted`: flips `completed` and sets the matching status.
    pub fn toggle_completed(&mut self) {
        self.completed = !self.completed;
        self.status = status_for_completed(self.completed);
    }
}

/// `completed` true means `Status::Completed`; false means `Status::Planned`, as the server does.
fn status_for_completed(completed: bool) -> Status {
    if completed {
        Status::Completed
    } else {
        Status::Planned
    }
}

/// A named group of items inside one section.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub items: Vec<Item>,
}

/// The full watchlist, as `GET /api/watchlist` returns it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WatchlistData {
    pub movies: Vec<Category>,
    pub series: Vec<Category>,
    pub books: Vec<Category>,
}

impl WatchlistData {
    pub fn section(&self, section: Section) -> &[Category] {
        match section {
            Section::Movies => &self.movies,
            Section::Series => &self.series,
            Section::Books => &self.books,
        }
    }

    pub fn category(&self, section: Section, cat_id: &str) -> Option<&Category> {
        self.section(section).iter().find(|c| c.id == cat_id)
    }
}

/// One item with its category, as the table shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row<'a> {
    pub cat_id: &'a str,
    pub cat_name: &'a str,
    pub item: &'a Item,
}

/// Solid's flattened table for `section`. `archived` selects the Archive Bin, which holds the
/// completed items. The live list holds the rest.
pub fn rows(data: &WatchlistData, section: Section, archived: bool) -> Vec<Row<'_>> {
    data.section(section)
        .iter()
        .flat_map(|cat| {
            cat.items.iter().map(move |item| Row {
                cat_id: &cat.id,
                cat_name: &cat.name,
                item,
            })
        })
        .filter(|row| row.item.completed == archived)
        .collect()
}

/// Solid's table search. Matches title, creator, genre, category, notes, recommender and status.
/// `query` is trimmed and compared case-insensitively. An empty query matches everything.
pub fn matches(row: &Row<'_>, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let item = row.item;
    [
        Some(item.title.as_str()),
        item.director.as_deref(),
        item.author.as_deref(),
        item.genre.as_deref(),
        item.notes.as_deref(),
        item.recommended_by.as_deref(),
        Some(row.cat_name),
        Some(item.status.label()),
    ]
    .into_iter()
    .flatten()
    .any(|field| field.to_lowercase().contains(&query))
}

/// Solid's `Details` column: duration for movies, `NS • NE` for series, `N pp` for books,
/// and an em dash when nothing applies.
pub fn details(section: Section, item: &Item) -> String {
    let parts: Vec<String> = match section {
        Section::Movies => item
            .duration
            .iter()
            .filter(|d| !d.is_empty())
            .cloned()
            .collect(),
        Section::Series => {
            let seasons = item.seasons.map(|n| format!("{n}S"));
            let episodes = item.episodes.map(|n| format!("• {n}E"));
            seasons.into_iter().chain(episodes).collect()
        }
        Section::Books => item.pages.map(|n| format!("{n} pp")).into_iter().collect(),
    };
    if parts.is_empty() {
        "—".to_owned()
    } else {
        parts.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------

/// Partial item body. A `None` field is omitted from the JSON, so the server keeps the stored
/// value. This matches Solid, where `undefined` fields are dropped by `JSON.stringify`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub director: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasons: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episodes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_by: Option<String>,
}

impl ItemPatch {
    /// Solid's quick status select: `{ status, completed }` only.
    pub fn status_only(status: Status) -> ItemPatch {
        ItemPatch {
            status: Some(status),
            completed: Some(status == Status::Completed),
            ..ItemPatch::default()
        }
    }
}

/// The add and edit form, as text. The app binds each field to a text buffer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub title: String,
    /// Director for movies and series, author for books.
    pub creator: String,
    pub genre: String,
    pub duration: String,
    pub year: String,
    pub rating: String,
    pub status: Status,
    pub seasons: String,
    pub episodes: String,
    pub pages: String,
    pub notes: String,
    pub recommended_by: String,
}

impl Draft {
    /// The quick-add defaults from Solid's `QuickWatchlistModal`: this year, rating 8.5, one season.
    pub fn new_entry(year: i64) -> Draft {
        Draft {
            year: year.to_string(),
            rating: "8.5".to_owned(),
            seasons: "1".to_owned(),
            ..Draft::default()
        }
    }

    /// Solid's edit prefill. Zero numbers show as empty, as `item.year ? String(...) : ""` does.
    pub fn from_item(item: &Item) -> Draft {
        Draft {
            title: item.title.clone(),
            creator: first_text(&[item.director.as_deref(), item.author.as_deref()]),
            genre: text_of(&item.genre),
            duration: text_of(&item.duration),
            year: num_text(item.year),
            rating: num_text(item.rating),
            status: item.status,
            seasons: num_text(item.seasons),
            episodes: num_text(item.episodes),
            pages: num_text(item.pages),
            notes: text_of(&item.notes),
            recommended_by: text_of(&item.recommended_by),
        }
    }

    /// Solid's `handleSaveModal` payload. Returns `None` when the title is blank.
    /// Fields that do not apply to `section` are left out, as Solid leaves them `undefined`.
    pub fn to_patch(&self, section: Section) -> Option<ItemPatch> {
        let title = self.title.trim();
        if title.is_empty() {
            return None;
        }
        let creator = Some(self.creator.trim().to_owned());
        let (director, author) = match section {
            Section::Books => (None, creator),
            _ => (creator, None),
        };
        Some(ItemPatch {
            title: Some(title.to_owned()),
            director,
            author,
            genre: Some(self.genre.trim().to_owned()),
            duration: (section == Section::Movies).then(|| self.duration.trim().to_owned()),
            year: parse_num(&self.year),
            rating: parse_num(&self.rating),
            status: Some(self.status),
            completed: Some(self.status == Status::Completed),
            seasons: (section == Section::Series)
                .then(|| parse_num(&self.seasons))
                .flatten(),
            episodes: (section == Section::Series)
                .then(|| parse_num(&self.episodes))
                .flatten(),
            pages: (section == Section::Books)
                .then(|| parse_num(&self.pages))
                .flatten(),
            notes: Some(self.notes.trim().to_owned()),
            recommended_by: Some(self.recommended_by.trim().to_owned()),
        })
    }
}

fn text_of(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

fn first_text(values: &[Option<&str>]) -> String {
    values
        .iter()
        .flatten()
        .find(|v| !v.is_empty())
        .map(|v| (*v).to_owned())
        .unwrap_or_default()
}

/// Zero shows as empty, matching Solid's truthiness check.
fn num_text<T: Display + Default + PartialEq>(value: Option<T>) -> String {
    match value {
        Some(n) if n != T::default() => n.to_string(),
        _ => String::new(),
    }
}

/// Blank input is `None`. Anything else that fails to parse is also `None` here, where Solid
/// would send `NaN` as `null`.
fn parse_num<T: std::str::FromStr>(value: &str) -> Option<T> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

// ---------------------------------------------------------------------------
// CSV export (`exportable` on Solid's RLTable)
// ---------------------------------------------------------------------------

/// Column headers for the CSV export, in the table's order.
pub const CSV_HEADER: [&str; 11] = [
    "Done",
    "Title",
    "Creator",
    "Category",
    "Genre",
    "Year",
    "Rating",
    "Status",
    "Details",
    "Recommended By",
    "Notes",
];

/// The export: a header line, then one line per row. Lines end with `\n`.
pub fn to_csv(section: Section, rows: &[Row<'_>]) -> String {
    let mut out = String::new();
    out.push_str(&CSV_HEADER.join(","));
    out.push('\n');
    for row in rows {
        let item = row.item;
        let fields = [
            if item.completed { "Yes" } else { "No" }.to_owned(),
            item.title.clone(),
            first_text(&[item.director.as_deref(), item.author.as_deref()]),
            row.cat_name.to_owned(),
            text_of(&item.genre),
            num_text(item.year),
            num_text(item.rating),
            item.status.label().to_owned(),
            details(section, item),
            text_of(&item.recommended_by),
            text_of(&item.notes),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// RFC 4180 quoting: wrap when the field has a comma, quote or line break; double inner quotes.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

// ---------------------------------------------------------------------------
// Decoding and requests
// ---------------------------------------------------------------------------

fn decode_error(error: serde_json::Error) -> ApiError {
    ApiError::Decode(error.to_string())
}

/// Decodes the watchlist from `GET` and from every `POST` answer. The body is either the raw
/// object or `{ data: {...} }`. Missing sections become empty lists.
pub fn decode_watchlist(bytes: &[u8]) -> Result<WatchlistData, ApiError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(decode_error)?;
    let payload = value
        .get("data")
        .filter(|data| data.is_object())
        .cloned()
        .unwrap_or(value);
    serde_json::from_value(payload).map_err(decode_error)
}

/// One request body for `POST /api/watchlist`. Every action carries `action` and `section`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionBody<'a> {
    action: &'static str,
    section: Section,
    #[serde(skip_serializing_if = "Option::is_none")]
    cat_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<&'a ItemPatch>,
}

fn action(name: &'static str, section: Section) -> ActionBody<'static> {
    ActionBody {
        action: name,
        section,
        cat_id: None,
        item_id: None,
        name: None,
        item: None,
    }
}

impl ApiClient {
    /// `GET /api/watchlist`.
    pub fn watchlist(
        &self,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/watchlist")),
            decode_watchlist,
            on_done,
        );
    }

    /// `POST /api/watchlist` with `addCategory`.
    pub fn watchlist_add_category(
        &self,
        section: Section,
        name: &str,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            name: Some(name),
            ..action("addCategory", section)
        };
        self.watchlist_action(&body, on_done);
    }

    /// `POST /api/watchlist` with `deleteCategory`. The caller confirms first.
    pub fn watchlist_delete_category(
        &self,
        section: Section,
        cat_id: &str,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            cat_id: Some(cat_id),
            ..action("deleteCategory", section)
        };
        self.watchlist_action(&body, on_done);
    }

    /// `POST /api/watchlist` with `addItem`.
    pub fn watchlist_add_item(
        &self,
        section: Section,
        cat_id: &str,
        item: &ItemPatch,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            cat_id: Some(cat_id),
            item: Some(item),
            ..action("addItem", section)
        };
        self.watchlist_action(&body, on_done);
    }

    /// `POST /api/watchlist` with `updateItem`. Only the fields set in `item` change.
    pub fn watchlist_update_item(
        &self,
        section: Section,
        cat_id: &str,
        item_id: &str,
        item: &ItemPatch,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            cat_id: Some(cat_id),
            item_id: Some(item_id),
            item: Some(item),
            ..action("updateItem", section)
        };
        self.watchlist_action(&body, on_done);
    }

    /// `POST /api/watchlist` with `deleteItem`. The caller confirms first.
    pub fn watchlist_delete_item(
        &self,
        section: Section,
        cat_id: &str,
        item_id: &str,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            cat_id: Some(cat_id),
            item_id: Some(item_id),
            ..action("deleteItem", section)
        };
        self.watchlist_action(&body, on_done);
    }

    /// `POST /api/watchlist` with `toggleCompleted`. The server flips `completed` and sets the status.
    pub fn watchlist_toggle_completed(
        &self,
        section: Section,
        cat_id: &str,
        item_id: &str,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let body = ActionBody {
            cat_id: Some(cat_id),
            item_id: Some(item_id),
            ..action("toggleCompleted", section)
        };
        self.watchlist_action(&body, on_done);
    }

    fn watchlist_action(
        &self,
        body: &ActionBody<'_>,
        on_done: impl FnOnce(Result<WatchlistData, ApiError>) + Send + 'static,
    ) {
        let bytes = serde_json::to_vec(body).expect("plain struct serializes");
        self.send(
            json_request(Method::POST, self.url("/watchlist"), bytes),
            decode_watchlist,
            on_done,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str, completed: bool) -> Item {
        Item {
            id: id.to_owned(),
            title: title.to_owned(),
            completed,
            status: status_for_completed(completed),
            ..Item::default()
        }
    }

    fn sample() -> WatchlistData {
        WatchlistData {
            movies: vec![
                Category {
                    id: "c1".into(),
                    name: "Favorites".into(),
                    items: vec![item("a", "Alien", false), item("b", "Heat", true)],
                },
                Category {
                    id: "c2".into(),
                    name: "Re-watch".into(),
                    items: vec![item("c", "Brazil", true)],
                },
            ],
            series: vec![Category {
                id: "s1".into(),
                name: "Sci-fi".into(),
                items: vec![item("d", "Severance", false)],
            }],
            books: Vec::new(),
        }
    }

    #[test]
    fn rows_split_by_section_and_archive_flag() {
        let data = sample();
        let live: Vec<&str> = rows(&data, Section::Movies, false)
            .iter()
            .map(|r| r.item.id.as_str())
            .collect();
        assert_eq!(live, ["a"]);
        let archived: Vec<&str> = rows(&data, Section::Movies, true)
            .iter()
            .map(|r| r.item.id.as_str())
            .collect();
        assert_eq!(archived, ["b", "c"]);
        assert_eq!(rows(&data, Section::Series, false).len(), 1);
        assert!(rows(&data, Section::Books, false).is_empty());
    }

    #[test]
    fn rows_carry_their_category() {
        let data = sample();
        let archived = rows(&data, Section::Movies, true);
        assert_eq!(archived[1].cat_id, "c2");
        assert_eq!(archived[1].cat_name, "Re-watch");
    }

    #[test]
    fn search_matches_title_category_and_status() {
        let data = sample();
        let row = rows(&data, Section::Movies, true)[1];
        assert!(matches(&row, "  BRAZIL "));
        assert!(matches(&row, "re-watch"));
        assert!(matches(&row, "completed"));
        assert!(!matches(&row, "alien"));
        assert!(matches(&row, ""));
    }

    #[test]
    fn toggle_sets_status_both_ways() {
        let mut it = item("x", "Thing", false);
        it.toggle_completed();
        assert!(it.completed);
        assert_eq!(it.status, Status::Completed);
        it.toggle_completed();
        assert!(!it.completed);
        assert_eq!(it.status, Status::Planned);
    }

    #[test]
    fn status_only_patch_sets_completed_from_status() {
        let patch = ItemPatch::status_only(Status::Dropped);
        assert_eq!(patch.status, Some(Status::Dropped));
        assert_eq!(patch.completed, Some(false));
        let json = serde_json::to_value(&patch).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({"status": "dropped", "completed": false})
        );
    }

    #[test]
    fn draft_book_sends_author_and_pages_only() {
        let draft = Draft {
            title: "  Dune ".into(),
            creator: "Herbert".into(),
            duration: "999m".into(),
            seasons: "3".into(),
            pages: "412".into(),
            year: "1965".into(),
            rating: "9.5".into(),
            ..Draft::default()
        };
        let patch = draft.to_patch(Section::Books).expect("title present");
        assert_eq!(patch.title.as_deref(), Some("Dune"));
        assert_eq!(patch.author.as_deref(), Some("Herbert"));
        assert_eq!(patch.director, None);
        assert_eq!(patch.duration, None);
        assert_eq!(patch.seasons, None);
        assert_eq!(patch.pages, Some(412));
        assert_eq!(patch.year, Some(1965));
        assert_eq!(patch.rating, Some(9.5));
    }

    #[test]
    fn draft_series_sends_seasons_and_episodes() {
        let draft = Draft {
            title: "Severance".into(),
            creator: "Dan".into(),
            duration: "60m".into(),
            seasons: "2".into(),
            episodes: "19".into(),
            pages: "10".into(),
            status: Status::Completed,
            ..Draft::default()
        };
        let patch = draft.to_patch(Section::Series).expect("title present");
        assert_eq!(patch.director.as_deref(), Some("Dan"));
        assert_eq!(patch.author, None);
        assert_eq!(patch.duration, None);
        assert_eq!(patch.seasons, Some(2));
        assert_eq!(patch.episodes, Some(19));
        assert_eq!(patch.pages, None);
        assert_eq!(patch.completed, Some(true));
    }

    #[test]
    fn draft_requires_a_title() {
        let draft = Draft {
            title: "   ".into(),
            ..Draft::default()
        };
        assert!(draft.to_patch(Section::Movies).is_none());
    }

    #[test]
    fn draft_round_trips_an_item_prefill() {
        let mut source = item("m", "Alien", false);
        source.author = Some("Scott".into());
        source.year = Some(1979);
        source.rating = Some(0.0);
        let draft = Draft::from_item(&source);
        assert_eq!(draft.creator, "Scott");
        assert_eq!(draft.year, "1979");
        assert_eq!(draft.rating, "", "zero rating prefills as empty");
        assert_eq!(draft.seasons, "");
    }

    #[test]
    fn details_text_per_section() {
        let mut movie = item("m", "Alien", false);
        movie.duration = Some("117m".into());
        assert_eq!(details(Section::Movies, &movie), "117m");

        let mut show = item("s", "Severance", false);
        show.seasons = Some(2);
        show.episodes = Some(19);
        assert_eq!(details(Section::Series, &show), "2S • 19E");

        let mut book = item("b", "Dune", false);
        book.pages = Some(412);
        assert_eq!(details(Section::Books, &book), "412 pp");
        assert_eq!(details(Section::Books, &item("e", "Empty", false)), "—");
    }

    #[test]
    fn csv_quotes_fields_with_separators() {
        let mut quoted = item("q", "Say \"hi\", friend", true);
        quoted.notes = Some("line\nbreak".into());
        let data = WatchlistData {
            movies: vec![Category {
                id: "c".into(),
                name: "Favorites".into(),
                items: vec![quoted],
            }],
            ..WatchlistData::default()
        };
        let csv = to_csv(Section::Movies, &rows(&data, Section::Movies, true));
        let mut lines = csv.split('\n');
        assert_eq!(lines.next(), Some(CSV_HEADER.join(",").as_str()));
        assert!(csv.contains("Yes,\"Say \"\"hi\"\", friend\",,Favorites,"));
        assert!(csv.contains("\"line\nbreak\""));
    }

    #[test]
    fn decode_accepts_raw_and_wrapped_bodies() {
        let raw = br#"{"movies":[{"id":"c","name":"F","items":[{"id":"i","title":"T","status":"in-progress","completed":false,"recommendedBy":"Ana"}]}],"series":[],"books":[]}"#;
        let wrapped = br#"{"data":{"movies":[{"id":"c","name":"F","items":[{"id":"i","title":"T","status":"in-progress","completed":false,"recommendedBy":"Ana"}]}]}}"#;
        let a = decode_watchlist(raw).expect("raw decodes");
        let b = decode_watchlist(wrapped).expect("wrapped decodes");
        assert_eq!(a, b);
        assert_eq!(a.movies[0].items[0].status, Status::InProgress);
        assert_eq!(a.movies[0].items[0].recommended_by.as_deref(), Some("Ana"));
        assert!(a.books.is_empty());
    }

    #[test]
    fn decode_rejects_bad_json() {
        assert!(matches!(
            decode_watchlist(b"nope"),
            Err(ApiError::Decode(_))
        ));
    }
}
