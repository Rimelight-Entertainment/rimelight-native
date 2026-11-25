//! Collections admin (CMS.6, `/admin/content/collections`): the content types pages are made from. List, create, edit (label,
//! plural label, icon, colour, routable path pattern, taxonomies feature, and the field list), delete (the 409 "has pages"
//! error is shown as a sentence).
//!
//! Field types are the registry's vocabulary (`rimelight_content` registry `FieldDef.type`): text, textarea, richtext, number,
//! boolean, select, url, media, json. UNSURE: the CMS's full field list is not in this repo; add a type here and in the
//! server's validation together if a kind is missing.
//!
//! The list returns each collection's fields and features, so an edit opens the whole form. Saving sends the whole collection
//! (PATCH), fields included. UNSURE: a field whose stored type the form does not know is read as a text field, so saving
//! such a collection changes that field's type.

use serde_json::{Value, json};
use space_client::content::{CollectionInput, ContentCollection};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{DataCell, DataSource, error_text};
use std::sync::{Arc, Mutex};

use crate::view::{parse_hex, valid_slug};

/// The field types a collection's field can have (the registry vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Textarea,
    Richtext,
    Number,
    Boolean,
    Select,
    Url,
    Media,
    Json,
}

impl FieldKind {
    /// Every kind, in the order the picker shows them.
    pub const ALL: [FieldKind; 9] = [
        FieldKind::Text,
        FieldKind::Textarea,
        FieldKind::Richtext,
        FieldKind::Number,
        FieldKind::Boolean,
        FieldKind::Select,
        FieldKind::Url,
        FieldKind::Media,
        FieldKind::Json,
    ];

    /// The name the server stores (`type` in the field JSON).
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Textarea => "textarea",
            FieldKind::Richtext => "richtext",
            FieldKind::Number => "number",
            FieldKind::Boolean => "boolean",
            FieldKind::Select => "select",
            FieldKind::Url => "url",
            FieldKind::Media => "media",
            FieldKind::Json => "json",
        }
    }

    /// Parses a stored or picked name. Unknown names are `None`.
    pub fn from_name(name: &str) -> Option<FieldKind> {
        FieldKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// One field of a collection, as the editor holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDraft {
    pub name: String,
    pub kind: FieldKind,
    pub label: String,
    pub required: bool,
    /// Select options, one per line in the form ("value" each, shown as the label too).
    pub options: Vec<String>,
}

impl FieldDraft {
    /// Checks one field: a lowercase name (letters, digits, underscore, starting with a letter), a label, and at least one
    /// option for a select.
    pub fn validate(&self) -> Result<(), String> {
        let name = &self.name;
        let ok_name = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ok_name {
            return Err(format!(
                "Field name \"{name}\" must start with a letter and use lowercase letters, digits and _."
            ));
        }
        if self.label.trim().is_empty() {
            return Err(format!("Field \"{name}\" needs a label."));
        }
        if self.kind == FieldKind::Select && self.options.iter().all(|o| o.trim().is_empty()) {
            return Err(format!(
                "Select field \"{name}\" needs at least one option."
            ));
        }
        Ok(())
    }

    /// The field as the server stores it (the registry's `FieldDef` shape: type, label per locale, required, options).
    /// Reads a field as the server returns it (the shape [`FieldDraft::to_json`] writes). `None` when it has no name.
    pub fn from_json(value: &Value) -> Option<FieldDraft> {
        let name = value.get("name")?.as_str()?.to_string();
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .and_then(FieldKind::from_name)
            .unwrap_or(FieldKind::Text);
        let label = value
            .get("label")
            .and_then(|l| l.get("en").and_then(Value::as_str).or_else(|| l.as_str()))
            .unwrap_or_default()
            .to_string();
        let options = value
            .get("options")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|o| o.get("value").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Some(FieldDraft {
            name,
            kind,
            label,
            required: value
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            options,
        })
    }

    pub fn to_json(&self) -> Value {
        let mut field = json!({
            "name": self.name,
            "type": self.kind.name(),
            "label": { "en": self.label },
            "required": self.required,
        });
        if self.kind == FieldKind::Select {
            let options: Vec<Value> = self
                .options
                .iter()
                .map(|o| o.trim())
                .filter(|o| !o.is_empty())
                .map(|o| json!({ "value": o, "label": { "en": o } }))
                .collect();
            field["options"] = Value::Array(options);
        }
        field
    }
}

/// The collection as the admin form holds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CollectionDraft {
    /// Set for an existing collection (its slug cannot change).
    pub editing: Option<String>,
    pub slug: String,
    pub label: String,
    pub label_plural: String,
    pub icon: String,
    pub color: String,
    /// The routable path pattern, for example `/wiki/:slug` (empty: not routable).
    pub path_pattern: String,
    pub taxonomies: bool,
    pub fields: Vec<FieldDraft>,
}

impl CollectionDraft {
    /// Checks the whole form before any request.
    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() {
            return Err("Give the collection a label.".to_string());
        }
        if self.editing.is_none() && !valid_slug(&self.slug) {
            return Err(
                "The slug may use lowercase letters, digits and single hyphens.".to_string(),
            );
        }
        if !self.color.trim().is_empty() && parse_hex(self.color.trim()).is_none() {
            return Err("The colour must be #rrggbb.".to_string());
        }
        let mut names: Vec<&str> = self.fields.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return Err("Two fields have the same name.".to_string());
        }
        for field in &self.fields {
            field.validate()?;
        }
        Ok(())
    }

    /// The request body (`POST` and `PATCH`). The fields and the path pattern go in `features` and `fields`.
    pub fn to_input(&self) -> CollectionInput {
        CollectionInput {
            slug: self.slug.clone(),
            label: self.label.trim().to_string(),
            label_plural: if self.label_plural.trim().is_empty() {
                self.label.trim().to_string()
            } else {
                self.label_plural.trim().to_string()
            },
            icon: if self.icon.trim().is_empty() {
                "file-text".to_string()
            } else {
                self.icon.trim().to_string()
            },
            color: (!self.color.trim().is_empty()).then(|| self.color.trim().to_string()),
            features: json!({
                "taxonomies": self.taxonomies,
                "pathPattern": self.path_pattern.trim(),
            }),
            fields: Value::Array(self.fields.iter().map(FieldDraft::to_json).collect()),
        }
    }
}

/// The message for a failed delete or save. A 409 on delete means the collection still has pages.
pub fn admin_error(error: &ClientError) -> String {
    match error {
        ClientError::Api { status: 409, .. } => {
            "This collection has pages. Move or delete them first.".to_string()
        }
        other => error_text(other),
    }
}

/// One row of the collections list.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionRow {
    pub slug: String,
    pub label: String,
    pub icon: String,
    pub chip: (u8, u8, u8),
    pub mode: String,
}

/// Maps the list to its rows.
pub fn rows(collections: &[ContentCollection]) -> Vec<CollectionRow> {
    collections
        .iter()
        .map(|c| CollectionRow {
            slug: c.slug.clone(),
            label: c.name.clone(),
            icon: c.icon.clone(),
            chip: crate::view::chip_colour(Some(c)),
            mode: c.mode.clone(),
        })
        .collect()
}

/// The window side of the collections admin page.
pub trait CollectionsWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_rows(&self, rows: Vec<CollectionRow>);
    /// The form: `None` closes it.
    fn show_form(&self, draft: Option<CollectionDraft>);
    fn show_form_error(&self, text: &str);
    fn show_notice(&self, text: &str);
    fn ask_reload(&self);
    fn on_reload(&self, f: impl Fn() + 'static);
    fn on_new(&self, f: impl Fn() + 'static);
    fn on_edit(&self, f: impl Fn(String) + 'static);
    fn on_cancel(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn() + 'static);
    fn on_delete(&self, f: impl Fn(String) + 'static);
    /// A form value changed: index -1 is the collection's own field; otherwise the field at that index. `key` names it.
    fn on_form_field(&self, f: impl Fn(i32, String, String) + 'static);
    fn on_field_add(&self, f: impl Fn() + 'static);
    fn on_field_remove(&self, f: impl Fn(i32) + 'static);
}

#[derive(Default)]
struct Shared {
    generation: u64,
    collections: Vec<ContentCollection>,
    loaded: bool,
    error: Option<String>,
    draft: Option<CollectionDraft>,
}

/// The collections admin page. Clones share the state.
pub struct Collections<W: CollectionsWindow> {
    ui: slint::Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: CollectionsWindow> Clone for Collections<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the page's callbacks on `ui`. `data` is the session's slot, as the other pages use it.
pub fn wire<W: CollectionsWindow>(ui: &W, data: DataCell) -> Collections<W> {
    let page = Collections {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
    };
    let p = page.clone();
    ui.on_reload(move || p.load());
    let p = page.clone();
    ui.on_new(move || p.open_form(None));
    let p = page.clone();
    ui.on_edit(move |slug| p.open_form(Some(slug)));
    let p = page.clone();
    ui.on_cancel(move || p.close_form());
    let p = page.clone();
    ui.on_save(move || p.save());
    let p = page.clone();
    ui.on_delete(move |slug| p.delete(&slug));
    let p = page.clone();
    ui.on_form_field(move |index, key, value| p.set_field(index, &key, &value));
    let p = page.clone();
    ui.on_field_add(move || p.add_field());
    let p = page.clone();
    ui.on_field_remove(move |index| p.remove_field(index));
    page
}

impl<W: CollectionsWindow> Collections<W> {
    fn run<T, F, Fut, D>(&self, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(Arc<space_client::api::ApiClient>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, ClientError>>,
        D: FnOnce(&W, Result<T, ClientError>) + Send + 'static,
    {
        let source: Option<Arc<DataSource>> = self.data.borrow().clone();
        if let Some(source) = source {
            source.fetch(&self.ui, work, done);
        }
    }

    /// Loads the collections. The shell calls it when the collections route opens.
    pub fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            s.generation += 1;
            s.loaded = false;
            s.error = None;
            ui.show_state(LoadState::Loading);
            s.generation
        };
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move { api.content_collections(&token).await },
            move |ui, result| {
                let mut s = lock(&shared);
                if s.generation != generation {
                    return;
                }
                match result {
                    Ok(collections) => {
                        s.collections = collections;
                        s.loaded = true;
                        ui.show_rows(rows(&s.collections));
                        ui.show_state(if s.collections.is_empty() {
                            LoadState::Empty
                        } else {
                            LoadState::Loaded
                        });
                    }
                    Err(e) => {
                        let text = error_text(&e);
                        s.error = Some(text.clone());
                        ui.show_state(LoadState::Error);
                        ui.show_error(&text);
                    }
                }
            },
        );
    }

    /// Opens the form: blank for a new collection, else the list's labels for `slug`.
    fn open_form(&self, slug: Option<String>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let draft = match slug {
            None => CollectionDraft {
                taxonomies: false,
                ..CollectionDraft::default()
            },
            Some(slug) => {
                let s = lock(&self.shared);
                let Some(c) = s.collections.iter().find(|c| c.slug == slug) else {
                    return;
                };
                CollectionDraft {
                    editing: Some(c.slug.clone()),
                    slug: c.slug.clone(),
                    label: c.name.clone(),
                    label_plural: c.label_plural.clone().unwrap_or_else(|| c.name.clone()),
                    icon: c.icon.clone(),
                    color: c.color.clone().unwrap_or_default(),
                    path_pattern: c
                        .features
                        .get("pathPattern")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    taxonomies: c
                        .features
                        .get("taxonomies")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    fields: c.fields.iter().filter_map(FieldDraft::from_json).collect(),
                }
            }
        };
        lock(&self.shared).draft = Some(draft.clone());
        ui.show_form(Some(draft));
    }

    fn close_form(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        lock(&self.shared).draft = None;
        ui.show_form(None);
    }

    /// One field of the form changed. `index` -1 is the collection's own field; `key` names it.
    fn set_field(&self, index: i32, key: &str, value: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let Some(draft) = s.draft.as_mut() else {
            return;
        };
        if index < 0 {
            match key {
                "slug" => draft.slug = value.to_string(),
                "label" => draft.label = value.to_string(),
                "plural" => draft.label_plural = value.to_string(),
                "icon" => draft.icon = value.to_string(),
                "color" => draft.color = value.to_string(),
                "path" => draft.path_pattern = value.to_string(),
                "taxonomies" => draft.taxonomies = value == "true",
                _ => {}
            }
        } else if let Some(field) = usize::try_from(index)
            .ok()
            .and_then(|i| draft.fields.get_mut(i))
        {
            match key {
                "name" => field.name = value.to_string(),
                "kind" => field.kind = FieldKind::from_name(value).unwrap_or(field.kind),
                "label" => field.label = value.to_string(),
                "required" => field.required = value == "true",
                "options" => field.options = value.lines().map(str::to_string).collect(),
                _ => {}
            }
        }
        ui.show_form(Some(draft.clone()));
    }

    fn add_field(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let Some(draft) = s.draft.as_mut() else {
            return;
        };
        draft.fields.push(FieldDraft {
            name: format!("field_{}", draft.fields.len() + 1),
            kind: FieldKind::Text,
            label: String::new(),
            required: false,
            options: Vec::new(),
        });
        ui.show_form(Some(draft.clone()));
    }

    fn remove_field(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        let Some(draft) = s.draft.as_mut() else {
            return;
        };
        if let Ok(i) = usize::try_from(index)
            && i < draft.fields.len()
        {
            draft.fields.remove(i);
        }
        ui.show_form(Some(draft.clone()));
    }

    /// Saves the form. A new collection is created; an existing one is updated with its whole form (fields included).
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let draft = lock(&self.shared).draft.clone();
        let Some(draft) = draft else { return };
        if let Err(text) = draft.validate() {
            ui.show_form_error(&text);
            return;
        }
        let editing = draft.editing.clone();
        let input = draft.to_input();
        let done = move |ui: &W, result: Result<Value, ClientError>| match result {
            Ok(_) => {
                ui.show_form(None);
                ui.ask_reload();
            }
            Err(e) => ui.show_form_error(&admin_error(&e)),
        };
        match editing {
            Some(slug) => self.run(
                move |api, token| async move { api.update_content_collection(&token, &slug, &input).await },
                done,
            ),
            None => self.run(
                move |api, token| async move { api.create_content_collection(&token, &input).await },
                done,
            ),
        }
    }

    fn delete(&self, slug: &str) {
        let slug = slug.to_string();
        self.run(
            move |api, token| async move { api.delete_content_collection(&token, &slug).await },
            move |ui, result| match result {
                Ok(()) => ui.ask_reload(),
                Err(e) => ui.show_notice(&admin_error(&e)),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, kind: FieldKind, label: &str) -> FieldDraft {
        FieldDraft {
            name: name.into(),
            kind,
            label: label.into(),
            required: false,
            options: Vec::new(),
        }
    }

    fn draft() -> CollectionDraft {
        CollectionDraft {
            slug: "wiki".into(),
            label: "Wiki page".into(),
            ..CollectionDraft::default()
        }
    }

    #[test]
    fn field_kinds_round_trip_their_names() {
        for kind in FieldKind::ALL {
            assert_eq!(FieldKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(FieldKind::from_name("date"), None);
    }

    #[test]
    fn a_good_form_validates_and_builds_the_input() {
        let mut d = draft();
        d.fields
            .push(field("summary", FieldKind::Textarea, "Summary"));
        assert_eq!(d.validate(), Ok(()));
        let input = d.to_input();
        assert_eq!(input.label_plural, "Wiki page");
        assert_eq!(input.icon, "file-text");
        assert_eq!(input.color, None);
        assert_eq!(input.fields[0]["type"], "textarea");
    }

    #[test]
    fn bad_forms_are_refused_with_a_sentence() {
        let mut no_label = draft();
        no_label.label = " ".into();
        assert!(no_label.validate().is_err());

        let mut bad_slug = draft();
        bad_slug.slug = "Wiki Page".into();
        assert!(bad_slug.validate().is_err());

        let mut bad_colour = draft();
        bad_colour.color = "red".into();
        assert!(bad_colour.validate().is_err());

        let mut dup = draft();
        dup.fields.push(field("a", FieldKind::Text, "A"));
        dup.fields.push(field("a", FieldKind::Text, "A again"));
        assert!(dup.validate().is_err());

        let mut select = draft();
        let mut f = field("kind", FieldKind::Select, "Kind");
        f.options = vec!["  ".into()];
        select.fields.push(f);
        assert!(select.validate().is_err());
    }

    #[test]
    fn a_409_on_delete_reads_as_has_pages() {
        let err = ClientError::Api {
            status: 409,
            error: "conflict".into(),
            message: "has pages".into(),
        };
        assert_eq!(
            admin_error(&err),
            "This collection has pages. Move or delete them first."
        );
    }
}
