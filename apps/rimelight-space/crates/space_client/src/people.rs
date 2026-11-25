//! Typed client for the people and org endpoints (SP.3a): the directory, profiles, the org chart, departments, teams and
//! positions (reads), the self-service profile edit and photo upload, and the admin user, role and org writes (SP.3e).
//!
//! Every call takes the session token. Ids are checked to be plain path segments before they reach the URL.

use space_proto::people::{
    AdminCreateUser, AdminUpdateUser, Department, DepartmentInput, DepartmentNode, Location,
    LocationInput, OrgNode, OrgTreeEntry, Page, PersonProfile, PersonSummary, Position,
    PositionInput, Profile, ROLES, RoleMembers, Seat, SeatInput, SeatNode, Team, TeamBudget,
    TeamBudgetInput, TeamDetail, TeamInput, UpdateMyProfile,
};

use crate::api::{ApiClient, Method, RawBody, encode_query_value, parse};
use crate::error::ClientError;

/// Filters and paging for `GET /v1/people`. Empty fields are left out of the query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryQuery {
    pub q: Option<String>,
    pub team_id: Option<String>,
    pub department_id: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl DirectoryQuery {
    /// The query string, with a leading `?` when there is anything to send.
    fn to_query_string(&self) -> String {
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        for (key, value) in [
            ("q", self.q.as_deref()),
            ("team", self.team_id.as_deref()),
            ("department", self.department_id.as_deref()),
            ("cursor", self.cursor.as_deref()),
        ] {
            if let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) {
                pairs.push((key, value));
            }
        }
        let limit = self.limit.map(|l| l.to_string());
        let mut out = String::new();
        for (key, value) in &pairs {
            out.push(if out.is_empty() { '?' } else { '&' });
            out.push_str(key);
            out.push('=');
            out.push_str(&encode_query_value(value));
        }
        if let Some(limit) = limit {
            out.push(if out.is_empty() { '?' } else { '&' });
            out.push_str("limit=");
            out.push_str(&limit);
        }
        out
    }
}

/// An id used as a path segment: the server's ids are UUIDs, so anything else is refused before a request is made.
fn segment(id: &str) -> Result<&str, ClientError> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        Ok(id)
    } else {
        Err(ClientError::Protocol(format!("not a valid id: {id}")))
    }
}

impl ApiClient {
    /// `GET /v1/people`: the active directory, one page at a time.
    pub async fn directory(
        &self,
        token: &str,
        query: &DirectoryQuery,
    ) -> Result<Page<PersonSummary>, ClientError> {
        let path = format!("/v1/people{}", query.to_query_string());
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/people/{id}`: one profile, with personal fields when the caller may see them.
    pub async fn person(&self, token: &str, id: &str) -> Result<PersonProfile, ClientError> {
        let path = format!("/v1/people/{}", segment(id)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/people/{id}/org`: the manager chain and direct reports.
    pub async fn org(&self, token: &str, id: &str) -> Result<OrgNode, ClientError> {
        let path = format!("/v1/people/{}/org", segment(id)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/org/tree`: every active person with their manager id, for the whole-company org chart.
    pub async fn org_tree(&self, token: &str) -> Result<Vec<OrgTreeEntry>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/org/tree", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/me/profile`: the caller's own profile with all personal fields.
    pub async fn my_profile(&self, token: &str) -> Result<PersonProfile, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/me/profile", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/me/profile`: edits the caller's own self-editable fields. Returns the updated profile.
    pub async fn update_my_profile(
        &self,
        token: &str,
        body: &UpdateMyProfile,
    ) -> Result<PersonProfile, ClientError> {
        let json = serde_json::to_string(body).map_err(|e| ClientError::Protocol(e.to_string()))?;
        let reply = self
            .send(Method::Patch, "/v1/me/profile", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PUT /v1/me/avatar`: uploads a PNG, JPEG or WebP photo (at most 5 MB, checked by the server).
    pub async fn upload_avatar(
        &self,
        token: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<PersonProfile, ClientError> {
        let raw = RawBody {
            file_name: None,
            content_type: content_type.to_string(),
            bytes,
        };
        let reply = self
            .send_raw(Method::Put, "/v1/me/avatar", Some(token), raw)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/avatars/{key}`: the stored photo as the bytes the server sent (PNG, JPEG or WebP). `avatar_url` is the
    /// path the directory and profiles carry (`/v1/avatars/{key}`). Anything else is refused before a request is made.
    pub async fn avatar(&self, token: &str, avatar_url: &str) -> Result<Vec<u8>, ClientError> {
        avatar_url
            .strip_prefix("/v1/avatars/")
            .filter(|key| {
                !key.is_empty()
                    && !key.contains("..")
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
            .ok_or_else(|| ClientError::Protocol(format!("not an avatar url: {avatar_url}")))?;
        let reply = self
            .send(Method::Get, avatar_url, Some(token), None)
            .await?;
        Ok(reply.bytes)
    }

    /// `GET /v1/departments`: every department, flat, with member counts.
    pub async fn departments(&self, token: &str) -> Result<Vec<Department>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/departments", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/departments/tree`: the department tree from the roots down.
    pub async fn department_tree(&self, token: &str) -> Result<Vec<DepartmentNode>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/departments/tree", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/teams`: every team, with its lead id and member count.
    pub async fn teams(&self, token: &str) -> Result<Vec<Team>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/teams", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/teams/{id}`: one team with its members.
    pub async fn team(&self, token: &str, id: &str) -> Result<TeamDetail, ClientError> {
        let path = format!("/v1/teams/{}", segment(id)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/positions`: the job positions.
    pub async fn positions(&self, token: &str) -> Result<Vec<Position>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/positions", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/admin/users` (admin only): every person, deactivated ones included, with the full work profile.
    pub async fn admin_users(
        &self,
        token: &str,
        query: &DirectoryQuery,
    ) -> Result<Page<Profile>, ClientError> {
        let path = format!("/v1/admin/users{}", query.to_query_string());
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/admin/users` (admin only): pre-provisions a person who signs in later with that email.
    pub async fn admin_create_user(
        &self,
        token: &str,
        body: &AdminCreateUser,
    ) -> Result<Profile, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(Method::Post, "/v1/admin/users", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/users/{id}` (admin only): org fields. An absent field is unchanged, an empty one is cleared.
    pub async fn admin_update_user(
        &self,
        token: &str,
        id: &str,
        body: &AdminUpdateUser,
    ) -> Result<Profile, ClientError> {
        let path = format!("/v1/admin/users/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PUT` (grant) or `DELETE` (revoke) `/v1/admin/users/{id}/roles/{role}` (admin only). Returns the person's profile.
    pub async fn admin_set_role(
        &self,
        token: &str,
        id: &str,
        role: &str,
        granted: bool,
    ) -> Result<Profile, ClientError> {
        if !ROLES.contains(&role) {
            return Err(ClientError::Protocol(format!("not a role: {role}")));
        }
        let path = format!("/v1/admin/users/{}/roles/{role}", segment(id)?);
        let method = if granted { Method::Put } else { Method::Delete };
        let reply = self.send(method, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/admin/roles` (admin only): each built-in role with its members.
    pub async fn admin_roles(&self, token: &str) -> Result<Vec<RoleMembers>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/admin/roles", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/admin/departments` (admin only).
    pub async fn create_department(
        &self,
        token: &str,
        body: &DepartmentInput,
    ) -> Result<Department, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(
                Method::Post,
                "/v1/admin/departments",
                Some(token),
                Some(json),
            )
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/departments/{id}` (admin only). Absent fields are unchanged.
    pub async fn update_department(
        &self,
        token: &str,
        id: &str,
        body: &DepartmentInput,
    ) -> Result<Department, ClientError> {
        let path = format!("/v1/admin/departments/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/admin/departments/{id}` (admin only). Refused with 409 while it has people, teams or child departments.
    pub async fn delete_department(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/departments/{}", segment(id)?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `POST /v1/admin/teams` (admin only).
    pub async fn create_team(&self, token: &str, body: &TeamInput) -> Result<Team, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(Method::Post, "/v1/admin/teams", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/teams/{id}` (admin only).
    pub async fn update_team(
        &self,
        token: &str,
        id: &str,
        body: &TeamInput,
    ) -> Result<Team, ClientError> {
        let path = format!("/v1/admin/teams/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/admin/teams/{id}` (admin only). Refused with 409 while people are in it.
    pub async fn delete_team(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/teams/{}", segment(id)?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `POST /v1/admin/positions` (admin only).
    pub async fn create_position(
        &self,
        token: &str,
        body: &PositionInput,
    ) -> Result<Position, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(Method::Post, "/v1/admin/positions", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/positions/{id}` (admin only).
    pub async fn update_position(
        &self,
        token: &str,
        id: &str,
        body: &PositionInput,
    ) -> Result<Position, ClientError> {
        let path = format!("/v1/admin/positions/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/admin/positions/{id}` (admin only). Refused with 409 while people hold it.
    pub async fn delete_position(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/positions/{}", segment(id)?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `GET /v1/locations`: every company location by name (any signed-in person).
    pub async fn locations(&self, token: &str) -> Result<Vec<Location>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/locations", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/admin/locations` (admin only). Name and kind are required.
    pub async fn create_location(
        &self,
        token: &str,
        body: &LocationInput,
    ) -> Result<Location, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(Method::Post, "/v1/admin/locations", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/locations/{id}` (admin only).
    pub async fn update_location(
        &self,
        token: &str,
        id: &str,
        body: &LocationInput,
    ) -> Result<Location, ClientError> {
        let path = format!("/v1/admin/locations/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/admin/locations/{id}` (admin only).
    pub async fn delete_location(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/locations/{}", segment(id)?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `GET /v1/org/seats`: the org chart built from seats, without money (every employee).
    pub async fn org_seats(&self, token: &str) -> Result<Vec<SeatNode>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/org/seats", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/admin/seats`: every seat with its salary range (admin and HR).
    pub async fn admin_seats(&self, token: &str) -> Result<Vec<Seat>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/admin/seats", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/admin/seats` (admin only).
    pub async fn create_seat(&self, token: &str, body: &SeatInput) -> Result<Seat, ClientError> {
        let json = to_json(body)?;
        let reply = self
            .send(Method::Post, "/v1/admin/seats", Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/admin/seats/{id}` (admin only). Absent fields keep their value; `""` clears.
    pub async fn update_seat(
        &self,
        token: &str,
        id: &str,
        body: &SeatInput,
    ) -> Result<Seat, ClientError> {
        let path = format!("/v1/admin/seats/{}", segment(id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/admin/seats/{id}` (admin only). Refused while the seat is filled or has reports.
    pub async fn delete_seat(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/admin/seats/{}", segment(id)?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `GET /v1/admin/team-budgets`: every team's cap, used and remaining (admin and HR).
    pub async fn team_budgets(&self, token: &str) -> Result<Vec<TeamBudget>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/admin/team-budgets", Some(token), None)
            .await?;
        parse(&reply.body)
    }

    /// `PUT /v1/admin/teams/{id}/budget` (admin only). An empty `cap` removes the budget.
    pub async fn set_team_budget(
        &self,
        token: &str,
        team_id: &str,
        body: &TeamBudgetInput,
    ) -> Result<TeamBudget, ClientError> {
        let path = format!("/v1/admin/teams/{}/budget", segment(team_id)?);
        let json = to_json(body)?;
        let reply = self
            .send(Method::Put, &path, Some(token), Some(json))
            .await?;
        parse(&reply.body)
    }
}

/// Serializes a request body. A value that cannot be serialized is a protocol error.
fn to_json<T: serde::Serialize>(body: &T) -> Result<String, ClientError> {
    serde_json::to_string(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use space_proto::people::{Profile, UpdateMyProfile};

    use super::*;
    use crate::api::Method;
    use crate::testing::{ScriptedTransport, json_response};

    const TEAM: &str = "7e000000-0000-4000-8000-000000000001";
    const PERSON: &str = "f0000000-0000-4000-8000-000000000005";

    fn client(
        replies: Vec<Result<crate::api::HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("https://api.test/", transport.clone());
        (api, transport)
    }

    /// The bare profile, as the admin writes return it.
    fn bare_profile_json() -> String {
        let value: serde_json::Value = serde_json::from_str(&profile_json()).expect("json");
        value["profile"].to_string()
    }

    fn profile_json() -> String {
        let profile = Profile {
            id: PERSON.to_string(),
            email: "mina@example.com".to_string(),
            display_name: "Mina".to_string(),
            preferred_name: None,
            pronouns: Some("she/her".to_string()),
            title: Some("Developer".to_string()),
            position_id: None,
            department_id: None,
            department_name: Some("Engineering".to_string()),
            team_id: Some(TEAM.to_string()),
            team_name: Some("Gameplay".to_string()),
            manager_id: None,
            location: None,
            timezone: None,
            work_phone: None,
            bio: Some("Hello".to_string()),
            skills: vec!["Rust".to_string()],
            start_date: None,
            avatar_url: None,
            is_active: true,
            roles: vec![],
        };
        serde_json::json!({ "profile": profile, "personal": null }).to_string()
    }

    #[test]
    fn directory_sends_filters_cursor_and_limit() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"items":[],"next_cursor":null}"#,
        )]);
        let query = DirectoryQuery {
            q: Some("mina b".to_string()),
            team_id: Some(TEAM.to_string()),
            department_id: None,
            cursor: Some(PERSON.to_string()),
            limit: Some(20),
        };
        let page = pollster::block_on(api.directory("tok", &query)).expect("page");
        assert!(page.items.is_empty());
        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Get);
        assert_eq!(sent[0].bearer.as_deref(), Some("tok"));
        let url = &sent[0].url;
        assert!(url.starts_with("https://api.test/v1/people?q="), "{url}");
        assert!(url.contains("&team=7e000000"), "{url}");
        assert!(
            !url.contains("department="),
            "empty filters are left out: {url}"
        );
        assert!(url.contains("&cursor=f0000000"), "{url}");
        assert!(url.ends_with("&limit=20"), "{url}");
        assert!(!url.contains("mina b"), "the query is encoded: {url}");
    }

    #[test]
    fn directory_with_no_filters_has_no_query_string() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"items":[],"next_cursor":null}"#,
        )]);
        pollster::block_on(api.directory("tok", &DirectoryQuery::default())).expect("page");
        assert_eq!(transport.requests()[0].url, "https://api.test/v1/people");
    }

    #[test]
    fn person_org_and_team_use_their_paths() {
        let (api, transport) = client(vec![
            json_response(200, &profile_json()),
            json_response(
                200,
                r#"{"person":{"id":"x","display_name":"X","title":null,"team":null,"avatar_url":null},"manager_chain":[],"direct_reports":[]}"#,
            ),
            json_response(
                200,
                &format!(
                    r#"{{"team":{{"id":"{TEAM}","name":"Gameplay","department_id":"d0000000-0000-4000-8000-000000000001","lead_id":null,"member_count":0}},"members":[]}}"#
                ),
            ),
        ]);
        let profile = pollster::block_on(api.person("tok", PERSON)).expect("profile");
        assert_eq!(profile.profile.display_name, "Mina");
        assert!(profile.personal.is_none());
        pollster::block_on(api.org("tok", PERSON)).expect("org");
        let team = pollster::block_on(api.team("tok", TEAM)).expect("team");
        assert_eq!(team.team.name, "Gameplay");
        let urls: Vec<String> = transport.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(
            urls,
            vec![
                format!("https://api.test/v1/people/{PERSON}"),
                format!("https://api.test/v1/people/{PERSON}/org"),
                format!("https://api.test/v1/teams/{TEAM}"),
            ]
        );
    }

    #[test]
    fn avatar_get_sends_the_bearer_and_returns_the_bytes() {
        let png = [0x89, b'P', b'N', b'G', 0, 255];
        let (api, transport) = client(vec![crate::testing::bytes_response(200, &png)]);
        let bytes = pollster::block_on(api.avatar("tok", "/v1/avatars/abc-1.png")).expect("bytes");
        assert_eq!(bytes, png.to_vec());
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Get);
        assert_eq!(sent.url, "https://api.test/v1/avatars/abc-1.png");
        assert_eq!(sent.bearer.as_deref(), Some("tok"));
        assert!(sent.json_body.is_none() && sent.raw_body.is_none());
    }

    #[test]
    fn a_bad_avatar_url_is_refused_before_any_request() {
        let (api, transport) = client(vec![]);
        for bad in [
            "/v1/avatars/",
            "/v1/avatars/../me",
            "/v1/people/abc",
            "/v1/avatars/a/b",
            "/v1/avatars/a b",
        ] {
            let err = pollster::block_on(api.avatar("tok", bad)).expect_err("refused");
            assert!(matches!(err, ClientError::Protocol(_)), "{bad}");
        }
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn a_bad_id_is_refused_before_any_request() {
        let (api, transport) = client(vec![]);
        let err = pollster::block_on(api.person("tok", "../admin")).expect_err("refused");
        assert!(matches!(err, ClientError::Protocol(_)));
        let err = pollster::block_on(api.team("tok", "")).expect_err("refused");
        assert!(matches!(err, ClientError::Protocol(_)));
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn update_my_profile_patches_json_and_returns_the_profile() {
        let (api, transport) = client(vec![json_response(200, &profile_json())]);
        let body = UpdateMyProfile {
            bio: Some("Hello".to_string()),
            ..UpdateMyProfile::default()
        };
        let updated = pollster::block_on(api.update_my_profile("tok", &body)).expect("ok");
        assert_eq!(updated.profile.bio.as_deref(), Some("Hello"));
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Patch);
        assert_eq!(sent.url, "https://api.test/v1/me/profile");
        assert_eq!(sent.bearer.as_deref(), Some("tok"));
        let json: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(json["bio"], "Hello");
    }

    #[test]
    fn upload_avatar_puts_the_bytes_with_their_type() {
        let (api, transport) = client(vec![json_response(200, &profile_json())]);
        pollster::block_on(api.upload_avatar("tok", "image/png", vec![1, 2, 3])).expect("ok");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Put);
        assert_eq!(sent.url, "https://api.test/v1/me/avatar");
        assert!(sent.json_body.is_none());
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert_eq!(raw.content_type, "image/png");
        assert_eq!(raw.bytes, vec![1, 2, 3]);
    }

    #[test]
    fn a_server_error_keeps_its_code_and_message() {
        let (api, _) = client(vec![json_response(
            422,
            r#"{"error":"invalid_field","message":"bio: too long"}"#,
        )]);
        let err = pollster::block_on(api.update_my_profile("tok", &UpdateMyProfile::default()))
            .expect_err("refused");
        match err {
            ClientError::Api {
                status,
                error,
                message,
            } => {
                assert_eq!(status, 422);
                assert_eq!(error, "invalid_field");
                assert_eq!(message, "bio: too long");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn lists_parse_departments_tree_teams_and_positions() {
        let (api, _) = client(vec![
            json_response(
                200,
                r#"[{"id":"d1","name":"Eng","parent_department_id":null,"head_id":null,"member_count":3}]"#,
            ),
            json_response(
                200,
                r#"[{"department":{"id":"d1","name":"Eng","parent_department_id":null,"head_id":null,"member_count":3},"children":[]}]"#,
            ),
            json_response(
                200,
                r#"[{"id":"t1","name":"Gameplay","department_id":"d1","lead_id":null,"member_count":2}]"#,
            ),
            json_response(
                200,
                r#"[{"id":"p1","title":"Developer","level":"Individual","description":"","salary_min":null,"salary_max":null,"currency":null,"seat_count":1,"filled_count":1,"open_count":0}]"#,
            ),
            json_response(
                200,
                r#"[{"id":"a1","name":"Ann","title":null,"avatar_url":null,"manager_id":null,"team":null}]"#,
            ),
        ]);
        assert_eq!(
            pollster::block_on(api.departments("tok")).expect("d").len(),
            1
        );
        let tree = pollster::block_on(api.department_tree("tok")).expect("tree");
        assert_eq!(tree[0].department.member_count, 3);
        assert_eq!(
            pollster::block_on(api.teams("tok")).expect("t")[0].member_count,
            2
        );
        assert_eq!(
            pollster::block_on(api.positions("tok")).expect("p")[0].title,
            "Developer"
        );
        let org = pollster::block_on(api.org_tree("tok")).expect("org tree");
        assert_eq!(org[0].name, "Ann");
        assert_eq!(org[0].manager_id, None);
    }
    #[test]
    fn admin_user_list_lists_deactivated_people_with_their_paging() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"items":[],"next_cursor":null}"#,
        )]);
        let query = DirectoryQuery {
            q: Some("mi".to_string()),
            limit: Some(50),
            ..DirectoryQuery::default()
        };
        pollster::block_on(api.admin_users("tok", &query)).expect("ok");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Get);
        assert_eq!(sent.url, "https://api.test/v1/admin/users?q=mi&limit=50");
    }

    #[test]
    fn admin_create_and_update_send_their_json_and_return_the_profile() {
        let (api, transport) = client(vec![
            json_response(201, &bare_profile_json()),
            json_response(200, &bare_profile_json()),
        ]);
        let create = AdminCreateUser {
            email: "mina@example.com".to_string(),
            display_name: "Mina".to_string(),
            ..AdminCreateUser::default()
        };
        let made = pollster::block_on(api.admin_create_user("tok", &create)).expect("created");
        assert_eq!(made.id, PERSON);
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/admin/users");
        let json: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(json["email"], "mina@example.com");

        let update = AdminUpdateUser {
            is_active: Some(false),
            ..AdminUpdateUser::default()
        };
        pollster::block_on(api.admin_update_user("tok", PERSON, &update)).expect("ok");
        let sent = &transport.requests()[1];
        assert_eq!(sent.method, Method::Patch);
        assert_eq!(
            sent.url,
            format!("https://api.test/v1/admin/users/{PERSON}")
        );
        let json: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(json["is_active"], false);
    }

    #[test]
    fn roles_are_granted_by_put_and_revoked_by_delete_and_only_known_roles_are_sent() {
        let (api, transport) = client(vec![
            json_response(200, &bare_profile_json()),
            json_response(200, &bare_profile_json()),
        ]);
        pollster::block_on(api.admin_set_role("tok", PERSON, "hr", true)).expect("granted");
        pollster::block_on(api.admin_set_role("tok", PERSON, "admin", false)).expect("revoked");
        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Put);
        assert_eq!(
            sent[0].url,
            format!("https://api.test/v1/admin/users/{PERSON}/roles/hr")
        );
        assert_eq!(sent[1].method, Method::Delete);
        assert_eq!(
            sent[1].url,
            format!("https://api.test/v1/admin/users/{PERSON}/roles/admin")
        );

        let err = pollster::block_on(api.admin_set_role("tok", PERSON, "owner", true))
            .expect_err("unknown role");
        assert!(matches!(err, ClientError::Protocol(_)));
        assert_eq!(
            transport.requests().len(),
            2,
            "no request for an unknown role"
        );
    }

    #[test]
    fn org_writes_post_patch_and_delete_on_their_paths() {
        let department = r#"{"id":"d1","name":"Art","parent_department_id":null,"head_id":null,"member_count":0}"#;
        let (api, transport) = client(vec![
            json_response(201, department),
            json_response(200, department),
            json_response(200, r#"{"ok":true}"#),
            json_response(
                409,
                r#"{"error":"department_has_members","message":"Move its 3 people first."}"#,
            ),
        ]);
        let input = DepartmentInput {
            name: Some("Art".to_string()),
            ..DepartmentInput::default()
        };
        assert_eq!(
            pollster::block_on(api.create_department("tok", &input))
                .expect("created")
                .name,
            "Art"
        );
        pollster::block_on(api.update_department("tok", "d1", &input)).expect("updated");
        pollster::block_on(api.delete_department("tok", "d1")).expect("deleted");
        let err = pollster::block_on(api.delete_department("tok", "d2")).expect_err("409");
        match err {
            ClientError::Api {
                status, message, ..
            } => {
                assert_eq!(status, 409);
                assert_eq!(message, "Move its 3 people first.");
            }
            other => panic!("expected an api error, got {other:?}"),
        }
        let sent = transport.requests();
        assert_eq!(sent[0].method, Method::Post);
        assert_eq!(sent[0].url, "https://api.test/v1/admin/departments");
        assert_eq!(sent[1].method, Method::Patch);
        assert_eq!(sent[1].url, "https://api.test/v1/admin/departments/d1");
        assert_eq!(sent[2].method, Method::Delete);
        assert_eq!(sent[2].url, "https://api.test/v1/admin/departments/d1");
    }

    #[test]
    fn admin_writes_refuse_bad_ids_before_any_request() {
        let (api, transport) = client(vec![]);
        assert!(pollster::block_on(api.delete_team("tok", "../x")).is_err());
        assert!(pollster::block_on(api.delete_position("tok", "")).is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn seat_calls_use_the_seat_routes_and_money_stays_on_admin_reads() {
        let seat = r#"{"id":"5e000000-0000-4000-8000-000000000001","team_id":"7e000000-0000-4000-8000-000000000001","team_name":"Gameplay","job_profile_id":"9e000000-0000-4000-8000-000000000001","job_profile_title":"Lead","level":"","salary_min":null,"salary_max":null,"currency":null,"state":"open","person_id":null,"person_name":null,"reports_to_seat_id":null}"#;
        let (api, transport) = client(vec![
            json_response(200, &format!("[{{\"seat\":{seat},\"reports\":[]}}]")),
            json_response(200, &format!("[{seat}]")),
            json_response(201, seat),
            json_response(200, "{\"ok\":true}"),
            json_response(
                200,
                r#"{"team_id":"7e000000-0000-4000-8000-000000000001","team_name":"Gameplay","cap":"200000.00","currency":"GBP","used":"0.00","remaining":"200000.00"}"#,
            ),
        ]);
        let chart = pollster::block_on(api.org_seats("tok")).expect("chart");
        assert_eq!(chart[0].seat.id, "5e000000-0000-4000-8000-000000000001");
        pollster::block_on(api.admin_seats("tok")).expect("seats");
        let body = SeatInput {
            team_id: Some(TEAM.to_string()),
            job_profile_id: Some("9e000000-0000-4000-8000-000000000001".to_string()),
            ..SeatInput::default()
        };
        pollster::block_on(api.create_seat("tok", &body)).expect("created");
        pollster::block_on(api.delete_seat("tok", "5e000000-0000-4000-8000-000000000001"))
            .expect("deleted");
        let budget = TeamBudgetInput {
            cap: Some("200000".to_string()),
            currency: Some("GBP".to_string()),
        };
        pollster::block_on(api.set_team_budget("tok", TEAM, &budget)).expect("budget");

        let sent = transport.requests();
        assert_eq!(sent[0].url, "https://api.test/v1/org/seats");
        assert_eq!(sent[1].url, "https://api.test/v1/admin/seats");
        assert_eq!(sent[2].method, Method::Post);
        assert_eq!(
            sent[3].url,
            "https://api.test/v1/admin/seats/5e000000-0000-4000-8000-000000000001"
        );
        assert_eq!(sent[4].method, Method::Put);
        assert!(
            sent[4]
                .url
                .ends_with("/v1/admin/teams/7e000000-0000-4000-8000-000000000001/budget")
        );
        assert!(pollster::block_on(api.delete_seat("tok", "../x")).is_err());
    }
}
