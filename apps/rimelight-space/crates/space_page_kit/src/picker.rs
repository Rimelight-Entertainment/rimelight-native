//! The pickers' plain choice rows (select and combobox options) and the date shape check. The rows carry the value and the
//! label only; the window glue (`picker_glue!`) turns them into the generated `SelectOption`.

use space_proto::people::{Department, OrgTreeEntry, Position, Team};

/// One choice in a select or combobox: its value and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

/// One choice with a value and a label.
pub fn choice(value: &str, label: &str) -> Choice {
    Choice {
        value: value.to_string(),
        label: label.to_string(),
    }
}

/// The people for a manager, head or lead picker, with "None" first when `none` is set.
pub fn person_choices(people: &[OrgTreeEntry], none: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if none {
        out.push(choice("", "None"));
    }
    out.extend(people.iter().map(|p| choice(&p.id, &p.name)));
    out
}

/// The departments for a picker, with "None" first when `none` is set.
pub fn department_choices(departments: &[Department], none: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if none {
        out.push(choice("", "None"));
    }
    out.extend(departments.iter().map(|d| choice(&d.id, &d.name)));
    out
}

/// The teams for a picker, labelled with their department, with "None" first when `none` is set.
pub fn team_choices(teams: &[Team], departments: &[Department], none: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if none {
        out.push(choice("", "None"));
    }
    out.extend(teams.iter().map(|t| {
        let dept = departments
            .iter()
            .find(|d| d.id == t.department_id)
            .map(|d| d.name.as_str())
            .unwrap_or("");
        let label = if dept.is_empty() {
            t.name.clone()
        } else {
            format!("{} · {dept}", t.name)
        };
        choice(&t.id, &label)
    }));
    out
}

/// The positions for a picker, with "None" first when `none` is set.
pub fn position_choices(positions: &[Position], none: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if none {
        out.push(choice("", "None"));
    }
    out.extend(positions.iter().map(|p| choice(&p.id, &p.title)));
    out
}

/// A start date as the form takes it: empty, or `YYYY-MM-DD` (the shape only; the server checks the calendar).
pub fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.is_empty()
        || (bytes.len() == 10
            && bytes.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b'-',
                _ => b.is_ascii_digit(),
            }))
}

/// The window glue for the pickers: expands in the app crate, where the generated `SelectOption` is in scope. Emits
/// `option` (one row) and `options` (a model of rows), both by path under the caller's module.
#[macro_export]
macro_rules! picker_glue {
    ($SelectOption:ident) => {
        /// One plain choice as the generated select option (no icon, badge or group).
        pub fn option(choice: $crate::picker::Choice) -> $SelectOption {
            $SelectOption {
                value: choice.value.into(),
                label: choice.label.into(),
                icon: Default::default(),
                badge: Default::default(),
                disabled: false,
                group: Default::default(),
            }
        }

        /// A model of the generated select options for the plain choices, in order.
        pub fn options(
            choices: Vec<$crate::picker::Choice>,
        ) -> $crate::__slint::ModelRc<$SelectOption> {
            $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(
                choices.into_iter().map(option).collect::<Vec<_>>(),
            ))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_empty_or_the_iso_shape() {
        assert!(valid_date(""));
        assert!(valid_date("2026-11-15"));
        assert!(!valid_date("15/11/2026"));
        assert!(!valid_date("2026-1-5"));
        assert!(!valid_date("abcd-ef-gh"));
    }

    #[test]
    fn team_choices_name_their_department_and_none_comes_first() {
        let departments = vec![Department {
            id: "d1".into(),
            name: "Engineering".into(),
            parent_department_id: None,
            head_id: None,
            member_count: 0,
        }];
        let teams = vec![Team {
            id: "t1".into(),
            name: "Gameplay".into(),
            department_id: "d1".into(),
            lead_id: None,
            member_count: 0,
        }];
        let options = team_choices(&teams, &departments, true);
        assert_eq!(options[0].label, "None");
        assert_eq!(options[1].label, "Gameplay · Engineering");
        assert_eq!(options[1].value, "t1");
        assert_eq!(team_choices(&teams, &departments, false).len(), 1);
    }
}
