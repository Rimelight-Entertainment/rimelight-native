//! Profile (SP.3c): one person's profile at `/company/employees/{id}`, and the signed-in person's own profile (the avatar menu's
//! Profile row opens it). Work fields, personal fields when the server returned them, the reporting line and the direct
//! reports. Own profile: Edit (self-editable fields, PATCH `/v1/me/profile`) and Change photo (native desktop only, `rfd`).
//!
//! The page data pattern (`space_page_kit::page_data`). The page's markup is `ui/profile.slint`. One profile is held at a time:
//! a second profile open in another tab replaces it. This module is plain Rust. The window side is [`ProfileWindow`], which
//! `wire_company!` implements for the generated window.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{Image, Timer, TimerMode, Weak};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{self, DataCell, DataSource, Pictures};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use space_page_kit::person::content_type;
use space_page_kit::person::initials;
use space_proto::people::{OrgNode, PersonProfile, PersonSummary, UpdateMyProfile};

/// Whether this build can pick a photo from a native file dialog (desktop only; web and Android hide the button).
pub const PHOTO_NATIVE: bool = cfg!(all(not(target_arch = "wasm32"), not(target_os = "android")));

/// The route prefix of a profile page (`/company/employees/{id}`).
const PREFIX: &str = "/company/employees";

/// The person id of a profile route (`/company/employees/{id}`), or `None` for any other route. The id is a plain UUID-like
/// segment, so it can be used in a request.
pub fn id_of(route: &str) -> Option<&str> {
    let id = route.strip_prefix(PREFIX)?.strip_prefix('/')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')).then_some(id)
}

/// A loaded profile: the person, their reporting line, and whether it is the signed-in person's own.
struct Loaded {
    profile: PersonProfile,
    org: OrgNode,
    own: bool,
}

/// The result of a photo pick and upload, on the worker.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
enum PhotoOutcome {
    /// The dialog was closed.
    Cancelled,
    /// The file was not a photo this app accepts. The text says why.
    Rejected(String),
    /// The photo was stored; the server's profile comes back.
    Saved(Box<PersonProfile>),
}

/// The page's request state. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    /// The route on show, and its person id.
    route: String,
    id: String,
    loaded: Option<Loaded>,
    /// The text of the last failed load. `None` while loading or loaded.
    error: Option<String>,
    editing: bool,
    saving: bool,
    save_error: String,
    photo_busy: bool,
    photo_error: String,
    pictures: Pictures,
}

/// A person's name and title in a reporting-line or direct-report link (plain values).
#[derive(Clone, Default)]
pub struct PersonLinkRow {
    pub id: String,
    pub name: String,
    pub title: String,
}

/// The profile as the window draws it (plain values).
#[derive(Clone, Default)]
pub struct ProfileData {
    pub id: String,
    pub name: String,
    pub initials: String,
    pub avatar: Image,
    pub preferred_name: String,
    pub pronouns: String,
    pub title: String,
    pub team: String,
    pub department: String,
    pub email: String,
    pub work_phone: String,
    pub location: String,
    pub timezone: String,
    pub timezone_label: String,
    pub start_date: String,
    pub manager_id: String,
    pub manager_name: String,
    pub bio: String,
    pub skills: Vec<String>,
    pub skills_text: String,
    pub has_personal: bool,
    pub personal_phone: String,
    pub personal_email: String,
    pub address: String,
    pub birthday: String,
    pub emergency_contact: String,
    pub chain: Vec<PersonLinkRow>,
    pub reports: Vec<PersonLinkRow>,
}

/// The edit form's drafts as the window hands them back on save (plain values).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileDraft {
    pub preferred_name: String,
    pub pronouns: String,
    pub bio: String,
    pub work_phone: String,
    pub timezone: String,
    pub location: String,
    pub skills: String,
    pub personal_phone: String,
    pub personal_email: String,
    pub address: String,
    pub birthday: String,
    pub emergency_contact: String,
}

/// The window side of the profile page. `wire_company!` implements it for the generated window.
pub trait ProfileWindow: slint::ComponentHandle + 'static {
    fn show_photo_supported(&self, on: bool);
    /// The signed-in person's id.
    fn user_id(&self) -> String;
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_view(&self, view: ProfileData);
    fn show_own(&self, on: bool);
    fn show_editing(&self, on: bool);
    fn show_saving(&self, on: bool);
    fn show_save_error(&self, text: &str);
    fn show_photo_busy(&self, on: bool);
    fn show_photo_error(&self, text: &str);
    fn on_route_is_profile(&self, f: impl Fn(&str) -> bool + 'static);
    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_start_edit(&self, f: impl Fn() + 'static);
    fn on_cancel_edit(&self, f: impl Fn() + 'static);
    fn on_save(&self, f: impl Fn(ProfileDraft) + 'static);
    fn on_change_photo(&self, f: impl Fn() + 'static);
}

/// The profile page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Profile<W: ProfileWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
    /// Redraws the timezone's local time while the page is open. Kept alive here: a dropped timer stops.
    _clock: Rc<Timer>,
}

impl<W: ProfileWindow> Clone for Profile<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
            _clock: self._clock.clone(),
        }
    }
}

/// How often the profile's local time is redrawn.
const CLOCK_EVERY: Duration = Duration::from_secs(60);

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, loads do nothing and the page
/// stays in its loading state.
pub fn wire<W: ProfileWindow>(ui: &W, data: DataCell) -> Profile<W> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    // Every minute the view is drawn again from the held state, so the local time moves. Nothing happens without a loaded profile.
    let clock = Timer::default();
    let (clock_ui, clock_shared) = (ui.as_weak(), shared.clone());
    clock.start(TimerMode::Repeated, CLOCK_EVERY, move || {
        if let Some(ui) = clock_ui.upgrade() {
            let s = lock(&clock_shared);
            if s.loaded.is_some() {
                ui.show_view(view(&s));
            }
        }
    });
    let profile = Profile {
        ui: ui.as_weak(),
        data,
        shared,
        _clock: Rc::new(clock),
    };
    ui.show_photo_supported(PHOTO_NATIVE);

    ui.on_route_is_profile(|route| id_of(route).is_some());

    let page = profile.clone();
    ui.on_opened(move |route| page.open(&route, false));

    let page = profile.clone();
    ui.on_retry(move || {
        let route = lock(&page.shared).route.clone();
        page.open(&route, true);
    });

    let page = profile.clone();
    ui.on_start_edit(move || page.start_edit());

    let page = profile.clone();
    ui.on_cancel_edit(move || page.cancel_edit());

    let page = profile.clone();
    ui.on_save(move |draft| page.save(&draft));

    let page = profile.clone();
    ui.on_change_photo(move || page.change_photo());

    profile
}

impl<W: ProfileWindow> Profile<W> {
    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// Delivers a load's result for the load with `generation`, as the worker would. For tests: no source is set, so nothing is
    /// fetched.
    pub fn deliver(&self, generation: u64, result: Result<(PersonProfile, OrgNode), ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver(&ui, &self.shared, None, generation, result);
    }

    /// Delivers a save's result for the save started at `generation`, as the worker would. For tests.
    pub fn deliver_save(&self, generation: u64, result: Result<PersonProfile, ClientError>) {
        let Some(ui) = self.ui.upgrade() else { return };
        deliver_save(&ui, &self.shared, generation, result);
    }

    /// Starts a load of the profile at `route` (ignored when it is not a profile route). A load for the route already on show is
    /// not repeated, unless `force` (Try again).
    fn open(&self, route: &str, force: bool) {
        let Some(id) = id_of(route).map(str::to_string) else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = lock(&self.shared);
            // Loaded or loading for this route already (no error): a page that opens again needs no new request.
            if !force && s.route == route && s.error.is_none() {
                return;
            }
            s.generation += 1;
            s.route = route.to_string();
            s.id = id.clone();
            s.loaded = None;
            s.error = None;
            s.editing = false;
            s.saving = false;
            s.save_error.clear();
            s.photo_error.clear();
            show(&ui, &s);
            s.generation
        };

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let is_self = ui.user_id() == id;
        let shared = self.shared.clone();
        let done_source = source.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move {
                let profile = if is_self {
                    api.my_profile(&token).await?
                } else {
                    api.person(&token, &id).await?
                };
                let org = api.org(&token, &id).await?;
                Ok((profile, org))
            },
            move |ui, result| deliver(ui, &shared, Some(&done_source), generation, result),
        );
    }

    /// Opens the editor. Only the own profile has one.
    fn start_edit(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        if !s.loaded.as_ref().is_some_and(|l| l.own) || s.saving {
            return;
        }
        s.editing = true;
        s.save_error.clear();
        show(&ui, &s);
    }

    /// Leaves the editor. The form's drafts are copied again the next time it opens, so the saved values come back.
    fn cancel_edit(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut s = lock(&self.shared);
        if s.saving {
            return;
        }
        s.editing = false;
        s.save_error.clear();
        show(&ui, &s);
    }

    /// Sends the form to `PATCH /v1/me/profile`. Every self-editable field is sent, so an empty field clears it.
    fn save(&self, draft: &ProfileDraft) {
        let Some(ui) = self.ui.upgrade() else { return };
        let body = body_of(draft);
        let generation = {
            let mut s = lock(&self.shared);
            if !s.loaded.as_ref().is_some_and(|l| l.own) || s.saving {
                return;
            }
            s.saving = true;
            s.save_error.clear();
            show(&ui, &s);
            s.generation
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { api.update_my_profile(&token, &body).await },
            move |ui, result| deliver_save(ui, &shared, generation, result),
        );
    }

    /// Picks a photo and uploads it (own profile, desktop only). Web and Android have no picker, so the button is hidden there.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn change_photo(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let generation = {
            let mut s = lock(&self.shared);
            if s.photo_busy || !s.loaded.as_ref().is_some_and(|l| l.own) {
                return;
            }
            s.photo_busy = true;
            s.photo_error.clear();
            show(&ui, &s);
            s.generation
        };
        let shared = self.shared.clone();
        let done_source = source.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { pick_and_upload(&api, &token).await },
            move |ui, result| deliver_photo(ui, &shared, &done_source, generation, result),
        );
    }

    /// No photo picker on this platform: the button is hidden, so nothing runs.
    #[cfg(not(all(not(target_arch = "wasm32"), not(target_os = "android"))))]
    fn change_photo(&self) {}
}

/// Opens the native file dialog, reads the photo and uploads it. Runs on the worker.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn pick_and_upload(api: &ApiClient, token: &str) -> Result<PhotoOutcome, ClientError> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Choose a photo")
        .add_filter("Photos", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
        .await
    else {
        return Ok(PhotoOutcome::Cancelled);
    };
    let Some(content_type) = content_type(&file.file_name()) else {
        return Ok(PhotoOutcome::Rejected(
            "Choose a PNG, JPEG or WebP photo.".to_string(),
        ));
    };
    let bytes = file.read().await;
    if bytes.len() > space_proto::people::AVATAR_MAX_BYTES {
        return Ok(PhotoOutcome::Rejected(
            "The photo is larger than 5 MB.".to_string(),
        ));
    }
    let profile = api.upload_avatar(token, content_type, bytes).await?;
    Ok(PhotoOutcome::Saved(Box::new(profile)))
}

/// The save body: every self-editable field, with an empty value clearing it. Skills are split on commas.
fn body_of(draft: &ProfileDraft) -> UpdateMyProfile {
    let skills: Vec<String> = draft
        .skills
        .as_str()
        .split(',')
        .map(str::trim)
        .filter(|skill| !skill.is_empty())
        .map(str::to_string)
        .collect();
    UpdateMyProfile {
        preferred_name: Some(draft.preferred_name.to_string()),
        pronouns: Some(draft.pronouns.to_string()),
        bio: Some(draft.bio.to_string()),
        work_phone: Some(draft.work_phone.to_string()),
        timezone: Some(draft.timezone.to_string()),
        location: Some(draft.location.to_string()),
        skills: Some(skills),
        personal_phone: Some(draft.personal_phone.to_string()),
        personal_email: Some(draft.personal_email.to_string()),
        address: Some(draft.address.to_string()),
        birthday: Some(draft.birthday.to_string()),
        emergency_contact: Some(draft.emergency_contact.to_string()),
    }
}

/// Keeps a load's result if it is for the latest load, then shows the profile or the error. Starts the picture load for the avatar.
fn deliver<W: ProfileWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    source: Option<&Arc<DataSource>>,
    generation: u64,
    result: Result<(PersonProfile, OrgNode), ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    match result {
        Ok((profile, org)) => {
            let own = profile.profile.id == ui.user_id();
            let wanted = profile.profile.avatar_url.clone().filter(|url| {
                !s.pictures.ready.contains_key(url) && !s.pictures.pending.contains(url)
            });
            s.loaded = Some(Loaded { profile, org, own });
            if let Some(url) = &wanted {
                s.pictures.pending.insert(url.clone());
            }
            show(ui, &s);
            drop(s);
            if let (Some(url), Some(source)) = (wanted, source) {
                fetch_picture(ui.as_weak(), source, shared.clone(), url);
            }
        }
        Err(error) => {
            s.error = Some(page_data::error_text(&error));
            show(ui, &s);
        }
    }
}

/// Keeps a save's result if its load is still the one on show. A saved profile replaces the one shown, and editing ends.
fn deliver_save<W: ProfileWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    result: Result<PersonProfile, ClientError>,
) {
    let mut s = lock(shared);
    if s.generation != generation {
        return;
    }
    s.saving = false;
    match result {
        Ok(profile) => {
            if let Some(loaded) = s.loaded.as_mut() {
                loaded.profile = profile;
            }
            s.editing = false;
            s.save_error.clear();
        }
        // A refused save (422 and the rest) keeps the form open with the server's message.
        Err(error) => s.save_error = page_data::error_text(&error),
    }
    show(ui, &s);
}

/// Keeps a photo result. A stored photo replaces the picture shown, and the new picture is loaded.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn deliver_photo<W: ProfileWindow>(
    ui: &W,
    shared: &Arc<Mutex<Shared>>,
    source: &Arc<DataSource>,
    generation: u64,
    result: Result<PhotoOutcome, ClientError>,
) {
    let mut s = lock(shared);
    s.photo_busy = false;
    if s.generation != generation {
        show(ui, &s);
        return;
    }
    match result {
        Ok(PhotoOutcome::Cancelled) => {}
        Ok(PhotoOutcome::Rejected(message)) => s.photo_error = message,
        Ok(PhotoOutcome::Saved(profile)) => {
            let url = profile.profile.avatar_url.clone();
            if let Some(loaded) = s.loaded.as_mut() {
                loaded.profile = *profile;
            }
            let mut refetch = None;
            if let Some(url) = url {
                // The cached picture may be the old photo under the same path, so it is dropped and loaded again.
                s.pictures.ready.remove(&url);
                if s.pictures.pending.insert(url.clone()) {
                    refetch = Some(url);
                }
            }
            show(ui, &s);
            drop(s);
            if let Some(url) = refetch {
                fetch_picture(ui.as_weak(), source, shared.clone(), url);
            }
            return;
        }
        Err(error) => s.photo_error = page_data::error_text(&error),
    }
    show(ui, &s);
}

/// Loads one avatar into the picture cache and shows it.
fn fetch_picture<W: ProfileWindow>(
    ui: Weak<W>,
    source: &Arc<DataSource>,
    shared: Arc<Mutex<Shared>>,
    url: String,
) {
    let key = url.clone();
    source.fetch(
        &ui,
        move |api, token| async move {
            let bytes = api.avatar(&token, &url).await?;
            page_data::decode_picture(&bytes)
        },
        move |ui, result| {
            let mut s = lock(&shared);
            s.pictures.pending.remove(&key);
            if let Ok(buffer) = result {
                s.pictures.ready.insert(key, buffer);
                show(ui, &s);
            }
        },
    );
}

/// The window's state, from the request state. Runs on the UI thread.
fn show<W: ProfileWindow>(ui: &W, s: &Shared) {
    let state = if s.loaded.is_some() {
        LoadState::Loaded
    } else if s.error.is_some() {
        LoadState::Error
    } else {
        LoadState::Loading
    };
    ui.show_state(state);
    ui.show_error(s.error.as_deref().unwrap_or_default());
    ui.show_view(view(s));
    ui.show_own(s.loaded.as_ref().is_some_and(|l| l.own));
    ui.show_editing(s.editing);
    ui.show_saving(s.saving);
    ui.show_save_error(&s.save_error);
    ui.show_photo_busy(s.photo_busy);
    ui.show_photo_error(&s.photo_error);
}

/// The profile as the page draws it. Empty when nothing is loaded.
fn view(s: &Shared) -> ProfileData {
    let Some(loaded) = &s.loaded else {
        return ProfileData::default();
    };
    let p = &loaded.profile.profile;
    let personal = loaded.profile.personal.as_ref();
    let name = p
        .preferred_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&p.display_name)
        .to_string();
    // The manager chain comes back direct manager first; the page shows the root first.
    let chain = loaded
        .org
        .manager_chain
        .iter()
        .rev()
        .map(link)
        .collect::<Vec<_>>();
    let manager = loaded.org.manager_chain.first();
    let reports = loaded
        .org
        .direct_reports
        .iter()
        .map(link)
        .collect::<Vec<_>>();
    ProfileData {
        id: p.id.clone(),
        name: name.clone(),
        initials: initials(&name),
        avatar: p
            .avatar_url
            .as_ref()
            .and_then(|url| s.pictures.ready.get(url))
            .map(page_data::picture)
            .unwrap_or_default(),
        preferred_name: text(p.preferred_name.as_deref()),
        pronouns: text(p.pronouns.as_deref()),
        title: text(p.title.as_deref()),
        team: text(p.team_name.as_deref()),
        department: text(p.department_name.as_deref()),
        email: p.email.clone(),
        work_phone: text(p.work_phone.as_deref()),
        location: text(p.location.as_deref()),
        timezone: text(p.timezone.as_deref()),
        timezone_label: timezone_label(p.timezone.as_deref().unwrap_or(""), jiff::Timestamp::now()),
        start_date: text(p.start_date.as_deref()),
        manager_id: text(manager.map(|m| m.id.as_str())),
        manager_name: text(manager.map(|m| m.display_name.as_str())),
        bio: text(p.bio.as_deref()),
        skills: p.skills.clone(),
        skills_text: p.skills.join(", "),
        has_personal: personal.is_some(),
        personal_phone: text(personal.and_then(|x| x.personal_phone.as_deref())),
        personal_email: text(personal.and_then(|x| x.personal_email.as_deref())),
        address: text(personal.and_then(|x| x.address.as_deref())),
        birthday: text(personal.and_then(|x| x.birthday.as_deref())),
        emergency_contact: text(personal.and_then(|x| x.emergency_contact.as_deref())),
        chain,
        reports,
    }
}

/// The timezone's name and its local time at `now`, as the profile shows it: `Europe/London (Currently 14:05)`. An unknown
/// name shows the name alone, and an empty one nothing.
fn timezone_label(name: &str, now: jiff::Timestamp) -> String {
    let name = name.trim();
    if name.is_empty() {
        return String::new();
    }
    match jiff::tz::TimeZone::get(name) {
        Ok(zone) => format!(
            "{name} (Currently {})",
            now.to_zoned(zone).strftime("%H:%M")
        ),
        Err(_) => name.to_string(),
    }
}

/// A reporting-line or direct-report link.
fn link(person: &PersonSummary) -> PersonLinkRow {
    PersonLinkRow {
        id: person.id.clone(),
        name: person.display_name.clone(),
        title: person.title.clone().unwrap_or_default(),
    }
}

/// An optional text as a plain string: empty when absent.
fn text(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_route_gives_its_id_and_other_routes_do_not() {
        assert_eq!(
            id_of("/company/employees/f0000000-0000-4000-8000-000000000001"),
            Some("f0000000-0000-4000-8000-000000000001")
        );
        assert_eq!(id_of("/company/employees"), None);
        assert_eq!(id_of("/company/employees/"), None);
        assert_eq!(id_of("/company/employees/../admin"), None);
        assert_eq!(id_of("/company/teams/abc"), None);
    }

    #[test]
    fn the_timezone_shows_its_local_time_at_a_fixed_instant() {
        let now: jiff::Timestamp = "2026-11-15T14:05:30Z".parse().unwrap();
        assert_eq!(
            timezone_label("Europe/London", now),
            "Europe/London (Currently 14:05)"
        );
        assert_eq!(
            timezone_label("America/New_York", now),
            "America/New_York (Currently 09:05)"
        );
        assert_eq!(
            timezone_label("Mars/Olympus_Mons", now),
            "Mars/Olympus_Mons"
        );
        assert_eq!(timezone_label("  ", now), "");
    }

    #[test]
    fn the_save_body_sends_every_field_and_splits_skills() {
        let draft = ProfileDraft {
            preferred_name: "Mina".into(),
            pronouns: "".into(),
            bio: "Hi".into(),
            work_phone: "".into(),
            timezone: "Europe/Lisbon".into(),
            location: "".into(),
            skills: " Rust, , Slint ,".into(),
            personal_phone: "".into(),
            personal_email: "".into(),
            address: "".into(),
            birthday: "1990-05-01".into(),
            emergency_contact: "".into(),
        };
        let body = body_of(&draft);
        assert_eq!(body.preferred_name.as_deref(), Some("Mina"));
        assert_eq!(body.pronouns.as_deref(), Some(""), "empty clears the field");
        assert_eq!(
            body.skills,
            Some(vec!["Rust".to_string(), "Slint".to_string()])
        );
        assert_eq!(body.birthday.as_deref(), Some("1990-05-01"));
    }
}
