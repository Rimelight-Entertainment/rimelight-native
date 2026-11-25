//! Home > Dashboard (S2). Each card's loading, loaded, empty and error states, Try again, the dropped stale answer, and the
//! routes the rows and View all open, on the real window with results injected through the page's handle. No session is set,
//! so nothing is fetched.

use std::time::Duration;

use slint::{ComponentHandle as _, Model};
use space_client::error::ClientError;
use space_proto::canvases::CanvasView;
use space_proto::people::Page;
use space_proto::projects::{IssueSummary, UserRef, WorkflowState};
use space_ui::slint_ui::{HomeState, PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

/// Moves the mock clock on, so the page's `init` (which asks for both cards) has run.
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn issue(key: &str, summary: &str, state: &str, priority: &str) -> IssueSummary {
    IssueSummary {
        id: format!("id-{key}"),
        key: key.to_string(),
        summary: summary.to_string(),
        issue_type: "Task".to_string(),
        priority: priority.to_string(),
        state: WorkflowState {
            id: "s1".to_string(),
            name: state.to_string(),
            category: "started".to_string(),
            position: 1,
        },
        assignee: Some(UserRef {
            id: "u1".to_string(),
            display_name: "Me".to_string(),
        }),
        parent_id: None,
        sprint_id: None,
        estimate_points: None,
        due_date: None,
        tags: Vec::new(),
        backlog_rank: String::new(),
        updated_at: 0,
    }
}

fn issues(count: usize, next_cursor: Option<&str>) -> Page<IssueSummary> {
    Page {
        items: (1..=count)
            .map(|n| {
                issue(
                    &format!("EMB-{n}"),
                    &format!("Issue {n}"),
                    "In Progress",
                    "High",
                )
            })
            .collect(),
        next_cursor: next_cursor.map(str::to_string),
    }
}

fn canvas(id: &str, title: &str, updated_at: &str) -> CanvasView {
    CanvasView {
        id: id.to_string(),
        title: title.to_string(),
        home: "personal".to_string(),
        owner_id: Some("u1".to_string()),
        project_key: None,
        created_by: "u1".to_string(),
        seq: 1,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: updated_at.to_string(),
        archived: false,
        my_access: "owner".to_string(),
    }
}

pub fn home_cards_start_loading_and_show_their_loaded_rows() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<HomeState>();

    // Loading: nothing has come back yet.
    assert_eq!(state.get_assigned_state(), PageLoadState::Loading);
    assert_eq!(state.get_canvases_state(), PageLoadState::Loading);

    // Assigned to me: twelve open issues come back, the card shows the first eight, and the header counts all twelve.
    app.home
        .deliver_assigned(app.home.assigned_generation(), Ok(issues(12, None)));
    assert_eq!(state.get_assigned_state(), PageLoadState::Loaded);
    assert_eq!(state.get_assigned_rows().row_count(), 8);
    assert_eq!(state.get_assigned_count(), "12");
    let first = state.get_assigned_rows().row_data(0).expect("row");
    assert_eq!(first.key, "EMB-1");
    assert_eq!(first.summary, "Issue 1");
    assert_eq!(first.state, "In Progress");
    assert_eq!(first.priority, "High");
    assert_eq!(first.project, "EMB");
    assert_eq!(first.route, "/p/EMB/issues/EMB-1");
    assert_eq!(
        state.get_canvases_state(),
        PageLoadState::Loading,
        "the other card is not touched"
    );

    // A next page means the count is a floor.
    app.home
        .deliver_assigned(app.home.assigned_generation(), Ok(issues(50, Some("c2"))));
    assert_eq!(state.get_assigned_count(), "50+");

    // Recent canvases: seven canvases, the card shows the five newest, newest first.
    let canvases = vec![
        canvas("c1", "One", "2026-11-01T10:00:00Z"),
        canvas("c2", "Two", "2026-11-07T10:00:00Z"),
        canvas("c3", "Three", "2026-11-03T10:00:00Z"),
        canvas("c4", "Four", "2026-11-06T10:00:00Z"),
        canvas("c5", "Five", "2026-11-05T10:00:00Z"),
        canvas("c6", "Six", "2026-11-04T10:00:00Z"),
        canvas("c7", "Seven", "2026-10-01T10:00:00Z"),
    ];
    app.home
        .deliver_canvases(app.home.canvases_generation(), Ok(canvases));
    assert_eq!(state.get_canvases_state(), PageLoadState::Loaded);
    assert_eq!(state.get_canvases_rows().row_count(), 5);
    let top = state.get_canvases_rows().row_data(0).expect("row");
    assert_eq!(top.title, "Two");
    assert_eq!(top.route, "/home/canvases/c2");
    assert_eq!(top.updated, "2026-11-07 10:00");
}

pub fn home_cards_show_their_empty_states() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<HomeState>();

    app.home
        .deliver_assigned(app.home.assigned_generation(), Ok(issues(0, None)));
    assert_eq!(state.get_assigned_state(), PageLoadState::Empty);
    assert_eq!(state.get_assigned_rows().row_count(), 0);
    assert_eq!(state.get_assigned_count(), "");

    app.home
        .deliver_canvases(app.home.canvases_generation(), Ok(Vec::new()));
    assert_eq!(state.get_canvases_state(), PageLoadState::Empty);
    assert_eq!(state.get_canvases_rows().row_count(), 0);
}

pub fn home_cards_fail_and_retry_on_their_own() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<HomeState>();

    // The canvases card loads, the assigned card fails: the failure does not blank the canvases.
    app.home.deliver_canvases(
        app.home.canvases_generation(),
        Ok(vec![canvas("c1", "One", "2026-11-01T10:00:00Z")]),
    );
    let failed = app.home.assigned_generation();
    app.home
        .deliver_assigned(failed, Err(ClientError::Network("refused".to_string())));
    assert_eq!(state.get_assigned_state(), PageLoadState::Error);
    assert!(
        state
            .get_assigned_error()
            .as_str()
            .contains("could not be reached"),
        "the error says what happened"
    );
    assert_eq!(state.get_canvases_state(), PageLoadState::Loaded);
    assert_eq!(state.get_canvases_rows().row_count(), 1);

    // Try again on the failed card: a new request, the loading state, and the error is cleared. The other card is not asked
    // again.
    let canvases_before = app.home.canvases_generation();
    state.invoke_retry_assigned();
    assert_eq!(state.get_assigned_state(), PageLoadState::Loading);
    assert_eq!(state.get_assigned_error(), "");
    assert_eq!(app.home.assigned_generation(), failed + 1);
    assert_eq!(app.home.canvases_generation(), canvases_before);
    assert_eq!(state.get_canvases_state(), PageLoadState::Loaded);

    // A late answer to the request before the retry is dropped.
    app.home.deliver_assigned(failed, Ok(issues(3, None)));
    assert_eq!(
        state.get_assigned_state(),
        PageLoadState::Loading,
        "a stale result changes nothing"
    );

    // The same for the canvases card.
    let failed = app.home.canvases_generation();
    app.home
        .deliver_canvases(failed, Err(ClientError::Unauthorized));
    assert_eq!(state.get_canvases_state(), PageLoadState::Error);
    assert_eq!(state.get_canvases_rows().row_count(), 0);
    state.invoke_retry_canvases();
    assert_eq!(state.get_canvases_state(), PageLoadState::Loading);
    assert_eq!(app.home.canvases_generation(), failed + 1);
    app.home.deliver_canvases(failed, Ok(Vec::new()));
    assert_eq!(state.get_canvases_state(), PageLoadState::Loading);
}

pub fn home_rows_and_view_all_open_their_routes() {
    crate::support::init();
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    wait(10);
    let ui = &app.ui;
    let state = ui.global::<HomeState>();
    assert_eq!(ui.get_page_route(), "/home");

    app.home
        .deliver_assigned(app.home.assigned_generation(), Ok(issues(2, None)));
    let route = state
        .get_assigned_rows()
        .row_data(1)
        .expect("row")
        .route
        .to_string();
    state.invoke_open(route.as_str().into());
    assert_eq!(ui.get_page_route(), "/p/EMB/issues/EMB-2");

    state.invoke_open("/home".into());
    assert_eq!(ui.get_page_route(), "/home");
    state.invoke_open("/home/canvases/c2".into());
    assert_eq!(ui.get_page_route(), "/home/canvases/c2");

    state.invoke_open("/home".into());
    state.invoke_view_all_canvases();
    assert_eq!(ui.get_page_route(), "/home/canvases");
}
