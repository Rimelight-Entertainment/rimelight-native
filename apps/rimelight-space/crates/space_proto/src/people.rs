//! People, org structure and admin contract types (decision 0019 and the owner addendum of 2026-11-15).
//!
//! Work fields are visible to every signed-in employee. `PersonalInfo` is returned only to the
//! person, their manager chain, and holders of the `admin` or `hr` role. The API enforces that.
//!
//! PATCH bodies use absent fields to mean "unchanged" and an empty string to mean "clear".

use serde::{Deserialize, Serialize};

/// Built-in role: full admin, including user, org and role management.
pub const ROLE_ADMIN: &str = "admin";
/// Built-in role: HR. Sees personal contact fields of everyone.
pub const ROLE_HR: &str = "hr";
/// The only role names the API accepts.
pub const ROLES: &[&str] = &[ROLE_ADMIN, ROLE_HR];

/// Largest avatar upload accepted, in bytes (5 MB).
pub const AVATAR_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const DEFAULT_PAGE_LIMIT: u32 = 50;
pub const MAX_PAGE_LIMIT: u32 = 100;

/// Work fields of one person, as returned to any signed-in employee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// UUID in canonical text form.
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub preferred_name: Option<String>,
    pub pronouns: Option<String>,
    /// The position's title when a position is set, else the person's own title.
    pub title: Option<String>,
    pub position_id: Option<String>,
    pub department_id: Option<String>,
    pub department_name: Option<String>,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub manager_id: Option<String>,
    pub location: Option<String>,
    pub timezone: Option<String>,
    pub work_phone: Option<String>,
    pub bio: Option<String>,
    pub skills: Vec<String>,
    /// `YYYY-MM-DD`.
    pub start_date: Option<String>,
    /// Relative URL under `/v1/avatars/`, or absent when there is no photo.
    pub avatar_url: Option<String>,
    pub is_active: bool,
    /// Built-in role names held by this person (`admin`, `hr`).
    pub roles: Vec<String>,
}

/// Personal contact fields. Filled only when the caller may see them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalInfo {
    pub personal_phone: Option<String>,
    pub personal_email: Option<String>,
    pub address: Option<String>,
    /// `YYYY-MM-DD`.
    pub birthday: Option<String>,
    pub emergency_contact: Option<String>,
}

/// Reply of `GET /v1/people/{id}` and `GET /v1/me/profile`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonProfile {
    pub profile: Profile,
    /// `None` when the caller may not see personal fields.
    pub personal: Option<PersonalInfo>,
}

/// Short form used in lists, the directory and the org chart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonSummary {
    pub id: String,
    pub display_name: String,
    pub title: Option<String>,
    pub team: Option<String>,
    pub avatar_url: Option<String>,
}

/// Reply of `GET /v1/people/{id}/org`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgNode {
    pub person: PersonSummary,
    /// Direct manager first, then up to the root.
    pub manager_chain: Vec<PersonSummary>,
    pub direct_reports: Vec<PersonSummary>,
}

/// One active person in the whole-company org chart, from `GET /v1/org/tree`. `manager_id` is `None` for the CEO. `team` is
/// the team's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgTreeEntry {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
    pub avatar_url: Option<String>,
    pub manager_id: Option<String>,
    pub team: Option<String>,
}

/// Page of results. `next_cursor` is passed back as `cursor=` for the next page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Body of `PATCH /v1/me/profile`. Self-editable fields only; unknown fields are refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMyProfile {
    pub preferred_name: Option<String>,
    pub pronouns: Option<String>,
    pub bio: Option<String>,
    pub work_phone: Option<String>,
    pub timezone: Option<String>,
    pub location: Option<String>,
    pub skills: Option<Vec<String>>,
    pub personal_phone: Option<String>,
    pub personal_email: Option<String>,
    pub address: Option<String>,
    pub birthday: Option<String>,
    pub emergency_contact: Option<String>,
}

/// Body of `PATCH /v1/admin/users/{id}`. Org fields only; unknown fields are refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminUpdateUser {
    pub title: Option<String>,
    pub position_id: Option<String>,
    pub department_id: Option<String>,
    pub team_id: Option<String>,
    pub manager_id: Option<String>,
    pub start_date: Option<String>,
    pub is_active: Option<bool>,
}

/// Body of `POST /v1/admin/users`: pre-provisions an employee who signs in later.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminCreateUser {
    pub email: String,
    pub display_name: String,
    pub title: Option<String>,
    pub position_id: Option<String>,
    pub department_id: Option<String>,
    pub team_id: Option<String>,
    pub manager_id: Option<String>,
    pub start_date: Option<String>,
}

/// A department in the org tree. `member_count` counts non-deleted people in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Department {
    pub id: String,
    pub name: String,
    pub parent_department_id: Option<String>,
    pub head_id: Option<String>,
    pub member_count: u32,
}

/// A department with its child departments, from `GET /v1/departments/tree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentNode {
    pub department: Department,
    pub children: Vec<DepartmentNode>,
}

/// Body of department create (name required) and update (absent fields unchanged).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentInput {
    pub name: Option<String>,
    pub parent_department_id: Option<String>,
    pub head_id: Option<String>,
}

/// A team. `member_count` counts non-deleted people in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub department_id: String,
    pub lead_id: Option<String>,
    pub member_count: u32,
}

/// Reply of `GET /v1/teams/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamDetail {
    pub team: Team,
    pub members: Vec<PersonSummary>,
}

/// Body of team create (name and department required) and update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamInput {
    pub name: Option<String>,
    pub department_id: Option<String>,
    pub lead_id: Option<String>,
}

/// A job position: a job title (decision 0042). It has no team and no department. `seat_count` counts
/// every seat that holds it, `filled_count` the seats with a person, `open_count` the open seats.
/// Salary is a decimal string such as `"52000.00"`, visible to admin and HR only (`None` otherwise).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub title: String,
    pub level: String,
    pub description: String,
    pub salary_min: Option<String>,
    pub salary_max: Option<String>,
    pub currency: Option<String>,
    pub seat_count: u32,
    pub filled_count: u32,
    pub open_count: u32,
}

/// Body of position create (title required) and update (absent fields unchanged).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionInput {
    pub title: Option<String>,
    pub level: Option<String>,
    pub description: Option<String>,
    pub salary_min: Option<String>,
    pub salary_max: Option<String>,
    pub currency: Option<String>,
}

/// Seat states (decision 0036). A seat is planned (on the plan, not approved), open (approved,
/// hiring), filled (one person) or closed (removed from the plan, kept for history).
pub const SEAT_PLANNED: &str = "planned";
pub const SEAT_OPEN: &str = "open";
pub const SEAT_FILLED: &str = "filled";
pub const SEAT_CLOSED: &str = "closed";
pub const SEAT_STATES: &[&str] = &[SEAT_PLANNED, SEAT_OPEN, SEAT_FILLED, SEAT_CLOSED];

/// Error code of a seat write that would take its team over budget (HTTP 409).
pub const ERROR_OVER_BUDGET: &str = "over_budget";

/// A seat: one slot in one team holding one position (`job_profile_id`) and at most one person. A seat has
/// no title, level or salary of its own; `job_profile_title` and `level` come from its position. Money is
/// visible to admin and HR only; the org chart and the team reads leave `salary_*` and `currency` out (`None`).
/// Money is a decimal string such as `"52000.00"`. All of these fields are read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub id: String,
    pub team_id: String,
    pub team_name: String,
    pub job_profile_id: Option<String>,
    pub job_profile_title: Option<String>,
    pub level: String,
    pub salary_min: Option<String>,
    pub salary_max: Option<String>,
    pub currency: Option<String>,
    pub state: String,
    pub person_id: Option<String>,
    pub person_name: Option<String>,
    pub reports_to_seat_id: Option<String>,
}

/// Body of seat create (team and `job_profile_id` required) and update (absent fields unchanged, empty clears).
/// `person_id` seats the person (the seat becomes filled); an empty `person_id` vacates it (open).
/// `state` is `planned`, `open` or `closed`; `filled` follows from `person_id`. A write that takes the team
/// over its budget is refused with `ERROR_OVER_BUDGET`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatInput {
    pub team_id: Option<String>,
    pub job_profile_id: Option<String>,
    pub state: Option<String>,
    pub person_id: Option<String>,
    pub reports_to_seat_id: Option<String>,
}

/// Body of `PUT /v1/admin/teams/{id}/budget`. Empty `cap` removes the budget.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamBudgetInput {
    pub cap: Option<String>,
    pub currency: Option<String>,
}

/// A team's yearly budget cap (admin and HR only). `used` is the top of the salary range of its
/// filled and open seats in the budget currency; `remaining` is `cap` minus `used`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamBudget {
    pub team_id: String,
    pub team_name: String,
    pub cap: Option<String>,
    pub currency: Option<String>,
    pub used: String,
    pub remaining: Option<String>,
}

/// A seat in the org chart, with the seats that report to it. Built from `reports_to_seat_id`;
/// a seat whose parent is missing or closed is a root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatNode {
    pub seat: Seat,
    pub reports: Vec<SeatNode>,
}

/// Reply of `GET /v1/admin/roles`: one entry per built-in role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleMembers {
    pub role: String,
    pub members: Vec<PersonSummary>,
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

    #[test]
    fn profile_and_personal_round_trip() {
        let profile = Profile {
            id: "0190a6d2-7c1e-7b3a-9f4e-2d5c8a1b6e10".to_string(),
            email: "a@example.com".to_string(),
            display_name: "A".to_string(),
            preferred_name: None,
            pronouns: Some("they/them".to_string()),
            title: Some("Lead".to_string()),
            position_id: None,
            department_id: None,
            department_name: None,
            team_id: None,
            team_name: None,
            manager_id: None,
            location: None,
            timezone: None,
            work_phone: None,
            bio: None,
            skills: vec!["Rust".to_string()],
            start_date: Some("2024-01-02".to_string()),
            avatar_url: None,
            is_active: true,
            roles: vec![ROLE_HR.to_string()],
        };
        round_trip(&PersonProfile {
            profile,
            personal: Some(PersonalInfo {
                personal_phone: None,
                personal_email: None,
                address: None,
                birthday: Some("1990-05-01".to_string()),
                emergency_contact: None,
            }),
        });
    }

    #[test]
    fn patch_bodies_refuse_unknown_fields() {
        let self_ok: Result<UpdateMyProfile, _> = serde_json::from_str(r#"{"bio":"hi"}"#);
        assert!(self_ok.is_ok());
        let self_title: Result<UpdateMyProfile, _> = serde_json::from_str(r#"{"title":"CEO"}"#);
        assert!(self_title.is_err(), "title is not self-editable");
        let admin_bad: Result<AdminUpdateUser, _> = serde_json::from_str(r#"{"bio":"x"}"#);
        assert!(admin_bad.is_err());
    }

    #[test]
    fn page_and_department_round_trip() {
        round_trip(&Page::<PersonSummary> {
            items: vec![],
            next_cursor: Some("0190a6d2-7c1e-7b3a-9f4e-2d5c8a1b6e10".to_string()),
        });
        round_trip(&Department {
            id: "d".to_string(),
            name: "Art".to_string(),
            parent_department_id: None,
            head_id: None,
            member_count: 3,
        });
    }
}

/// Location kinds (Company > Locations): a company office, a site, or a remote hub.
pub const LOCATION_OFFICE: &str = "office";
pub const LOCATION_SITE: &str = "site";
pub const LOCATION_REMOTE_HUB: &str = "remote_hub";
/// The only location kinds the API accepts.
pub const LOCATION_KINDS: &[&str] = &[LOCATION_OFFICE, LOCATION_SITE, LOCATION_REMOTE_HUB];

/// A company location: an office, site or remote hub. Every signed-in person can read them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub address: String,
    pub city: String,
    pub country: String,
    pub timezone: String,
    pub note: String,
}

/// Body of location create (name and kind required) and update. Absent fields keep their value;
/// an empty string clears a text field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationInput {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
    pub note: Option<String>,
}
