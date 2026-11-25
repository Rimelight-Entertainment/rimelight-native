//! Projects > New sprint: the dialog (`ui/new-sprint.slint`) that the Issues page's New sprint button opens.
//! The project is the one on the route (`/p/:key/issues`). The name and both dates are needed; the goal may be empty. The
//! dates are picked from calendars, as the New issue due date is. Create sends the sprint, then closes the dialog.
//!
//! Plain Rust. The window side is [`NewSprintWindow`], which `wire_projects!` implements for the generated window.

use std::sync::{Arc, Mutex, MutexGuard};

use slint::{ComponentHandle, Weak};
use space_client::error::ClientError;
use space_proto::projects::{CreateSprint, Sprint};

use super::issue::is_date;
use space_page_kit::calendar::{self, Date};
use space_page_kit::dates::{CalendarCell, cells, iso, month_title, parse_iso, today};
use space_page_kit::page_data::{self, DataCell};
use space_page_kit::projects::project_of_route;

/// Which date picker a callback is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Start,
    End,
}

/// The dialog's form values as the window has them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub goal: String,
    pub start: String,
    pub end: String,
}

/// The window side of the New sprint dialog. `wire_projects!` implements it for the generated window.
pub trait NewSprintWindow: ComponentHandle + 'static {
    /// The route the window is on.
    fn route(&self) -> String;
    /// The form as the person has it now.
    fn form(&self) -> Form;
    /// The text of one date field (empty for none).
    fn date(&self, field: Field) -> String;
    fn show_date(&self, field: Field, text: &str);
    /// Whether a create is in flight.
    fn creating(&self) -> bool;
    fn show_creating(&self, on: bool);
    fn show_error(&self, text: &str);
    /// Empties the name, goal and dates, and clears the error and the in-flight flag.
    fn clear_form(&self);
    fn show_weekday_labels(&self, labels: Vec<String>);
    /// The 42 cells of a date picker's month and its title.
    fn show_days(&self, field: Field, cells: Vec<CalendarCell>, title: String);
    /// Opens the dialog.
    fn show_dialog(&self);
    /// Closes the dialog after a create.
    fn close_dialog(&self);
    fn on_requested(&self, f: impl Fn() + 'static);
    fn on_create(&self, f: impl Fn() + 'static);
    fn on_picked(&self, field: Field, f: impl Fn(i32) + 'static);
    fn on_step(&self, field: Field, f: impl Fn(i32) + 'static);
}

/// The dialog's state: the project it creates in, and the month each date picker shows once moved.
#[derive(Default)]
struct Shared {
    project: String,
    start_shown: Option<(i32, u32)>,
    end_shown: Option<(i32, u32)>,
}

/// The New sprint dialog. Clones share the state.
pub struct NewSprint<W: NewSprintWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: NewSprintWindow> Clone for NewSprint<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the dialog's callbacks on `ui`. `data` is the slot the session driver fills.
pub fn wire<W: NewSprintWindow>(ui: &W, data: DataCell) -> NewSprint<W> {
    let page = NewSprint {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };
    let open = page.clone();
    ui.on_requested(move || {
        let Some(ui) = open.ui.upgrade() else { return };
        let route = ui.route();
        open.open(&ui, project_of_route(&route));
    });
    let create = page.clone();
    ui.on_create(move || create.create());
    let start_pick = page.clone();
    ui.on_picked(Field::Start, move |index| {
        start_pick.pick(Field::Start, index)
    });
    let start_step = page.clone();
    ui.on_step(Field::Start, move |step| {
        start_step.step(Field::Start, step)
    });
    let end_pick = page.clone();
    ui.on_picked(Field::End, move |index| end_pick.pick(Field::End, index));
    let end_step = page.clone();
    ui.on_step(Field::End, move |step| end_step.step(Field::End, step));
    page
}

/// The create body from the form. The name and both dates are needed; the goal may be empty.
pub fn create_body(name: &str, goal: &str, start: &str, end: &str) -> Result<CreateSprint, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A sprint needs a name.".to_owned());
    }
    let (start, end) = (start.trim(), end.trim());
    if !is_date(start) || !is_date(end) {
        return Err("Pick both dates.".to_owned());
    }
    let goal = goal.trim();
    Ok(CreateSprint {
        name: name.to_owned(),
        goal: (!goal.is_empty()).then(|| goal.to_owned()),
        start_date: start.to_owned(),
        end_date: end.to_owned(),
        state: None,
    })
}

impl<W: NewSprintWindow> NewSprint<W> {
    /// Opens the dialog for the route's project, with an empty form. Off a project route it does nothing.
    fn open(&self, ui: &W, project: Option<&str>) {
        let Some(project) = project else { return };
        {
            let mut s = lock(&self.shared);
            s.project = project.to_owned();
            s.start_shown = None;
            s.end_shown = None;
        }
        ui.clear_form();
        refresh(ui, Field::Start, None);
        refresh(ui, Field::End, None);
        ui.show_dialog();
    }

    /// A day picked in one of the date pickers: the cell at `index` of its shown month becomes the date.
    fn pick(&self, field: Field, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let shown = shown_of(&lock(&self.shared), field);
        let (year, month) = month_of(&ui, field, shown);
        let cell = usize::try_from(index)
            .ok()
            .and_then(|i| calendar::month_grid(year, month, 0).get(i).copied());
        let Some(cell) = cell.filter(|cell| cell.in_month) else {
            return;
        };
        ui.show_date(field, iso(cell.date).as_str());
        set_shown(&mut lock(&self.shared), field, Some((year, month)));
        refresh(&ui, field, Some((year, month)));
    }

    /// A date picker's shown month moves by `step` months.
    fn step(&self, field: Field, step: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let shown = shown_of(&lock(&self.shared), field);
        let (year, month) = month_of(&ui, field, shown);
        let Some(first) = Date::new(year, month, 1) else {
            return;
        };
        let moved = first.add_months(step);
        let next = (moved.year, moved.month);
        set_shown(&mut lock(&self.shared), field, Some(next));
        refresh(&ui, field, Some(next));
    }

    /// Creates the sprint from the form. A refused form shows its message and sends nothing.
    fn create(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.creating() {
            return;
        }
        let form = ui.form();
        let body = match create_body(&form.name, &form.goal, &form.start, &form.end) {
            Ok(body) => body,
            Err(message) => {
                ui.show_error(&message);
                return;
            }
        };
        let project = lock(&self.shared).project.clone();
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        ui.show_creating(true);
        ui.show_error("");
        source.fetch(
            &self.ui,
            move |api, token| async move { api.create_sprint(&token, &project, &body).await },
            move |ui, result: Result<Sprint, ClientError>| match result {
                Ok(_) => {
                    ui.show_creating(false);
                    ui.close_dialog();
                }
                Err(error) => {
                    ui.show_creating(false);
                    ui.show_error(&page_data::error_text(&error));
                }
            },
        );
    }
}

fn shown_of(s: &Shared, field: Field) -> Option<(i32, u32)> {
    match field {
        Field::Start => s.start_shown,
        Field::End => s.end_shown,
    }
}

fn set_shown(s: &mut Shared, field: Field, shown: Option<(i32, u32)>) {
    match field {
        Field::Start => s.start_shown = shown,
        Field::End => s.end_shown = shown,
    }
}

/// The month a date picker shows: the month it was moved to, else the chosen date's, else this month.
fn month_of<W: NewSprintWindow>(ui: &W, field: Field, shown: Option<(i32, u32)>) -> (i32, u32) {
    let text = ui.date(field);
    shown
        .or_else(|| parse_iso(text.as_str()).map(|date| (date.year, date.month)))
        .unwrap_or_else(|| {
            let now = today();
            (now.year, now.month)
        })
}

/// Fills a date picker's grid, weekday labels and title for the month it shows.
fn refresh<W: NewSprintWindow>(ui: &W, field: Field, shown: Option<(i32, u32)>) {
    let weekdays: Vec<String> = (0..7)
        .map(|day| calendar::weekday_label(day).to_owned())
        .collect();
    ui.show_weekday_labels(weekdays);
    let chosen: Option<Date> = parse_iso(ui.date(field).as_str());
    let month = month_of(ui, field, shown);
    ui.show_days(field, cells(month, chosen), month_title(month));
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sprint_needs_a_name_and_two_dates() {
        assert!(create_body("", "", "2026-11-01", "2026-11-14").is_err());
        assert!(create_body("Sprint 1", "", "", "2026-11-14").is_err());
        let body = create_body(" Sprint 1 ", " ", "2026-11-01", "2026-11-14").expect("valid");
        assert_eq!(body.name, "Sprint 1");
        assert_eq!(body.goal, None, "a blank goal is left out");
    }
}
