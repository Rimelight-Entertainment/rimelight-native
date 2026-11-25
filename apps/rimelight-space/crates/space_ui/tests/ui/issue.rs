//! Projects > Issue (S1.3, part 3). One issue at `/p/EMB/issues/EMB-12` on the real window, with results injected through the page's
//! handle and the writes read back from `last_request` (no session is set, so nothing is fetched). Covers the loaded and error
//! states, the edit modal (one edit button; only the changed fields are sent), a subtask, a link, comments (own only), the
//! activity lines, the viewer and
//! non-member read-only rule, and the delete confirm.

use std::time::Duration;

use slint::Model;
use space_area_projects::issue::{IssueData, Request};
use space_client::error::ClientError;
use space_proto::projects::{
    ActivityEntry, Comment, CreateIssue, CreateLink, CreateWorkItem, Issue, IssueLink,
    IssueSummary, Member, Page, Project, UpdateIssue, UserRef, WorkItem, WorkflowState,
};
use space_ui::slint_ui::{PageLoadState, app_shell};
use space_ui::theme::ThemeMode;

const ROUTE: &str = "/p/EMB/issues/EMB-12";
const ANA: &str = "u-ana";
const BEN: &str = "u-ben";

/// Moves the mock clock on, so the page's `init` runs and its first load starts.
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn open_window() -> app_shell::App {
    crate::support::init();
    let app = app_shell::build(ROUTE, ThemeMode::Light).expect("window builds");
    wait(10);
    app
}

fn user(id: &str, name: &str) -> UserRef {
    UserRef {
        id: id.to_string(),
        display_name: name.to_string(),
    }
}

fn state(id: &str, name: &str, position: i32) -> WorkflowState {
    WorkflowState {
        id: id.to_string(),
        name: name.to_string(),
        category: "Started".to_string(),
        position,
    }
}

fn summary(key: &str, text: &str) -> IssueSummary {
    IssueSummary {
        id: format!("id-{key}"),
        key: key.to_string(),
        summary: text.to_string(),
        issue_type: "Task".to_string(),
        priority: "Normal".to_string(),
        state: state("s1", "Open", 1),
        assignee: None,
        parent_id: None,
        sprint_id: None,
        estimate_points: None,
        due_date: None,
        tags: Vec::new(),
        backlog_rank: "0".to_string(),
        updated_at: 0,
    }
}

fn comment(id: &str, author: UserRef, body: &str) -> Comment {
    Comment {
        id: id.to_string(),
        body: body.to_string(),
        author,
        created_at: 0,
        updated_at: 0,
    }
}

/// EMB-12, with the caller's `role` in its project (`None` for a global admin who is not a member).
fn issue_data(role: Option<&str>) -> IssueData {
    let issue = Issue {
        id: "id-EMB-12".to_string(),
        key: "EMB-12".to_string(),
        summary: "Crash on load".to_string(),
        description: "Steps to reproduce".to_string(),
        issue_type: "Bug".to_string(),
        priority: "High".to_string(),
        state: state("s1", "Open", 1),
        assignee: Some(user(BEN, "Ben Brook")),
        reporter: user(ANA, "Ana Silva"),
        parent: None,
        subtasks: vec![summary("EMB-13", "Fix the menu")],
        links: vec![IssueLink {
            id: "l1".to_string(),
            kind: "blocks".to_string(),
            direction: "outward".to_string(),
            label: "blocks".to_string(),
            issue: summary("EMB-3", "Launch"),
        }],
        sprint: None,
        estimate_points: Some(3),
        due_date: None,
        tags: vec!["ui".to_string()],
        backlog_rank: "0".to_string(),
        created_at: 0,
        updated_at: 0,
        resolved_at: None,
        updated_by: None,
        votes: 0,
        voted_by_me: false,
        starred_by_me: false,
        spent_minutes: 90,
        attachments: Vec::new(),
    };
    let project = Project {
        image_url: None,
        id: "p1".to_string(),
        key: "EMB".to_string(),
        name: "Embers".to_string(),
        description: String::new(),
        lead: None,
        my_role: role.map(str::to_string),
        workflow: vec![state("s1", "Open", 1), state("s2", "In Progress", 2)],
        members: vec![
            Member {
                user: user(ANA, "Ana Silva"),
                role: "admin".to_string(),
            },
            Member {
                user: user(BEN, "Ben Brook"),
                role: "member".to_string(),
            },
        ],
    };
    IssueData {
        issue,
        project,
        sprints: Vec::new(),
        areas: Vec::new(),
        work_items: vec![WorkItem {
            id: "w1".to_string(),
            minutes: 90,
            spent_on: "2026-11-14".to_string(),
            note: "Reproduced".to_string(),
            author: user(ANA, "Ana Silva"),
            created_at: 0,
            updated_at: 0,
        }],
        comments: vec![
            comment("c1", user(ANA, "Ana Silva"), "First"),
            comment("c2", user(BEN, "Ben Brook"), "Second"),
        ],
        activity: vec![
            ActivityEntry {
                id: "a1".to_string(),
                kind: "created".to_string(),
                field: None,
                old_value: None,
                new_value: None,
                actor: Some(user(ANA, "Ana Silva")),
                created_at: 0,
            },
            ActivityEntry {
                id: "a2".to_string(),
                kind: "field_changed".to_string(),
                field: Some("state_id".to_string()),
                old_value: Some(serde_json::Value::String("s1".to_string())),
                new_value: Some(serde_json::Value::String("s2".to_string())),
                actor: Some(user(ANA, "Ana Silva")),
                created_at: 0,
            },
        ],
    }
}

/// Opens the window as `me`, with the issue loaded for `role`.
fn loaded(role: Option<&str>, me: &str) -> app_shell::App {
    let app = open_window();
    app.ui.set_user_id(me.into());
    app.issue
        .deliver_load(app.issue.generation(), Ok(issue_data(role)));
    assert_eq!(app.ui.get_issue_state(), PageLoadState::Loaded);
    app
}

pub fn issue_page_shows_loading_error_then_loaded() {
    let app = open_window();
    let ui = &app.ui;
    assert_eq!(ui.get_issue_state(), PageLoadState::Loading);

    app.issue.deliver_load(
        app.issue.generation(),
        Err(ClientError::Api {
            status: 404,
            error: "not_found".to_string(),
            message: "Issue not found.".to_string(),
        }),
    );
    assert_eq!(ui.get_issue_state(), PageLoadState::Error);
    assert_eq!(ui.get_issue_error().as_str(), "Issue not found.");

    // Try again asks again for the same route.
    ui.invoke_issue_retry();
    assert_eq!(ui.get_issue_state(), PageLoadState::Loading);
    app.issue
        .deliver_load(app.issue.generation(), Ok(issue_data(Some("admin"))));
    assert_eq!(ui.get_issue_state(), PageLoadState::Loaded);
    let view = ui.get_issue_view();
    assert_eq!(view.key.as_str(), "EMB-12");
    assert_eq!(view.summary.as_str(), "Crash on load");
    assert_eq!(view.estimate.as_str(), "3");
    assert_eq!(view.assignee_name.as_str(), "Ben Brook");
    assert_eq!(ui.get_issue_subtasks().row_count(), 1);
    assert_eq!(ui.get_issue_links().row_count(), 1);
    assert_eq!(ui.get_issue_comments().row_count(), 2);
}

/// The one edit button opens the edit modal with the issue's fields. Only the fields the form changed are sent, a clearing
/// estimate is `null` on the wire, and cancelling sends nothing.
pub fn issue_edit_modal_prefills_and_saves_only_the_changed_fields() {
    let app = loaded(Some("member"), ANA);
    let ui = &app.ui;
    ui.invoke_issue_edit_open();
    assert!(ui.get_new_issue_edit_mode(), "the edit modal opens");
    assert_eq!(ui.get_new_issue_summary().as_str(), "Crash on load");
    assert_eq!(
        ui.get_new_issue_description().as_str(),
        "Steps to reproduce"
    );
    assert_eq!(ui.get_new_issue_state().as_str(), "s1");
    assert_eq!(ui.get_new_issue_assignee().as_str(), BEN);
    assert_eq!(ui.get_new_issue_estimate().as_str(), "3");
    assert_eq!(ui.get_new_issue_tags().as_str(), "ui", "the areas");
    assert_eq!(app.new_issue.last_update(), None, "opening sends nothing");

    // Save with nothing changed sends nothing, and closes the modal.
    ui.invoke_new_issue_create();
    assert_eq!(app.new_issue.last_update(), None, "no change, no request");
    assert!(!ui.get_new_issue_creating(), "no save is in flight");

    ui.invoke_issue_edit_open();
    ui.set_new_issue_summary("Crash on start".into());
    ui.set_new_issue_state("s2".into());
    ui.set_new_issue_estimate("".into());
    ui.invoke_new_issue_create();
    assert_eq!(
        app.new_issue.last_update(),
        Some(UpdateIssue {
            summary: Some("Crash on start".to_string()),
            state_id: Some("s2".to_string()),
            estimate_points: Some(None),
            ..UpdateIssue::default()
        }),
        "only the changed fields are sent"
    );
    assert!(ui.get_new_issue_creating(), "the save is on its way");
}

pub fn issue_add_subtask_sends_it_under_this_issue() {
    let app = loaded(Some("member"), ANA);
    app.ui.invoke_issue_subtask_add("Write the test".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::Subtask(CreateIssue {
            summary: "Write the test".to_string(),
            parent_id: Some("id-EMB-12".to_string()),
            ..CreateIssue::default()
        }))
    );
    // A blank summary sends nothing.
    app.issue.deliver_action(Ok(()));
    app.ui.invoke_issue_subtask_add("  ".into());
    assert!(app.ui.get_issue_action_error().as_str().contains("title"));
    assert!(matches!(
        app.issue.last_request(),
        Some(Request::Subtask(_))
    ));
}

pub fn issue_links_add_and_remove() {
    let app = loaded(Some("member"), ANA);
    let ui = &app.ui;
    let links = ui.get_issue_links();
    assert_eq!(links.row_data(0).expect("link").heading.as_str(), "blocks");

    ui.invoke_issue_link_add("emb-3".into(), "relates".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::AddLink(CreateLink {
            target: "EMB-3".to_string(),
            kind: "relates".to_string(),
        }))
    );
    app.issue.deliver_action(Ok(()));

    ui.invoke_issue_link_remove("l1".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::RemoveLink("l1".to_string()))
    );
}

pub fn issue_comments_add_and_edit_own_only() {
    let app = loaded(Some("member"), ANA);
    let ui = &app.ui;

    ui.invoke_issue_comment_add("Thanks".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::AddComment("Thanks".to_string()))
    );
    app.issue.deliver_action(Ok(()));

    // Ben's comment (c2) is not Ana's: a member cannot edit it, so the editor does not open.
    ui.invoke_issue_comment_edit_start("c2".into());
    assert!(
        !ui.get_issue_comments().row_data(1).expect("row").editing,
        "not own"
    );
    // Ana's own comment (c1) opens its editor with the body in the draft.
    ui.invoke_issue_comment_edit_start("c1".into());
    let own = ui.get_issue_comments().row_data(0).expect("row");
    assert!(own.editing && own.changeable);
    assert_eq!(ui.get_issue_comment_edit_draft().as_str(), "First");
    ui.invoke_issue_comment_edit_save("c1".into(), "First, edited".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::EditComment {
            id: "c1".to_string(),
            body: "First, edited".to_string(),
        })
    );
    app.issue.deliver_action(Ok(()));

    // Delete of Ben's comment is refused for a member: the last request is still the edit.
    ui.invoke_issue_comment_delete("c2".into());
    assert!(matches!(
        app.issue.last_request(),
        Some(Request::EditComment { .. })
    ));
}

pub fn issue_activity_lines_read_as_sentences() {
    let app = loaded(Some("member"), ANA);
    let activity = app.ui.get_issue_activity();
    assert_eq!(activity.row_count(), 2);
    let created = activity.row_data(0).expect("created line");
    assert!(
        created
            .as_str()
            .starts_with("Ana Silva created the issue · "),
        "{created}"
    );
    let changed = activity.row_data(1).expect("state line");
    assert!(
        changed
            .as_str()
            .starts_with("Ana Silva changed State from Open to In Progress · "),
        "the state ids are shown as names: {changed}"
    );
}

pub fn issue_viewer_and_non_member_see_it_read_only() {
    for role in [Some("viewer"), None] {
        let app = loaded(role, ANA);
        let ui = &app.ui;
        assert!(!ui.get_issue_can_edit(), "{role:?} cannot edit");
        assert!(!ui.get_issue_can_delete());
        ui.invoke_issue_edit_open();
        assert!(!ui.get_new_issue_edit_mode(), "{role:?}: no edit modal");
        ui.invoke_issue_comment_add("hello".into());
        ui.invoke_issue_subtask_add("Nope".into());
        ui.invoke_issue_link_add("EMB-3".into(), "blocks".into());
        ui.invoke_issue_delete_ask();
        assert_eq!(
            app.issue.last_request(),
            None,
            "{role:?}: no write is recorded"
        );
        assert!(!ui.get_issue_confirm_delete());
        assert_eq!(
            ui.get_issue_comments().row_count(),
            2,
            "the comments are still shown"
        );
    }
}

pub fn issue_delete_needs_a_confirm_step_and_an_admin() {
    // A member cannot ask to delete.
    let member = loaded(Some("member"), ANA);
    member.ui.invoke_issue_delete_ask();
    assert!(!member.ui.get_issue_confirm_delete());

    let app = loaded(Some("admin"), ANA);
    let ui = &app.ui;
    assert!(ui.get_issue_can_delete());
    // Confirming before asking sends nothing.
    ui.invoke_issue_delete_confirm();
    assert_eq!(app.issue.last_request(), None);

    ui.invoke_issue_delete_ask();
    assert!(ui.get_issue_confirm_delete(), "the confirm step shows");
    assert_eq!(app.issue.last_request(), None, "asking sends nothing");
    ui.invoke_issue_delete_cancel();
    assert!(!ui.get_issue_confirm_delete());

    ui.invoke_issue_delete_ask();
    ui.invoke_issue_delete_confirm();
    assert_eq!(app.issue.last_request(), Some(Request::Delete));
    assert!(ui.get_issue_saving());
}

pub fn issue_vote_and_star_send_their_requests_for_any_role() {
    // A viewer and a person with no role may vote and star, as they may see the issue.
    for role in [Some("viewer"), None] {
        let app = loaded(role, ANA);
        let ui = &app.ui;
        assert_eq!(ui.get_issue_view().votes, 0);
        assert!(!ui.get_issue_view().voted && !ui.get_issue_view().starred);

        ui.invoke_issue_vote(true);
        assert_eq!(
            app.issue.last_request(),
            Some(Request::Vote(true)),
            "{role:?}"
        );
        assert!(ui.get_issue_saving(), "the vote is on its way");
        app.issue.deliver_action(Ok(()));
        assert!(!ui.get_issue_saving());

        ui.invoke_issue_star(true);
        assert_eq!(
            app.issue.last_request(),
            Some(Request::Star(true)),
            "{role:?}"
        );
        app.issue.deliver_action(Ok(()));

        ui.invoke_issue_vote(false);
        assert_eq!(app.issue.last_request(), Some(Request::Vote(false)));
    }
}

pub fn issue_add_work_item_sends_the_entry_and_refuses_bad_minutes() {
    let app = loaded(Some("member"), ANA);
    let ui = &app.ui;
    let work = ui.get_issue_work();
    assert_eq!(work.row_count(), 1, "one spent-time entry is listed");
    assert_eq!(work.row_data(0).expect("entry").minutes.as_str(), "1h 30m");
    assert_eq!(ui.get_issue_view().spent.as_str(), "1h 30m", "the sum");

    ui.invoke_issue_work_add("45".into(), "2026-11-15".into(), " tests ".into());
    assert_eq!(
        app.issue.last_request(),
        Some(Request::AddWorkItem(CreateWorkItem {
            minutes: 45,
            spent_on: Some("2026-11-15".to_string()),
            note: Some("tests".to_string()),
        }))
    );
    app.issue.deliver_action(Ok(()));

    // Minutes must be a whole number; a blank date is today, so it sends nothing bad.
    ui.invoke_issue_work_add("many".into(), "".into(), "".into());
    assert!(
        ui.get_issue_action_error()
            .as_str()
            .contains("whole number")
    );
    assert!(!ui.get_issue_saving(), "a refused entry sends nothing");
    ui.invoke_issue_work_add("30".into(), "15/11/2026".into(), "".into());
    assert!(ui.get_issue_action_error().as_str().contains("YYYY-MM-DD"));

    // A viewer cannot record time: no request is recorded.
    let viewer = loaded(Some("viewer"), ANA);
    viewer
        .ui
        .invoke_issue_work_add("30".into(), "".into(), "".into());
    assert_eq!(viewer.issue.last_request(), None);
}

pub fn issue_prev_and_next_follow_the_list_it_was_opened_from() {
    let app = loaded(Some("member"), ANA);
    let ui = &app.ui;
    assert_eq!(
        ui.get_issue_view().position.as_str(),
        "",
        "no list yet, so the arrows are hidden"
    );

    // The list of EMB, with EMB-12 in the middle.
    ui.invoke_issues_open("/p/EMB/issues".into());
    // The list's `init` runs on the next tick, which is what asks it for its project.
    wait(10);
    let items = ["EMB-11", "EMB-12", "EMB-13"]
        .iter()
        .map(|key| summary(key, "x"))
        .collect();
    app.issues.deliver(
        app.issues.generation(),
        Ok(Page {
            items,
            next_cursor: None,
        }),
    );

    ui.invoke_issues_open(ROUTE.into());
    wait(10);
    let view = ui.get_issue_view();
    assert_eq!(view.position.as_str(), "2 of 3");
    assert!(view.has_prev && view.has_next, "both arrows are enabled");

    ui.invoke_issue_next();
    assert_eq!(ui.get_page_route(), "/p/EMB/issues/EMB-13");
    ui.invoke_issues_open(ROUTE.into());
    wait(10);
    ui.invoke_issue_prev();
    assert_eq!(ui.get_page_route(), "/p/EMB/issues/EMB-11");
}
