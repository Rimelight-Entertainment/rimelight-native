//! Projects > Overview: the summary of one project at `/p/:key` (the first row of the Projects sidebar section).
//!
//! The page loads on the page data pattern (`space_page_kit::page_data`): the project, its issues (every page, up to
//! [`MAX_PAGES`]), its sprints and the issue areas. From those it shows a project card (name, key, description, lead,
//! members), the issue counts by state and by priority with open against closed, the [`RECENT`] most recently updated
//! issues (a row opens the issue), the current sprint, and the areas with their colours. The project's created date is not
//! in the API, so the card does not show it. The page's markup is `ui/overview.slint`.
//!
//! Plain Rust. The window side is [`OverviewWindow`], which `wire_projects!` implements for the generated window.

use std::sync::{Arc, Mutex, MutexGuard};

use slint::Weak;
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_client::projects::IssueFilter;
use space_proto::projects::{Area, IssueSummary, PRIORITIES, Project, Sprint, WorkflowState};

use super::issues::{IssueRowView, PAGE_SIZE, colour_rgb, issue_route, relative_time};
use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataCell};
use space_page_kit::person::initials;

/// The most issue pages the counts read (each of [`PAGE_SIZE`] issues). A larger project shows its counts as partial.
pub const MAX_PAGES: usize = 20;
/// The recently updated issues the page lists.
pub const RECENT: usize = 5;

/// The route of one project's overview, `/p/:key`.
pub fn overview_route(key: &str) -> String {
    format!("/p/{key}")
}

/// The project key of the overview route (`/p/<key>`), or `None` for any other route.
pub fn project_of(route: &str) -> Option<&str> {
    let key = route.strip_prefix("/p/")?;
    let plain = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    plain.then_some(key)
}

/// Whether a workflow category is still open. `done` and `cancelled` are closed.
pub fn is_open(category: &str) -> bool {
    !matches!(category, "Done" | "Cancelled")
}

/// One line of a count list: a label and how many issues it counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountRow {
    pub label: String,
    pub count: usize,
}

/// The issue counts of a project: the total, open against closed, and the counts by state (in workflow order) and by
/// priority (most urgent first, every priority listed even at zero).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub total: usize,
    pub open: usize,
    pub closed: usize,
    pub by_state: Vec<CountRow>,
    pub by_priority: Vec<CountRow>,
}

/// Counts the issues. `workflow` gives the order of the state rows; a state no workflow column names is added at the end.
pub fn stats(issues: &[IssueSummary], workflow: &[WorkflowState]) -> Stats {
    let mut by_state: Vec<CountRow> = workflow
        .iter()
        .map(|state| CountRow {
            label: state.name.clone(),
            count: 0,
        })
        .collect();
    for issue in issues {
        match by_state
            .iter_mut()
            .find(|row| row.label == issue.state.name)
        {
            Some(row) => row.count += 1,
            None => by_state.push(CountRow {
                label: issue.state.name.clone(),
                count: 1,
            }),
        }
    }
    let by_priority = PRIORITIES
        .iter()
        .map(|&priority| CountRow {
            label: priority.to_string(),
            count: issues
                .iter()
                .filter(|issue| issue.priority == priority)
                .count(),
        })
        .collect();
    let open = issues
        .iter()
        .filter(|issue| is_open(&issue.state.category))
        .count();
    Stats {
        total: issues.len(),
        open,
        closed: issues.len() - open,
        by_state,
        by_priority,
    }
}

/// The issues updated most recently, newest first, at most [`RECENT`].
pub fn recent(issues: &[IssueSummary]) -> Vec<&IssueSummary> {
    let mut sorted: Vec<&IssueSummary> = issues.iter().collect();
    sorted.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.key.cmp(&b.key))
    });
    sorted.truncate(RECENT);
    sorted
}

/// The current sprint: the one that is active, if any.
pub fn current_sprint(sprints: &[Sprint]) -> Option<&Sprint> {
    sprints.iter().find(|sprint| sprint.state == "Active")
}

/// How many issues carry an area's name as a tag (names are compared without regard to case).
pub fn area_issue_count(area: &Area, issues: &[IssueSummary]) -> usize {
    issues
        .iter()
        .filter(|issue| {
            issue
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case(&area.name))
        })
        .count()
}

/// What one load brings back: the project, its issues (`truncated` when the issue pages stopped at [`MAX_PAGES`]), its
/// sprints and the issue areas.
pub struct Summary {
    project: Project,
    issues: Vec<IssueSummary>,
    truncated: bool,
    sprints: Vec<Sprint>,
    areas: Vec<Area>,
}

/// Reads everything the overview shows. Issues are read page by page, so the counts cover the whole project.
async fn load(api: &ApiClient, token: &str, key: &str) -> Result<Summary, ClientError> {
    let project = api.project(token, key).await?;
    let mut issues = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    let mut truncated = false;
    loop {
        let filter = IssueFilter {
            cursor: cursor.take(),
            limit: Some(PAGE_SIZE),
            ..IssueFilter::default()
        };
        let page = api.issues(token, key, &filter).await?;
        issues.extend(page.items);
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
        if pages >= MAX_PAGES {
            truncated = true;
            break;
        }
    }
    let sprints = api.sprints(token, key).await?;
    let areas = api.areas(token).await?;
    Ok(Summary {
        project,
        issues,
        truncated,
        sprints,
        areas,
    })
}

/// One count line as the window draws it: the label, the count, and the bar fraction of the total.
#[derive(Debug, Clone, PartialEq)]
pub struct CountView {
    pub label: String,
    pub count: String,
    pub fraction: f32,
}

/// The current sprint's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintView {
    pub name: String,
    pub dates: String,
    pub goal: String,
    pub counts: String,
}

/// One issue area row: its name, its colour as red, green and blue bytes, and its issue count text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaView {
    pub name: String,
    pub colour: (u8, u8, u8),
    pub count: String,
}

/// Everything the overview draws, as plain values.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryView {
    pub project_name: String,
    pub project_key: String,
    pub description: String,
    pub owner: String,
    pub members: String,
    pub total: String,
    pub open: String,
    pub closed: String,
    pub truncated: bool,
    pub states: Vec<CountView>,
    pub priorities: Vec<CountView>,
    pub recent: Vec<IssueRowView>,
    /// `None` when there is no active sprint.
    pub sprint: Option<SprintView>,
    pub areas: Vec<AreaView>,
}

/// The window side of the overview page. `wire_projects!` implements it for the generated window.
pub trait OverviewWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_summary(&self, view: SummaryView);
    /// Opens the issues page at `route` (the app shell's issues-open).
    fn open_issues(&self, route: String);
    /// Whether a route is the overview's route. The window asks through a callback.
    fn on_is_route(&self, f: impl Fn(String) -> bool + 'static);
    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_open_issue(&self, f: impl Fn(String) + 'static);
}

/// The overview page. Built by [`wire`]. Clones share the state.
pub struct Overview<W: OverviewWindow> {
    ui: Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
}

impl<W: OverviewWindow> Clone for Overview<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// The request state of the page. Behind a `Mutex`, because a worker's result reads it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A result for an older number is dropped.
    generation: u64,
    /// The key of the project on show. The open-issue action needs it.
    key: String,
}

/// Wires the page's callbacks on `ui`. `data` is the slot the session driver fills; until it is set, a load does nothing and
/// the page stays in its loading state.
pub fn wire<W: OverviewWindow>(ui: &W, data: DataCell) -> Overview<W> {
    let overview = Overview {
        ui: ui.as_weak(),
        data,
        shared: Arc::default(),
    };

    ui.on_is_route(|route| project_of(route.as_str()).is_some());

    let page = overview.clone();
    ui.on_opened(move |route| page.open(route.as_str()));

    let page = overview.clone();
    ui.on_retry(move || {
        let key = lock(&page.shared).key.clone();
        page.open(&overview_route(&key));
    });

    // A recent issue opens at its route in the same page tab (the app shell's issues-open).
    let page = overview.clone();
    ui.on_open_issue(move |key| {
        if let Some(ui) = page.ui.upgrade() {
            let project = lock(&page.shared).key.clone();
            ui.open_issues(issue_route(&project, &key));
        }
    });

    overview
}

impl<W: OverviewWindow> Overview<W> {
    /// Opens the overview at `route` (`/p/:key`) and loads it.
    fn open(&self, route: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(key) = project_of(route).map(str::to_owned) else {
            return;
        };
        let generation = {
            let mut s = lock(&self.shared);
            s.generation += 1;
            s.key = key.clone();
            s.generation
        };
        ui.show_state(LoadState::Loading);
        ui.show_error("");

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let shared = self.shared.clone();
        source.fetch(
            &self.ui,
            move |api, token| async move { load(&api, &token, &key).await },
            move |ui, result| deliver(ui, &shared, generation, result),
        );
    }
}

/// Keeps a result if it is for the latest load, then shows the summary or the error.
fn deliver<W: OverviewWindow>(
    ui: &W,
    shared: &Mutex<Shared>,
    generation: u64,
    result: Result<Summary, ClientError>,
) {
    if lock(shared).generation != generation {
        return;
    }
    match result {
        Ok(summary) => {
            ui.show_summary(view(&summary, chrono::Utc::now().timestamp()));
            ui.show_error("");
            ui.show_state(LoadState::Loaded);
        }
        Err(error) => {
            ui.show_error(&page_data::error_text(&error));
            ui.show_state(LoadState::Error);
        }
    }
}

/// The summary as the window draws it.
fn view(summary: &Summary, now: i64) -> SummaryView {
    let project = &summary.project;
    let counts = stats(&summary.issues, &project.workflow);

    let recent = recent(&summary.issues)
        .into_iter()
        .map(|issue| IssueRowView {
            key: issue.key.clone(),
            issue_type: issue.issue_type.clone(),
            summary: issue.summary.clone(),
            state: issue.state.name.clone(),
            priority: issue.priority.clone(),
            assignee: issue
                .assignee
                .as_ref()
                .map(|user| initials(&user.display_name))
                .unwrap_or_default(),
            updated: relative_time(issue.updated_at, now),
        })
        .collect();

    let sprint = current_sprint(&summary.sprints).map(|sprint| {
        let in_sprint: Vec<&IssueSummary> = summary
            .issues
            .iter()
            .filter(|issue| issue.sprint_id.as_deref() == Some(sprint.id.as_str()))
            .collect();
        let closed = in_sprint
            .iter()
            .filter(|issue| !is_open(&issue.state.category))
            .count();
        SprintView {
            name: sprint.name.clone(),
            dates: format!("{} to {}", sprint.start_date, sprint.end_date),
            goal: sprint.goal.clone(),
            counts: format!("{} issues, {closed} closed", in_sprint.len()),
        }
    });

    let areas = summary
        .areas
        .iter()
        .map(|area| AreaView {
            name: area.name.clone(),
            colour: colour_rgb(&area.color),
            count: format!("{} issues", area_issue_count(area, &summary.issues)),
        })
        .collect();

    SummaryView {
        project_name: project.name.clone(),
        project_key: project.key.clone(),
        description: project.description.clone(),
        owner: project
            .lead
            .as_ref()
            .map(|lead| lead.display_name.clone())
            .unwrap_or_else(|| "Not set".to_owned()),
        members: format!("{} members", project.members.len()),
        total: counts.total.to_string(),
        open: counts.open.to_string(),
        closed: counts.closed.to_string(),
        truncated: summary.truncated,
        states: counts
            .by_state
            .iter()
            .map(|row| count_view(row, counts.total))
            .collect(),
        priorities: counts
            .by_priority
            .iter()
            .map(|row| count_view(row, counts.total))
            .collect(),
        recent,
        sprint,
        areas,
    }
}

/// One count line: the label, the count, and the bar fraction of the total.
fn count_view(row: &CountRow, total: usize) -> CountView {
    let fraction = match total {
        0 => 0.0,
        total => row.count as f32 / total as f32,
    };
    CountView {
        label: row.label.clone(),
        count: row.count.to_string(),
        fraction,
    }
}

/// Locks the request state. A panic in another holder does not stop the page.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str, category: &str, position: i32) -> WorkflowState {
        WorkflowState {
            id: name.to_lowercase(),
            name: name.to_owned(),
            category: category.to_owned(),
            position,
        }
    }

    fn issue(key: &str, state: &WorkflowState, priority: &str, updated_at: i64) -> IssueSummary {
        IssueSummary {
            id: key.to_owned(),
            key: key.to_owned(),
            summary: format!("Issue {key}"),
            issue_type: "Task".to_owned(),
            priority: priority.to_owned(),
            state: state.clone(),
            assignee: None,
            parent_id: None,
            sprint_id: None,
            estimate_points: None,
            due_date: None,
            tags: Vec::new(),
            backlog_rank: String::new(),
            updated_at,
        }
    }

    fn workflow() -> Vec<WorkflowState> {
        vec![
            state("To do", "Unstarted", 0),
            state("Doing", "Started", 1),
            state("Done", "Done", 2),
        ]
    }

    #[test]
    fn overview_takes_only_the_project_route() {
        assert_eq!(project_of("/p/GT"), Some("GT"));
        assert_eq!(project_of("/p/grand-tale"), Some("grand-tale"));
        assert_eq!(
            project_of("/p/GT/issues"),
            None,
            "a sub-page is not the overview"
        );
        assert_eq!(project_of("/p/"), None);
        assert_eq!(project_of("/p/../x"), None);
        assert_eq!(overview_route("GT"), "/p/GT");
    }

    #[test]
    fn counts_split_open_and_closed_and_follow_the_workflow() {
        let todo = state("To do", "Unstarted", 0);
        let done = state("Done", "Done", 2);
        let issues = vec![
            issue("GT-1", &todo, "High", 10),
            issue("GT-2", &todo, "Critical", 20),
            issue("GT-3", &done, "High", 30),
        ];
        let counts = stats(&issues, &workflow());
        assert_eq!((counts.total, counts.open, counts.closed), (3, 2, 1));
        let states: Vec<(&str, usize)> = counts
            .by_state
            .iter()
            .map(|row| (row.label.as_str(), row.count))
            .collect();
        assert_eq!(states, [("To do", 2), ("Doing", 0), ("Done", 1)]);
        let priorities: Vec<(&str, usize)> = counts
            .by_priority
            .iter()
            .map(|row| (row.label.as_str(), row.count))
            .collect();
        assert_eq!(
            priorities,
            [("Critical", 1), ("High", 2), ("Normal", 0), ("Low", 0)]
        );
    }

    #[test]
    fn recent_is_the_latest_five_newest_first() {
        let todo = state("To do", "Unstarted", 0);
        let issues: Vec<IssueSummary> = (1..=7)
            .map(|n| issue(&format!("GT-{n}"), &todo, "Low", n * 10))
            .collect();
        let keys: Vec<&str> = recent(&issues).iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["GT-7", "GT-6", "GT-5", "GT-4", "GT-3"]);
    }

    #[test]
    fn current_sprint_is_the_active_one() {
        let sprint = |id: &str, state: &str| Sprint {
            id: id.to_owned(),
            name: id.to_owned(),
            goal: String::new(),
            start_date: "2026-11-01".to_owned(),
            end_date: "2026-11-14".to_owned(),
            state: state.to_owned(),
        };
        let sprints = [
            sprint("s1", "Closed"),
            sprint("s2", "Active"),
            sprint("s3", "Planned"),
        ];
        assert_eq!(current_sprint(&sprints).map(|s| s.id.as_str()), Some("s2"));
        assert!(current_sprint(&sprints[..1]).is_none());
    }

    #[test]
    fn area_counts_use_the_tag_name_without_regard_to_case() {
        let area = Area {
            color: "#1659C4".to_owned(),
            id: "a1".to_owned(),
            name: "Combat".to_owned(),
        };
        let todo = state("To do", "Unstarted", 0);
        let mut tagged = issue("GT-1", &todo, "Low", 0);
        tagged.tags = vec!["combat".to_owned()];
        let plain = issue("GT-2", &todo, "Low", 0);
        assert_eq!(area_issue_count(&area, &[tagged, plain]), 1);
    }
}
