//! The org tree and the people behind it (SP.3d): the org chart on Company > Teams, the department tree on Company >
//! Departments, and the team lead and head lookups. Pure data: no window, so the tests need no UI.
//!
//! A [`Tree`] is built from `(id, parent id)` pairs. Only the rows under an expanded node are listed ([`Tree::rows`]), so a
//! tree of 300 people draws a few dozen rows. A node whose parent is missing (or not in the list) is a root.

use std::collections::{HashMap, HashSet};

use space_proto::people::{OrgTreeEntry, SEAT_OPEN, SEAT_PLANNED, SeatNode};

/// One listed node: its id, its depth (roots are 0), how many children it has, and whether it is expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub depth: usize,
    pub children: usize,
    pub expanded: bool,
}

/// A forest of nodes by id. Children keep the order they were given in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    roots: Vec<String>,
    children: HashMap<String, Vec<String>>,
    parent: HashMap<String, String>,
}

impl Tree {
    /// Builds the tree from `(id, parent id)` pairs. A node that names itself as its parent is a root.
    pub fn new<I>(nodes: I) -> Self
    where
        I: IntoIterator<Item = (String, Option<String>)>,
    {
        let nodes: Vec<(String, Option<String>)> = nodes.into_iter().collect();
        let ids: HashSet<String> = nodes.iter().map(|(id, _)| id.clone()).collect();
        let mut tree = Tree::default();
        for (id, parent) in nodes {
            match parent {
                Some(p) if p != id && ids.contains(&p) => {
                    tree.children.entry(p.clone()).or_default().push(id.clone());
                    tree.parent.insert(id, p);
                }
                _ => tree.roots.push(id),
            }
        }
        tree
    }

    /// True when the tree lists nothing.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// The ids above `id`, nearest first, up to its root. Empty for a root or an id the tree does not hold.
    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut at: &str = id;
        // A parent link cycle (which the server does not allow) would never end, so the walk is bounded.
        while let Some(parent) = self.parent.get(at) {
            if out.len() > self.parent.len() {
                break;
            }
            out.push(parent.clone());
            at = parent;
        }
        out
    }

    /// The nodes that start expanded: the roots and their children, so two levels show.
    pub fn initial_expanded(&self) -> HashSet<String> {
        let mut expanded: HashSet<String> = self.roots.iter().cloned().collect();
        for root in &self.roots {
            if let Some(kids) = self.children.get(root) {
                expanded.extend(kids.iter().cloned());
            }
        }
        expanded
    }

    /// The rows that are visible: the roots, and under each expanded node its children, depth first.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<Row> {
        let mut out = Vec::new();
        let mut stack: Vec<(String, usize)> =
            self.roots.iter().rev().map(|id| (id.clone(), 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            let kids = self.children.get(&id);
            let open = expanded.contains(&id);
            if let Some(kids) = kids.filter(|_| open) {
                for kid in kids.iter().rev() {
                    stack.push((kid.clone(), depth + 1));
                }
            }
            out.push(Row {
                children: kids.map_or(0, Vec::len),
                id,
                depth,
                expanded: open,
            });
        }
        out
    }

    /// Expands every ancestor of `id`, so the node is listed.
    pub fn reveal(&self, expanded: &mut HashSet<String>, id: &str) {
        expanded.extend(self.ancestors(id));
    }
}

/// The company's people for the org chart: the list in the server's name order, by id, and the tree of managers.
#[derive(Debug, Clone, Default)]
pub struct People {
    list: Vec<OrgTreeEntry>,
    index: HashMap<String, usize>,
    pub tree: Tree,
}

impl People {
    /// Builds the lookups from `GET /v1/org/tree`. A person whose manager is not in the list is a root.
    pub fn new(list: Vec<OrgTreeEntry>) -> Self {
        let index = list
            .iter()
            .enumerate()
            .map(|(at, person)| (person.id.clone(), at))
            .collect();
        let tree = Tree::new(
            list.iter()
                .map(|person| (person.id.clone(), person.manager_id.clone())),
        );
        Self { list, index, tree }
    }

    /// One person by id.
    pub fn get(&self, id: &str) -> Option<&OrgTreeEntry> {
        self.index.get(id).and_then(|&at| self.list.get(at))
    }

    /// Every person, in the server's order.
    pub fn all(&self) -> &[OrgTreeEntry] {
        &self.list
    }

    /// The first person (in order) whose name contains `query`, ignoring case. `None` for an empty query.
    pub fn find(&self, query: &str) -> Option<&OrgTreeEntry> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        self.list
            .iter()
            .find(|person| person.name.to_lowercase().contains(&needle))
    }
}

/// A seat as the org chart draws it (SP.5). `vacant` is true for an open or planned seat (no person yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatRow {
    pub id: String,
    pub title: String,
    pub team: String,
    pub person_id: Option<String>,
    pub vacant: bool,
    pub planned: bool,
}

/// The org chart from seats (`GET /v1/org/seats`): one tree node per seat, with the seat rows by id and the seat each person
/// fills. Built once per load; closed seats are not in the chart.
#[derive(Debug, Clone, Default)]
pub struct Chart {
    pub tree: Tree,
    seats: HashMap<String, SeatRow>,
    seat_of_person: HashMap<String, String>,
}

impl Chart {
    /// Builds the chart from the server's nested seats. Siblings keep the server's order.
    pub fn new(nodes: &[SeatNode]) -> Self {
        let mut pairs: Vec<(String, Option<String>)> = Vec::new();
        let mut seats = HashMap::new();
        let mut seat_of_person = HashMap::new();
        let mut stack: Vec<(&SeatNode, Option<String>)> =
            nodes.iter().rev().map(|node| (node, None)).collect();
        while let Some((node, parent)) = stack.pop() {
            let seat = &node.seat;
            pairs.push((seat.id.clone(), parent));
            if let Some(person) = &seat.person_id {
                seat_of_person.insert(person.clone(), seat.id.clone());
            }
            seats.insert(
                seat.id.clone(),
                SeatRow {
                    id: seat.id.clone(),
                    title: seat.job_profile_title.clone().unwrap_or_default(),
                    team: seat.team_name.clone(),
                    person_id: seat.person_id.clone(),
                    vacant: seat.state == SEAT_OPEN || seat.state == SEAT_PLANNED,
                    planned: seat.state == SEAT_PLANNED,
                },
            );
            for child in node.reports.iter().rev() {
                stack.push((child, Some(seat.id.clone())));
            }
        }
        Self {
            tree: Tree::new(pairs),
            seats,
            seat_of_person,
        }
    }

    /// One seat by id.
    pub fn seat(&self, id: &str) -> Option<&SeatRow> {
        self.seats.get(id)
    }

    /// The seat a person fills, if any.
    pub fn seat_for_person(&self, person_id: &str) -> Option<&str> {
        self.seat_of_person.get(person_id).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small company: A is the root, B and C report to A, D to B, E to D, F to C.
    fn sample() -> Tree {
        Tree::new(vec![
            ("A".into(), None),
            ("B".into(), Some("A".into())),
            ("C".into(), Some("A".into())),
            ("D".into(), Some("B".into())),
            ("E".into(), Some("D".into())),
            ("F".into(), Some("C".into())),
        ])
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn only_the_root_shows_when_nothing_is_expanded() {
        let tree = sample();
        let rows = tree.rows(&HashSet::new());
        assert_eq!(ids(&rows), vec!["A"]);
        assert_eq!(rows[0].children, 2, "the direct-report count is kept");
        assert!(!rows[0].expanded);
    }

    #[test]
    fn two_levels_start_expanded_and_deeper_nodes_stay_closed() {
        let tree = sample();
        let rows = tree.rows(&tree.initial_expanded());
        assert_eq!(ids(&rows), vec!["A", "B", "D", "C", "F"]);
        let depths: Vec<usize> = rows.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 1, 2]);
        assert!(rows.iter().find(|r| r.id == "D").expect("D").children == 1);
        assert!(!rows.iter().find(|r| r.id == "D").expect("D").expanded);
    }

    #[test]
    fn reveal_expands_the_path_to_a_deep_node() {
        let tree = sample();
        let mut expanded = tree.initial_expanded();
        tree.reveal(&mut expanded, "E");
        let rows = tree.rows(&expanded);
        assert_eq!(ids(&rows), vec!["A", "B", "D", "E", "C", "F"]);
        assert_eq!(rows[3].depth, 3);
    }

    #[test]
    fn ancestors_run_nearest_first_and_stop_at_the_root() {
        let tree = sample();
        assert_eq!(tree.ancestors("E"), vec!["D", "B", "A"]);
        assert!(tree.ancestors("A").is_empty());
        assert!(tree.ancestors("nobody").is_empty());
    }

    #[test]
    fn a_missing_manager_makes_a_root_and_a_cycle_is_not_listed() {
        let tree = Tree::new(vec![
            ("X".into(), Some("gone".into())),
            ("Y".into(), Some("Z".into())),
            ("Z".into(), Some("Y".into())),
            ("W".into(), Some("W".into())),
        ]);
        let all: HashSet<String> = ["X", "Y", "Z", "W"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ids(&tree.rows(&all)), vec!["X", "W"]);
        assert!(tree.ancestors("Y").len() <= 4, "the walk is bounded");
    }

    #[test]
    fn people_find_the_first_match_and_refuse_an_empty_query() {
        let entry = |id: &str, name: &str, manager: Option<&str>| OrgTreeEntry {
            id: id.to_string(),
            name: name.to_string(),
            title: None,
            avatar_url: None,
            manager_id: manager.map(str::to_string),
            team: None,
        };
        let people = People::new(vec![
            entry("1", "Ann Adams", None),
            entry("2", "Ben Brook", Some("1")),
            entry("3", "Bea Bell", Some("2")),
        ]);
        assert_eq!(
            people.find("BE").map(|p| p.id.as_str()),
            Some("2"),
            "the first match in order"
        );
        assert_eq!(people.find("bea").map(|p| p.id.as_str()), Some("3"));
        assert!(people.find("   ").is_none());
        assert!(people.find("zed").is_none());
        assert_eq!(people.get("2").map(|p| p.name.as_str()), Some("Ben Brook"));
        assert_eq!(people.all().len(), 3);
        assert_eq!(people.tree.ancestors("3"), vec!["2", "1"]);
    }

    #[test]
    fn the_seat_chart_keeps_vacant_seats_and_maps_people_to_seats() {
        use space_proto::people::{SEAT_FILLED, Seat};
        let seat = |id: &str, state: &str, person: Option<&str>| Seat {
            id: id.to_string(),
            team_id: "t".to_string(),
            team_name: "Gameplay".to_string(),
            job_profile_id: None,
            job_profile_title: Some(format!("Seat {id}")),
            level: String::new(),
            state: state.to_string(),
            person_id: person.map(str::to_string),
            person_name: None,
            reports_to_seat_id: None,
            salary_min: None,
            salary_max: None,
            currency: None,
        };
        let node = |s: Seat, reports: Vec<SeatNode>| SeatNode { seat: s, reports };
        let chart = Chart::new(&[node(
            seat("top", SEAT_FILLED, Some("P1")),
            vec![
                node(seat("open", SEAT_OPEN, None), vec![]),
                node(seat("plan", SEAT_PLANNED, None), vec![]),
            ],
        )]);
        let rows = chart.tree.rows(&chart.tree.initial_expanded());
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["top", "open", "plan"]
        );
        assert!(chart.seat("open").expect("open").vacant);
        assert!(chart.seat("plan").expect("plan").planned);
        assert!(!chart.seat("top").expect("top").vacant);
        assert_eq!(chart.seat_for_person("P1"), Some("top"));
        assert_eq!(chart.seat_for_person("P9"), None);
    }
}
