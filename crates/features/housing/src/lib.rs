//! The Housing page controller (ported from `crates/app/src/housing.rs`; behaviour from the web app's
//! `src/components/housing/PetsManager.tsx`, `WishlistManager.tsx` and `src/components/groceries/GroceriesManager.tsx`).
//!
//! Plain Rust with no Slint. [`HousingController`] holds the state of the three tabs (pets, wishlist, groceries), calls
//! the endpoints through [`HousingApi`], and reports each change as a [`HousingView`]. The Slint glue in
//! `apps/desktop/src/pages/housing.rs` turns views into the models of `ui/housing.slint`.
//!
//! Threading follows the Todos page: calls return at once and answer on a worker thread; the lock is never held across
//! a call; `notify` runs after the lock is released.
//!
//! Row ids: `pet:<id>`, `care:<pet>/<entry>`, `weight:<pet>/<entry>`, `wish:<id>`, `cat:<id>`, `item:<cat>/<item>`,
//! `store:<id>`. Sections with a non-empty id can be deleted, and the delete is confirmed first.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use idantity_core::housing::{
    GroceriesData, GroceryAction, GroceryItem, GroceryItemDraft, Pet, PetAction, PetDraft,
    PetHistoryDraft, PetWeightDraft, StockStatus, WishlistDraft, WishlistItem,
};
use idantity_core::{ApiClient, ApiError};

/// The page's Slint markup, for the shell's build script (added as `@idantity_housing`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Care-log types, as the web `PetsManager` offers them.
pub const CARE_TYPES: &[&str] = &["vet", "vaccine", "medication", "other", "grooming"];

type ListDone<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the calls the page makes. [`ApiClient`] implements it over HTTP; tests inject a fake.
pub trait HousingApi: Send + Sync {
    fn pets(&self, on_done: ListDone<Vec<Pet>>);
    fn pet_action(&self, action: &PetAction<'_>, on_done: ListDone<Vec<Pet>>);
    fn wishlist(&self, on_done: ListDone<Vec<WishlistItem>>);
    fn create_wishlist_item(&self, item: &WishlistDraft<'_>, on_done: ListDone<WishlistItem>);
    fn set_wishlist_purchased(&self, id: &str, is_purchased: bool, on_done: ListDone<()>);
    fn delete_wishlist_item(&self, id: &str, on_done: ListDone<()>);
    fn groceries(&self, on_done: ListDone<GroceriesData>);
    fn grocery_action(&self, action: &GroceryAction<'_>, on_done: ListDone<GroceriesData>);
}

impl HousingApi for ApiClient {
    fn pets(&self, on_done: ListDone<Vec<Pet>>) {
        ApiClient::pets(self, on_done);
    }

    fn pet_action(&self, action: &PetAction<'_>, on_done: ListDone<Vec<Pet>>) {
        ApiClient::pet_action(self, action, on_done);
    }

    fn wishlist(&self, on_done: ListDone<Vec<WishlistItem>>) {
        ApiClient::wishlist(self, on_done);
    }

    fn create_wishlist_item(&self, item: &WishlistDraft<'_>, on_done: ListDone<WishlistItem>) {
        ApiClient::create_wishlist_item(self, item, on_done);
    }

    fn set_wishlist_purchased(&self, id: &str, is_purchased: bool, on_done: ListDone<()>) {
        ApiClient::set_wishlist_purchased(self, id, is_purchased, on_done);
    }

    fn delete_wishlist_item(&self, id: &str, on_done: ListDone<()>) {
        ApiClient::delete_wishlist_item(self, id, on_done);
    }

    fn groceries(&self, on_done: ListDone<GroceriesData>) {
        ApiClient::groceries(self, on_done);
    }

    fn grocery_action(&self, action: &GroceryAction<'_>, on_done: ListDone<GroceriesData>) {
        ApiClient::grocery_action(self, action, on_done);
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn HousingApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The page's tabs. Each is a route of `/housing` on the web: `/housing` and `/housing/pets`, `/housing/wishlist`,
/// `/housing/groceries`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HousingTab {
    #[default]
    Pets,
    Wishlist,
    Groceries,
}

impl HousingTab {
    pub const ALL: [HousingTab; 3] = [Self::Pets, Self::Wishlist, Self::Groceries];

    pub fn key(self) -> &'static str {
        match self {
            Self::Pets => "pets",
            Self::Wishlist => "wishlist",
            Self::Groceries => "groceries",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pets => "Pets",
            Self::Wishlist => "Wishlist",
            Self::Groceries => "Groceries",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.key() == key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormKind {
    Pet,
    Care,
    Weight,
    Wish,
    Category,
    Store,
    Item,
}

/// One form field: key, label, placeholder, required.
type Spec = (&'static str, &'static str, &'static str, bool);

const PET_FIELDS: &[Spec] = &[
    ("name", "Name", "", true),
    ("species", "Species", "", false),
    ("breed", "Breed", "", false),
    ("dateOfBirth", "Date of birth", "YYYY-MM-DD", false),
    ("adoptionDate", "Adoption date", "YYYY-MM-DD", false),
    ("weight", "Weight", "", false),
    ("weightUnit", "Weight unit", "", false),
    ("microchipId", "Microchip ID", "", false),
];

const CARE_FIELDS: &[Spec] = &[
    ("date", "Date", "YYYY-MM-DD", true),
    ("type", "Type", "", false),
    ("title", "Title", "", true),
    ("notes", "Details", "", false),
];

const WEIGHT_FIELDS: &[Spec] = &[
    ("date", "Date", "YYYY-MM-DD", true),
    ("weight", "Weight", "", true),
    ("unit", "Unit", "", false),
    ("notes", "Notes", "", false),
];

const WISH_FIELDS: &[Spec] = &[
    ("name", "Name", "", true),
    ("price", "Estimated price", "", false),
    ("category", "Category", "", false),
    ("url", "Store link", "https://", false),
    ("priority", "Priority", "", false),
];

const CATEGORY_FIELDS: &[Spec] = &[(
    "name",
    "Category name",
    "New category, e.g. Bakery, freezer",
    true,
)];

const STORE_FIELDS: &[Spec] = &[
    ("name", "Store name", "", true),
    ("color", "Colour (name or hex)", "", false),
];

const ITEM_FIELDS: &[Spec] = &[
    ("categoryId", "Category", "", true),
    ("name", "Name", "", true),
    ("amount", "Amount", "1", false),
    ("unit", "Unit", "", false),
    ("brand", "Brand", "", false),
    ("price", "Price", "", false),
    ("storeId", "Store", "", false),
    ("stockStatus", "Stock status", "", false),
];

impl FormKind {
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "pet" => Self::Pet,
            "care" => Self::Care,
            "weight" => Self::Weight,
            "wish" => Self::Wish,
            "category" => Self::Category,
            "store" => Self::Store,
            "item" => Self::Item,
            _ => return None,
        })
    }

    fn spec(self) -> &'static [Spec] {
        match self {
            Self::Pet => PET_FIELDS,
            Self::Care => CARE_FIELDS,
            Self::Weight => WEIGHT_FIELDS,
            Self::Wish => WISH_FIELDS,
            Self::Category => CATEGORY_FIELDS,
            Self::Store => STORE_FIELDS,
            Self::Item => ITEM_FIELDS,
        }
    }

    fn title(self, editing: bool) -> &'static str {
        match (self, editing) {
            (Self::Pet, false) => "Add Pet",
            (Self::Pet, true) => "Edit Pet",
            (Self::Care, _) => "Care Log",
            (Self::Weight, _) => "Log Weight",
            (Self::Wish, _) => "Add Wishlist Item",
            (Self::Category, _) => "Add Category",
            (Self::Store, _) => "Manage Stores",
            (Self::Item, false) => "Add Detailed Item",
            (Self::Item, true) => "Edit Grocery Item",
        }
    }

    fn submit_label(self) -> &'static str {
        match self {
            Self::Pet => "Save Pet",
            Self::Wish => "Add Item",
            _ => "Save",
        }
    }
}

fn default_value(kind: FormKind, key: &str, today: &str) -> String {
    match (kind, key) {
        (FormKind::Pet, "species") => "Dog",
        (FormKind::Pet, "weightUnit") | (FormKind::Weight, "unit") => "kg",
        (FormKind::Care, "type") => "vet",
        (FormKind::Care, "date") | (FormKind::Weight, "date") => return today.to_owned(),
        (FormKind::Wish, "category") => "Home Gear",
        (FormKind::Wish, "priority") => "medium",
        (FormKind::Item, "amount") => "1",
        (FormKind::Item, "stockStatus") => "in-stock",
        _ => "",
    }
    .to_owned()
}

/// Options for the choice fields, as `(value, label)`. Empty for free-text fields.
fn choices(kind: FormKind, key: &str, data: &GroceriesData) -> Vec<(String, String)> {
    let plain = |items: &[&str]| -> Vec<(String, String)> {
        items
            .iter()
            .map(|i| ((*i).to_owned(), (*i).to_owned()))
            .collect()
    };
    match (kind, key) {
        (FormKind::Care, "type") => CARE_TYPES
            .iter()
            .map(|t| ((*t).to_owned(), (*t).to_owned()))
            .collect(),
        (FormKind::Pet, "weightUnit") | (FormKind::Weight, "unit") => plain(&["kg", "lb"]),
        (FormKind::Wish, "priority") => plain(&["low", "medium", "high"]),
        (FormKind::Item, "categoryId") => data
            .categories
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect(),
        (FormKind::Item, "storeId") => std::iter::once((String::new(), "No store".to_owned()))
            .chain(data.stores.iter().map(|s| (s.id.clone(), s.name.clone())))
            .collect(),
        (FormKind::Item, "stockStatus") => vec![
            ("in-stock".to_owned(), "In stock".to_owned()),
            ("low".to_owned(), "Low stock".to_owned()),
            ("out-of-stock".to_owned(), "Out of stock".to_owned()),
        ],
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Form {
    kind: FormKind,
    /// The row being edited: a pet id, a grocery item `cat/item`, or empty.
    editing: Option<String>,
    /// The pet a care or weight entry belongs to.
    parent: Option<String>,
    fields: Vec<(String, String)>,
    error: Option<String>,
}

impl Form {
    fn new(kind: FormKind, editing: Option<String>, parent: Option<String>, today: &str) -> Self {
        Self {
            kind,
            editing,
            parent,
            fields: kind
                .spec()
                .iter()
                .map(|(key, _, _, _)| ((*key).to_owned(), default_value(kind, key, today)))
                .collect(),
            error: None,
        }
    }

    fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map_or("", |(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: impl Into<String>) {
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| k.as_str() == key) {
            slot.1 = value.into();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatView {
    pub label: String,
    pub value: String,
}

/// One row. An empty `primary_label` or `secondary_label` hides that button.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowView {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub value: String,
    pub note: String,
    pub primary_label: String,
    pub secondary_label: String,
    pub can_edit: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionView {
    /// Empty for sections that cannot be deleted. Otherwise the row id of the section (`cat:<id>` or `store:<id>`).
    pub id: String,
    pub title: String,
    pub empty: String,
    pub can_delete: bool,
    pub rows: Vec<RowView>,
}

/// A chip. `group` says what a click means: `new` opens the form named by `value`; `filter` sets the wishlist filter
/// (`active`, `purchased`, `all`); `mode` and `store` filter the groceries; `bulk` runs `reset-out-of-stock` or
/// `clear-checked`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChipView {
    pub group: String,
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChoiceView {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldView {
    pub key: String,
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub required: bool,
    /// `text`, `select` (the quick choices in `choices`; the shared Select shows them) or `date` (the shared DatePicker,
    /// which shows `calendar`).
    pub kind: String,
    pub choices: Vec<ChoiceView>,
    pub calendar: Vec<CalendarCellView>,
    pub calendar_title: String,
    pub weekday_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormView {
    pub title: String,
    pub submit_label: String,
    pub error: Option<String>,
    pub fields: Vec<FieldView>,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HousingView {
    pub tab: HousingTab,
    pub loading: bool,
    pub error: Option<String>,
    pub search: String,
    pub stats: Vec<StatView>,
    pub chips: Vec<ChipView>,
    pub sections: Vec<SectionView>,
    pub form: Option<FormView>,
    /// The question waiting for a yes or no, when a delete needs confirming.
    pub confirm: Option<String>,
}

/// Groceries filter modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    All,
    ToBuy,
    LowOrOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Pet(String),
    Category(String),
    Store(String),
}

#[derive(Default)]
struct State {
    tab: HousingTab,
    loading: u8,
    error: Option<String>,
    pets: Vec<Pet>,
    wishlist: Vec<WishlistItem>,
    groceries: GroceriesData,
    today: String,
    search: String,
    /// `active`, `purchased` or `all`.
    wish_filter: String,
    mode: Mode,
    /// Store id to filter by; empty for all.
    store: String,
    form: Option<Form>,
    /// The pet whose care log and weight timeline the page shows. None shows every pet.
    pet: Option<String>,
    date_nav: BTreeMap<String, (i64, i64)>,
    pending: Option<Pending>,
}

fn today_string() -> String {
    let day = idantity_core::local_date::today();
    format!("{:04}-{:02}-{:02}", day.year, day.month, day.day)
}

fn opt(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn text(form: &Form, key: &str) -> String {
    form.get(key).trim().to_owned()
}

fn parse_num(form: &Form, key: &str) -> Result<Option<f64>, String> {
    let raw = form.get(key).trim();
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<f64>()
        .map(Some)
        .map_err(|_| "This field must be a number.".to_owned())
}

fn stock_from(value: &str) -> StockStatus {
    match value {
        "low" => StockStatus::Low,
        "out-of-stock" => StockStatus::OutOfStock,
        _ => StockStatus::InStock,
    }
}

fn stock_wire(status: StockStatus) -> &'static str {
    match status {
        StockStatus::InStock => "in-stock",
        StockStatus::Low => "low",
        StockStatus::OutOfStock => "out-of-stock",
    }
}

fn stock_label(status: StockStatus) -> &'static str {
    match status {
        StockStatus::InStock => "In stock",
        StockStatus::Low => "Low stock",
        StockStatus::OutOfStock => "Out of stock",
    }
}

fn split2(id: &str) -> Option<(&str, &str)> {
    id.split_once('/')
}

fn pet_row(p: &Pet, selected: bool) -> RowView {
    let weight = p
        .weight
        .map(|w| {
            format!(
                "{w} {}",
                p.weight_unit.clone().unwrap_or_else(|| "kg".to_owned())
            )
        })
        .unwrap_or_default();
    let subtitle = [p.species.as_str(), p.breed.as_deref().unwrap_or("")]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let note = [
        p.date_of_birth
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("born {d}")),
        p.adoption_date
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("adopted {d}")),
        p.microchip_id
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("chip {d}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    RowView {
        id: format!("pet:{}", p.id),
        title: p.name.clone(),
        primary_label: if selected { "Open ✓" } else { "Open" }.to_owned(),
        subtitle,
        value: weight,
        note,
        can_edit: true,
        can_delete: true,
        ..Default::default()
    }
}

fn build_view(s: &State) -> HousingView {
    let mut stats = Vec::new();
    let mut chips = Vec::new();
    let mut sections = Vec::new();
    let needle = s.search.trim().to_lowercase();

    match s.tab {
        HousingTab::Pets => {
            chips.push(new_chip("pet", "Add Pet"));
            sections.push(SectionView {
                title: "Pets".to_owned(),
                empty: "No pets yet. Add a pet profile to start tracking.".to_owned(),
                rows: s
                    .pets
                    .iter()
                    .map(|p| pet_row(p, s.pet.as_deref() == Some(p.id.as_str())))
                    .collect(),
                ..Default::default()
            });
            // The care log and weight timeline of every pet, newest first, as the web shows them for the open profile.
            let care: Vec<RowView> = s
                .pets
                .iter()
                .filter(|p| s.pet.as_deref().is_none_or(|sel| sel == p.id))
                .flat_map(|p| {
                    p.history.iter().map(move |e| RowView {
                        id: format!("care:{}/{}", p.id, e.id),
                        title: format!("{} · {}", p.name, e.title),
                        subtitle: format!("{} · {}", e.kind, e.date),
                        value: String::new(),
                        note: e.notes.clone().unwrap_or_default(),
                        can_delete: true,
                        ..Default::default()
                    })
                })
                .collect();
            sections.push(SectionView {
                title: "Medical History".to_owned(),
                empty: "No care or medical history logged yet.".to_owned(),
                rows: care,
                ..Default::default()
            });
            let weights: Vec<RowView> = s
                .pets
                .iter()
                .filter(|p| s.pet.as_deref().is_none_or(|sel| sel == p.id))
                .flat_map(|p| {
                    p.weight_history.iter().enumerate().map(move |(i, w)| {
                        let change = p
                            .weight_history
                            .get(i + 1)
                            .map(|prev| w.weight - prev.weight);
                        RowView {
                            id: format!("weight:{}/{}", p.id, w.id),
                            title: format!("{} · {} {}", p.name, w.weight, w.unit),
                            subtitle: w.date.clone(),
                            value: change.map_or(String::new(), |d| format!("{d:+.1}")),
                            note: w.notes.clone().unwrap_or_default(),
                            can_delete: true,
                            ..Default::default()
                        }
                    })
                })
                .collect();
            sections.push(SectionView {
                title: "Weight Timeline".to_owned(),
                empty: "No weigh-ins logged yet.".to_owned(),
                rows: weights,
                ..Default::default()
            });
            chips.push(new_chip("care", "+ Care Log"));
            chips.push(new_chip("weight", "+ Log Weight"));
        }
        HousingTab::Wishlist => {
            stats.push(StatView {
                label: "Items".to_owned(),
                value: s.wishlist.len().to_string(),
            });
            for (value, label) in [
                ("active", "Active"),
                ("purchased", "Purchased"),
                ("all", "All"),
            ] {
                chips.push(ChipView {
                    group: "filter".to_owned(),
                    value: value.to_owned(),
                    label: label.to_owned(),
                    selected: s.wish_filter == value,
                });
            }
            chips.push(new_chip("wish", "+ Add Item"));
            let rows = s
                .wishlist
                .iter()
                .filter(|w| match s.wish_filter.as_str() {
                    "active" => !w.is_purchased,
                    "purchased" => w.is_purchased,
                    _ => true,
                })
                .map(|w| RowView {
                    id: format!("wish:{}", w.id),
                    title: w.name.clone(),
                    subtitle: format!("{} · {}", w.category, w.priority),
                    value: w.price.clone().unwrap_or_default(),
                    note: w.url.clone().unwrap_or_default(),
                    primary_label: if w.is_purchased {
                        "Purchased ✓"
                    } else {
                        "Mark Bought"
                    }
                    .to_owned(),
                    can_delete: true,
                    ..Default::default()
                })
                .collect();
            sections.push(SectionView {
                title: "Wishlist".to_owned(),
                empty: "Nothing on the wishlist.".to_owned(),
                rows,
                ..Default::default()
            });
        }
        HousingTab::Groceries => {
            let g = &s.groceries;
            let remaining: usize = g
                .categories
                .iter()
                .flat_map(|c| c.items.iter())
                .filter(|i| !i.is_bought)
                .count();
            let cart: f64 = g
                .categories
                .iter()
                .flat_map(|c| c.items.iter())
                .map(item_trip_cost)
                .sum();
            stats.push(StatView {
                label: "Items remaining".to_owned(),
                value: remaining.to_string(),
            });
            stats.push(StatView {
                label: "Est. Trip Cost".to_owned(),
                value: format!("${cart:.2}"),
            });
            for (value, label) in [
                ("all", "All Items"),
                ("to-buy", "To Buy"),
                ("low", "Low / Out of Stock"),
            ] {
                chips.push(ChipView {
                    group: "mode".to_owned(),
                    value: value.to_owned(),
                    label: label.to_owned(),
                    selected: s.mode == mode_of(value),
                });
            }
            chips.push(ChipView {
                group: "store".to_owned(),
                value: String::new(),
                label: "All Stores".to_owned(),
                selected: s.store.is_empty(),
            });
            for store in &g.stores {
                chips.push(ChipView {
                    group: "store".to_owned(),
                    value: store.id.clone(),
                    label: store.name.clone(),
                    selected: s.store == store.id,
                });
            }
            chips.push(new_chip("item", "Add Detailed Item"));
            chips.push(new_chip("category", "Add Category"));
            chips.push(new_chip("store", "Manage Stores"));
            chips.push(ChipView {
                group: "bulk".to_owned(),
                value: "reset-out-of-stock".to_owned(),
                label: "Reset Out-of-Stock".to_owned(),
                selected: false,
            });
            chips.push(ChipView {
                group: "bulk".to_owned(),
                value: "clear-checked".to_owned(),
                label: "Clear Checked".to_owned(),
                selected: false,
            });

            sections.push(SectionView {
                id: String::new(),
                title: "Stores".to_owned(),
                empty: "No stores yet.".to_owned(),
                can_delete: false,
                rows: g
                    .stores
                    .iter()
                    .map(|st| RowView {
                        id: format!("store:{}", st.id),
                        title: st.name.clone(),
                        subtitle: st.color.clone().unwrap_or_default(),
                        can_delete: true,
                        ..Default::default()
                    })
                    .collect(),
            });
            if g.categories.is_empty() {
                sections.push(SectionView {
                    title: "Groceries".to_owned(),
                    empty: "No categories yet. Create one with Add Category.".to_owned(),
                    ..Default::default()
                });
            }
            for cat in &g.categories {
                let rows: Vec<RowView> = cat
                    .items
                    .iter()
                    .filter(|i| match s.mode {
                        Mode::All => true,
                        Mode::ToBuy => !i.is_bought,
                        Mode::LowOrOut => i.stock_status != StockStatus::InStock,
                    })
                    .filter(|i| {
                        s.store.is_empty() || i.store_id.as_deref() == Some(s.store.as_str())
                    })
                    .filter(|i| {
                        needle.is_empty()
                            || i.name.to_lowercase().contains(&needle)
                            || i.brand
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&needle)
                    })
                    .map(|i| {
                        let store = i
                            .store_id
                            .as_deref()
                            .and_then(|sid| g.stores.iter().find(|st| st.id == sid))
                            .map(|st| st.name.clone())
                            .unwrap_or_default();
                        let amount = format!("{} {}", i.amount, i.unit.clone().unwrap_or_default());
                        RowView {
                            id: format!("item:{}/{}", cat.id, i.id),
                            title: i.name.clone(),
                            subtitle: [
                                i.brand.clone().unwrap_or_default(),
                                amount.trim().to_owned(),
                                store,
                            ]
                            .into_iter()
                            .filter(|p| !p.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                            value: i.last_price.map_or(String::new(), |p| format!("{p:.2}")),
                            note: stock_label(i.stock_status).to_owned(),
                            primary_label: if i.is_bought { "Bought ✓" } else { "Bought" }
                                .to_owned(),
                            secondary_label: format!(
                                "Set: {}",
                                stock_label(next_stock(i.stock_status))
                            ),
                            can_edit: true,
                            can_delete: true,
                        }
                    })
                    .collect();
                let cat_total: f64 = cat.items.iter().map(item_trip_cost).sum();
                sections.push(SectionView {
                    id: format!("cat:{}", cat.id),
                    title: format!("{} ({cat_total:.2})", cat.name),
                    empty: "No items in this category.".to_owned(),
                    can_delete: true,
                    rows,
                });
            }
        }
    }

    HousingView {
        tab: s.tab,
        loading: s.loading > 0,
        error: s.error.clone(),
        search: s.search.clone(),
        stats,
        chips,
        sections,
        form: form_view(s),
        confirm: s.pending.as_ref().map(|p| match p {
            Pending::Pet(_) => "Are you sure you want to delete this pet profile?".to_owned(),
            Pending::Category(_) => {
                "Are you sure you want to delete this category and all its items?".to_owned()
            }
            Pending::Store(_) => "Are you sure you want to delete this store?".to_owned(),
        }),
    }
}

fn mode_of(value: &str) -> Mode {
    match value {
        "to-buy" => Mode::ToBuy,
        "low" => Mode::LowOrOut,
        _ => Mode::All,
    }
}

fn next_stock(status: StockStatus) -> StockStatus {
    match status {
        StockStatus::InStock => StockStatus::Low,
        StockStatus::Low => StockStatus::OutOfStock,
        StockStatus::OutOfStock => StockStatus::InStock,
    }
}

fn new_chip(value: &str, label: &str) -> ChipView {
    ChipView {
        group: "new".to_owned(),
        value: value.to_owned(),
        label: label.to_owned(),
        selected: false,
    }
}

fn form_view(s: &State) -> Option<FormView> {
    let form = s.form.as_ref()?;
    let today = s.today.as_str();
    let fields = form
        .kind
        .spec()
        .iter()
        .map(|(key, label, placeholder, required)| {
            let value = form.get(key).to_owned();
            let options = choices(form.kind, key, &s.groceries);
            let is_date = DATE_KEYS.contains(key);
            let kind = if is_date {
                "date"
            } else if options.is_empty() {
                "text"
            } else {
                "select"
            };
            let (calendar, calendar_title) = if is_date {
                let cal = calendar_for(&value, s.date_nav.get(*key).copied(), today);
                (cal.cells, cal.title)
            } else {
                (Vec::new(), String::new())
            };
            let choices = options
                .into_iter()
                .map(|(v, l)| ChoiceView {
                    selected: v == value,
                    value: v,
                    label: l,
                })
                .collect();
            FieldView {
                key: (*key).to_owned(),
                label: (*label).to_owned(),
                placeholder: (*placeholder).to_owned(),
                required: *required,
                kind: kind.to_owned(),
                choices,
                calendar,
                calendar_title,
                weekday_labels: if is_date {
                    WEEKDAYS.iter().map(|d| (*d).to_owned()).collect()
                } else {
                    Vec::new()
                },
                value,
            }
        })
        .collect();
    Some(FormView {
        title: form.kind.title(form.editing.is_some()).to_owned(),
        submit_label: form.kind.submit_label().to_owned(),
        error: form.error.clone(),
        fields,
    })
}

struct Shared {
    api: Arc<dyn HousingApi>,
    state: Mutex<State>,
    notify: Arc<dyn Fn(HousingView) + Send + Sync>,
}

/// The Housing page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct HousingController {
    shared: Arc<Shared>,
}

impl HousingController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn HousingApi>,
        notify: impl Fn(HousingView) + Send + Sync + 'static,
    ) -> Self {
        let state = State {
            today: today_string(),
            wish_filter: "active".to_owned(),
            ..State::default()
        };
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::new(state),
                notify: Arc::new(notify),
            }),
        }
    }

    pub fn view(&self) -> HousingView {
        build_view(&self.lock())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn change(&self, f: impl FnOnce(&mut State)) {
        let view = {
            let mut s = self.lock();
            f(&mut s);
            build_view(&s)
        };
        (self.shared.notify)(view);
    }

    fn fail(&self, error: ApiError) {
        self.change(|s| {
            s.error = Some(error.to_string());
            s.loading = 0;
        });
    }

    /// Loads the current tab's data.
    pub fn refresh(&self) {
        let tab = self.lock().tab;
        self.load_tab(tab);
    }

    fn load_tab(&self, tab: HousingTab) {
        self.change(|s| {
            s.loading = 1;
            s.today = today_string();
        });
        let me = self.clone();
        match tab {
            HousingTab::Pets => self.shared.api.pets(Box::new(
                move |result: Result<Vec<Pet>, ApiError>| match result {
                    Ok(pets) => me.change(|s| {
                        s.pets = pets;
                        s.loading = 0;
                        s.error = None;
                    }),
                    Err(e) => me.fail(e),
                },
            )),
            HousingTab::Wishlist => self.shared.api.wishlist(Box::new(
                move |result: Result<Vec<WishlistItem>, ApiError>| match result {
                    Ok(items) => me.change(|s| {
                        s.wishlist = items;
                        s.loading = 0;
                        s.error = None;
                    }),
                    Err(e) => me.fail(e),
                },
            )),
            HousingTab::Groceries => self.shared.api.groceries(Box::new(
                move |result: Result<GroceriesData, ApiError>| match result {
                    Ok(data) => me.change(|s| {
                        s.groceries = data;
                        s.loading = 0;
                        s.error = None;
                    }),
                    Err(e) => me.fail(e),
                },
            )),
        }
    }

    /// Switches tab and loads its data.
    pub fn set_tab(&self, tab: HousingTab) {
        self.change(|s| {
            s.tab = tab;
            s.form = None;
            s.pending = None;
        });
        self.load_tab(tab);
    }

    pub fn set_search(&self, text: &str) {
        let text = text.to_owned();
        self.change(|s| s.search = text);
    }

    /// Chip clicks: `new` opens a form, `filter` sets the wishlist filter, `mode` and `store` filter the groceries,
    /// and `bulk` runs the two bulk actions.
    pub fn choose(&self, group: &str, value: &str) {
        let value = value.to_owned();
        match group {
            "new" => self.open_form(&value),
            "filter" => self.change(move |s| s.wish_filter = value),
            "mode" => self.change(move |s| s.mode = mode_of(&value)),
            "store" => self.change(move |s| s.store = value),
            "bulk" => {
                let action = if value == "reset-out-of-stock" {
                    GroceryAction::ResetAllToNeedBuy
                } else {
                    GroceryAction::ClearBought
                };
                self.grocery(&action);
            }
            _ => {}
        }
    }

    /// Opens a new form. `key`: `pet`, `care`, `weight`, `wish`, `category`, `store` or `item`. `care` and `weight`
    /// go to the open pet, or the first pet when none is open.
    pub fn open_form(&self, key: &str) {
        let Some(kind) = FormKind::from_key(key) else {
            return;
        };
        self.change(|s| {
            let parent = if matches!(kind, FormKind::Care | FormKind::Weight) {
                s.pet
                    .clone()
                    .or_else(|| s.pets.first().map(|p| p.id.clone()))
            } else {
                None
            };
            let form = Form::new(kind, None, parent, &s.today);
            s.date_nav.clear();
            s.form = Some(form);
        });
    }

    /// Opens the form for the row `row_id`, prefilled. Pets, grocery items.
    pub fn edit(&self, row_id: &str) {
        let Some((prefix, id)) = row_id.split_once(':') else {
            return;
        };
        let form = {
            let s = self.lock();
            match prefix {
                "pet" => s.pets.iter().find(|p| p.id == id).map(|p| {
                    let mut f = Form::new(FormKind::Pet, Some(id.to_owned()), None, &s.today);
                    f.set("name", p.name.as_str());
                    f.set("species", p.species.as_str());
                    f.set("breed", p.breed.clone().unwrap_or_default());
                    f.set("dateOfBirth", p.date_of_birth.clone().unwrap_or_default());
                    f.set("adoptionDate", p.adoption_date.clone().unwrap_or_default());
                    f.set("weight", p.weight.map_or(String::new(), |w| w.to_string()));
                    f.set(
                        "weightUnit",
                        p.weight_unit.clone().unwrap_or_else(|| "kg".to_owned()),
                    );
                    f.set("microchipId", p.microchip_id.clone().unwrap_or_default());
                    f
                }),
                "item" => split2(id).and_then(|(cat_id, item_id)| {
                    s.groceries
                        .categories
                        .iter()
                        .find(|c| c.id == cat_id)
                        .and_then(|c| {
                            c.items.iter().find(|i| i.id == item_id).map(|i| {
                                let mut f =
                                    Form::new(FormKind::Item, Some(id.to_owned()), None, &s.today);
                                f.set("categoryId", cat_id);
                                f.set("name", i.name.as_str());
                                f.set("amount", i.amount.to_string());
                                f.set("unit", i.unit.clone().unwrap_or_default());
                                f.set("brand", i.brand.clone().unwrap_or_default());
                                f.set(
                                    "price",
                                    i.last_price.map_or(String::new(), |p| p.to_string()),
                                );
                                f.set("storeId", i.store_id.clone().unwrap_or_default());
                                f.set("stockStatus", stock_wire(i.stock_status));
                                f
                            })
                        })
                }),
                _ => None,
            }
        };
        if let Some(form) = form {
            self.change(|s| s.form = Some(form));
        }
    }

    pub fn set_field(&self, key: &str, value: &str) {
        let (key, value) = (key.to_owned(), value.to_owned());
        self.change(|s| {
            if let Some(form) = s.form.as_mut() {
                form.set(&key, value);
            }
        });
    }

    /// Moves the date picker of the field `key` by `delta` months.
    pub fn date_step(&self, key: &str, delta: i64) {
        let key = key.to_owned();
        self.change(move |s| {
            let Some(form) = s.form.as_ref() else { return };
            let (year, month) =
                shown_month(form.get(&key), s.date_nav.get(&key).copied(), &s.today);
            let next = shift_month(year, month, delta);
            s.date_nav.insert(key, next);
        });
    }

    /// Sets the date field `key` to the day of the picker cell at `index`.
    pub fn pick_date(&self, key: &str, index: usize) {
        let key = key.to_owned();
        self.change(move |s| {
            let Some(form) = s.form.as_ref() else { return };
            let shown = shown_month(form.get(&key), s.date_nav.get(&key).copied(), &s.today);
            let date = cell_date(shown, index);
            if let Some(form) = s.form.as_mut() {
                form.set(&key, date);
            }
        });
    }

    pub fn cancel_form(&self) {
        self.change(|s| s.form = None);
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    /// Saves the open form. Returns `false`, and calls nothing, when the input is refused.
    pub fn submit(&self) -> bool {
        let form = {
            let s = self.lock();
            match s.form.clone() {
                Some(f) => f,
                None => return false,
            }
        };
        for (key, label, _, required) in form.kind.spec() {
            if *required && form.get(key).trim().is_empty() {
                self.refuse(format!("{label} is required."));
                return false;
            }
        }
        match form.kind {
            FormKind::Pet => self.save_pet(&form),
            FormKind::Care => self.add_care(&form),
            FormKind::Weight => self.add_weight(&form),
            FormKind::Wish => self.add_wish(&form),
            FormKind::Category => self.grocery(&GroceryAction::AddCategory {
                name: form.get("name").trim(),
            }),
            FormKind::Store => self.add_store(&form),
            FormKind::Item => self.save_item(&form),
        }
        true
    }

    fn refuse(&self, message: String) {
        self.change(|s| {
            if let Some(f) = s.form.as_mut() {
                f.error = Some(message);
            }
        });
    }

    /// Closes the form and takes the new pet list.
    fn pet_done(&self) -> ListDone<Vec<Pet>> {
        let me = self.clone();
        Box::new(move |result: Result<Vec<Pet>, ApiError>| match result {
            Ok(pets) => me.change(|s| {
                s.pets = pets;
                s.form = None;
                s.error = None;
            }),
            Err(e) => me.fail(e),
        })
    }

    fn save_pet(&self, form: &Form) {
        let weight = match parse_num(form, "weight") {
            Ok(w) => w,
            Err(message) => return self.refuse(message),
        };
        let draft = PetDraft {
            name: form.get("name").trim(),
            species: form.get("species").trim(),
            breed: form.get("breed").trim(),
            date_of_birth: form.get("dateOfBirth").trim(),
            adoption_date: form.get("adoptionDate").trim(),
            weight,
            weight_unit: form.get("weightUnit").trim(),
            microchip_id: form.get("microchipId").trim(),
        };
        let action = match &form.editing {
            Some(pet_id) => PetAction::UpdatePet {
                pet_id,
                pet: &draft,
            },
            None => PetAction::AddPet { pet: &draft },
        };
        self.shared.api.pet_action(&action, self.pet_done());
    }

    fn add_care(&self, form: &Form) {
        let Some(pet_id) = form.parent.clone() else {
            return self.refuse("No pet selected.".to_owned());
        };
        let entry = PetHistoryDraft {
            date: form.get("date").trim(),
            kind: form.get("type").trim(),
            title: form.get("title").trim(),
            notes: form.get("notes").trim(),
        };
        self.shared.api.pet_action(
            &PetAction::AddHistoryEntry {
                pet_id: &pet_id,
                entry: &entry,
            },
            self.pet_done(),
        );
    }

    fn add_weight(&self, form: &Form) {
        let Some(pet_id) = form.parent.clone() else {
            return self.refuse("No pet selected.".to_owned());
        };
        let weight = match parse_num(form, "weight") {
            Ok(Some(w)) => w,
            Ok(None) => return self.refuse("Weight is required.".to_owned()),
            Err(message) => return self.refuse(message),
        };
        let entry = PetWeightDraft {
            date: form.get("date").trim(),
            weight,
            unit: form.get("unit").trim(),
            notes: form.get("notes").trim(),
        };
        self.shared.api.pet_action(
            &PetAction::AddWeightEntry {
                pet_id: &pet_id,
                entry: &entry,
            },
            self.pet_done(),
        );
    }

    fn add_wish(&self, form: &Form) {
        let name = text(form, "name");
        let price = text(form, "price");
        let url = text(form, "url");
        let draft = WishlistDraft {
            name: &name,
            price: opt(&price),
            url: opt(&url),
            priority: form.get("priority").trim(),
            category: form.get("category").trim(),
        };
        let me = self.clone();
        self.shared.api.create_wishlist_item(
            &draft,
            Box::new(move |result: Result<WishlistItem, ApiError>| match result {
                Ok(item) => me.change(|s| {
                    s.wishlist.push(item);
                    s.form = None;
                    s.error = None;
                }),
                Err(e) => me.fail(e),
            }),
        );
    }

    fn add_store(&self, form: &Form) {
        let name = text(form, "name");
        let color = text(form, "color");
        self.grocery(&GroceryAction::AddStore {
            name: &name,
            color: &color,
        });
        self.change(|s| s.form = None);
    }

    fn save_item(&self, form: &Form) {
        let amount = match parse_num(form, "amount") {
            Ok(a) => a.unwrap_or(1.0),
            Err(message) => return self.refuse(message),
        };
        let last_price = match parse_num(form, "price") {
            Ok(p) => p,
            Err(message) => return self.refuse(message),
        };
        let name = text(form, "name");
        let unit = text(form, "unit");
        let brand = text(form, "brand");
        let store = text(form, "storeId");
        let draft = GroceryItemDraft {
            name: &name,
            amount,
            unit: &unit,
            brand: &brand,
            last_price,
            store_id: opt(&store),
            stock_status: stock_from(form.get("stockStatus")),
            is_bought: false,
        };
        match &form.editing {
            Some(id) => {
                let Some((cat_id, item_id)) = split2(id) else {
                    return;
                };
                let is_bought = self
                    .lock()
                    .groceries
                    .categories
                    .iter()
                    .flat_map(|c| c.items.iter())
                    .find(|i| i.id == item_id)
                    .is_some_and(|i| i.is_bought);
                let draft = GroceryItemDraft { is_bought, ..draft };
                self.grocery(&GroceryAction::UpdateItem {
                    cat_id,
                    item_id,
                    item: &draft,
                });
            }
            None => {
                let cat_id = text(form, "categoryId");
                self.grocery(&GroceryAction::AddItem {
                    cat_id: &cat_id,
                    item: &draft,
                });
            }
        }
        self.change(|s| {
            if s.error.is_none() {
                s.form = None;
            }
        });
    }

    /// Sends a grocery action and takes the list it returns.
    fn grocery(&self, action: &GroceryAction<'_>) {
        let me = self.clone();
        self.shared.api.grocery_action(
            action,
            Box::new(
                move |result: Result<GroceriesData, ApiError>| match result {
                    Ok(data) => me.change(|s| {
                        s.groceries = data;
                        s.error = None;
                        s.form = None;
                    }),
                    Err(e) => me.fail(e),
                },
            ),
        );
    }

    /// The row's primary action: open a pet, toggle a wishlist item, or mark a grocery item bought.
    pub fn primary(&self, row_id: &str) {
        let Some((prefix, id)) = row_id.split_once(':') else {
            return;
        };
        match prefix {
            "pet" => {
                let pet_id = id.to_owned();
                self.change(move |s| s.pet = Some(pet_id));
            }
            "wish" => {
                let purchased = self
                    .lock()
                    .wishlist
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| !w.is_purchased);
                if let Some(purchased) = purchased {
                    let me = self.clone();
                    let wish_id = id.to_owned();
                    self.shared.api.set_wishlist_purchased(
                        &wish_id.clone(),
                        purchased,
                        Box::new(move |result: Result<(), ApiError>| match result {
                            Ok(()) => me.change(move |s| {
                                if let Some(w) = s.wishlist.iter_mut().find(|w| w.id == wish_id) {
                                    w.is_purchased = purchased;
                                }
                            }),
                            Err(e) => me.fail(e),
                        }),
                    );
                }
            }
            "item" => {
                let Some((cat_id, item_id)) = split2(id) else {
                    return;
                };
                let action = {
                    let s = self.lock();
                    s.groceries
                        .categories
                        .iter()
                        .find(|c| c.id == cat_id)
                        .and_then(|c| c.items.iter().find(|i| i.id == item_id))
                        .map(|_| GroceryAction::ToggleBought { cat_id, item_id })
                };
                if let Some(action) = action {
                    let owned = OwnedAction::from(action);
                    self.grocery(&owned.borrow());
                }
            }
            _ => {}
        }
    }

    /// The row's secondary action: cycle a grocery item's stock status.
    pub fn secondary(&self, row_id: &str) {
        let Some(("item", id)) = row_id.split_once(':') else {
            return;
        };
        let Some((cat_id, item_id)) = split2(id) else {
            return;
        };
        let next = {
            let s = self.lock();
            s.groceries
                .categories
                .iter()
                .find(|c| c.id == cat_id)
                .and_then(|c| c.items.iter().find(|i| i.id == item_id))
                .map(|i| next_stock(i.stock_status))
        };
        if let Some(status) = next {
            let owned = OwnedAction::from(GroceryAction::SetStockStatus {
                cat_id,
                item_id,
                status,
            });
            self.grocery(&owned.borrow());
        }
    }

    /// Delete button. Pets, categories and stores ask for confirmation first; the other rows delete at once.
    pub fn delete(&self, row_id: &str) {
        let Some((prefix, id)) = row_id.split_once(':') else {
            return;
        };
        match prefix {
            "pet" => self.ask(Pending::Pet(id.to_owned())),
            "cat" => self.ask(Pending::Category(id.to_owned())),
            "store" => self.ask(Pending::Store(id.to_owned())),
            "care" => {
                if let Some((pet, entry)) = split2(id) {
                    let (pet, entry) = (pet.to_owned(), entry.to_owned());
                    self.shared.api.pet_action(
                        &PetAction::DeleteHistoryEntry {
                            pet_id: &pet,
                            entry_id: &entry,
                        },
                        pet_list_done(self.clone()),
                    );
                }
            }
            "weight" => {
                if let Some((pet, entry)) = split2(id) {
                    let (pet, entry) = (pet.to_owned(), entry.to_owned());
                    self.shared.api.pet_action(
                        &PetAction::DeleteWeightEntry {
                            pet_id: &pet,
                            entry_id: &entry,
                        },
                        pet_list_done(self.clone()),
                    );
                }
            }
            "wish" => {
                let wish_id = id.to_owned();
                let me = self.clone();
                self.shared.api.delete_wishlist_item(
                    &wish_id.clone(),
                    Box::new(move |result: Result<(), ApiError>| match result {
                        Ok(()) => me.change(move |s| s.wishlist.retain(|w| w.id != wish_id)),
                        Err(e) => me.fail(e),
                    }),
                );
            }
            _ => {}
        }
    }

    fn ask(&self, pending: Pending) {
        self.change(move |s| s.pending = Some(pending));
    }

    /// Answers the confirmation: yes runs the delete, no drops it.
    pub fn confirm(&self, yes: bool) {
        let pending = {
            let mut s = self.lock();
            s.pending.take()
        };
        let Some(pending) = pending else { return };
        if !yes {
            self.change(|_| {});
            return;
        }
        match pending {
            Pending::Pet(id) => {
                self.shared.api.pet_action(
                    &PetAction::DeletePet { pet_id: &id },
                    pet_list_done(self.clone()),
                );
            }
            Pending::Category(id) => self.grocery(&GroceryAction::DeleteCategory { cat_id: &id }),
            Pending::Store(id) => self.grocery(&GroceryAction::DeleteStore { store_id: &id }),
        }
    }
}

/// Owned form of [`GroceryAction`] for the actions built from row ids, which need their strings kept alive.
enum OwnedAction {
    Toggle(String, String),
    Status(String, String, StockStatus),
}

impl From<GroceryAction<'_>> for OwnedAction {
    fn from(action: GroceryAction<'_>) -> Self {
        match action {
            GroceryAction::ToggleBought { cat_id, item_id } => {
                Self::Toggle(cat_id.to_owned(), item_id.to_owned())
            }
            GroceryAction::SetStockStatus {
                cat_id,
                item_id,
                status,
            } => Self::Status(cat_id.to_owned(), item_id.to_owned(), status),
            _ => Self::Toggle(String::new(), String::new()),
        }
    }
}

impl OwnedAction {
    fn borrow(&self) -> GroceryAction<'_> {
        match self {
            Self::Toggle(cat_id, item_id) => GroceryAction::ToggleBought { cat_id, item_id },
            Self::Status(cat_id, item_id, status) => GroceryAction::SetStockStatus {
                cat_id,
                item_id,
                status: *status,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Dates and the date picker: the calendar grid the shared DatePicker shows (Sunday first, 42 cells)
// ---------------------------------------------------------------------------

const WEEKDAYS: &[&str] = &["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

const MONTHS: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The form fields that take a date. They open the date picker.
const DATE_KEYS: &[&str] = &["date", "dateOfBirth", "adoptionDate"];

/// One day of the picker's 42-cell grid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarCellView {
    pub day: i32,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
}

struct Calendar {
    cells: Vec<CalendarCellView>,
    title: String,
}

/// `(year, month, day)` of a `YYYY-MM-DD` date (anything after the day is ignored).
fn ymd(value: &str) -> Option<(i64, i64, i64)> {
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<i64>().ok()?;
    let day = value.get(8..10)?.parse::<i64>().ok()?;
    let shaped = value.get(4..5) == Some("-") && value.get(7..8) == Some("-");
    (shaped && (1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

/// Days since 1970-01-01 (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The `(year, month, day)` of a day count from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The month `delta` months after `(year, month)`.
fn shift_month(year: i64, month: i64, delta: i64) -> (i64, i64) {
    let total = year * 12 + (month - 1) + delta;
    (total.div_euclid(12), total.rem_euclid(12) + 1)
}

/// The month a date picker opens on: the navigated month, else the value's month, else today's.
fn shown_month(value: &str, nav: Option<(i64, i64)>, today: &str) -> (i64, i64) {
    nav.or_else(|| ymd(value).map(|(y, m, _)| (y, m)))
        .or_else(|| ymd(today).map(|(y, m, _)| (y, m)))
        .unwrap_or((1970, 1))
}

/// The day count of the first cell: the Sunday on or before the 1st of the month.
fn grid_start(year: i64, month: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    first - (first + 4).rem_euclid(7)
}

fn calendar_for(value: &str, nav: Option<(i64, i64)>, today: &str) -> Calendar {
    let (year, month) = shown_month(value, nav, today);
    let start = grid_start(year, month);
    let selected = ymd(value);
    let now = ymd(today);
    let cells = (0..42)
        .map(|i| {
            let date = civil_from_days(start + i);
            CalendarCellView {
                day: date.2 as i32,
                in_month: date.0 == year && date.1 == month,
                today: now == Some(date),
                selected: selected == Some(date),
            }
        })
        .collect();
    Calendar {
        cells,
        title: format!("{} {year}", MONTHS[(month - 1) as usize]),
    }
}

/// The `YYYY-MM-DD` of the picker cell at `index`, in the grid of `shown`.
fn cell_date(shown: (i64, i64), index: usize) -> String {
    let (year, month, day) = civil_from_days(grid_start(shown.0, shown.1) + index as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// What a grocery item adds to the trip estimate, as the web's `estimatedCartTotal`: the last price times the amount
/// (an amount of zero counts as one), for an item not bought that has a last price.
fn item_trip_cost(item: &GroceryItem) -> f64 {
    match item.last_price {
        Some(price) if !item.is_bought && price != 0.0 => {
            price * if item.amount == 0.0 { 1.0 } else { item.amount }
        }
        _ => 0.0,
    }
}

/// Takes the pet list a pet call returns, without closing a form.
fn pet_list_done(me: HousingController) -> ListDone<Vec<Pet>> {
    Box::new(move |result: Result<Vec<Pet>, ApiError>| match result {
        Ok(pets) => me.change(|s| {
            s.pets = pets;
            s.error = None;
        }),
        Err(e) => me.fail(e),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use idantity_core::ApiError;
    use idantity_core::housing::{
        GroceriesData, GroceryAction, GroceryCategory, GroceryItem, GroceryStore, Pet, PetAction,
        PetHistoryEntry, PetWeightEntry, StockStatus, WishlistDraft, WishlistItem,
    };

    use super::*;

    /// Records every call and answers at once from the scripted data. `fail` makes every call answer with an error.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        pets: Vec<Pet>,
        wishlist: Vec<WishlistItem>,
        groceries: GroceriesData,
        fail: bool,
    }

    impl Fake {
        fn record(&self, call: String) {
            self.calls.lock().expect("calls lock").push(call);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn reply<T>(&self, value: T, on_done: ListDone<T>) {
            on_done(if self.fail {
                Err(ApiError::Transport("offline".to_owned()))
            } else {
                Ok(value)
            });
        }
    }

    impl HousingApi for Fake {
        fn pets(&self, on_done: ListDone<Vec<Pet>>) {
            self.record("pets".to_owned());
            self.reply(self.pets.clone(), on_done);
        }

        fn pet_action(&self, action: &PetAction<'_>, on_done: ListDone<Vec<Pet>>) {
            let call = match action {
                PetAction::AddPet { pet } => format!("add_pet:{}", pet.name),
                PetAction::UpdatePet { pet_id, .. } => format!("update_pet:{pet_id}"),
                PetAction::DeletePet { pet_id } => format!("delete_pet:{pet_id}"),
                PetAction::AddHistoryEntry { pet_id, entry } => {
                    format!("add_history:{pet_id}:{}", entry.kind)
                }
                PetAction::DeleteHistoryEntry { pet_id, entry_id } => {
                    format!("delete_history:{pet_id}:{entry_id}")
                }
                PetAction::AddWeightEntry { pet_id, entry } => {
                    format!("add_weight:{pet_id}:{}", entry.weight)
                }
                PetAction::DeleteWeightEntry { pet_id, entry_id } => {
                    format!("delete_weight:{pet_id}:{entry_id}")
                }
            };
            self.record(call);
            self.reply(self.pets.clone(), on_done);
        }

        fn wishlist(&self, on_done: ListDone<Vec<WishlistItem>>) {
            self.record("wishlist".to_owned());
            self.reply(self.wishlist.clone(), on_done);
        }

        fn create_wishlist_item(&self, item: &WishlistDraft<'_>, on_done: ListDone<WishlistItem>) {
            self.record(format!("create_wish:{}", item.name));
            let created = WishlistItem {
                id: "new".to_owned(),
                name: item.name.to_owned(),
                price: item.price.map(str::to_owned),
                url: item.url.map(str::to_owned),
                priority: item.priority.to_owned(),
                category: item.category.to_owned(),
                is_purchased: false,
            };
            self.reply(created, on_done);
        }

        fn set_wishlist_purchased(&self, id: &str, is_purchased: bool, on_done: ListDone<()>) {
            self.record(format!("set_wish:{id}:{is_purchased}"));
            self.reply((), on_done);
        }

        fn delete_wishlist_item(&self, id: &str, on_done: ListDone<()>) {
            self.record(format!("delete_wish:{id}"));
            self.reply((), on_done);
        }

        fn groceries(&self, on_done: ListDone<GroceriesData>) {
            self.record("groceries".to_owned());
            self.reply(self.groceries.clone(), on_done);
        }

        fn grocery_action(&self, action: &GroceryAction<'_>, on_done: ListDone<GroceriesData>) {
            let call = match action {
                GroceryAction::AddCategory { name } => format!("add_category:{name}"),
                GroceryAction::DeleteCategory { cat_id } => format!("delete_category:{cat_id}"),
                GroceryAction::AddItem { cat_id, item } => {
                    format!("add_item:{cat_id}:{}", item.name)
                }
                GroceryAction::UpdateItem {
                    cat_id,
                    item_id,
                    item,
                } => {
                    format!("update_item:{cat_id}/{item_id}:{}", item.name)
                }
                GroceryAction::DeleteItem { cat_id, item_id } => {
                    format!("delete_item:{cat_id}/{item_id}")
                }
                GroceryAction::ToggleBought { cat_id, item_id } => {
                    format!("toggle:{cat_id}/{item_id}")
                }
                GroceryAction::SetStockStatus {
                    cat_id,
                    item_id,
                    status,
                } => {
                    format!("status:{cat_id}/{item_id}:{status:?}")
                }
                GroceryAction::AddStore { name, color } => format!("add_store:{name}:{color}"),
                GroceryAction::DeleteStore { store_id } => format!("delete_store:{store_id}"),
                GroceryAction::ResetAllToNeedBuy => "reset".to_owned(),
                GroceryAction::ClearBought => "clear".to_owned(),
            };
            self.record(call);
            self.reply(self.groceries.clone(), on_done);
        }
    }

    fn pet(id: &str, name: &str, with_history: bool) -> Pet {
        Pet {
            id: id.to_owned(),
            name: name.to_owned(),
            species: "Dog".to_owned(),
            breed: None,
            date_of_birth: None,
            adoption_date: None,
            weight: Some(10.0),
            weight_unit: Some("kg".to_owned()),
            microchip_id: None,
            history: if with_history {
                vec![PetHistoryEntry {
                    id: "h1".to_owned(),
                    date: "2026-01-01".to_owned(),
                    kind: "vet".to_owned(),
                    title: "Checkup".to_owned(),
                    notes: None,
                }]
            } else {
                Vec::new()
            },
            weight_history: vec![
                PetWeightEntry {
                    id: "w1".to_owned(),
                    date: "2026-02-01".to_owned(),
                    weight: 10.5,
                    unit: "kg".to_owned(),
                    notes: None,
                },
                PetWeightEntry {
                    id: "w0".to_owned(),
                    date: "2026-01-01".to_owned(),
                    weight: 10.0,
                    unit: "kg".to_owned(),
                    notes: None,
                },
            ],
        }
    }

    fn wish(id: &str, name: &str, purchased: bool) -> WishlistItem {
        WishlistItem {
            id: id.to_owned(),
            name: name.to_owned(),
            price: Some("100".to_owned()),
            url: None,
            priority: "high".to_owned(),
            category: "Home Gear".to_owned(),
            is_purchased: purchased,
        }
    }

    fn groceries() -> GroceriesData {
        GroceriesData {
            stores: vec![GroceryStore {
                id: "s1".to_owned(),
                name: "Padaria".to_owned(),
                color: None,
            }],
            categories: vec![GroceryCategory {
                id: "c1".to_owned(),
                name: "Bakery".to_owned(),
                items: vec![
                    GroceryItem {
                        id: "i1".to_owned(),
                        name: "Bread".to_owned(),
                        last_price: Some(5.0),
                        amount: 2.0,
                        unit: Some("kg".to_owned()),
                        brand: None,
                        store_id: Some("s1".to_owned()),
                        is_bought: false,
                        stock_status: StockStatus::Low,
                    },
                    GroceryItem {
                        id: "i2".to_owned(),
                        name: "Milk".to_owned(),
                        last_price: Some(3.0),
                        amount: 0.0,
                        unit: None,
                        brand: Some("Brand".to_owned()),
                        store_id: None,
                        is_bought: true,
                        stock_status: StockStatus::InStock,
                    },
                ],
            }],
        }
    }

    fn full() -> Fake {
        Fake {
            pets: vec![pet("p1", "Rex", true), pet("p2", "Milo", false)],
            wishlist: vec![wish("w1", "Desk", false), wish("w2", "Lamp", true)],
            groceries: groceries(),
            ..Default::default()
        }
    }

    fn controller(fake: Fake) -> (HousingController, Arc<Fake>) {
        let fake = Arc::new(fake);
        let api: Arc<dyn HousingApi> = fake.clone();
        (HousingController::new(api, |_| {}), fake)
    }

    fn has_call(fake: &Fake, call: &str) -> bool {
        fake.calls().iter().any(|c| c == call)
    }

    fn stat(view: &HousingView, label: &str) -> Option<String> {
        view.stats
            .iter()
            .find(|s| s.label == label)
            .map(|s| s.value.clone())
    }

    fn section<'a>(view: &'a HousingView, title: &str) -> &'a SectionView {
        view.sections
            .iter()
            .find(|s| s.title == title)
            .expect("section present")
    }

    #[test]
    fn pets_tab_loads_the_list_and_shows_each_pet() {
        let (c, fake) = controller(full());
        c.refresh();
        assert!(has_call(&fake, "pets"));
        let view = c.view();
        assert_eq!(view.tab, HousingTab::Pets);
        assert_eq!(section(&view, "Pets").rows.len(), 2);
        assert!(!view.loading);
    }

    #[test]
    fn adding_a_pet_needs_a_name_then_posts_it() {
        let (c, fake) = controller(full());
        c.refresh();
        c.open_form("pet");
        assert!(!c.submit());
        assert_eq!(
            c.view().form.and_then(|f| f.error).as_deref(),
            Some("Name is required.")
        );
        c.set_field("name", "Milo");
        assert!(c.submit());
        assert!(has_call(&fake, "add_pet:Milo"));
        assert!(c.view().form.is_none());
    }

    #[test]
    fn editing_a_pet_prefills_and_updates_it() {
        let (c, fake) = controller(full());
        c.refresh();
        c.edit("pet:p1");
        let view = c.view();
        let form = view.form.expect("form open");
        assert_eq!(form.title, "Edit Pet");
        assert_eq!(
            form.fields
                .iter()
                .find(|f| f.key == "name")
                .map(|f| f.value.clone())
                .as_deref(),
            Some("Rex")
        );
        c.set_field("name", "Rex II");
        assert!(c.submit());
        assert!(has_call(&fake, "update_pet:p1"));
    }

    #[test]
    fn selecting_a_pet_limits_the_care_log_and_weight_timeline() {
        let (c, _) = controller(full());
        c.refresh();
        assert_eq!(section(&c.view(), "Medical History").rows.len(), 1);
        c.primary("pet:p2");
        assert_eq!(section(&c.view(), "Medical History").rows.len(), 0);
        c.primary("pet:p1");
        assert_eq!(section(&c.view(), "Medical History").rows.len(), 1);
        assert!(
            section(&c.view(), "Weight Timeline")
                .rows
                .iter()
                .all(|r| r.id.starts_with("weight:p1/"))
        );
    }

    #[test]
    fn deleting_a_pet_asks_first() {
        let (c, fake) = controller(full());
        c.refresh();
        c.delete("pet:p1");
        assert!(c.view().confirm.is_some());
        c.confirm(false);
        assert!(c.view().confirm.is_none());
        assert!(!has_call(&fake, "delete_pet:p1"));
        c.delete("pet:p1");
        c.confirm(true);
        assert!(has_call(&fake, "delete_pet:p1"));
    }

    #[test]
    fn care_log_needs_a_title_then_posts_under_the_selected_pet() {
        let (c, fake) = controller(full());
        c.refresh();
        c.primary("pet:p1");
        c.open_form("care");
        assert!(!c.submit());
        c.set_field("title", "Vacina");
        assert!(c.submit());
        assert!(has_call(&fake, "add_history:p1:vet"));
        c.delete("care:p1/h1");
        assert!(has_call(&fake, "delete_history:p1:h1"));
    }

    #[test]
    fn weight_must_be_a_number_then_posts() {
        let (c, fake) = controller(full());
        c.refresh();
        c.primary("pet:p1");
        c.open_form("weight");
        c.set_field("weight", "abc");
        assert!(!c.submit());
        assert_eq!(
            c.view().form.and_then(|f| f.error).as_deref(),
            Some("This field must be a number.")
        );
        c.set_field("weight", "12.5");
        assert!(c.submit());
        assert!(has_call(&fake, "add_weight:p1:12.5"));
        c.delete("weight:p1/w1");
        assert!(has_call(&fake, "delete_weight:p1:w1"));
    }

    #[test]
    fn wishlist_filters_adds_toggles_and_deletes() {
        let (c, fake) = controller(full());
        c.set_tab(HousingTab::Wishlist);
        assert!(has_call(&fake, "wishlist"));
        assert_eq!(section(&c.view(), "Wishlist").rows.len(), 1);
        c.choose("filter", "all");
        assert_eq!(section(&c.view(), "Wishlist").rows.len(), 2);

        c.open_form("wish");
        assert!(!c.submit());
        c.set_field("name", "Desk");
        assert!(c.submit());
        assert!(has_call(&fake, "create_wish:Desk"));

        c.primary("wish:w1");
        assert!(has_call(&fake, "set_wish:w1:true"));
        c.delete("wish:w1");
        assert!(has_call(&fake, "delete_wish:w1"));
    }

    #[test]
    fn groceries_show_the_trip_estimate_and_filter() {
        let (c, fake) = controller(full());
        c.set_tab(HousingTab::Groceries);
        assert!(has_call(&fake, "groceries"));
        let view = c.view();
        // Bread: last price 5 times amount 2. Milk is bought, so it adds nothing.
        assert_eq!(stat(&view, "Est. Trip Cost").as_deref(), Some("$10.00"));
        assert_eq!(stat(&view, "Items remaining").as_deref(), Some("1"));
        c.set_search("milk");
        assert_eq!(section(&c.view(), "Bakery (10.00)").rows.len(), 1);
        c.set_search("");
        c.choose("mode", "low");
        assert_eq!(section(&c.view(), "Bakery (10.00)").rows.len(), 1);
        c.choose("mode", "all");
        c.choose("store", "s1");
        assert_eq!(section(&c.view(), "Bakery (10.00)").rows.len(), 1);
        c.choose("store", "");
    }

    #[test]
    fn grocery_items_add_edit_toggle_and_cycle_stock() {
        let (c, fake) = controller(full());
        c.set_tab(HousingTab::Groceries);

        c.open_form("item");
        assert!(!c.submit());
        c.set_field("categoryId", "c1");
        c.set_field("name", "Eggs");
        assert!(c.submit());
        assert!(has_call(&fake, "add_item:c1:Eggs"));

        c.edit("item:c1/i1");
        assert_eq!(
            c.view().form.map(|f| f.title).as_deref(),
            Some("Edit Grocery Item")
        );
        assert!(c.submit());
        assert!(has_call(&fake, "update_item:c1/i1:Bread"));

        c.primary("item:c1/i1");
        assert!(has_call(&fake, "toggle:c1/i1"));
        c.secondary("item:c1/i1");
        assert!(has_call(&fake, "status:c1/i1:OutOfStock"));
    }

    #[test]
    fn grocery_categories_stores_and_bulk_actions() {
        let (c, fake) = controller(full());
        c.set_tab(HousingTab::Groceries);

        c.open_form("category");
        c.set_field("name", "Frutas");
        assert!(c.submit());
        assert!(has_call(&fake, "add_category:Frutas"));

        c.open_form("store");
        c.set_field("name", "Feira");
        assert!(c.submit());
        assert!(has_call(&fake, "add_store:Feira:"));

        c.delete("cat:c1");
        c.confirm(true);
        assert!(has_call(&fake, "delete_category:c1"));

        c.delete("store:s1");
        c.confirm(true);
        assert!(has_call(&fake, "delete_store:s1"));

        c.choose("bulk", "reset-out-of-stock");
        assert!(has_call(&fake, "reset"));
        c.choose("bulk", "clear-checked");
        assert!(has_call(&fake, "clear"));
    }

    #[test]
    fn a_refused_load_shows_the_error_and_dismisses() {
        let (c, _) = controller(Fake {
            fail: true,
            ..Default::default()
        });
        c.refresh();
        assert_eq!(c.view().error.as_deref(), Some("network error: offline"));
        c.dismiss_error();
        assert!(c.view().error.is_none());
    }

    #[test]
    fn tabs_round_trip_their_keys() {
        for tab in HousingTab::ALL {
            assert_eq!(HousingTab::from_key(tab.key()), Some(tab));
        }
    }
}
