//! Projects, issues, sprints and backlog contract types (PLAN section 4, S1; decision 0026).
//!
//! Timestamps are unix seconds (UTC). Dates are `YYYY-MM-DD`. Ids are UUIDs in canonical text form.
//! Issues are addressed by key (`EMB-12`): the project key, a dash, and the per-project number.
//!
//! PATCH bodies: an absent field means "unchanged". For text fields an empty string means "clear".
//! For `estimate_points` a JSON `null` clears it (see `double_option`).
//!
//! Enum-like fields are plain strings so that the wire format can grow without a breaking change.
//! The constants below list the values the API accepts.

use serde::{Deserialize, Deserializer, Serialize};

/// The paged-list envelope shared with the people API (`items` and `next_cursor`).
pub use crate::people::Page;

/// Project roles, in `Member::role` and `ProjectSummary::my_role`.
pub const PROJECT_ROLES: &[&str] = &["admin", "member", "viewer"];
/// Workflow state categories.
pub const STATE_CATEGORIES: &[&str] = &["Unstarted", "Started", "Done", "Cancelled"];
/// Issue types.
pub const ISSUE_TYPES: &[&str] = &["Task", "Bug", "Feature", "Epic"];
/// Issue priorities, most urgent first.
pub const PRIORITIES: &[&str] = &["Critical", "High", "Normal", "Low"];
/// Link kinds as stored. The inverse is derived per side (see `IssueLink::label`).
pub const LINK_KINDS: &[&str] = &["relates", "blocks", "duplicates"];
/// Sprint states.
pub const SPRINT_STATES: &[&str] = &["Planned", "Active", "Closed"];
/// Activity kinds.
pub const ACTIVITY_KINDS: &[&str] = &[
    "created",
    "field_changed",
    "comment_added",
    "link_added",
    "link_removed",
];

/// Largest number of characters in a text field, by name.
pub const SUMMARY_MAX: usize = 255;
pub const DESCRIPTION_MAX: usize = 20_000;
pub const COMMENT_MAX: usize = 10_000;
pub const TAG_MAX: usize = 40;
pub const TAGS_MAX: usize = 20;

/// Reads a JSON field that may be absent, `null` or a value. Absent is `None` (via `serde(default)`),
/// `null` is `Some(None)`, a value is `Some(Some(v))`.
fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(de)?))
}

/// A person as shown on a project, issue or comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRef {
    pub id: String,
    pub display_name: String,
}

/// A project as listed by `GET /v1/projects`. `my_role` is the caller's role, or `None` when they
/// are a global admin who is not a member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub key: String,
    pub name: String,
    pub lead: Option<UserRef>,
    pub my_role: Option<String>,
    /// The project picture's address, or `None` when the project has none.
    #[serde(default)]
    pub image_url: Option<String>,
}

/// Reply of `GET /v1/projects/{key}` and of project create and update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub lead: Option<UserRef>,
    pub my_role: Option<String>,
    /// The project picture's address, or `None` when the project has none.
    #[serde(default)]
    pub image_url: Option<String>,
    /// Workflow states in position order.
    pub workflow: Vec<WorkflowState>,
    /// Members in name order.
    pub members: Vec<Member>,
}

/// Body of `POST /v1/projects` (global admin only). The creator becomes a project admin.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProject {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub lead_id: Option<String>,
}

/// Body of `PATCH /v1/projects/{key}`. A `key` changes the project's short id (2 to 10 letters or digits, starting with a letter).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProject {
    pub key: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Empty string clears the lead.
    pub lead_id: Option<String>,
}

/// A project member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user: UserRef,
    pub role: String,
}

/// Body of `PUT /v1/projects/{key}/members/{user_id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetMember {
    pub role: String,
}

/// One column of a project's workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    pub category: String,
    pub position: i32,
}

/// Body of `POST /v1/projects/{key}/workflow`. `position` defaults to the end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateState {
    pub name: String,
    pub category: String,
    pub position: Option<i32>,
}

/// Body of `PATCH /v1/projects/{key}/workflow/{state_id}`. Rename, change category, or reorder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateState {
    pub name: Option<String>,
    pub category: Option<String>,
    pub position: Option<i32>,
}

/// Short form of an issue, used in lists, parent and subtask lists, and link targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueSummary {
    pub id: String,
    pub key: String,
    pub summary: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub priority: String,
    pub state: WorkflowState,
    pub assignee: Option<UserRef>,
    pub parent_id: Option<String>,
    pub sprint_id: Option<String>,
    pub estimate_points: Option<u32>,
    pub due_date: Option<String>,
    pub tags: Vec<String>,
    pub backlog_rank: String,
    pub updated_at: i64,
}

/// Sprint as shown on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SprintSummary {
    pub id: String,
    pub name: String,
    pub state: String,
}

/// A link from one issue to another, seen from the issue it is read on.
/// `direction` is `outward` when this issue is the link's source, else `inward`.
/// `label` is the sentence for this side, e.g. `blocks` or `is blocked by`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueLink {
    pub id: String,
    pub kind: String,
    pub direction: String,
    pub label: String,
    pub issue: IssueSummary,
}

/// Full issue, from `GET /v1/issues/{key}` and create and update replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub key: String,
    pub summary: String,
    pub description: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub priority: String,
    pub state: WorkflowState,
    pub assignee: Option<UserRef>,
    pub reporter: UserRef,
    pub parent: Option<IssueSummary>,
    pub subtasks: Vec<IssueSummary>,
    pub links: Vec<IssueLink>,
    pub sprint: Option<SprintSummary>,
    pub estimate_points: Option<u32>,
    pub due_date: Option<String>,
    pub tags: Vec<String>,
    pub backlog_rank: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    /// Who last changed the issue with a PATCH. `None` until then.
    #[serde(default)]
    pub updated_by: Option<UserRef>,
    /// Number of votes, and whether the caller has voted (S1.5a).
    #[serde(default)]
    pub votes: u32,
    #[serde(default)]
    pub voted_by_me: bool,
    #[serde(default)]
    pub starred_by_me: bool,
    /// Sum of the live spent-time entries, in minutes.
    #[serde(default)]
    pub spent_minutes: u32,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

/// A file attached to an issue. `url` is the path to download it from (`GET`), relative to the API base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub url: String,
}

/// Largest attachment accepted, in bytes (10 MB).
pub const ATTACHMENT_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Spent time on an issue. `spent_on` is `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: String,
    pub minutes: u32,
    pub spent_on: String,
    pub note: String,
    pub author: UserRef,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Body of `POST /v1/issues/{key}/work-items`. `spent_on` defaults to today (UTC), `note` to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkItem {
    pub minutes: u32,
    pub spent_on: Option<String>,
    pub note: Option<String>,
}

/// Reply of `PUT` and `DELETE /v1/issues/{key}/vote`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteState {
    pub votes: u32,
    pub voted_by_me: bool,
}

/// Reply of `PUT` and `DELETE /v1/issues/{key}/star`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarState {
    pub starred_by_me: bool,
}

/// Body of `POST /v1/projects/{key}/issues`. `state_id` defaults to the first unstarted state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    pub summary: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub issue_type: Option<String>,
    pub priority: Option<String>,
    pub state_id: Option<String>,
    pub assignee_id: Option<String>,
    pub parent_id: Option<String>,
    pub sprint_id: Option<String>,
    pub estimate_points: Option<u32>,
    pub due_date: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Body of `PATCH /v1/issues/{key}`. Absent fields are unchanged. Empty text clears a nullable field.
///
/// Absent fields are left out when serialized too: a `null` for `estimate_points` means "clear", so writing an absent
/// field as `null` would wipe it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprint_id: Option<String>,
    /// Absent: unchanged. `null`: clear. A number: set.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub estimate_points: Option<Option<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Body of `POST /v1/issues/{key}/links`. `target` is another issue's key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLink {
    pub target: String,
    pub kind: String,
}

/// A comment on an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub author: UserRef,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Body of `POST /v1/issues/{key}/comments` and `PATCH` (the same shape).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentBody {
    pub body: String,
}

/// One entry of an issue's activity log. `actor` is `None` for system events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: String,
    pub kind: String,
    pub field: Option<String>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub actor: Option<UserRef>,
    pub created_at: i64,
}

/// A sprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sprint {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub start_date: String,
    pub end_date: String,
    pub state: String,
}

/// Body of `POST /v1/projects/{key}/sprints`. `state` defaults to `planned`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSprint {
    pub name: String,
    pub goal: Option<String>,
    pub start_date: String,
    pub end_date: String,
    pub state: Option<String>,
}

/// Body of `PATCH /v1/projects/{key}/sprints/{sprint_id}`.
///
/// Starting a sprint (`state: active`) is refused with 409 while another sprint is active.
/// Closing a sprint may move its unfinished issues: `move_unfinished_to` is a sprint id, or
/// `backlog` to clear the sprint. When absent the unfinished issues stay in the closed sprint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSprint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_unfinished_to: Option<String>,
}

/// Body of `POST /v1/projects/{key}/backlog/rank`. Places `issue` directly after `after` and/or
/// directly before `before` (issue keys). With neither, the issue moves to the top.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankIssue {
    pub issue: String,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, value, "round trip changed the value: {json}");
    }

    fn state() -> WorkflowState {
        WorkflowState {
            id: "s1".to_string(),
            name: "Open".to_string(),
            category: "Unstarted".to_string(),
            position: 1,
        }
    }

    #[test]
    fn issue_round_trips_and_uses_type_on_the_wire() {
        let summary = IssueSummary {
            id: "i2".to_string(),
            key: "EMB-2".to_string(),
            summary: "Child".to_string(),
            issue_type: "Bug".to_string(),
            priority: "High".to_string(),
            state: state(),
            assignee: Some(UserRef {
                id: "u1".to_string(),
                display_name: "A".to_string(),
            }),
            parent_id: Some("i1".to_string()),
            sprint_id: None,
            estimate_points: Some(3),
            due_date: Some("2026-12-01".to_string()),
            tags: vec!["ui".to_string()],
            backlog_rank: "00000001".to_string(),
            updated_at: 1_800_000_000,
        };
        let json = serde_json::to_string(&summary).expect("serialize");
        assert!(json.contains("\"type\":\"Bug\""), "{json}");
        round_trip(&summary);
        round_trip(&Issue {
            id: "i1".to_string(),
            key: "EMB-1".to_string(),
            summary: "Parent".to_string(),
            description: String::new(),
            issue_type: "Epic".to_string(),
            priority: "Normal".to_string(),
            state: state(),
            assignee: None,
            reporter: UserRef {
                id: "u2".to_string(),
                display_name: "B".to_string(),
            },
            parent: None,
            subtasks: vec![summary.clone()],
            links: vec![IssueLink {
                id: "l1".to_string(),
                kind: "blocks".to_string(),
                direction: "inward".to_string(),
                label: "is blocked by".to_string(),
                issue: summary,
            }],
            sprint: Some(SprintSummary {
                id: "sp".to_string(),
                name: "Sprint 1".to_string(),
                state: "Active".to_string(),
            }),
            estimate_points: None,
            due_date: None,
            tags: vec![],
            backlog_rank: "00000002".to_string(),
            created_at: 1,
            updated_at: 2,
            resolved_at: None,
            updated_by: Some(UserRef {
                id: "u1".to_string(),
                display_name: "A".to_string(),
            }),
            votes: 3,
            voted_by_me: true,
            starred_by_me: false,
            spent_minutes: 90,
            attachments: vec![Attachment {
                id: "a1".to_string(),
                file_name: "log.txt".to_string(),
                content_type: "text/plain".to_string(),
                size_bytes: 12,
                url: "/v1/issues/EMB-1/attachments/a1".to_string(),
            }],
        });
        round_trip(&WorkItem {
            id: "w1".to_string(),
            minutes: 30,
            spent_on: "2026-11-15".to_string(),
            note: "Repro".to_string(),
            author: UserRef {
                id: "u1".to_string(),
                display_name: "A".to_string(),
            },
            created_at: 1,
            updated_at: 1,
        });
    }

    #[test]
    fn activity_keeps_json_values() {
        round_trip(&ActivityEntry {
            id: "a".to_string(),
            kind: "field_changed".to_string(),
            field: Some("priority".to_string()),
            old_value: Some(serde_json::json!("Normal")),
            new_value: Some(serde_json::json!("High")),
            actor: None,
            created_at: 5,
        });
    }

    #[test]
    fn update_issue_tells_absent_from_null_for_estimate() {
        let absent: UpdateIssue = serde_json::from_str("{}").expect("empty patch");
        assert_eq!(absent.estimate_points, None);
        // An absent field is not written, so a patch of one field does not clear the others.
        let json = serde_json::to_string(&UpdateIssue::default()).expect("serialize");
        assert_eq!(json, "{}");
        let cleared: UpdateIssue =
            serde_json::from_str(r#"{"estimate_points":null}"#).expect("null clears");
        assert_eq!(cleared.estimate_points, Some(None));
        let set: UpdateIssue =
            serde_json::from_str(r#"{"estimate_points":5}"#).expect("number sets");
        assert_eq!(set.estimate_points, Some(Some(5)));
    }

    #[test]
    fn update_sprint_leaves_absent_fields_out_of_the_body() {
        // Only the named field is written, so starting a sprint does not send `name: null` and the others.
        let start = UpdateSprint {
            state: Some("Active".to_string()),
            ..UpdateSprint::default()
        };
        let json = serde_json::to_string(&start).expect("serialize");
        assert_eq!(json, r#"{"state":"Active"}"#);
        let close = UpdateSprint {
            state: Some("Closed".to_string()),
            move_unfinished_to: Some("backlog".to_string()),
            ..UpdateSprint::default()
        };
        let json = serde_json::to_string(&close).expect("serialize");
        assert_eq!(json, r#"{"state":"Closed","move_unfinished_to":"backlog"}"#);
        assert_eq!(
            serde_json::to_string(&UpdateSprint::default()).expect("serialize"),
            "{}"
        );
    }

    #[test]
    fn bodies_refuse_unknown_fields() {
        assert!(serde_json::from_str::<UpdateIssue>(r#"{"reporter_id":"x"}"#).is_err());
        assert!(serde_json::from_str::<CreateProject>(r#"{"key":"A","name":"a","x":1}"#).is_err());
        assert!(serde_json::from_str::<RankIssue>(r#"{"issue":"EMB-1","x":1}"#).is_err());
    }

    #[test]
    fn page_of_issue_summaries_round_trips() {
        round_trip(&Page::<IssueSummary> {
            items: vec![],
            next_cursor: Some("50".to_string()),
        });
    }
}

/// An issue area: an admin-created name an issue can carry (its tags). `GET /v1/areas` lists them by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    /// The colour that shows the area, as `#RRGGBB`.
    #[serde(default = "default_colour")]
    pub color: String,
    pub id: String,
    pub name: String,
}

/// Body of `POST /v1/admin/areas` (the `admin` role). Names are unique without regard to case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateArea {
    pub name: String,
    /// The colour as `#RRGGBB`. Absent keeps the server default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// The colour an area or issue type has until an admin picks one.
fn default_colour() -> String {
    "#1659C4".to_owned()
}

/// An issue type and its colour, from `GET /v1/issue-types`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueType {
    pub name: String,
    pub color: String,
}

/// Body of `PATCH /v1/admin/areas/{id}` and `PATCH /v1/admin/issue-types/{name}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetColour {
    pub color: String,
}
