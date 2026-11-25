//! space_policy: access decisions for projects and issues (PLAN-archive 7.6, decision 0026).
//!
//! This is the first piece of the relationship-based engine. Today a decision takes two inputs:
//! whether the caller holds the global `admin` role, and the caller's role on the project (if
//! they are a member). Later steps read the same relations from Postgres tuples. The functions
//! here are pure: no I/O, no clock, no storage.
//!
//! Rules:
//! - The global `admin` role overrides everything: admins view and manage every project.
//! - Project roles are `admin`, `member` and `viewer`. Only members of a project have a role.
//! - Non-members cannot see a project at all.
//! - Viewers see a project but cannot change its issues.

/// A role on one project. Stored in `project_members.role`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRole {
    /// Manages the project: members, workflow and sprints, and edits issues.
    Admin,
    /// Works on the project: creates and edits issues, comments and links.
    Member,
    /// Reads the project only.
    Viewer,
}

impl ProjectRole {
    /// Parses the database value (`admin`, `member` or `viewer`). Anything else is `None`.
    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }
}

/// What the caller may do on a project: their global role and their project role, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    /// True when the caller holds the global `admin` role.
    pub global_admin: bool,
    /// The caller's role on the project. `None` when they are not a member.
    pub project_role: Option<ProjectRole>,
}

impl Access {
    /// Access for a caller with no global admin role and the given project role.
    #[must_use]
    pub const fn for_member(role: Option<ProjectRole>) -> Self {
        Self {
            global_admin: false,
            project_role: role,
        }
    }

    /// Access for a caller with the global `admin` role. The project role does not matter.
    #[must_use]
    pub const fn global_admin() -> Self {
        Self {
            global_admin: true,
            project_role: None,
        }
    }
}

/// True when the caller may see the project and its issues.
///
/// Global admins and any project member (of any role) may view. Non-members may not.
#[must_use]
pub fn can_view_project(access: Access) -> bool {
    access.global_admin || access.project_role.is_some()
}

/// True when the caller may create, edit, comment on and link issues in the project.
///
/// Global admins, project admins and project members may edit. Viewers may not.
#[must_use]
pub fn can_edit_issues(access: Access) -> bool {
    access.global_admin
        || matches!(
            access.project_role,
            Some(ProjectRole::Admin | ProjectRole::Member)
        )
}

/// True when the caller may change the project itself: its members, workflow, sprints and details.
///
/// Only global admins and project admins may manage. Members and viewers may not.
#[must_use]
pub fn can_manage_project(access: Access) -> bool {
    access.global_admin || access.project_role == Some(ProjectRole::Admin)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLES: [Option<ProjectRole>; 4] = [
        None,
        Some(ProjectRole::Viewer),
        Some(ProjectRole::Member),
        Some(ProjectRole::Admin),
    ];

    #[test]
    fn view_requires_membership_unless_global_admin() {
        assert!(!can_view_project(Access::for_member(None)));
        for role in ROLES.iter().skip(1) {
            assert!(can_view_project(Access::for_member(*role)), "{role:?}");
        }
        assert!(can_view_project(Access::global_admin()));
    }

    #[test]
    fn edit_is_refused_to_viewers_and_non_members() {
        assert!(!can_edit_issues(Access::for_member(None)));
        assert!(!can_edit_issues(Access::for_member(Some(
            ProjectRole::Viewer
        ))));
        assert!(can_edit_issues(Access::for_member(Some(
            ProjectRole::Member
        ))));
        assert!(can_edit_issues(Access::for_member(Some(
            ProjectRole::Admin
        ))));
        assert!(can_edit_issues(Access::global_admin()));
    }

    #[test]
    fn manage_is_for_project_admins_and_global_admins_only() {
        for role in ROLES {
            let expected = role == Some(ProjectRole::Admin);
            assert_eq!(
                can_manage_project(Access::for_member(role)),
                expected,
                "{role:?}"
            );
        }
        assert!(can_manage_project(Access::global_admin()));
    }

    #[test]
    fn global_admin_overrides_a_missing_or_low_project_role() {
        let overriding = Access {
            global_admin: true,
            project_role: None,
        };
        assert!(can_view_project(overriding));
        assert!(can_edit_issues(overriding));
        assert!(can_manage_project(overriding));

        let viewer_global_admin = Access {
            global_admin: true,
            project_role: Some(ProjectRole::Viewer),
        };
        assert!(can_edit_issues(viewer_global_admin));
        assert!(can_manage_project(viewer_global_admin));
    }

    #[test]
    fn each_level_implies_the_one_below() {
        for role in ROLES {
            let access = Access::for_member(role);
            if can_manage_project(access) {
                assert!(can_edit_issues(access), "{role:?} manages but cannot edit");
            }
            if can_edit_issues(access) {
                assert!(can_view_project(access), "{role:?} edits but cannot view");
            }
        }
    }

    #[test]
    fn parses_database_role_values() {
        assert_eq!(ProjectRole::from_db("admin"), Some(ProjectRole::Admin));
        assert_eq!(ProjectRole::from_db("member"), Some(ProjectRole::Member));
        assert_eq!(ProjectRole::from_db("viewer"), Some(ProjectRole::Viewer));
        assert_eq!(ProjectRole::from_db("owner"), None);
        assert_eq!(ProjectRole::from_db("Admin"), None);
    }
}
