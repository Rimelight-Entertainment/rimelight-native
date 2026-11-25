//! Scopes of content (decision 0049 answer 2): a page belongs to a project, a team, a department or the company. The scope
//! picker's rules and the access checks for a move are plain functions here.

use space_client::content::{PageScope, ScopeKind};

/// Where a page lives. Company pages have no id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Company,
    Project(String),
    Team(String),
    Department(String),
}

impl Scope {
    /// The client's scope value for list and create calls.
    pub fn to_page_scope(&self) -> PageScope {
        match self {
            Scope::Company => PageScope {
                kind: ScopeKind::Company,
                id: None,
            },
            Scope::Project(id) => PageScope {
                kind: ScopeKind::Project,
                id: Some(id.clone()),
            },
            Scope::Team(id) => PageScope {
                kind: ScopeKind::Team,
                id: Some(id.clone()),
            },
            Scope::Department(id) => PageScope {
                kind: ScopeKind::Department,
                id: Some(id.clone()),
            },
        }
    }

    /// The scope a page's scope fields name (`scope_kind`, `scope_id`). A non-company kind without an id is Company.
    pub fn from_page(kind: ScopeKind, id: Option<&str>) -> Scope {
        match (kind, id) {
            (ScopeKind::Project, Some(id)) => Scope::Project(id.to_string()),
            (ScopeKind::Team, Some(id)) => Scope::Team(id.to_string()),
            (ScopeKind::Department, Some(id)) => Scope::Department(id.to_string()),
            _ => Scope::Company,
        }
    }

    /// The id inside the scope (`None` for Company).
    pub fn id(&self) -> Option<&str> {
        match self {
            Scope::Company => None,
            Scope::Project(id) | Scope::Team(id) | Scope::Department(id) => Some(id),
        }
    }
}

/// The caller's access on a scope, as `PageSummary.my_access` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    None,
    View,
    Edit,
    Manage,
}

/// Parses `my_access`. Unknown text is no access.
pub fn parse_access(text: &str) -> Access {
    match text {
        "manage" => Access::Manage,
        "edit" => Access::Edit,
        "view" => Access::View,
        _ => Access::None,
    }
}

/// Edit or better (new pages, drafts, the editor).
pub fn can_edit(access: Access) -> bool {
    matches!(access, Access::Edit | Access::Manage)
}

/// Manage (visibility, delete, invite, move out of the scope).
pub fn can_manage(access: Access) -> bool {
    access == Access::Manage
}

/// A move needs manage on the page's current scope and on the scope it goes to (CMS.4 default: PATCH .../scope).
pub fn can_move(from: Access, to: Access) -> bool {
    can_manage(from) && can_manage(to)
}

/// One row of the scope picker: the scope and its label ("Company", "Project: Atlas", ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeChoice {
    pub scope: Scope,
    pub label: String,
}

/// The picker's choices in order: Company first, then the projects, the teams and the departments the caller can see. Each
/// input is `(id, name)`.
pub fn scope_choices(
    projects: &[(String, String)],
    teams: &[(String, String)],
    departments: &[(String, String)],
) -> Vec<ScopeChoice> {
    let mut out = vec![ScopeChoice {
        scope: Scope::Company,
        label: "Company".to_string(),
    }];
    for (id, name) in projects {
        out.push(ScopeChoice {
            scope: Scope::Project(id.clone()),
            label: format!("Project: {name}"),
        });
    }
    for (id, name) in teams {
        out.push(ScopeChoice {
            scope: Scope::Team(id.clone()),
            label: format!("Team: {name}"),
        });
    }
    for (id, name) in departments {
        out.push(ScopeChoice {
            scope: Scope::Department(id.clone()),
            label: format!("Department: {name}"),
        });
    }
    out
}

/// The picker's choices without the page's current scope (a move to the same scope is refused before the request).
pub fn move_targets(current: &Scope, choices: &[ScopeChoice]) -> Vec<ScopeChoice> {
    choices
        .iter()
        .filter(|c| &c.scope != current)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn company_is_first_then_projects_teams_departments() {
        let choices = scope_choices(
            &pairs(&[("p1", "Atlas")]),
            &pairs(&[("t1", "Art")]),
            &pairs(&[("d1", "Ops")]),
        );
        let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Company", "Project: Atlas", "Team: Art", "Department: Ops"]
        );
    }

    #[test]
    fn teams_and_departments_keep_their_ids_and_order() {
        let choices = scope_choices(
            &[],
            &pairs(&[("t2", "Design"), ("t1", "Art")]),
            &pairs(&[("d9", "Ops")]),
        );
        assert_eq!(choices[0].scope, Scope::Company);
        assert_eq!(choices[1].scope, Scope::Team("t2".into()));
        assert_eq!(choices[1].label, "Team: Design");
        assert_eq!(choices[2].scope, Scope::Team("t1".into()));
        assert_eq!(choices[3].scope, Scope::Department("d9".into()));
        assert_eq!(choices[3].label, "Department: Ops");
        assert_eq!(choices.len(), 4);
    }

    #[test]
    fn move_targets_drop_the_current_scope() {
        let choices = scope_choices(&pairs(&[("p1", "Atlas")]), &[], &[]);
        let targets = move_targets(&Scope::Project("p1".into()), &choices);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].scope, Scope::Company);
    }

    #[test]
    fn move_needs_manage_on_both_scopes() {
        assert!(can_move(Access::Manage, Access::Manage));
        assert!(!can_move(Access::Manage, Access::Edit));
        assert!(!can_move(Access::Edit, Access::Manage));
        assert!(!can_move(Access::View, Access::View));
    }

    #[test]
    fn access_text_parses() {
        assert_eq!(parse_access("edit"), Access::Edit);
        assert_eq!(parse_access("manage"), Access::Manage);
        assert_eq!(parse_access("weird"), Access::None);
        assert!(can_edit(Access::Edit) && !can_edit(Access::View));
    }

    #[test]
    fn page_scope_round_trips() {
        let s = Scope::Team("t9".into());
        let p = s.to_page_scope();
        assert_eq!(Scope::from_page(p.kind, p.id.as_deref()), s);
        assert_eq!(Scope::from_page(ScopeKind::Company, None), Scope::Company);
        assert_eq!(Scope::Company.id(), None);
    }
}
