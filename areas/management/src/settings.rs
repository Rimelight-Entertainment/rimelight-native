//! Project settings (`ui/settings.slint`). The route's project is loaded with the viewer's role. A project admin (or a global
//! admin) can change the name, key and description, and the icon; everyone else sees the values read-only. The key is the
//! project's short id, so a change of it moves the page to the new key's route.
//!
//! Plain Rust. The window side is [`SettingsWindow`], which `wire_management!` implements for the generated window.

use std::sync::{Arc, Mutex};

use slint::{Image, Weak};
use space_client::api::ApiClient;
use space_client::error::ClientError;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use space_proto::people::AVATAR_MAX_BYTES;
use space_proto::projects::{Project, UpdateProject};

use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use space_page_kit::person::content_type;
use space_page_kit::person::{initials, is_admin};
use space_page_kit::projects::{self, project_of_route};

/// What the page loads: the project, and whether the caller may change it.
type Loaded = (Project, bool);

/// What the icon picker's work reports: nothing chosen, a file that was refused (with why), or a picture that was uploaded.
enum PickOutcome {
    Cancelled,
    Rejected(String),
    Uploaded,
}

/// The form's fields as the user has them (raw; the page trims them).
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub name: String,
    pub key: String,
    pub description: String,
}

/// The loaded project as the window draws it.
#[derive(Clone, Default)]
pub struct SettingsView {
    pub name: String,
    pub key: String,
    pub description: String,
    pub can_edit: bool,
    pub initials: String,
    pub has_image: bool,
    pub image: Image,
}

/// The window side of the settings page. `wire_management!` implements it for the generated window.
pub trait SettingsWindow: slint::ComponentHandle + 'static {
    /// The route the window is on.
    fn route(&self) -> String;
    /// Opens the Issues page of a route (the window's navigation, for a changed key).
    fn open_issues(&self, route: String);
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_action_error(&self, text: &str);
    fn show_saving(&self, on: bool);
    fn saving(&self) -> bool;
    fn can_edit(&self) -> bool;
    fn form(&self) -> Form;
    fn show_project(&self, view: SettingsView);
    /// Asks the page to read the project again (its own `retry` callback).
    fn retry_self(&self);
    fn on_opened(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn() + 'static);
    fn on_pick_icon(&self, f: impl Fn() + 'static);
}

/// Wires the page's callbacks on the window. `data` is the slot the session driver fills.
pub fn wire<W: SettingsWindow>(ui: &W, data: DataCell) {
    let page = Page {
        ui: ui.as_weak(),
        data,
        loaded: Arc::new(Mutex::new(None)),
    };

    let target = page.clone();
    ui.on_opened(move || target.load());

    let target = page.clone();
    ui.on_retry(move || target.load());

    let target = page.clone();
    ui.on_save(move || target.save());

    let target = page.clone();
    ui.on_pick_icon(move || target.pick_icon());
}

struct Page<W: SettingsWindow> {
    ui: Weak<W>,
    data: DataCell,
    /// The project as last loaded. Save compares the form with it and names only what changed.
    loaded: Arc<Mutex<Option<Project>>>,
}

impl<W: SettingsWindow> Clone for Page<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            loaded: self.loaded.clone(),
        }
    }
}

impl<W: SettingsWindow> Page<W> {
    fn source(&self) -> Option<Arc<DataSource>> {
        self.data.borrow().clone()
    }

    /// Loads the project of the route and whether the caller may edit it. A route without a project shows an error.
    fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.show_state(LoadState::Loading);
        ui.show_error("");
        ui.show_action_error("");
        let route = ui.route();
        let Some(key) = project_of_route(&route).map(str::to_owned) else {
            ui.show_error("This page needs a project in its address.");
            ui.show_state(LoadState::Error);
            return;
        };
        let Some(source) = self.source() else {
            return;
        };
        let loaded = self.loaded.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let project = api.project(&token, &key).await?;
                let me = api.my_profile(&token).await?;
                let can_edit = is_admin(&me) || project.my_role.as_deref() == Some("admin");
                Ok::<Loaded, ClientError>((project, can_edit))
            },
            move |ui, result: Result<Loaded, ClientError>| deliver_load(ui, &loaded, result),
        );
    }

    /// Sends the fields that changed. A blank name or key, or no change, sends nothing.
    fn save(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !ui.can_edit() || ui.saving() {
            return;
        }
        let Some(old) = lock(&self.loaded).clone() else {
            return;
        };
        let form = ui.form();
        let name = form.name.trim().to_owned();
        let key = form.key.trim().to_owned();
        let description = form.description.trim().to_owned();
        if name.is_empty() {
            ui.show_action_error("Enter a name for the project.");
            return;
        }
        if key.is_empty() {
            ui.show_action_error("Enter a key for the project.");
            return;
        }
        let body = UpdateProject {
            name: (name.as_str() != old.name.trim()).then(|| name.clone()),
            key: (key.as_str() != old.key.trim()).then(|| key.clone()),
            description: (description.as_str() != old.description.trim())
                .then(|| description.clone()),
            ..UpdateProject::default()
        };
        if body.name.is_none() && body.key.is_none() && body.description.is_none() {
            return;
        }
        let Some(source) = self.source() else {
            return;
        };
        ui.show_action_error("");
        ui.show_saving(true);
        let previous = old.key.clone();
        let previous_done = previous.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.update_project(&token, &previous, &body).await },
            move |ui, result: Result<Project, ClientError>| {
                deliver_save(ui, &previous_done, result);
            },
        );
    }

    /// Chooses one picture with the native dialog and uploads it as the project's icon. A refused file keeps the page as it was.
    fn pick_icon(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        if !ui.can_edit() || ui.saving() {
            return;
        }
        let Some(old) = lock(&self.loaded).clone() else {
            return;
        };
        let Some(source) = self.source() else {
            return;
        };
        ui.show_action_error("");
        ui.show_saving(true);
        let key = old.key.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { upload_icon(&api, &token, &key).await },
            move |ui, result: Result<PickOutcome, ClientError>| deliver_icon(ui, result),
        );
    }
}

/// Shows the loaded page, or the error state.
fn deliver_load<W: SettingsWindow>(
    ui: &W,
    loaded: &Mutex<Option<Project>>,
    result: Result<Loaded, ClientError>,
) {
    match result {
        Ok((project, can_edit)) => {
            let picture = projects::picture(&project.key);
            ui.show_project(SettingsView {
                name: project.name.clone(),
                key: project.key.clone(),
                description: project.description.clone(),
                can_edit,
                initials: initials(&project.name),
                has_image: project.image_url.is_some(),
                image: picture.unwrap_or_default(),
            });
            *lock(loaded) = Some(project);
            ui.show_state(LoadState::Loaded);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// A saved change: a new key moves the page to the new route, otherwise the project is read again.
fn deliver_save<W: SettingsWindow>(
    ui: &W,
    previous_key: &str,
    result: Result<Project, ClientError>,
) {
    ui.show_saving(false);
    match result {
        Ok(project) => {
            if project.key != previous_key {
                ui.open_issues(format!("/p/{}/settings", project.key));
            } else {
                ui.retry_self();
            }
        }
        Err(error) => ui.show_action_error(&page_data::error_text(&error)),
    }
}

/// The icon picker's answer: a picture uploaded reads the project again, a refused file or an error shows its message.
fn deliver_icon<W: SettingsWindow>(ui: &W, result: Result<PickOutcome, ClientError>) {
    ui.show_saving(false);
    match result {
        Ok(PickOutcome::Uploaded) => ui.retry_self(),
        Ok(PickOutcome::Cancelled) => {}
        Ok(PickOutcome::Rejected(message)) => ui.show_action_error(&message),
        Err(error) => ui.show_action_error(&page_data::error_text(&error)),
    }
}

/// Picks the file, then uploads it for the project with `key`. Runs on the worker.
async fn upload_icon(api: &ApiClient, token: &str, key: &str) -> Result<PickOutcome, ClientError> {
    match pick_icon_file().await {
        Ok(None) => Ok(PickOutcome::Cancelled),
        Err(message) => Ok(PickOutcome::Rejected(message)),
        Ok(Some((kind, bytes))) => {
            api.upload_project_image(token, key, bytes, kind).await?;
            Ok(PickOutcome::Uploaded)
        }
    }
}

/// The native picker for one picture (PNG, JPEG or WebP, at most 5 MB). `Ok(None)` when the dialog is closed without a file;
/// `Err` with the reason when the file is refused.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn pick_icon_file() -> Result<Option<(&'static str, Vec<u8>)>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Choose a picture")
        .add_filter("Pictures", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let Some(kind) = content_type(&file.file_name()) else {
        return Err("Choose a PNG, JPEG or WebP picture.".to_owned());
    };
    let bytes = file.read().await;
    if bytes.len() > AVATAR_MAX_BYTES {
        return Err("The picture is larger than 5 MB.".to_owned());
    }
    Ok(Some((kind, bytes)))
}

/// The web and Android builds have no native file dialog here, so no picture is chosen.
#[cfg(not(all(not(target_arch = "wasm32"), not(target_os = "android"))))]
async fn pick_icon_file() -> Result<Option<(&'static str, Vec<u8>)>, String> {
    Ok(None)
}
