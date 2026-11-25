//! Projects > New issue (S1.5c): the New issue dialog (`ui/new-issue.slint`), opened by the issues page's New issue button and
//! by the quick action's New issue entry.
//!
//! The dialog's project is the one on the route (`/p/:key/...`) when the person may create issues there, otherwise the first
//! project they may create in. The project's workflow, members and sprints fill the State, Assignee and Sprint choices; the
//! State starts at the first workflow state. Create sends the issue, then uploads the chosen files one by one, then opens the
//! new issue. A file that fails to upload keeps the dialog open, and the next Create attaches the rest without making a
//! second issue. Cancel discards the form; the dialog is the only create form.
//!
//! Plain Rust. The window side is [`NewIssueWindow`], which `wire_projects!` implements for the generated window.

use std::sync::{Arc, Mutex, MutexGuard};

use slint::{ComponentHandle, Image, Weak};
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_proto::projects::{
    ATTACHMENT_MAX_BYTES, Area, CreateIssue, DESCRIPTION_MAX, ISSUE_TYPES, Issue, PRIORITIES,
    Project, SUMMARY_MAX, Sprint, UpdateIssue,
};

use super::issue::{attachment_type, is_date};
use super::issues::issue_route;
use space_page_kit::calendar::{self, Date};
use space_page_kit::dates::{CalendarCell, cells, iso, month_title, parse_iso, today};
use space_page_kit::page_data::{self, DataCell};
use space_page_kit::person::initials;
use space_page_kit::picker::{Choice, choice};
use space_page_kit::projects;

/// The form's values as the dialog holds them. [`draft_body`] checks them and makes the create body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub summary: String,
    pub description: String,
    pub issue_type: String,
    pub state: String,
    pub priority: String,
    pub assignee: String,
    pub sprint: String,
    pub due_date: String,
    /// The estimate in points, as typed (a whole number, or empty for none).
    pub estimate: String,
    pub tags: String,
}

/// The create body for a draft. The summary must not be blank; an empty optional field is left out (the server's default
/// applies). The due date must be `YYYY-MM-DD`, the estimate a whole number, the tags comma separated.
pub fn draft_body(draft: &Draft) -> Result<CreateIssue, String> {
    let summary = draft.summary.trim();
    if summary.is_empty() {
        return Err("Enter a title.".to_owned());
    }
    if summary.chars().count() > SUMMARY_MAX {
        return Err(format!(
            "The title is longer than {SUMMARY_MAX} characters."
        ));
    }
    if draft.description.trim().chars().count() > DESCRIPTION_MAX {
        return Err(format!(
            "The description is longer than {DESCRIPTION_MAX} characters."
        ));
    }
    let due = draft.due_date.trim();
    let due_date = if due.is_empty() {
        None
    } else if is_date(due) {
        Some(due.to_owned())
    } else {
        return Err("Due date must be YYYY-MM-DD.".to_owned());
    };
    let estimate = draft.estimate.trim();
    let estimate_points = if estimate.is_empty() {
        None
    } else {
        Some(
            estimate
                .parse::<u32>()
                .map_err(|_| "The estimate is a whole number of points.".to_owned())?,
        )
    };
    let tags: Vec<String> = tags_of(&draft.tags);
    Ok(CreateIssue {
        summary: summary.to_owned(),
        description: Some(draft.description.trim().to_owned()).filter(|text| !text.is_empty()),
        issue_type: Some(draft.issue_type.clone()),
        priority: Some(draft.priority.clone()),
        state_id: Some(draft.state.clone()).filter(|id| !id.is_empty()),
        assignee_id: Some(draft.assignee.clone()).filter(|id| !id.is_empty()),
        parent_id: None,
        sprint_id: Some(draft.sprint.clone()).filter(|id| !id.is_empty()),
        estimate_points,
        due_date,
        tags: Some(tags).filter(|tags| !tags.is_empty()),
    })
}

/// The body of an edit: the fields the form changed from `original`, and nothing else. `Ok(None)` when nothing changed. The
/// same checks as the create apply. An empty assignee, sprint, estimate or due date clears that field on the server.
pub fn update_body(original: &Draft, now: &Draft) -> Result<Option<UpdateIssue>, String> {
    let checked = draft_body(now)?;
    let mut body = UpdateIssue::default();
    if now.summary.trim() != original.summary.trim() {
        body.summary = Some(checked.summary.clone());
    }
    if now.description.trim() != original.description.trim() {
        body.description = Some(checked.description.clone().unwrap_or_default());
    }
    if now.issue_type != original.issue_type {
        body.issue_type = Some(now.issue_type.clone());
    }
    if now.priority != original.priority {
        body.priority = Some(now.priority.clone());
    }
    if now.state != original.state && !now.state.is_empty() {
        body.state_id = Some(now.state.clone());
    }
    if now.assignee != original.assignee {
        body.assignee_id = Some(now.assignee.clone());
    }
    if now.sprint != original.sprint {
        body.sprint_id = Some(now.sprint.clone());
    }
    if now.due_date.trim() != original.due_date.trim() {
        body.due_date = Some(checked.due_date.clone().unwrap_or_default());
    }
    if now.estimate.trim() != original.estimate.trim() {
        body.estimate_points = Some(checked.estimate_points);
    }
    if tags_of(&now.tags) != tags_of(&original.tags) {
        body.tags = Some(tags_of(&now.tags));
    }
    Ok((body != UpdateIssue::default()).then_some(body))
}

/// The areas in a comma separated list, trimmed, without the empty ones.
fn tags_of(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What a create sent and got back. `uploaded` files went up in order; `failure` names the file that did not, with why.
#[derive(Debug)]
pub struct Outcome {
    pub key: String,
    pub uploaded: usize,
    pub failure: Option<(String, ClientError)>,
}

/// One row of the Project select: its value and label, and its picture (or, with none, the initials as the badge).
#[derive(Clone)]
pub struct ProjectOption {
    pub choice: Choice,
    pub icon: Option<Image>,
    pub badge: String,
}

/// One row of the areas picker: the area's name (value and label), its badge letter and whether the form has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaOption {
    pub value: String,
    pub label: String,
    pub badge: String,
    pub checked: bool,
}

/// The state shared by the dialog's handlers and the workers' results.
#[derive(Default)]
struct Shared {
    /// The project the form shows the choices of.
    project: String,
    /// Bumped by each project load. A late answer for an older load is dropped.
    details_gen: u64,
    /// The month the due date picker shows, once the person has moved it (else the chosen date's, else this month).
    due_shown: Option<(i32, u32)>,
    /// The areas the picker offers (the server's list, by name), and whether this person may add one.
    areas: Vec<Area>,
    /// The files to attach on create, with their bytes, in the order the person chose them.
    files: Vec<(String, Vec<u8>)>,
    /// The key of an issue created by a Create whose files did not all upload. The next Create does not make another one.
    created: Option<String>,
    /// The body of the last create the page started (read by the tests).
    last_body: Option<CreateIssue>,
    /// The issue the dialog edits, when it is in edit mode: its key and the form as it was when the edit opened.
    editing: Option<Edit>,
    /// The body of the last save the edit started (read by the tests).
    last_update: Option<UpdateIssue>,
}

/// The issue an edit is for, and the form as it was when the edit opened (what Save compares against).
#[derive(Debug, Clone)]
struct Edit {
    key: String,
    original: Draft,
}

/// The window side of the New issue dialog. `wire_projects!` implements it for the generated window.
pub trait NewIssueWindow: ComponentHandle + 'static {
    /// The route the window is on.
    fn route(&self) -> String;
    /// The form as the person has it now.
    fn draft(&self) -> Draft;
    /// The project the Project select shows (empty for none).
    fn project(&self) -> String;
    /// Whether a create or save is in flight.
    fn creating(&self) -> bool;
    fn show_creating(&self, on: bool);
    fn show_can_create(&self, on: bool);
    fn show_error(&self, text: &str);
    fn show_edit_mode(&self, on: bool);
    fn show_project(&self, key: &str);
    fn show_project_options(&self, options: Vec<ProjectOption>);
    fn show_type_options(&self, options: Vec<Choice>);
    fn show_priority_options(&self, options: Vec<Choice>);
    fn show_state_options(&self, options: Vec<Choice>);
    fn show_assignee_options(&self, options: Vec<Choice>);
    fn show_sprint_options(&self, options: Vec<Choice>);
    fn show_area_options(&self, options: Vec<AreaOption>);
    /// Sets the comma separated areas the form sends, and the picker trigger's summary.
    fn show_tags(&self, text: &str);
    fn show_summary(&self, text: &str);
    fn show_description(&self, text: &str);
    fn show_type(&self, text: &str);
    fn show_priority(&self, text: &str);
    fn show_state(&self, text: &str);
    fn show_assignee(&self, text: &str);
    fn show_sprint(&self, text: &str);
    fn show_due_date(&self, text: &str);
    fn show_estimate(&self, text: &str);
    /// The names of the files to attach, in order.
    fn show_files(&self, names: Vec<String>);
    fn show_weekday_labels(&self, labels: Vec<String>);
    /// The 42 cells of the due date picker's month and its title.
    fn show_due_days(&self, cells: Vec<CalendarCell>, title: String);
    /// Opens the dialog.
    fn show_dialog(&self);
    /// Closes the dialog after a create or a save.
    fn close_dialog(&self);
    /// Reloads the issue page after an edit is saved (its Try again).
    fn retry_issue(&self);
    /// Opens the issue page at `route` (the app shell's issues-open).
    fn open_issues(&self, route: String);
    fn on_requested(&self, f: impl Fn() + 'static);
    fn on_project_picked(&self, f: impl Fn(String) + 'static);
    fn on_summary_edited(&self, f: impl Fn() + 'static);
    fn on_attach(&self, f: impl Fn() + 'static);
    fn on_remove_file(&self, f: impl Fn(i32) + 'static);
    fn on_create(&self, f: impl Fn() + 'static);
    fn on_due_picked(&self, f: impl Fn(i32) + 'static);
    fn on_due_month_step(&self, f: impl Fn(i32) + 'static);
    fn on_area_toggled(&self, f: impl Fn(String) + 'static);
}

/// The New issue dialog: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct NewIssue<W: NewIssueWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: NewIssueWindow> Clone for NewIssue<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the dialog's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, requests do nothing.
pub fn wire<W: NewIssueWindow>(ui: &W, data: DataCell) -> NewIssue<W> {
    let page = NewIssue {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };
    ui.show_type_options(type_options());
    ui.show_priority_options(priority_options());
    ui.show_type("Task");
    ui.show_priority("Normal");

    // The issues page's New issue button and the quick action both open the dialog for the route they are on.
    let open = page.clone();
    ui.on_requested(move || {
        let Some(ui) = open.ui.upgrade() else { return };
        let route = ui.route();
        open.open(projects::project_of_route(&route));
    });

    let pick = page.clone();
    ui.on_project_picked(move |key| pick.pick_project(&key));

    let edited = page.clone();
    ui.on_summary_edited(move || edited.summary_edited());

    let attach = page.clone();
    ui.on_attach(move || attach.attach());

    let remove = page.clone();
    ui.on_remove_file(move |index| remove.remove_file(index));

    let create = page.clone();
    ui.on_create(move || create.create());

    let due = page.clone();
    ui.on_due_picked(move |index| due.pick_due(index));

    let due_step = page.clone();
    ui.on_due_month_step(move |step| due_step.step_due(step));

    let area_toggle = page.clone();
    ui.on_area_toggled(move |value| area_toggle.toggle_area(&value));

    page
}

/// A project in the Project select: its picture when it has one, else its initials in a square.
fn project_option(project: &projects::Project) -> ProjectOption {
    ProjectOption {
        choice: choice(&project.key, &project.name),
        icon: projects::picture(&project.key),
        badge: if projects::picture(&project.key).is_some() {
            String::new()
        } else {
            initials(&project.name)
        },
    }
}

/// The priority choices: the stored values, as the server sends them, labelled the same.
fn priority_options() -> Vec<Choice> {
    PRIORITIES
        .iter()
        .map(|value| choice(value, value))
        .collect()
}

/// Whether Create is enabled: a summary that is not blank, a project, and no create in progress.
fn refresh_can_create<W: NewIssueWindow>(ui: &W) {
    let draft = ui.draft();
    let can = !draft.summary.trim().is_empty() && !ui.project().is_empty() && !ui.creating();
    ui.show_can_create(can);
}

impl<W: NewIssueWindow> NewIssue<W> {
    /// Opens the dialog. `preset` is the project of the route: it is chosen when the person may create issues there;
    /// otherwise the first project they may create in is. A draft kept by "View in full page" is shown again.
    pub fn open(&self, preset: Option<&str>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let editable = projects::editable();
        let rows: Vec<ProjectOption> = editable.iter().map(project_option).collect();
        ui.show_project_options(rows);

        reset_form(&ui);
        lock(&self.shared).files.clear();
        lock(&self.shared).due_shown = None;
        lock(&self.shared).editing = None;
        ui.show_edit_mode(false);
        refresh_due(&ui, None);
        refresh_areas(&ui, &lock(&self.shared).areas);
        self.load_areas();
        let chosen = preset
            .filter(|key| editable.iter().any(|project| project.key == *key))
            .map(str::to_owned)
            .or_else(|| editable.first().map(|project| project.key.clone()));
        match chosen {
            Some(key) => {
                ui.show_project(&key);
                self.load_project(&ui, &key);
                ui.show_error("");
            }
            None => {
                ui.show_project("");
                ui.show_error("You cannot create issues in any project yet.");
            }
        }
        ui.show_files(names(&lock(&self.shared).files));
        refresh_can_create(&ui);
        ui.show_dialog();
    }

    /// The project picked in the dialog (the Project select). A pick of the project already shown changes nothing.
    fn pick_project(&self, key: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if key.is_empty() || lock(&self.shared).project == key {
            return;
        }
        self.load_project(&ui, key);
    }

    /// Makes `key` the project the form shows, clears the choices that belong to the last one, and asks for its workflow,
    /// members and sprints.
    fn load_project(&self, ui: &W, key: &str) {
        let generation = {
            let mut s = lock(&self.shared);
            s.project = key.to_owned();
            s.details_gen += 1;
            s.details_gen
        };
        ui.show_state_options(Vec::new());
        ui.show_state("");
        ui.show_assignee_options(vec![choice("", "Unassigned")]);
        ui.show_assignee("");
        ui.show_sprint_options(vec![choice("", "No sprint")]);
        ui.show_sprint("");
        refresh_can_create(ui);

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        let project = key.to_owned();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let loaded = api.project(&token, &project).await?;
                let sprints = api.sprints(&token, &project).await?;
                Ok((loaded, sprints))
            },
            move |ui, result| deliver_details(ui, &shared, generation, result),
        );
    }

    /// The summary changed: Create is checked.
    fn summary_edited(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        refresh_can_create(&ui);
    }

    /// A day picked in the due date picker: the cell at `index` of the shown month becomes the due date.
    fn pick_due(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let shown = lock(&self.shared).due_shown;
        let (year, month) = due_month(&ui, shown);
        let cell = usize::try_from(index)
            .ok()
            .and_then(|i| calendar::month_grid(year, month, 0).get(i).copied());
        let Some(cell) = cell.filter(|cell| cell.in_month) else {
            return;
        };
        ui.show_due_date(iso(cell.date).as_str());
        lock(&self.shared).due_shown = Some((year, month));
        refresh_due(&ui, Some((year, month)));
    }

    /// The due date picker's shown month moves by `step` months.
    fn step_due(&self, step: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        let shown = lock(&self.shared).due_shown;
        let (year, month) = due_month(&ui, shown);
        let Some(first) = Date::new(year, month, 1) else {
            return;
        };
        let moved = first.add_months(step);
        let next = (moved.year, moved.month);
        lock(&self.shared).due_shown = Some(next);
        refresh_due(&ui, Some(next));
    }

    /// Loads the areas the form offers. A failed load shows its message in the dialog.
    fn load_areas(&self) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            |api, token| async move {
                let areas = api.areas(&token).await?;
                Ok(areas)
            },
            move |ui, result: Result<Vec<Area>, ClientError>| match result {
                Ok(areas) => {
                    {
                        let mut s = lock(&shared);
                        s.areas = areas;
                    }
                    refresh_areas(ui, &lock(&shared).areas);
                }
                Err(error) => ui.show_error(&page_data::error_text(&error)),
            },
        );
    }

    /// An area clicked in the picker: it is chosen, or taken off when it was chosen.
    fn toggle_area(&self, name: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut tags = tags_of(&ui.draft().tags);
        match tags.iter().position(|tag| tag == name) {
            Some(index) => {
                tags.remove(index);
            }
            None => tags.push(name.to_owned()),
        }
        set_tags(&ui, &tags);
        refresh_areas(&ui, &lock(&self.shared).areas);
    }

    /// Chooses files with the native dialog. They are held until Create; a file over the limit is refused.
    fn attach(&self) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            |_api, _token| async move { Ok(pick_files().await) },
            move |ui, picked: Result<Vec<(String, Vec<u8>)>, ClientError>| {
                add_files(ui, &shared, picked.unwrap_or_default());
            },
        );
    }

    fn remove_file(&self, index: i32) {
        let Some(ui) = self.ui.upgrade() else { return };
        {
            let mut s = lock(&self.shared);
            if let Ok(index) = usize::try_from(index)
                && index < s.files.len()
            {
                s.files.remove(index);
            }
            ui.show_files(names(&s.files));
        }
    }

    /// Opens the dialog in edit mode for `issue`: the same form, filled from the issue, with Save instead of Create. The
    /// project's workflow, members and sprints (already loaded by the issue page) fill the choices.
    pub fn open_edit(&self, issue: &Issue, project: &Project, sprints: &[Sprint]) {
        let Some(ui) = self.ui.upgrade() else { return };
        let original = Draft {
            summary: issue.summary.clone(),
            description: issue.description.clone(),
            issue_type: issue.issue_type.clone(),
            state: issue.state.id.clone(),
            priority: issue.priority.clone(),
            assignee: issue
                .assignee
                .as_ref()
                .map(|user| user.id.clone())
                .unwrap_or_default(),
            sprint: issue
                .sprint
                .as_ref()
                .map(|sprint| sprint.id.clone())
                .unwrap_or_default(),
            due_date: issue.due_date.clone().unwrap_or_default(),
            estimate: issue
                .estimate_points
                .map(|points| points.to_string())
                .unwrap_or_default(),
            tags: issue.tags.join(", "),
        };
        {
            let mut s = lock(&self.shared);
            s.project = project.key.clone();
            s.details_gen += 1;
            s.files.clear();
            s.due_shown = None;
            s.created = None;
            s.last_update = None;
            s.editing = Some(Edit {
                key: issue.key.clone(),
                original: original.clone(),
            });
        }
        ui.show_edit_mode(true);
        ui.show_project_options(vec![ProjectOption {
            choice: choice(&project.key, &project.name),
            icon: projects::picture(&project.key),
            badge: String::new(),
        }]);
        ui.show_project(&project.key);
        ui.show_summary(&original.summary);
        ui.show_description(&original.description);
        ui.show_type(&original.issue_type);
        ui.show_priority(&original.priority);
        ui.show_due_date(&original.due_date);
        ui.show_estimate(&original.estimate);
        set_tags(&ui, &issue.tags);
        apply_details(&ui, project, sprints);
        // The choices are filled first, so the values the issue has are chosen after them.
        ui.show_state(&original.state);
        ui.show_assignee(&original.assignee);
        ui.show_sprint(&original.sprint);
        ui.show_files(names(&[]));
        ui.show_error("");
        refresh_due(&ui, None);
        refresh_areas(&ui, &lock(&self.shared).areas);
        self.load_areas();
        refresh_can_create(&ui);
        ui.show_dialog();
    }

    /// Saves the edit: sends the fields the form changed, then closes the dialog and reloads the issue page. Nothing is sent
    /// when nothing changed. A refused or failed save keeps the dialog open with its message.
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.creating() {
            return;
        }
        let Some(edit) = lock(&self.shared).editing.clone() else {
            return;
        };
        let body = match update_body(&edit.original, &ui.draft()) {
            Ok(Some(body)) => body,
            Ok(None) => {
                lock(&self.shared).editing = None;
                ui.close_dialog();
                return;
            }
            Err(message) => {
                ui.show_error(&message);
                return;
            }
        };
        lock(&self.shared).last_update = Some(body.clone());
        ui.show_creating(true);
        ui.show_error("");
        refresh_can_create(&ui);

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                api.update_issue(&token, &edit.key, &body).await.map(|_| ())
            },
            move |ui, result| finish_save(ui, &shared, result),
        );
    }

    /// Creates the issue from the form, uploads the chosen files and opens the issue. Nothing is sent for a blank summary.
    /// In edit mode the same button saves the edit instead.
    fn create(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if lock(&self.shared).editing.is_some() {
            self.save();
            return;
        }
        let draft = ui.draft();
        if ui.creating() || draft.summary.trim().is_empty() {
            return;
        }
        let project = lock(&self.shared).project.clone();
        if project.is_empty() {
            return;
        }
        let existing = lock(&self.shared).created.clone();
        let body = match draft_body(&draft) {
            Ok(body) => body,
            Err(message) => {
                ui.show_error(&message);
                return;
            }
        };
        if existing.is_none() {
            lock(&self.shared).last_body = Some(body.clone());
        }
        ui.show_creating(true);
        ui.show_error("");
        refresh_can_create(&ui);

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let files = lock(&self.shared).files.clone();
        let shared = self.shared.clone();
        let done_project = project.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                run_create(&api, &token, &project, existing, &body, files).await
            },
            move |ui, result| finish_create(ui, &shared, &done_project, result),
        );
    }

    /// Delivers the project's workflow, members and sprints, as the worker would. For tests, through the same path.
    pub fn deliver_details(&self, project: &Project, sprints: &[Sprint]) {
        let Some(ui) = self.ui.upgrade() else { return };
        apply_details(&ui, project, sprints);
        refresh_can_create(&ui);
    }

    /// Delivers the areas, as the worker would. For tests.
    pub fn deliver_areas(&self, areas: Vec<Area>) {
        let Some(ui) = self.ui.upgrade() else { return };
        lock(&self.shared).areas = areas;
        refresh_areas(&ui, &lock(&self.shared).areas);
    }

    /// Delivers a created issue with `key` with nothing to upload, as the worker would. For tests.
    pub fn deliver_created_key(&self, key: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let project = lock(&self.shared).project.clone();
        finish_create(
            &ui,
            &self.shared,
            &project,
            Ok(Outcome {
                key: key.to_owned(),
                uploaded: 0,
                failure: None,
            }),
        );
    }

    /// The body of the last create the page started, for the tests.
    pub fn last_body(&self) -> Option<CreateIssue> {
        lock(&self.shared).last_body.clone()
    }

    /// The body of the last edit save the page started, for the tests.
    pub fn last_update(&self) -> Option<UpdateIssue> {
        lock(&self.shared).last_update.clone()
    }
}

/// Clears the form to its defaults (a Task, Normal, nothing else chosen). The project and its choices are set by the caller.
fn reset_form<W: NewIssueWindow>(ui: &W) {
    ui.show_summary("");
    ui.show_description("");
    ui.show_type("Task");
    ui.show_priority("Normal");
    ui.show_due_date("");
    ui.show_estimate("");
    set_tags(ui, &[]);
}

/// Sets the chosen areas: the comma separated `tags` Create reads, and the trigger's summary.
fn set_tags<W: NewIssueWindow>(ui: &W, tags: &[String]) {
    ui.show_tags(&tags.join(", "));
}

/// The picker's rows: every area, checked when the form has it, with its first letter as the badge.
fn refresh_areas<W: NewIssueWindow>(ui: &W, areas: &[Area]) {
    let chosen = tags_of(&ui.draft().tags);
    let rows: Vec<AreaOption> = areas
        .iter()
        .map(|area| AreaOption {
            value: area.name.clone(),
            label: area.name.clone(),
            badge: area
                .name
                .chars()
                .next()
                .map(|letter| letter.to_uppercase().collect::<String>())
                .unwrap_or_default(),
            checked: chosen.contains(&area.name),
        })
        .collect();
    ui.show_area_options(rows);
}

/// The month the due date picker shows: the month the person moved it to, else the chosen date's, else this month.
fn due_month<W: NewIssueWindow>(ui: &W, shown: Option<(i32, u32)>) -> (i32, u32) {
    shown
        .or_else(|| parse_iso(ui.draft().due_date.as_str()).map(|date| (date.year, date.month)))
        .unwrap_or_else(|| {
            let now = today();
            (now.year, now.month)
        })
}

/// Fills the due date picker's grid, weekday labels and title for the month it shows.
fn refresh_due<W: NewIssueWindow>(ui: &W, shown: Option<(i32, u32)>) {
    let weekdays: Vec<String> = (0..7)
        .map(|day| calendar::weekday_label(day).to_owned())
        .collect();
    ui.show_weekday_labels(weekdays);
    let chosen: Option<Date> = parse_iso(ui.draft().due_date.as_str());
    let month = due_month(ui, shown);
    ui.show_due_days(cells(month, chosen), month_title(month));
}

/// Shows the project's choices when its answer is for the latest load. A failed load shows its message in the dialog.
fn deliver_details<W: NewIssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    result: Result<(Project, Vec<Sprint>), ClientError>,
) {
    if lock(shared).details_gen != generation {
        return;
    }
    match result {
        Ok((project, sprints)) => apply_details(ui, &project, &sprints),
        Err(error) => ui.show_error(&page_data::error_text(&error)),
    }
    refresh_can_create(ui);
}

/// Fills the State (the workflow, the first state chosen), Assignee (the members) and Sprint choices of a project.
fn apply_details<W: NewIssueWindow>(ui: &W, project: &Project, sprints: &[Sprint]) {
    let states: Vec<Choice> = project
        .workflow
        .iter()
        .map(|state| choice(&state.id, &state.name))
        .collect();
    let first = states.first().map(|state| state.value.clone());
    ui.show_state_options(states);
    if let Some(first) = first {
        ui.show_state(&first);
    }

    let mut assignees = vec![choice("", "Unassigned")];
    assignees.extend(
        project
            .members
            .iter()
            .map(|member| choice(&member.user.id, &member.user.display_name)),
    );
    ui.show_assignee_options(assignees);

    let mut sprint_rows = vec![choice("", "No sprint")];
    sprint_rows.extend(
        sprints
            .iter()
            .map(|sprint| choice(&sprint.id, &sprint.name)),
    );
    ui.show_sprint_options(sprint_rows);
}

/// The file names as the dialog lists them.
fn names(files: &[(String, Vec<u8>)]) -> Vec<String> {
    files.iter().map(|(name, _)| name.clone()).collect()
}

/// Keeps the files that arrived, skipping any over the limit (named in the dialog's message).
fn add_files<W: NewIssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    picked: Vec<(String, Vec<u8>)>,
) {
    let mut too_big = Vec::new();
    let mut s = lock(shared);
    for (name, bytes) in picked {
        if bytes.len() > ATTACHMENT_MAX_BYTES {
            too_big.push(name);
        } else {
            s.files.push((name, bytes));
        }
    }
    ui.show_files(names(&s.files));
    let message = match too_big.as_slice() {
        [] => String::new(),
        [one] => format!("{one} is larger than 10 MB."),
        many => format!("{} files are larger than 10 MB.", many.len()),
    };
    ui.show_error(&message);
}

/// Sends the create (unless a create already made the issue), then uploads the files in order. Runs on the worker.
async fn run_create(
    api: &ApiClient,
    token: &str,
    project: &str,
    existing: Option<String>,
    body: &CreateIssue,
    files: Vec<(String, Vec<u8>)>,
) -> Result<Outcome, ClientError> {
    let key = match existing {
        Some(key) => key,
        None => api.create_issue(token, project, body).await?.key,
    };
    let mut uploaded = 0;
    for (name, bytes) in files {
        let kind = attachment_type(&name);
        if let Err(error) = api.upload_attachment(token, &key, &name, kind, bytes).await {
            return Ok(Outcome {
                key,
                uploaded,
                failure: Some((name, error)),
            });
        }
        uploaded += 1;
    }
    Ok(Outcome {
        key,
        uploaded,
        failure: None,
    })
}

/// Shows the result of an edit save: an error keeps the dialog open with its message; a saved edit closes it and reloads the
/// issue page (the issue route's Try again, which loads the issue again).
fn finish_save<W: NewIssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    result: Result<(), ClientError>,
) {
    ui.show_creating(false);
    match result {
        Err(error) => ui.show_error(&page_data::error_text(&error)),
        Ok(()) => {
            lock(shared).editing = None;
            ui.close_dialog();
            ui.retry_issue();
        }
    }
    refresh_can_create(ui);
}

/// Shows the result of a create: an error keeps the form, a failed upload keeps it with the issue's key, and a clean
/// create closes the dialog and opens the issue.
fn finish_create<W: NewIssueWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    project: &str,
    result: Result<Outcome, ClientError>,
) {
    match result {
        Err(error) => {
            ui.show_creating(false);
            ui.show_error(&page_data::error_text(&error));
        }
        Ok(outcome) => {
            let mut s = lock(shared);
            let uploaded = outcome.uploaded.min(s.files.len());
            s.files.drain(..uploaded);
            ui.show_files(names(&s.files));
            match outcome.failure {
                Some((name, error)) => {
                    s.created = Some(outcome.key.clone());
                    drop(s);
                    ui.show_creating(false);
                    ui.show_error(&format!(
                        "{} was created, but {name} could not be attached: {}. Create again to attach the rest.",
                        outcome.key,
                        page_data::error_text(&error)
                    ));
                }
                None => {
                    s.created = None;
                    drop(s);
                    ui.show_creating(false);
                    ui.close_dialog();
                    ui.open_issues(issue_route(project, &outcome.key));
                }
            }
        }
    }
    refresh_can_create(ui);
}

/// The files the person chooses, with their bytes. The native dialog runs on the worker, as the issue page's picker does.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn pick_files() -> Vec<(String, Vec<u8>)> {
    let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("Attach files")
        .pick_files()
        .await
    else {
        return Vec::new();
    };
    let mut picked = Vec::new();
    for file in files {
        let name = file.file_name();
        let bytes = file.read().await;
        picked.push((name, bytes));
    }
    picked
}

/// The web and Android builds have no native file dialog here, so no files are chosen.
#[cfg(not(all(not(target_arch = "wasm32"), not(target_os = "android"))))]
async fn pick_files() -> Vec<(String, Vec<u8>)> {
    Vec::new()
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The issue type choices: the stored values, labelled the same.
fn type_options() -> Vec<Choice> {
    ISSUE_TYPES
        .iter()
        .map(|value| choice(value, value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(summary: &str) -> Draft {
        Draft {
            summary: summary.to_owned(),
            issue_type: "Task".to_owned(),
            priority: "Normal".to_owned(),
            ..Draft::default()
        }
    }

    #[test]
    fn a_blank_summary_makes_no_body() {
        assert!(draft_body(&draft("   ")).is_err());
    }

    #[test]
    fn the_body_trims_the_summary_and_leaves_empty_fields_out() {
        let body = draft_body(&draft("  Crash on load ")).expect("valid");
        assert_eq!(body.summary, "Crash on load");
        assert_eq!(body.issue_type.as_deref(), Some("Task"));
        assert_eq!(body.priority.as_deref(), Some("Normal"));
        assert_eq!(body.state_id, None);
        assert_eq!(body.assignee_id, None);
        assert_eq!(body.sprint_id, None);
        assert_eq!(body.description, None);
        assert_eq!(body.due_date, None);
        assert_eq!(body.tags, None);
    }

    #[test]
    fn the_optional_fields_are_checked_and_sent() {
        let mut form = draft("Ship it");
        form.state = "s-2".to_owned();
        form.assignee = "u-1".to_owned();
        form.sprint = "sp-3".to_owned();
        form.description = " Steps below ".to_owned();
        form.due_date = "2026-11-30".to_owned();
        form.tags = "ui, , backend".to_owned();
        let body = draft_body(&form).expect("valid");
        assert_eq!(body.state_id.as_deref(), Some("s-2"));
        assert_eq!(body.assignee_id.as_deref(), Some("u-1"));
        assert_eq!(body.sprint_id.as_deref(), Some("sp-3"));
        assert_eq!(body.description.as_deref(), Some("Steps below"));
        assert_eq!(body.due_date.as_deref(), Some("2026-11-30"));
        assert_eq!(body.tags, Some(vec!["ui".to_owned(), "backend".to_owned()]));

        form.due_date = "30/11/2026".to_owned();
        assert!(draft_body(&form).is_err(), "due date is YYYY-MM-DD");
    }

    #[test]
    fn the_route_gives_its_project_only_for_project_routes() {
        assert_eq!(projects::project_of_route("/p/EMB/issues"), Some("EMB"));
        assert_eq!(
            projects::project_of_route("/p/EMB/issues/EMB-3"),
            Some("EMB")
        );
        assert_eq!(projects::project_of_route("/p/"), None);
        assert_eq!(projects::project_of_route("/home"), None);
    }

    #[test]
    fn an_estimate_is_a_whole_number_or_empty() {
        let mut form = draft("Ship it");
        form.estimate = " 5 ".to_owned();
        assert_eq!(draft_body(&form).expect("valid").estimate_points, Some(5));
        form.estimate = "many".to_owned();
        assert!(draft_body(&form).is_err(), "the estimate is a whole number");
    }

    #[test]
    fn an_edit_sends_only_the_fields_that_changed() {
        let original = Draft {
            estimate: "3".to_owned(),
            tags: "UI".to_owned(),
            ..draft("Crash on load")
        };
        assert_eq!(update_body(&original, &original), Ok(None), "no change");

        let mut now = original.clone();
        now.summary = " Crash on start ".to_owned();
        now.estimate = String::new();
        now.tags = "UI, Code".to_owned();
        assert_eq!(
            update_body(&original, &now),
            Ok(Some(UpdateIssue {
                summary: Some("Crash on start".to_owned()),
                estimate_points: Some(None),
                tags: Some(vec!["UI".to_owned(), "Code".to_owned()]),
                ..UpdateIssue::default()
            }))
        );
    }

    #[test]
    fn an_edit_clears_an_assignee_or_a_due_date_when_emptied() {
        let original = Draft {
            assignee: "u-1".to_owned(),
            due_date: "2026-11-30".to_owned(),
            ..draft("Ship it")
        };
        let mut now = original.clone();
        now.assignee = String::new();
        now.due_date = String::new();
        let body = update_body(&original, &now)
            .expect("valid")
            .expect("changed");
        assert_eq!(body.assignee_id.as_deref(), Some(""));
        assert_eq!(body.due_date.as_deref(), Some(""));
        assert_eq!(body.summary, None, "an unchanged title is not sent");
    }

    #[test]
    fn an_edit_checks_the_fields_it_sends() {
        let original = draft("Ship it");
        let mut now = original.clone();
        now.summary = "   ".to_owned();
        assert!(update_body(&original, &now).is_err(), "a blank title");
        now.summary = "Ship it".to_owned();
        now.due_date = "30/11/2026".to_owned();
        assert!(update_body(&original, &now).is_err(), "a bad due date");
    }
}
