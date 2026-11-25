//! Typed client for the projects and issues endpoints (S1.3): projects and their members and workflow, the issue list and
//! search (YouTrack-style `q`, decision 0028), issue reads and writes, comments, links, activity and sprints.
//!
//! Every call takes the session token. Keys and ids are checked to be plain path segments before they reach the URL, so a
//! bad value is refused without a request. A search the server rejects with 400 `invalid_query` comes back as
//! [`ClientError::InvalidQuery`], with the span the UI underlines.

use space_proto::projects::{
    ActivityEntry, Area, Attachment, Comment, CommentBody, CreateArea, CreateIssue, CreateLink,
    CreateProject, CreateSprint, CreateWorkItem, Issue, IssueSummary, IssueType, Member, Page,
    Project, ProjectSummary, RankIssue, SetColour, SetMember, Sprint, StarState, UpdateIssue,
    UpdateProject, UpdateSprint, VoteState, WorkItem, WorkflowState,
};

use crate::api::{ApiClient, Method, RawBody, encode_query_value};
use crate::error::ClientError;

/// Filters, sort and paging for `GET /v1/projects/{key}/issues`. Empty fields are left out of the query string.
/// `q` is a YouTrack-style query (decision 0028). The server refuses the structured filters together with `q`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueFilter {
    pub q: Option<String>,
    pub state_id: Option<String>,
    pub assignee_id: Option<String>,
    pub issue_type: Option<String>,
    pub priority: Option<String>,
    /// A sprint id, or `none` for the backlog.
    pub sprint: Option<String>,
    /// The parent issue's key.
    pub parent: Option<String>,
    /// `number` (default), `-number`, `rank`, `priority` or `updated`. Sent only when set.
    pub sort: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl IssueFilter {
    /// The query string, with a leading `?` when there is anything to send.
    pub fn to_query_string(&self) -> String {
        let fields = [
            ("q", self.q.as_deref()),
            ("state", self.state_id.as_deref()),
            ("assignee", self.assignee_id.as_deref()),
            ("type", self.issue_type.as_deref()),
            ("priority", self.priority.as_deref()),
            ("sprint", self.sprint.as_deref()),
            ("parent", self.parent.as_deref()),
            ("sort", self.sort.as_deref()),
            ("cursor", self.cursor.as_deref()),
        ];
        let limit = self.limit.map(|l| l.to_string());
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        for (key, value) in fields {
            if let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) {
                pairs.push((key, value));
            }
        }
        if let Some(limit) = &limit {
            pairs.push(("limit", limit));
        }
        join_query(&pairs)
    }
}

/// Joins `key=value` pairs into a query string with a leading `?`, or an empty string when there are none.
fn join_query(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in pairs {
        out.push(if out.is_empty() { '?' } else { '&' });
        out.push_str(key);
        out.push('=');
        out.push_str(&encode_query_value(value));
    }
    out
}

/// A project key as a path segment: 2 to 10 ASCII letters or digits.
fn project_segment(key: &str) -> Result<&str, ClientError> {
    if (2..=10).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(key)
    } else {
        Err(ClientError::Protocol(format!(
            "not a valid project key: {key}"
        )))
    }
}

/// An issue key as a path segment, such as `EMB-12`: letters, digits and one dash.
fn issue_segment(key: &str) -> Result<&str, ClientError> {
    let valid = key.contains('-')
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !key.starts_with('-')
        && !key.ends_with('-');
    if valid {
        Ok(key)
    } else {
        Err(ClientError::Protocol(format!(
            "not a valid issue key: {key}"
        )))
    }
}

/// A UUID id as a path segment (hex digits and dashes).
fn id_segment(id: &str) -> Result<&str, ClientError> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        Ok(id)
    } else {
        Err(ClientError::Protocol(format!("not a valid id: {id}")))
    }
}

/// An issue type's stored name (`bug`, `task`) as a path segment. Names are words, not ids, so `id_segment` (hex digits
/// and dashes only) refuses them.
fn name_segment(name: &str) -> Result<&str, ClientError> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(name)
    } else {
        Err(ClientError::Protocol(format!("not a valid name: {name}")))
    }
}

fn to_json<T: serde::Serialize>(body: &T) -> Result<String, ClientError> {
    serde_json::to_string(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

fn parse_body<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, ClientError> {
    serde_json::from_str(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

impl ApiClient {
    /// `GET /v1/projects`: the projects the caller is a member of (every project for a global admin).
    /// `POST /v1/projects`: creates a project; the caller becomes its admin. Returns the new project.
    pub async fn create_project(
        &self,
        token: &str,
        body: &CreateProject,
    ) -> Result<Project, ClientError> {
        let reply = self
            .send(
                Method::Post,
                "/v1/projects",
                Some(token),
                Some(to_json(body)?),
            )
            .await?;
        parse_body(&reply.body)
    }

    pub async fn projects(&self, token: &str) -> Result<Vec<ProjectSummary>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/projects", Some(token), None)
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}`: one project with its workflow and members.
    pub async fn project(&self, token: &str, key: &str) -> Result<Project, ClientError> {
        let path = format!("/v1/projects/{}", project_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/members`: the members, in name order.
    pub async fn project_members(
        &self,
        token: &str,
        key: &str,
    ) -> Result<Vec<Member>, ClientError> {
        let path = format!("/v1/projects/{}/members", project_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/workflow`: the workflow states in position order.
    pub async fn workflow_states(
        &self,
        token: &str,
        key: &str,
    ) -> Result<Vec<WorkflowState>, ClientError> {
        let path = format!("/v1/projects/{}/workflow", project_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/sprints`: the sprints of a project.
    pub async fn sprints(&self, token: &str, key: &str) -> Result<Vec<Sprint>, ClientError> {
        let path = format!("/v1/projects/{}/sprints", project_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/projects/{key}`: changes the fields the body names (name, description, lead, and the key). Admins only.
    pub async fn update_project(
        &self,
        token: &str,
        key: &str,
        body: &UpdateProject,
    ) -> Result<Project, ClientError> {
        let path = format!("/v1/projects/{}", project_segment(key)?);
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `PUT /v1/projects/{key}/members/{user_id}`: adds a person to the project, or changes their role. Admins only.
    pub async fn set_member(
        &self,
        token: &str,
        key: &str,
        user_id: &str,
        role: &str,
    ) -> Result<Member, ClientError> {
        let path = format!(
            "/v1/projects/{}/members/{}",
            project_segment(key)?,
            id_segment(user_id)?
        );
        let body = SetMember {
            role: role.to_owned(),
        };
        let reply = self
            .send(Method::Put, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/projects/{key}/members/{user_id}`: takes a person off the project. Admins only.
    pub async fn remove_member(
        &self,
        token: &str,
        key: &str,
        user_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/projects/{}/members/{}",
            project_segment(key)?,
            id_segment(user_id)?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `DELETE /v1/admin/areas/{id}`: deletes an area. Admins only. Issues that carry it keep the name.
    pub async fn delete_area(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/areas/{}", id_segment(id)?);
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/issue-types`: the issue types with their colours. Any signed-in person may read them.
    pub async fn issue_types(&self, token: &str) -> Result<Vec<IssueType>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/issue-types", Some(token), None)
            .await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/admin/issue-types/{name}`: sets an issue type's colour. Admins only.
    pub async fn set_issue_type_colour(
        &self,
        token: &str,
        name: &str,
        colour: &str,
    ) -> Result<IssueType, ClientError> {
        let path = format!("/v1/admin/issue-types/{}", name_segment(name)?);
        let body = SetColour {
            color: colour.to_owned(),
        };
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/admin/areas/{id}`: sets an area's colour. Admins only.
    pub async fn set_area_colour(
        &self,
        token: &str,
        id: &str,
        colour: &str,
    ) -> Result<Area, ClientError> {
        let path = format!("/v1/admin/areas/{}", id_segment(id)?);
        let body = SetColour {
            color: colour.to_owned(),
        };
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/areas`: the issue areas, by name. Any signed-in person may read them.
    pub async fn areas(&self, token: &str) -> Result<Vec<Area>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/areas", Some(token), None)
            .await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/admin/areas`: creates an area (the `admin` role). Returns it as stored.
    pub async fn create_area(&self, token: &str, body: &CreateArea) -> Result<Area, ClientError> {
        let reply = self
            .send(
                Method::Post,
                "/v1/admin/areas",
                Some(token),
                Some(to_json(body)?),
            )
            .await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/projects/{key}/sprints`: creates a sprint. Returns it as stored. A second active sprint is refused with 409.
    pub async fn create_sprint(
        &self,
        token: &str,
        key: &str,
        body: &CreateSprint,
    ) -> Result<Sprint, ClientError> {
        let path = format!("/v1/projects/{}/sprints", project_segment(key)?);
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/projects/{key}/sprints/{sprint_id}`: changes the fields the body names. Starting a sprint, and closing one
    /// with `move_unfinished_to`, go through here.
    pub async fn update_sprint(
        &self,
        token: &str,
        key: &str,
        sprint_id: &str,
        body: &UpdateSprint,
    ) -> Result<Sprint, ClientError> {
        let path = format!(
            "/v1/projects/{}/sprints/{}",
            project_segment(key)?,
            id_segment(sprint_id)?
        );
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/backlog`: one page of the issues with no sprint, in rank order. `cursor` is the
    /// `next_cursor` of the previous page.
    pub async fn backlog(
        &self,
        token: &str,
        key: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page<IssueSummary>, ClientError> {
        let limit = limit.map(|l| l.to_string());
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if let Some(cursor) = cursor.map(str::trim).filter(|c| !c.is_empty()) {
            pairs.push(("cursor", cursor));
        }
        if let Some(limit) = &limit {
            pairs.push(("limit", limit));
        }
        let path = format!(
            "/v1/projects/{}/backlog{}",
            project_segment(key)?,
            join_query(&pairs)
        );
        self.issue_page(token, &path).await
    }

    /// `POST /v1/projects/{key}/backlog/rank`: places `body.issue` directly after `body.after` and before `body.before`.
    /// Returns the issue as it now is.
    pub async fn rank_backlog(
        &self,
        token: &str,
        key: &str,
        body: &RankIssue,
    ) -> Result<Issue, ClientError> {
        let path = format!("/v1/projects/{}/backlog/rank", project_segment(key)?);
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/issues`: one page of a project's issues, with filters, sort and `q`.
    pub async fn issues(
        &self,
        token: &str,
        key: &str,
        filter: &IssueFilter,
    ) -> Result<Page<IssueSummary>, ClientError> {
        let path = format!(
            "/v1/projects/{}/issues{}",
            project_segment(key)?,
            filter.to_query_string()
        );
        self.issue_page(token, &path).await
    }

    /// `GET /v1/issues/search?q=`: one page of issues across the caller's projects, for a YouTrack-style query.
    pub async fn search_issues(
        &self,
        token: &str,
        q: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page<IssueSummary>, ClientError> {
        let limit = limit.map(|l| l.to_string());
        let mut pairs: Vec<(&str, &str)> = vec![("q", q)];
        if let Some(cursor) = cursor.map(str::trim).filter(|c| !c.is_empty()) {
            pairs.push(("cursor", cursor));
        }
        if let Some(limit) = &limit {
            pairs.push(("limit", limit));
        }
        let path = format!("/v1/issues/search{}", join_query(&pairs));
        self.issue_page(token, &path).await
    }

    async fn issue_page(&self, token: &str, path: &str) -> Result<Page<IssueSummary>, ClientError> {
        let reply = self.send(Method::Get, path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/issues/{key}`: one issue with its parent, subtasks, links and sprint.
    pub async fn issue(&self, token: &str, key: &str) -> Result<Issue, ClientError> {
        let path = format!("/v1/issues/{}", issue_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/projects/{key}/issues`: creates an issue. Returns it as stored.
    pub async fn create_issue(
        &self,
        token: &str,
        key: &str,
        body: &CreateIssue,
    ) -> Result<Issue, ClientError> {
        let path = format!("/v1/projects/{}/issues", project_segment(key)?);
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/issues/{key}`: changes the fields the body names. Returns the updated issue.
    pub async fn update_issue(
        &self,
        token: &str,
        key: &str,
        body: &UpdateIssue,
    ) -> Result<Issue, ClientError> {
        let path = format!("/v1/issues/{}", issue_segment(key)?);
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/issues/{key}`.
    pub async fn delete_issue(&self, token: &str, key: &str) -> Result<(), ClientError> {
        let path = format!("/v1/issues/{}", issue_segment(key)?);
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/issues/{key}/comments`.
    pub async fn comments(&self, token: &str, key: &str) -> Result<Vec<Comment>, ClientError> {
        let path = format!("/v1/issues/{}/comments", issue_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/issues/{key}/comments`: adds a comment as the caller.
    pub async fn add_comment(
        &self,
        token: &str,
        key: &str,
        body: &str,
    ) -> Result<Comment, ClientError> {
        let path = format!("/v1/issues/{}/comments", issue_segment(key)?);
        let json = to_json(&CommentBody {
            body: body.to_string(),
        })?;
        let reply = self
            .send(Method::Post, &path, Some(token), Some(json))
            .await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/issues/{key}/comments/{id}`: edits a comment (its author, or a project admin).
    pub async fn update_comment(
        &self,
        token: &str,
        key: &str,
        comment_id: &str,
        body: &str,
    ) -> Result<Comment, ClientError> {
        let path = format!(
            "/v1/issues/{}/comments/{}",
            issue_segment(key)?,
            id_segment(comment_id)?
        );
        let json = to_json(&CommentBody {
            body: body.to_string(),
        })?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/issues/{key}/comments/{id}`.
    pub async fn delete_comment(
        &self,
        token: &str,
        key: &str,
        comment_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/issues/{}/comments/{}",
            issue_segment(key)?,
            id_segment(comment_id)?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `POST /v1/issues/{key}/links`: links this issue to `body.target` with `body.kind`. Returns this issue as it now is.
    pub async fn add_link(
        &self,
        token: &str,
        key: &str,
        body: &CreateLink,
    ) -> Result<Issue, ClientError> {
        let path = format!("/v1/issues/{}/links", issue_segment(key)?);
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/issues/{key}/links/{link_id}`.
    pub async fn remove_link(
        &self,
        token: &str,
        key: &str,
        link_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/issues/{}/links/{}",
            issue_segment(key)?,
            id_segment(link_id)?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/issues/{key}/activity`: the issue's log, in the order the server sends it.
    pub async fn activity(
        &self,
        token: &str,
        key: &str,
    ) -> Result<Vec<ActivityEntry>, ClientError> {
        let path = format!("/v1/issues/{}/activity", issue_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }
}

impl ApiClient {
    /// `PUT` (on) or `DELETE` (off) `/v1/issues/{key}/vote`: the caller's vote. Returns the new count.
    pub async fn set_vote(
        &self,
        token: &str,
        key: &str,
        on: bool,
    ) -> Result<VoteState, ClientError> {
        let path = format!("/v1/issues/{}/vote", issue_segment(key)?);
        let method = if on { Method::Put } else { Method::Delete };
        let reply = self.send(method, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `PUT` (on) or `DELETE` (off) `/v1/issues/{key}/star`: the caller's star.
    pub async fn set_star(
        &self,
        token: &str,
        key: &str,
        on: bool,
    ) -> Result<StarState, ClientError> {
        let path = format!("/v1/issues/{}/star", issue_segment(key)?);
        let method = if on { Method::Put } else { Method::Delete };
        let reply = self.send(method, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/issues/{key}/work-items`: spent-time entries, oldest first.
    pub async fn work_items(&self, token: &str, key: &str) -> Result<Vec<WorkItem>, ClientError> {
        let path = format!("/v1/issues/{}/work-items", issue_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/issues/{key}/work-items`: records spent time as the caller.
    pub async fn add_work_item(
        &self,
        token: &str,
        key: &str,
        body: &CreateWorkItem,
    ) -> Result<WorkItem, ClientError> {
        let path = format!("/v1/issues/{}/work-items", issue_segment(key)?);
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/issues/{key}/work-items/{id}`.
    pub async fn delete_work_item(
        &self,
        token: &str,
        key: &str,
        work_item_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/issues/{}/work-items/{}",
            issue_segment(key)?,
            id_segment(work_item_id)?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/issues/{key}/attachments`.
    pub async fn attachments(
        &self,
        token: &str,
        key: &str,
    ) -> Result<Vec<Attachment>, ClientError> {
        let path = format!("/v1/issues/{}/attachments", issue_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/issues/{key}/attachments`: the raw file, named by `X-File-Name` (percent-encoded here)
    /// and typed by `Content-Type`. The server refuses more than 10 MB.
    pub async fn upload_attachment(
        &self,
        token: &str,
        key: &str,
        file_name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Attachment, ClientError> {
        let path = format!("/v1/issues/{}/attachments", issue_segment(key)?);
        let raw = RawBody {
            content_type: content_type.to_string(),
            bytes,
            file_name: Some(encode_query_value(file_name)),
        };
        let reply = self.send_raw(Method::Post, &path, Some(token), raw).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/projects/{key}/image`: stores the project's picture from the raw bytes (PNG, JPEG or WebP). Returns the
    /// `image_url` the server gives the picture.
    pub async fn upload_project_image(
        &self,
        token: &str,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<String, ClientError> {
        #[derive(serde::Deserialize)]
        struct ImageReply {
            image_url: String,
        }
        let path = format!("/v1/projects/{}/image", project_segment(key)?);
        let raw = RawBody {
            content_type: content_type.to_string(),
            bytes,
            file_name: None,
        };
        let reply = self.send_raw(Method::Post, &path, Some(token), raw).await?;
        let image: ImageReply = parse_body(&reply.body)?;
        Ok(image.image_url)
    }

    /// `GET /v1/issues/{key}/attachments/{id}`: the file's bytes.
    pub async fn attachment_bytes(
        &self,
        token: &str,
        key: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, ClientError> {
        let path = format!(
            "/v1/issues/{}/attachments/{}",
            issue_segment(key)?,
            id_segment(attachment_id)?
        );
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        Ok(reply.bytes)
    }

    /// `DELETE /v1/issues/{key}/attachments/{id}`.
    pub async fn delete_attachment(
        &self,
        token: &str,
        key: &str,
        attachment_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/issues/{}/attachments/{}",
            issue_segment(key)?,
            id_segment(attachment_id)?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/projects/{key}/issues/similar?summary=`: up to five issues with overlapping words.
    pub async fn similar_issues(
        &self,
        token: &str,
        project_key: &str,
        summary: &str,
    ) -> Result<Vec<IssueSummary>, ClientError> {
        let path = format!(
            "/v1/projects/{}/issues/similar?summary={}",
            project_segment(project_key)?,
            encode_query_value(summary)
        );
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::api::Method;
    use crate::testing::{ScriptedTransport, json_response};

    fn client(
        replies: Vec<Result<crate::api::HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("https://api.test/", transport.clone());
        (api, transport)
    }

    const LINK: &str = "22222222-2222-4222-8222-222222222222";

    const ISSUE: &str = r#"{"id":"11111111-1111-4111-8111-111111111111","key":"EMB-12","summary":"Crash on load","description":"","type":"Bug","priority":"High","state":{"id":"s1","name":"Open","category":"Unstarted","position":1},"assignee":null,"reporter":{"id":"u1","display_name":"Ana"},"parent":null,"subtasks":[],"links":[],"sprint":null,"estimate_points":null,"due_date":null,"tags":[],"backlog_rank":"00000001","created_at":1,"updated_at":2,"resolved_at":null}"#;

    #[test]
    fn filter_sends_only_set_fields_and_encodes_q() {
        let filter = IssueFilter {
            q: Some("assignee: me sort by: -updated".to_string()),
            state_id: Some("  ".to_string()),
            sprint: Some("none".to_string()),
            cursor: Some("50".to_string()),
            limit: Some(25),
            ..IssueFilter::default()
        };
        let query = filter.to_query_string();
        assert!(query.starts_with("?q=assignee%3A%20me"), "{query}");
        assert!(query.contains("&sprint=none"), "{query}");
        assert!(query.contains("&cursor=50"), "{query}");
        assert!(query.ends_with("&limit=25"), "{query}");
        assert!(
            !query.contains("state="),
            "blank values are left out: {query}"
        );
        assert_eq!(IssueFilter::default().to_query_string(), "");
    }

    #[test]
    fn projects_list_and_project_read_use_their_paths() {
        let (api, transport) = client(vec![
            json_response(
                200,
                r#"[{"id":"p1","key":"EMB","name":"Embers","lead":null,"my_role":"member"}]"#,
            ),
            json_response(200, "{}"),
        ]);
        let list = pollster::block_on(api.projects("tok")).expect("list");
        assert_eq!(list[0].key, "EMB");
        assert!(
            pollster::block_on(api.project("tok", "EMB")).is_err(),
            "an empty object is not a project"
        );
        let sent = transport.requests();
        assert_eq!(sent[0].url, "https://api.test/v1/projects");
        assert_eq!(sent[0].bearer.as_deref(), Some("tok"));
        assert_eq!(sent[1].url, "https://api.test/v1/projects/EMB");
    }

    #[test]
    fn issue_list_sends_filters_and_returns_the_page() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"items":[],"next_cursor":"50"}"#,
        )]);
        let filter = IssueFilter {
            state_id: Some("s1".to_string()),
            sort: Some("-updated".to_string()),
            ..IssueFilter::default()
        };
        let page = pollster::block_on(api.issues("tok", "EMB", &filter)).expect("page");
        assert_eq!(page.next_cursor.as_deref(), Some("50"));
        let url = &transport.requests()[0].url;
        assert_eq!(
            url,
            "https://api.test/v1/projects/EMB/issues?state=s1&sort=-updated"
        );
    }

    #[test]
    fn search_sends_q_and_maps_400_to_invalid_query_with_its_span() {
        let body = r#"{"error":"invalid_query","message":"unknown field `staet` (columns 1-6)","span":{"start":0,"end":6}}"#;
        let (api, transport) = client(vec![
            json_response(200, r#"{"items":[],"next_cursor":null}"#),
            json_response(400, body),
        ]);
        pollster::block_on(api.search_issues("tok", "assignee: me", None, Some(10))).expect("ok");
        let url = &transport.requests()[0].url;
        assert_eq!(
            url,
            "https://api.test/v1/issues/search?q=assignee%3A%20me&limit=10"
        );

        let err = pollster::block_on(api.search_issues("tok", "staet: x", None, None))
            .expect_err("refused");
        assert_eq!(
            err,
            ClientError::InvalidQuery {
                message: "unknown field `staet` (columns 1-6)".to_string(),
                start: 0,
                end: 6,
            }
        );
    }

    #[test]
    fn a_400_that_is_not_a_query_error_stays_an_api_error() {
        let (api, _) = client(vec![json_response(
            400,
            r#"{"error":"invalid_request","message":"bad body"}"#,
        )]);
        let err = pollster::block_on(api.issues("tok", "EMB", &IssueFilter::default()))
            .expect_err("refused");
        assert!(
            matches!(err, ClientError::Api { status: 400, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn issue_read_patch_and_delete_use_the_issue_key_path() {
        let (api, transport) = client(vec![
            json_response(200, ISSUE),
            json_response(200, ISSUE),
            json_response(204, ""),
        ]);
        let issue = pollster::block_on(api.issue("tok", "EMB-12")).expect("issue");
        assert_eq!(issue.key, "EMB-12");
        let patch = UpdateIssue {
            priority: Some("Low".to_string()),
            ..UpdateIssue::default()
        };
        pollster::block_on(api.update_issue("tok", "EMB-12", &patch)).expect("patched");
        pollster::block_on(api.delete_issue("tok", "EMB-12")).expect("deleted");
        let sent = transport.requests();
        assert_eq!(sent[1].method, Method::Patch);
        assert_eq!(sent[1].url, "https://api.test/v1/issues/EMB-12");
        let json: serde_json::Value =
            serde_json::from_str(sent[1].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"priority": "Low"}),
            "only the named field is sent"
        );
        assert_eq!(sent[2].method, Method::Delete);
    }

    #[test]
    fn create_issue_posts_the_body_to_the_project() {
        let (api, transport) = client(vec![json_response(201, ISSUE)]);
        let body = CreateIssue {
            summary: "Crash on load".to_string(),
            issue_type: Some("Bug".to_string()),
            ..CreateIssue::default()
        };
        pollster::block_on(api.create_issue("tok", "EMB", &body)).expect("created");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/projects/EMB/issues");
        let json: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(json["type"], "Bug");
        assert_eq!(json["summary"], "Crash on load");
    }

    #[test]
    fn comments_add_edit_and_delete_by_id() {
        let comment = r#"{"id":"c1","body":"Hi","author":{"id":"u1","display_name":"Ana"},"created_at":1,"updated_at":1}"#;
        let (api, transport) = client(vec![
            json_response(200, &format!("[{comment}]")),
            json_response(201, comment),
            json_response(200, comment),
            json_response(204, ""),
        ]);
        let list = pollster::block_on(api.comments("tok", "EMB-12")).expect("list");
        assert_eq!(list[0].body, "Hi");
        pollster::block_on(api.add_comment("tok", "EMB-12", "Hi")).expect("add");
        pollster::block_on(api.update_comment("tok", "EMB-12", "c1", "Hello")).expect("edit");
        pollster::block_on(api.delete_comment("tok", "EMB-12", "c1")).expect("delete");
        let sent = transport.requests();
        assert_eq!(sent[1].url, "https://api.test/v1/issues/EMB-12/comments");
        assert_eq!(sent[2].method, Method::Patch);
        assert_eq!(sent[2].url, "https://api.test/v1/issues/EMB-12/comments/c1");
        assert_eq!(sent[3].method, Method::Delete);
    }

    #[test]
    fn links_activity_and_sprints_use_their_paths() {
        let (api, transport) = client(vec![
            json_response(201, ISSUE),
            json_response(204, ""),
            json_response(200, "[]"),
            json_response(200, "[]"),
        ]);
        let link = CreateLink {
            target: "EMB-3".to_string(),
            kind: "blocks".to_string(),
        };
        pollster::block_on(api.add_link("tok", "EMB-12", &link)).expect("linked");
        pollster::block_on(api.remove_link("tok", "EMB-12", LINK)).expect("unlinked");
        pollster::block_on(api.activity("tok", "EMB-12")).expect("activity");
        pollster::block_on(api.sprints("tok", "EMB")).expect("sprints");
        let sent = transport.requests();
        assert_eq!(sent[0].url, "https://api.test/v1/issues/EMB-12/links");
        assert_eq!(
            sent[1].url,
            "https://api.test/v1/issues/EMB-12/links/22222222-2222-4222-8222-222222222222"
        );
        assert_eq!(sent[2].url, "https://api.test/v1/issues/EMB-12/activity");
        assert_eq!(sent[3].url, "https://api.test/v1/projects/EMB/sprints");
    }

    const SPRINT: &str = r#"{"id":"55555555-5555-4555-8555-555555555555","name":"Sprint 4","goal":"","start_date":"2026-11-02","end_date":"2026-11-15","state":"Active"}"#;

    #[test]
    fn backlog_pages_and_rank_use_their_paths() {
        let (api, transport) = client(vec![
            json_response(200, r#"{"items":[],"next_cursor":"50"}"#),
            json_response(200, ISSUE),
        ]);
        let page =
            pollster::block_on(api.backlog("tok", "EMB", Some("50"), Some(50))).expect("backlog");
        assert_eq!(page.next_cursor.as_deref(), Some("50"));
        let body = RankIssue {
            issue: "EMB-12".to_string(),
            after: Some("EMB-3".to_string()),
            before: Some("EMB-4".to_string()),
        };
        pollster::block_on(api.rank_backlog("tok", "EMB", &body)).expect("ranked");
        let sent = transport.requests();
        assert_eq!(
            sent[0].url,
            "https://api.test/v1/projects/EMB/backlog?cursor=50&limit=50"
        );
        assert_eq!(sent[1].method, Method::Post);
        assert_eq!(sent[1].url, "https://api.test/v1/projects/EMB/backlog/rank");
        let json: serde_json::Value =
            serde_json::from_str(sent[1].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"issue": "EMB-12", "after": "EMB-3", "before": "EMB-4"})
        );
    }

    #[test]
    fn a_rank_without_neighbours_sends_null_and_the_top_placement() {
        let (api, transport) = client(vec![json_response(200, ISSUE)]);
        let body = RankIssue {
            issue: "EMB-12".to_string(),
            ..RankIssue::default()
        };
        pollster::block_on(api.rank_backlog("tok", "EMB", &body)).expect("ranked");
        let json: serde_json::Value =
            serde_json::from_str(transport.requests()[0].json_body.as_deref().expect("body"))
                .expect("json");
        assert_eq!(json["after"], serde_json::Value::Null);
        assert_eq!(json["before"], serde_json::Value::Null);
    }

    #[test]
    fn sprint_create_and_update_send_only_what_they_name() {
        let (api, transport) = client(vec![
            json_response(201, SPRINT),
            json_response(200, SPRINT),
            json_response(
                409,
                r#"{"error":"active_sprint_exists","message":"Another sprint is already active in this project. Close it first."}"#,
            ),
        ]);
        let create = CreateSprint {
            name: "Sprint 4".to_string(),
            start_date: "2026-11-02".to_string(),
            end_date: "2026-11-15".to_string(),
            ..CreateSprint::default()
        };
        let made = pollster::block_on(api.create_sprint("tok", "EMB", &create)).expect("created");
        assert_eq!(made.state, "Active");
        let start = UpdateSprint {
            state: Some("Active".to_string()),
            ..UpdateSprint::default()
        };
        pollster::block_on(api.update_sprint(
            "tok",
            "EMB",
            "55555555-5555-4555-8555-555555555555",
            &start,
        ))
        .expect("Started");
        let refused = pollster::block_on(api.update_sprint(
            "tok",
            "EMB",
            "66666666-6666-4666-8666-666666666666",
            &start,
        ))
        .expect_err("second active sprint");
        assert!(
            matches!(&refused, ClientError::Api { status: 409, error, .. } if error == "active_sprint_exists"),
            "{refused:?}"
        );
        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Post);
        assert_eq!(sent[0].url, "https://api.test/v1/projects/EMB/sprints");
        assert_eq!(sent[1].method, Method::Patch);
        assert_eq!(
            sent[1].url,
            "https://api.test/v1/projects/EMB/sprints/55555555-5555-4555-8555-555555555555"
        );
        assert_eq!(
            sent[1].json_body.as_deref(),
            Some(r#"{"state":"Active"}"#),
            "the start body names only the state"
        );
        assert_eq!(
            sent[2].url,
            "https://api.test/v1/projects/EMB/sprints/66666666-6666-4666-8666-666666666666"
        );
    }

    #[test]
    fn bad_keys_and_ids_are_refused_before_any_request() {
        let (api, transport) = client(vec![]);
        for bad in ["../admin", "", "EMB12x/../", "EMB-"] {
            assert!(
                matches!(
                    pollster::block_on(api.issue("tok", bad)),
                    Err(ClientError::Protocol(_))
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            pollster::block_on(api.project("tok", "E/M")),
            Err(ClientError::Protocol(_))
        ));
        assert!(matches!(
            pollster::block_on(api.delete_comment("tok", "EMB-1", "c1/../x")),
            Err(ClientError::Protocol(_))
        ));
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn a_401_is_the_unauthorized_error() {
        let (api, _) = client(vec![json_response(
            401,
            r#"{"error":"unauthorized","message":"x"}"#,
        )]);
        assert_eq!(
            pollster::block_on(api.projects("tok")),
            Err(ClientError::Unauthorized)
        );
    }
}

#[cfg(test)]
mod extras_tests {
    use std::sync::Arc;

    use space_proto::projects::{CreateWorkItem, IssueSummary};

    use super::*;
    use crate::api::Method;
    use crate::testing::{ScriptedTransport, bytes_response, json_response};

    fn client(
        replies: Vec<Result<crate::api::HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("https://api.test/", transport.clone());
        (api, transport)
    }

    const WORK: &str = r#"{"id":"w1","minutes":90,"spent_on":"2026-11-14","note":"Repro","author":{"id":"u1","display_name":"Ana"},"created_at":1,"updated_at":1}"#;
    const ATTACHMENT: &str = r#"{"id":"a1","file_name":"log.txt","content_type":"text/plain","size_bytes":5,"url":"/v1/issues/EMB-12/attachments/a1"}"#;

    #[test]
    fn vote_and_star_put_or_delete_the_caller_s_own() {
        let (api, transport) = client(vec![
            json_response(200, r#"{"votes":1,"voted_by_me":true}"#),
            json_response(200, r#"{"votes":0,"voted_by_me":false}"#),
            json_response(200, r#"{"starred_by_me":true}"#),
            json_response(200, r#"{"starred_by_me":false}"#),
        ]);
        let voted = pollster::block_on(api.set_vote("tok", "EMB-12", true)).expect("voted");
        assert_eq!(
            voted,
            VoteState {
                votes: 1,
                voted_by_me: true
            }
        );
        let unvoted = pollster::block_on(api.set_vote("tok", "EMB-12", false)).expect("unvoted");
        assert!(!unvoted.voted_by_me);
        let starred = pollster::block_on(api.set_star("tok", "EMB-12", true)).expect("starred");
        assert!(starred.starred_by_me);
        pollster::block_on(api.set_star("tok", "EMB-12", false)).expect("unstarred");
        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Put);
        assert_eq!(sent[0].url, "https://api.test/v1/issues/EMB-12/vote");
        assert_eq!(sent[1].method, Method::Delete);
        assert_eq!(sent[2].url, "https://api.test/v1/issues/EMB-12/star");
        assert_eq!(sent[3].method, Method::Delete);
        assert!(sent.iter().all(|r| r.bearer.as_deref() == Some("tok")));
    }

    #[test]
    fn work_items_list_add_and_delete_by_id() {
        let (api, transport) = client(vec![
            json_response(200, &format!("[{WORK}]")),
            json_response(201, WORK),
            json_response(204, ""),
        ]);
        let items = pollster::block_on(api.work_items("tok", "EMB-12")).expect("list");
        assert_eq!(items[0].minutes, 90);
        let body = CreateWorkItem {
            minutes: 90,
            spent_on: Some("2026-11-14".to_string()),
            note: Some("Repro".to_string()),
        };
        let added = pollster::block_on(api.add_work_item("tok", "EMB-12", &body)).expect("added");
        assert_eq!(added.author.display_name, "Ana");
        pollster::block_on(api.delete_work_item(
            "tok",
            "EMB-12",
            "11111111-1111-4111-8111-111111111111",
        ))
        .expect("deleted");
        let sent = transport.requests();
        assert_eq!(sent[1].method, Method::Post);
        assert_eq!(sent[1].url, "https://api.test/v1/issues/EMB-12/work-items");
        let json: serde_json::Value =
            serde_json::from_str(sent[1].json_body.as_deref().expect("body")).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"minutes": 90, "spent_on": "2026-11-14", "note": "Repro"})
        );
        assert_eq!(sent[2].method, Method::Delete);
        assert_eq!(
            sent[2].url,
            "https://api.test/v1/issues/EMB-12/work-items/11111111-1111-4111-8111-111111111111"
        );
    }

    #[test]
    fn upload_sends_the_raw_file_with_its_type_and_encoded_name() {
        let (api, transport) = client(vec![json_response(201, ATTACHMENT)]);
        let attachment = pollster::block_on(api.upload_attachment(
            "tok",
            "EMB-12",
            "Q4 notes é.txt",
            "text/plain",
            b"hello".to_vec(),
        ))
        .expect("uploaded");
        assert_eq!(attachment.file_name, "log.txt");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/issues/EMB-12/attachments");
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert_eq!(raw.content_type, "text/plain");
        assert_eq!(raw.bytes, b"hello".to_vec());
        assert_eq!(
            raw.file_name.as_deref(),
            Some("Q4%20notes%20%C3%A9.txt"),
            "the name is percent-encoded for the header"
        );
    }

    #[test]
    fn list_download_and_delete_attachments_use_their_paths() {
        let (api, transport) = client(vec![
            json_response(200, &format!("[{ATTACHMENT}]")),
            bytes_response(200, b"hello"),
            json_response(204, ""),
        ]);
        let listed = pollster::block_on(api.attachments("tok", "EMB-12")).expect("list");
        assert_eq!(listed[0].size_bytes, 5);
        let bytes = pollster::block_on(api.attachment_bytes("tok", "EMB-12", "a1")).expect("bytes");
        assert_eq!(bytes, b"hello".to_vec());
        pollster::block_on(api.delete_attachment("tok", "EMB-12", "a1")).expect("deleted");
        let sent = transport.requests();
        assert_eq!(sent[0].url, "https://api.test/v1/issues/EMB-12/attachments");
        assert_eq!(
            sent[1].url,
            "https://api.test/v1/issues/EMB-12/attachments/a1"
        );
        assert_eq!(sent[2].method, Method::Delete);
        assert_eq!(
            sent[2].url,
            "https://api.test/v1/issues/EMB-12/attachments/a1"
        );
    }

    #[test]
    fn similar_issues_send_the_summary_encoded() {
        let (api, transport) = client(vec![json_response(200, "[]")]);
        let found: Vec<IssueSummary> =
            pollster::block_on(api.similar_issues("tok", "EMB", "crash & login")).expect("similar");
        assert!(found.is_empty());
        assert_eq!(
            transport.requests()[0].url,
            "https://api.test/v1/projects/EMB/issues/similar?summary=crash%20%26%20login"
        );
    }

    #[test]
    fn bad_keys_and_ids_are_refused_before_any_request() {
        let (api, transport) = client(vec![]);
        assert!(pollster::block_on(api.set_vote("tok", "../etc", true)).is_err());
        assert!(pollster::block_on(api.attachment_bytes("tok", "EMB-12", "a/../b")).is_err());
        assert!(pollster::block_on(api.delete_work_item("tok", "EMB-12", "x?y")).is_err());
        assert!(pollster::block_on(api.similar_issues("tok", "e m", "crash")).is_err());
        assert!(transport.requests().is_empty());
    }
}
