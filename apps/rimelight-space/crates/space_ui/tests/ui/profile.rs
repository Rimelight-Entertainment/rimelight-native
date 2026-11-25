//! Profile (SP.3c). A person's profile on the real window, with results injected through the page's handle (no session, so
//! nothing is fetched): the loaded state, the personal section only when the server returned it, Edit only on the own profile,
//! the edit form's fields, a save that succeeds and one that the server refuses (422), the links to other profiles, and
//! Try again. The Edit button and the form's fields are checked on a small harness window with the page in it.

use std::time::Duration;

use slint::Model;
use space_client::error::ClientError;
use space_proto::people::{OrgNode, PersonProfile, PersonSummary, PersonalInfo, Profile};
use space_ui::slint_ui::{PageLoadState, ProfileDraft, app_shell};
use space_ui::theme::ThemeMode;

/// The harness window: the page on its own, so its Edit button and form fields can be read.
mod harness {
    pub use space_ui_test_slint::profile::*;
}
use harness::ProfileForm;

const ANN: &str = "f0000000-0000-4000-8000-000000000001";
const BEN: &str = "f0000000-0000-4000-8000-000000000002";
const CEO: &str = "f0000000-0000-4000-8000-000000000000";

/// Moves the mock clock on, so the page's `init` runs.
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn summary(id: &str, name: &str, title: Option<&str>) -> PersonSummary {
    PersonSummary {
        id: id.to_string(),
        display_name: name.to_string(),
        title: title.map(str::to_string),
        team: None,
        avatar_url: None,
    }
}

fn profile(id: &str, name: &str, manager: Option<&str>) -> Profile {
    Profile {
        id: id.to_string(),
        email: format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
        display_name: name.to_string(),
        preferred_name: None,
        pronouns: Some("they/them".to_string()),
        title: Some("Developer".to_string()),
        position_id: None,
        department_id: None,
        department_name: Some("Engineering".to_string()),
        team_id: None,
        team_name: Some("Gameplay".to_string()),
        manager_id: manager.map(str::to_string),
        location: Some("Lisbon".to_string()),
        timezone: Some("Europe/Lisbon".to_string()),
        work_phone: None,
        bio: Some("Hello".to_string()),
        skills: vec!["Rust".to_string()],
        start_date: Some("2024-01-02".to_string()),
        avatar_url: None,
        is_active: true,
        roles: vec![],
    }
}

fn personal() -> PersonalInfo {
    PersonalInfo {
        personal_phone: Some("+351 900 000 000".to_string()),
        personal_email: None,
        address: None,
        birthday: Some("1990-05-01".to_string()),
        emergency_contact: None,
    }
}

/// A profile as the server returns it: personal fields only when `personal` is given.
fn person(profile: Profile, personal: Option<PersonalInfo>) -> PersonProfile {
    PersonProfile { profile, personal }
}

/// Ben reports to Ann, who reports to the CEO. Ben has no direct reports.
fn ben_org() -> OrgNode {
    OrgNode {
        person: summary(BEN, "Ben Brook", Some("Developer")),
        manager_chain: vec![
            summary(ANN, "Ann Adams", Some("Lead")),
            summary(CEO, "Cy Earl", Some("CEO")),
        ],
        direct_reports: vec![],
    }
}

fn draft(bio: &str) -> ProfileDraft {
    ProfileDraft {
        preferred_name: "Ann".into(),
        pronouns: "she/her".into(),
        bio: bio.into(),
        work_phone: "".into(),
        timezone: "Europe/Lisbon".into(),
        location: "Lisbon".into(),
        skills: "Rust, Slint".into(),
        personal_phone: "".into(),
        personal_email: "".into(),
        address: "".into(),
        birthday: "1990-05-01".into(),
        emergency_contact: "".into(),
    }
}

pub fn profile_other_person_is_read_only_and_hides_personal_when_absent() {
    crate::support::init();
    let app = app_shell::build(&format!("/company/employees/{BEN}"), ThemeMode::Light)
        .expect("window builds");
    // The page opens on the event loop (its `init`), so pump it once: its load starts before the test checks.
    wait(10);
    let ui = &app.ui;
    assert_eq!(ui.get_profile_state(), PageLoadState::Loading);

    app.profile.deliver(
        app.profile.generation(),
        Ok((
            person(profile(BEN, "Ben Brook", Some(ANN)), None),
            ben_org(),
        )),
    );
    assert_eq!(ui.get_profile_state(), PageLoadState::Loaded);
    assert!(!ui.get_profile_own(), "someone else's profile");
    let view = ui.get_profile_view();
    assert_eq!(view.name, "Ben Brook");
    assert!(
        !view.has_personal,
        "no personal section when the server sent none"
    );
    assert_eq!(view.manager_id, ANN);
    assert_eq!(view.manager_name, "Ann Adams");
    assert_eq!(view.chain.row_count(), 2, "the reporting line is shown");
    assert_eq!(
        view.chain.row_data(0).expect("root first").id,
        CEO,
        "the chain runs from the root down"
    );
    assert_eq!(view.reports.row_count(), 0);
    assert!(!ui.get_profile_editing());

    // The Edit button is not shown on someone else's profile.
    let form = ProfileForm::new().expect("form builds");
    form.set_own(false);
    assert!(
        !form.get_edit_shown(),
        "no Edit on another person's profile"
    );
    assert!(
        !form.get_photo_shown(),
        "no Change photo on another person's profile"
    );
}

pub fn profile_own_profile_shows_edit_and_only_self_fields_in_the_form() {
    crate::support::init();
    let app = app_shell::build(&format!("/company/employees/{ANN}"), ThemeMode::Light)
        .expect("window builds");
    wait(10);
    let ui = &app.ui;
    ui.set_user_id(ANN.into());
    app.profile.deliver(
        app.profile.generation(),
        Ok((
            person(profile(ANN, "Ann Adams", Some(CEO)), Some(personal())),
            OrgNode {
                person: summary(ANN, "Ann Adams", Some("Lead")),
                manager_chain: vec![summary(CEO, "Cy Earl", Some("CEO"))],
                direct_reports: vec![summary(BEN, "Ben Brook", Some("Developer"))],
            },
        )),
    );
    assert!(
        ui.get_profile_own(),
        "the profile is the signed-in person's"
    );
    let view = ui.get_profile_view();
    assert!(view.has_personal, "the personal section shows when sent");
    assert_eq!(view.reports.row_count(), 1, "the direct reports are listed");

    let form = ProfileForm::new().expect("form builds");
    form.set_own(true);
    form.set_photo_supported(false);
    assert!(form.get_edit_shown(), "Edit is shown on the own profile");
    assert!(
        !form.get_photo_shown(),
        "Change photo is hidden where there is no picker"
    );
    form.set_photo_supported(true);
    assert!(
        form.get_photo_shown(),
        "Change photo is shown on the desktop"
    );

    // The form has the self-editable fields and none of the org fields.
    let labels: Vec<String> = form
        .get_edit_labels()
        .iter()
        .map(|l| l.to_string())
        .collect();
    assert!(labels.contains(&"Bio".to_string()));
    assert!(labels.contains(&"Preferred name".to_string()));
    for org_field in ["Title", "Team", "Department", "Manager", "Start date"] {
        assert!(
            !labels.iter().any(|l| l == org_field),
            "{org_field} is not editable here"
        );
    }
}

pub fn profile_save_success_and_refused_save_show_the_result() {
    crate::support::init();
    let app = app_shell::build(&format!("/company/employees/{ANN}"), ThemeMode::Light)
        .expect("window builds");
    wait(10);
    let ui = &app.ui;
    ui.set_user_id(ANN.into());
    app.profile.deliver(
        app.profile.generation(),
        Ok((
            person(profile(ANN, "Ann Adams", Some(CEO)), None),
            OrgNode {
                person: summary(ANN, "Ann Adams", None),
                manager_chain: vec![summary(CEO, "Cy Earl", None)],
                direct_reports: vec![],
            },
        )),
    );

    ui.invoke_profile_start_edit();
    assert!(ui.get_profile_editing(), "Edit opens the form");

    // The server refuses the bio: the form stays open with its message next to it.
    ui.invoke_profile_save(draft("x"));
    assert!(ui.get_profile_saving());
    let generation = app.profile.generation();
    app.profile.deliver_save(
        generation,
        Err(ClientError::Api {
            status: 422,
            error: "invalid_field".into(),
            message: "bio: too long".into(),
        }),
    );
    assert!(!ui.get_profile_saving());
    assert!(
        ui.get_profile_editing(),
        "a refused save keeps the form open"
    );
    assert_eq!(ui.get_profile_save_error(), "bio: too long");
    assert_eq!(
        ui.get_profile_view().bio,
        "Hello",
        "the shown bio is unchanged"
    );

    // A saved profile shows the new values, the form closes and the error goes.
    ui.invoke_profile_save(draft("Hi there"));
    let mut saved = profile(ANN, "Ann Adams", Some(CEO));
    saved.bio = Some("Hi there".to_string());
    app.profile
        .deliver_save(app.profile.generation(), Ok(person(saved, None)));
    assert!(!ui.get_profile_editing(), "a saved profile leaves the form");
    assert_eq!(ui.get_profile_save_error(), "");
    assert_eq!(ui.get_profile_view().bio, "Hi there");
}

pub fn profile_org_links_and_the_avatar_menu_open_profiles() {
    crate::support::init();
    let app = app_shell::build(&format!("/company/employees/{BEN}"), ThemeMode::Light)
        .expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.profile.deliver(
        app.profile.generation(),
        Ok((
            person(profile(BEN, "Ben Brook", Some(ANN)), None),
            ben_org(),
        )),
    );

    // A link in the reporting line (the manager) opens that person's profile in the same tab.
    ui.invoke_profile_open_person(ANN.into());
    assert_eq!(ui.get_page_route(), format!("/company/employees/{ANN}"));

    // The avatar menu's Profile row opens the signed-in person's own profile.
    ui.set_user_id(CEO.into());
    ui.invoke_profile_clicked();
    assert_eq!(ui.get_page_route(), format!("/company/employees/{CEO}"));
}

pub fn profile_error_then_try_again() {
    crate::support::init();
    let app = app_shell::build(&format!("/company/employees/{BEN}"), ThemeMode::Light)
        .expect("window builds");
    wait(10);
    let ui = &app.ui;
    app.profile.deliver(
        app.profile.generation(),
        Err(ClientError::Network("refused".to_string())),
    );
    assert_eq!(ui.get_profile_state(), PageLoadState::Error);
    assert!(
        ui.get_profile_error()
            .as_str()
            .contains("could not be reached"),
        "the error line says what happened"
    );

    // Try again starts a new load for the same route and shows the loading state.
    let failed = app.profile.generation();
    ui.invoke_profile_retry();
    assert_eq!(ui.get_profile_state(), PageLoadState::Loading);
    assert_eq!(ui.get_profile_error(), "", "a retry clears the error");
    assert_eq!(app.profile.generation(), failed + 1);

    // A late answer to the request before the retry changes nothing.
    app.profile.deliver(
        failed,
        Ok((person(profile(BEN, "Ben Brook", None), None), ben_org())),
    );
    assert_eq!(ui.get_profile_state(), PageLoadState::Loading);

    app.profile.deliver(
        app.profile.generation(),
        Ok((person(profile(BEN, "Ben Brook", None), None), ben_org())),
    );
    assert_eq!(ui.get_profile_state(), PageLoadState::Loaded);
}
