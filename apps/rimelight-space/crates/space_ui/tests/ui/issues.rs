//! Projects > Issues (S1.3). The list's loaded, empty and error states, the refused query's marked span, the chips that toggle
//! the query text, the column header that writes the sort, and the New issue button shown for a member and not a viewer.
//! The window is built at `/p/EMB/issues` with results injected through the page's handle. No session is set, so
//! nothing is fetched. The dialog that New issue opens is tested in `new_issue.rs`.

use std::time::Duration;

use slint::Model;
use space_client::error::ClientError;
use space_proto::projects::{IssueSummary, Page, WorkflowState};
use space_ui::slint_ui::{PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const PROJECT_ROUTE: &str = "/p/EMB/issues";

fn issue(key: &str, summary: &str) -> IssueSummary {
    IssueSummary {
        id: format!("id-{key}"),
        key: key.to_string(),
        summary: summary.to_string(),
        issue_type: "Bug".to_string(),
        priority: "High".to_string(),
        state: WorkflowState {
            id: "s1".to_string(),
            name: "Open".to_string(),
            category: "Unstarted".to_string(),
            position: 1,
        },
        assignee: None,
        parent_id: None,
        sprint_id: None,
        estimate_points: None,
        due_date: None,
        tags: Vec::new(),
        backlog_rank: "00000001".to_string(),
        updated_at: 0,
    }
}

fn page(items: Vec<IssueSummary>, next_cursor: Option<&str>) -> Page<IssueSummary> {
    Page {
        items,
        next_cursor: next_cursor.map(str::to_string),
    }
}

/// Moves the mock clock on, so the query timer can fire, and runs the page's `init` (its first load).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn open_window() -> app_shell::App {
    crate::support::init();
    let app = app_shell::build(PROJECT_ROUTE, ThemeMode::Light).expect("window builds");
    // The page opens on the event loop (its `init`), so pump it once: its first load runs before the test starts.
    wait(10);
    app
}

pub fn issues_list_shows_rows_empty_and_error_states() {
    let app = open_window();
    let ui = &app.ui;
    assert_eq!(ui.get_issues_state(), PageLoadState::Loading);

    let first = page(
        vec![issue("EMB-12", "Crash on load"), issue("EMB-13", "Typo")],
        Some("2"),
    );
    app.issues.deliver(app.issues.generation(), Ok(first));
    assert_eq!(ui.get_issues_state(), PageLoadState::Loaded);
    let rows = ui.get_issues_rows();
    assert_eq!(rows.row_count(), 2);
    let row = rows.row_data(0).expect("first row");
    assert_eq!(row.key, "EMB-12");
    assert_eq!(row.summary, "Crash on load");
    assert_eq!(row.issue_type, "Bug");
    assert_eq!(row.state, "Open");
    assert_eq!(row.priority, "High");
    assert_eq!(
        row.assignee, "",
        "nobody assigned is an empty initials cell"
    );
    // updated_at is 0 (decades ago), so the Updated cell is a relative age whatever the clock says.
    assert!(
        row.updated.ends_with(" ago"),
        "the Updated cell is relative: {}",
        row.updated
    );
    assert!(ui.get_issues_has_more(), "a cursor means Load more");

    // Load more appends the next page.
    ui.invoke_issues_load_more();
    let more = app.issues.generation();
    app.issues
        .deliver(more, Ok(page(vec![issue("EMB-14", "Next")], None)));
    assert_eq!(
        ui.get_issues_rows().row_count(),
        3,
        "the next page is appended"
    );
    assert!(!ui.get_issues_has_more());

    // Empty: a query that finds nobody.
    ui.invoke_issues_submit("nothing".into());
    app.issues
        .deliver(app.issues.generation(), Ok(page(vec![], None)));
    assert_eq!(ui.get_issues_state(), PageLoadState::Empty);
    assert_eq!(ui.get_issues_rows().row_count(), 0);

    // Error, then Try again starts a new request.
    app.issues.deliver(
        app.issues.generation(),
        Err(ClientError::Network("refused".to_string())),
    );
    assert_eq!(ui.get_issues_state(), PageLoadState::Error);
    let failed = app.issues.generation();
    ui.invoke_issues_retry();
    assert_eq!(ui.get_issues_state(), PageLoadState::Loading);
    assert_eq!(app.issues.generation(), failed + 1);
    app.issues.deliver(failed, Ok(page(vec![], None)));
    assert_eq!(
        ui.get_issues_state(),
        PageLoadState::Loading,
        "a late answer to the request before the retry changes nothing"
    );
}

pub fn issues_refused_query_marks_its_span_under_the_bar() {
    let app = open_window();
    let ui = &app.ui;
    ui.invoke_issues_submit("staet: me".into());
    assert_eq!(app.issues.query(), "staet: me");
    app.issues.deliver(
        app.issues.generation(),
        Err(ClientError::InvalidQuery {
            message: "unknown field `staet` (columns 1-6)".to_string(),
            start: 0,
            end: 6,
        }),
    );
    assert_eq!(
        ui.get_issues_query_message(),
        "unknown field `staet` (columns 1-6)"
    );
    assert_eq!(ui.get_issues_query_before(), "");
    assert_eq!(ui.get_issues_query_span(), "staet:", "the span is marked");
    assert_eq!(ui.get_issues_query_after(), " me");
    assert_eq!(
        ui.get_issues_rows().row_count(),
        0,
        "no rows for a refused query"
    );
    assert_eq!(ui.get_issues_state(), PageLoadState::Empty);

    // A good query clears the message.
    ui.invoke_issues_submit("me".into());
    app.issues.deliver(
        app.issues.generation(),
        Ok(page(vec![issue("EMB-12", "x")], None)),
    );
    assert_eq!(ui.get_issues_query_message(), "");
}

pub fn issues_chips_toggle_their_clause_in_the_query() {
    let app = open_window();
    let ui = &app.ui;
    ui.invoke_issues_chip(0);
    assert_eq!(ui.get_issues_query(), "assignee: me");
    assert!(ui.get_issues_chip_on().row_data(0).expect("chip 0"));
    ui.invoke_issues_chip(1);
    assert_eq!(ui.get_issues_query(), "assignee: me state: #Unresolved");
    assert!(ui.get_issues_chip_on().row_data(1).expect("chip 1"));

    // A second press takes the clause out again.
    ui.invoke_issues_chip(0);
    assert_eq!(ui.get_issues_query(), "state: #Unresolved");
    assert!(!ui.get_issues_chip_on().row_data(0).expect("chip 0"));
    assert_eq!(
        app.issues.query(),
        "state: #Unresolved",
        "the list is asked for the text"
    );
}

pub fn issues_header_click_writes_the_sort_and_toggles_it() {
    let app = open_window();
    let ui = &app.ui;
    ui.invoke_issues_sort("updated".into());
    assert_eq!(ui.get_issues_query(), "sort by: updated asc");
    assert_eq!(ui.get_issues_sort_field(), "updated");
    assert!(!ui.get_issues_sort_desc());
    ui.invoke_issues_sort("updated".into());
    assert_eq!(ui.get_issues_query(), "sort by: updated desc");
    assert!(ui.get_issues_sort_desc());
    ui.invoke_issues_sort("key".into());
    assert_eq!(
        ui.get_issues_query(),
        "sort by: key asc",
        "another column replaces it"
    );
    ui.invoke_issues_sort("assignee".into());
    assert_eq!(
        ui.get_issues_query(),
        "sort by: key asc",
        "a field that cannot sort changes nothing"
    );
}

pub fn new_issue_is_hidden_for_a_viewer_and_shown_for_a_member() {
    let app = open_window();
    let ui = &app.ui;
    app.issues.deliver_role(Some("viewer"));
    assert!(!ui.get_issues_can_create(), "a viewer cannot create");
    app.issues.deliver_role(None);
    assert!(!ui.get_issues_can_create(), "not a member cannot create");
    app.issues.deliver_role(Some("member"));
    assert!(ui.get_issues_can_create(), "a member can create");
}

pub fn a_row_opens_its_issue_route() {
    let app = open_window();
    let ui = &app.ui;
    app.issues.deliver(
        app.issues.generation(),
        Ok(page(vec![issue("EMB-12", "Crash")], None)),
    );
    ui.invoke_issues_open_issue("EMB-12".into());
    assert_eq!(ui.get_page_route(), "/p/EMB/issues/EMB-12");
}
