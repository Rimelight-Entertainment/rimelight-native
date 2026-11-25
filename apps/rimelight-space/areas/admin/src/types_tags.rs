//! Admin > Projects > Types & Tags (`ui/types-tags.slint`). Shows the issue types and the areas an issue can carry, each in a
//! table with a colour an admin can pick. Only an admin may change anything (the server checks the role too); anyone else sees
//! the page's denied state.
//!
//! Plain Rust: colours travel as `#RRGGBB` text. The window side is [`TypesTagsWindow`], which `wire_admin!` implements for the
//! generated window (it turns the text into the window's colours, and changes a row's colour in place).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{Color, TimerMode, Weak};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::person::is_admin;
use space_proto::projects::{Area, CreateArea, IssueType};

/// What the page loads: whether the caller is an admin, the issue types, and the areas.
type Loaded = (bool, Vec<IssueType>, Vec<Area>);

/// The colour a new area starts with, as `#RRGGBB`.
const DEFAULT_COLOUR: &str = "#1659C4";

/// One issue type's table row, as plain values: the stored name (also its label) and its colour as `#RRGGBB`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeView {
    pub name: String,
    pub shade: String,
}

/// One area's row, as plain values: its id, its name and its colour as `#RRGGBB`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AreaView {
    pub id: String,
    pub name: String,
    pub shade: String,
}

/// The window side of the types and tags page. `wire_admin!` implements it for the generated window.
pub trait TypesTagsWindow: slint::ComponentHandle + 'static {
    /// Sets the colour the new-area picker starts with.
    fn show_new_colour(&self, hex: &str);
    /// The new area's name, as the user has it.
    fn new_name(&self) -> String;
    /// The new area's colour as `#RRGGBB`.
    fn new_colour(&self) -> String;
    fn show_new_name(&self, name: &str);
    fn show_state(&self, state: LoadState);
    fn show_denied(&self, denied: bool);
    fn show_error(&self, text: &str);
    fn show_action_error(&self, text: &str);
    fn show_types(&self, rows: Vec<TypeView>);
    fn show_areas(&self, rows: Vec<AreaView>);
    /// The area rows the page shows now.
    fn areas(&self) -> Vec<AreaView>;
    /// Sets one area row's colour in place (the row model is not replaced, so an open picker stays open). Returns the `#RRGGBB`
    /// the row had, or `None` when no row has the id.
    fn set_area_shade(&self, id: &str, hex: &str) -> Option<String>;
    /// Sets one issue type row's colour in place. Returns the `#RRGGBB` the row had, or `None` when no row has the name.
    fn set_type_shade(&self, name: &str, hex: &str) -> Option<String>;
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_add(&self, f: impl Fn() + 'static);
    fn on_remove(&self, f: impl Fn(String) + 'static);
    fn on_set_area_colour(&self, f: impl Fn(String, String) + 'static);
    fn on_set_type_colour(&self, f: impl Fn(String, String) + 'static);
}

/// Wires the page's callbacks on the window. `data` is the slot the session driver fills.
pub fn wire<W: TypesTagsWindow>(ui: &W, data: DataCell) {
    ui.show_new_colour(DEFAULT_COLOUR);

    let page = Page {
        ui: ui.as_weak(),
        data,
        writing: Arc::new(Mutex::new(false)),
        saves: Rc::new(RefCell::new(ColourSaves {
            pending: Vec::new(),
            timer: slint::Timer::default(),
        })),
    };

    let target = page.clone();
    ui.on_opened(move || target.load());

    let target = page.clone();
    ui.on_retry(move || target.load());

    let target = page.clone();
    ui.on_add(move || target.add());

    let target = page.clone();
    ui.on_remove(move |id| target.remove(&id));

    let target = page.clone();
    ui.on_set_area_colour(move |id, hex| target.set_area_colour(&id, &hex));

    let target = page.clone();
    ui.on_set_type_colour(move |name, hex| target.set_type_colour(&name, &hex));
}

/// The page's handles. Clones share the state.
pub struct Page<W: TypesTagsWindow> {
    ui: Weak<W>,
    data: DataCell,
    /// True while an add is on its way, so a second press waits for it.
    writing: Arc<Mutex<bool>>,
    /// Colour changes waiting to be saved (see `queue_colour`). Only touched on the UI thread.
    saves: Rc<RefCell<ColourSaves>>,
}

impl<W: TypesTagsWindow> Clone for Page<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            writing: self.writing.clone(),
            saves: self.saves.clone(),
        }
    }
}

/// The colour saves waiting to be sent. A change shows at once and is saved after a short quiet time, so a drag that ends in
/// several releases sends one request per colour.
struct ColourSaves {
    pending: Vec<PendingColour>,
    timer: slint::Timer,
}

/// Which row a colour save is for.
#[derive(Clone, PartialEq)]
enum ColourTarget {
    Area(String),
    Type(String),
}

/// A colour change not yet sent: the `#RRGGBB` to save, and the colour the row had before the first change of this batch,
/// which the row goes back to if the save fails.
struct PendingColour {
    target: ColourTarget,
    hex: String,
    previous: String,
}

/// How long a colour stays unsaved after its last change.
const COLOUR_SAVE_DELAY: Duration = Duration::from_millis(400);

impl<W: TypesTagsWindow> Page<W> {
    fn source(&self) -> Option<Arc<DataSource>> {
        self.data.borrow().clone()
    }

    /// Loads whether the caller is an admin, the issue types and the areas. A denied caller sees the denied state only.
    fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.show_state(LoadState::Loading);
        ui.show_error("");
        let Some(source) = self.source() else {
            return;
        };
        source.fetch(
            &self.ui,
            |api, token| async move {
                let me = api.my_profile(&token).await?;
                let types = api.issue_types(&token).await?;
                let areas = if is_admin(&me) {
                    api.areas(&token).await?
                } else {
                    Vec::new()
                };
                Ok::<Loaded, ClientError>((is_admin(&me), types, areas))
            },
            move |ui, result: Result<Loaded, ClientError>| deliver_load(ui, result),
        );
    }

    /// Creates the area named in the new-area form. Blank names and a second press during a write send nothing.
    fn add(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let name = ui.new_name().trim().to_owned();
        let colour = ui.new_colour();
        if name.is_empty() {
            ui.show_action_error("Enter a name for the area.");
            return;
        }
        {
            let mut writing = self
                .writing
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *writing {
                return;
            }
            *writing = true;
        }
        ui.show_action_error("");
        let Some(source) = self.source() else {
            *self.writing.lock().unwrap_or_else(|p| p.into_inner()) = false;
            return;
        };
        let writing = self.writing.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.create_area(
                    &token,
                    &CreateArea {
                        name,
                        color: Some(colour),
                    },
                )
                .await
            },
            move |ui, result: Result<Area, ClientError>| {
                *writing.lock().unwrap_or_else(|p| p.into_inner()) = false;
                match result {
                    Ok(area) => {
                        // The list takes the area the server just stored, in its sorted place. It is not read again: a
                        // read straight after the write has shown the old list.
                        let mut rows = ui.areas();
                        rows.push(AreaView {
                            id: area.id,
                            name: area.name,
                            shade: area.color,
                        });
                        rows.sort_by_key(|row| row.name.to_lowercase());
                        ui.show_areas(rows);
                        ui.show_new_name("");
                        ui.show_action_error("");
                    }
                    Err(error) => {
                        ui.show_action_error(&page_data::error_text(&error));
                    }
                }
            },
        );
    }

    /// Deletes the area with this id, then reads the list again.
    fn remove(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.show_action_error("");
        let Some(source) = self.source() else { return };
        let id = id.to_owned();
        let removed_id = id.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.delete_area(&token, &id).await },
            move |ui, result: Result<(), ClientError>| match result {
                Ok(()) => {
                    // Drop the row here too, for the same reason as after an add.
                    let rows: Vec<AreaView> = ui
                        .areas()
                        .into_iter()
                        .filter(|row| row.id != removed_id)
                        .collect();
                    ui.show_areas(rows);
                }
                Err(error) => ui.show_action_error(&page_data::error_text(&error)),
            },
        );
    }

    /// Sets an area's colour. The row shows it at once; the save follows after a short quiet time.
    fn set_area_colour(&self, id: &str, hex: &str) {
        self.queue_colour(ColourTarget::Area(id.to_owned()), hex);
    }

    /// Sets an issue type's colour. The row shows it at once; the save follows after a short quiet time.
    fn set_type_colour(&self, name: &str, hex: &str) {
        self.queue_colour(ColourTarget::Type(name.to_owned()), hex);
    }

    /// Shows a colour on its row in place (the row model is not replaced, so an open picker stays open), then queues the
    /// save. A further change within the delay replaces the colour to save and keeps the colour from before the batch.
    fn queue_colour(&self, target: ColourTarget, hex: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let previous = match &target {
            ColourTarget::Area(id) => ui.set_area_shade(id, hex),
            ColourTarget::Type(name) => ui.set_type_shade(name, hex),
        };
        let Some(previous) = previous else { return };
        ui.show_action_error("");
        let mut saves = self.saves.borrow_mut();
        match saves.pending.iter_mut().find(|save| save.target == target) {
            Some(save) => save.hex = hex.to_owned(),
            None => saves.pending.push(PendingColour {
                target,
                hex: hex.to_owned(),
                previous,
            }),
        }
        let page = self.clone();
        saves
            .timer
            .start(TimerMode::SingleShot, COLOUR_SAVE_DELAY, move || {
                page.flush_colours();
            });
    }

    /// Sends the queued colour saves. A failed save puts its row back to the colour it had, and shows the error.
    fn flush_colours(&self) {
        let batch = std::mem::take(&mut self.saves.borrow_mut().pending);
        for save in batch {
            let Some(source) = self.source() else { return };
            let PendingColour {
                target,
                hex,
                previous,
            } = save;
            match target {
                ColourTarget::Area(id) => {
                    let row_id = id.clone();
                    source.fetch(
                        &self.ui,
                        move |api, token| async move { api.set_area_colour(&token, &id, &hex).await },
                        move |ui, result: Result<Area, ClientError>| {
                            if let Err(error) = result {
                                ui.set_area_shade(&row_id, &previous);
                                ui.show_action_error(&page_data::error_text(&error));
                            }
                        },
                    );
                }
                ColourTarget::Type(name) => {
                    let row_name = name.clone();
                    source.fetch(
                        &self.ui,
                        move |api, token| async move {
                            api.set_issue_type_colour(&token, &name, &hex).await
                        },
                        move |ui, result: Result<IssueType, ClientError>| {
                            if let Err(error) = result {
                                ui.set_type_shade(&row_name, &previous);
                                ui.show_action_error(&page_data::error_text(&error));
                            }
                        },
                    );
                }
            }
        }
    }
}

/// Shows the loaded page, or the denied state for a caller without the admin role.
fn deliver_load<W: TypesTagsWindow>(ui: &W, result: Result<Loaded, ClientError>) {
    match result {
        Ok((admin, types, areas)) => {
            ui.show_denied(!admin);
            ui.show_types(types.into_iter().map(type_row).collect());
            ui.show_areas(
                areas
                    .into_iter()
                    .map(|area| AreaView {
                        id: area.id,
                        name: area.name,
                        shade: area.color,
                    })
                    .collect(),
            );
            ui.show_state(LoadState::Loaded);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// One issue type's table row: the stored name, the label shown is the same, and its colour.
fn type_row(issue_type: IssueType) -> TypeView {
    TypeView {
        name: issue_type.name,
        shade: issue_type.color,
    }
}

/// The colour of a `#RRGGBB` value, for the window. A bad value shows the default blue.
pub fn colour_of(hex: &str) -> Color {
    let parsed = hex
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|digits| u32::from_str_radix(digits, 16).ok());
    match parsed {
        Some(value) => Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8),
        None => Color::from_rgb_u8(0x16, 0x59, 0xC4),
    }
}

/// The `#RRGGBB` text of a colour, upper case: what the API stores.
pub fn hex_of(colour: Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        colour.red(),
        colour.green(),
        colour.blue()
    )
}

#[cfg(test)]
mod tests {
    use super::{colour_of, hex_of};

    #[test]
    fn each_channel_comes_from_its_own_byte() {
        let c = colour_of("#102030");
        assert_eq!((c.red(), c.green(), c.blue()), (0x10, 0x20, 0x30));
        assert_eq!(hex_of(colour_of("#FF0000")), "#FF0000");
        assert_eq!(hex_of(colour_of("#00FF00")), "#00FF00");
        assert_eq!(hex_of(colour_of("#0000FF")), "#0000FF");
    }

    #[test]
    fn hex_round_trips_through_the_colour() {
        assert_eq!(hex_of(colour_of("#1659c4")), "#1659C4");
        assert_eq!(hex_of(colour_of("#000000")), "#000000");
        assert_eq!(hex_of(colour_of("#FFFFFF")), "#FFFFFF");
    }

    #[test]
    fn a_bad_value_shows_the_default_blue() {
        assert_eq!(hex_of(colour_of("1659C4")), "#1659C4");
        assert_eq!(hex_of(colour_of("#12345")), "#1659C4");
        assert_eq!(hex_of(colour_of("#GG0000")), "#1659C4");
    }
}
