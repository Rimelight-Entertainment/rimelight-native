//! Projects (IA sections 2.6 and 3; tail C3): the project list and the current project.
//!
//! The list is the one `GET /v1/projects` returns for the signed-in person (S1.3, decision 0030). It is empty until the
//! first load, and [`set_list`] sets it. The current project is kept here, per thread, as the shell keeps its other state
//! (the collapsed headings in `shell.rs`). It starts on [`DEFAULT_PROJECT_KEY`]; when the list loads and that key is not in
//! it, the first project becomes the current one. The Projects sidebar lists the project pages for the current key, and the
//! project switcher changes it.

use std::cell::RefCell;
use std::collections::HashMap;

use slint::SharedPixelBuffer;
use space_proto::projects::ProjectSummary;

use crate::page_data::{self, Pictures};
/// The project key a fresh window starts on: none (the first project of the list becomes current).
pub const DEFAULT_PROJECT_KEY: &str = "";

/// One project of the list: its key (the `:key` of its `/p/:key/...` pages) and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub key: String,
    pub name: String,
}

thread_local! {
    /// The projects the person is a member of, in the server's order.
    static LIST: RefCell<Vec<Project>> = const { RefCell::new(Vec::new()) };
    /// The key of the current project.
    static CURRENT: RefCell<String> = RefCell::new(DEFAULT_PROJECT_KEY.to_owned());
    /// The keys of the projects the person may create issues in (see [`set_editable`]).
    static EDITABLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The picture path of each project that has one, by project key (see [`set_picture_paths`]).
    static PICTURE_PATHS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// The decoded project pictures by their path, and the paths on their way.
    static PICTURES: RefCell<Pictures> = RefCell::new(Pictures::default());
}

/// Sets the picture path of each project that has one (`image_url` of the list), replacing the earlier paths.
pub fn set_picture_paths(summaries: &[ProjectSummary]) {
    let paths: HashMap<String, String> = summaries
        .iter()
        .filter_map(|project| {
            project
                .image_url
                .clone()
                .map(|url| (project.key.clone(), url))
        })
        .collect();
    PICTURE_PATHS.with(|map| *map.borrow_mut() = paths);
}

/// The picture paths of the listed projects that are not decoded yet and not on their way. Marks them as on their way.
pub fn take_pictures_to_fetch() -> Vec<String> {
    let paths: Vec<String> = PICTURE_PATHS.with(|map| map.borrow().values().cloned().collect());
    PICTURES.with(|pictures| {
        let mut pictures = pictures.borrow_mut();
        let mut wanted = Vec::new();
        for path in paths {
            if !pictures.ready.contains_key(&path) && pictures.pending.insert(path.clone()) {
                wanted.push(path);
            }
        }
        wanted
    })
}

/// A picture finished loading (or failed, with `None`). A failed one is asked for again on the next list load.
pub fn picture_loaded(path: &str, buffer: Option<SharedPixelBuffer<slint::Rgba8Pixel>>) {
    PICTURES.with(|pictures| {
        let mut pictures = pictures.borrow_mut();
        pictures.pending.remove(path);
        if let Some(buffer) = buffer {
            pictures.ready.insert(path.to_owned(), buffer);
        }
    });
}

/// The decoded picture of the project with `key`, or `None` when it has none or it is not loaded yet.
pub fn picture(key: &str) -> Option<slint::Image> {
    let path = PICTURE_PATHS.with(|map| map.borrow().get(key).cloned())?;
    PICTURES.with(|pictures| pictures.borrow().ready.get(&path).map(page_data::picture))
}

/// Replaces the list with `projects` (from `GET /v1/projects`). The current project stays when it is in the list; otherwise
/// the first project becomes the current one.
pub fn set_list(projects: Vec<Project>) {
    let first = projects.first().map(|project| project.key.clone());
    let keep = CURRENT.with(|current| {
        let current = current.borrow();
        projects.iter().any(|project| project.key == *current)
    });
    LIST.with(|list| *list.borrow_mut() = projects);
    if !keep && let Some(first) = first {
        CURRENT.with(|current| *current.borrow_mut() = first);
    }
}

/// The list the server sent, as the picker's projects.
pub fn from_summaries(summaries: Vec<ProjectSummary>) -> Vec<Project> {
    summaries
        .into_iter()
        .map(|project| Project {
            key: project.key,
            name: project.name,
        })
        .collect()
}

/// A fixed list of three projects, for the headless captures and the tests. It is not used by the app.
pub fn fixture() -> Vec<Project> {
    [
        ("grand-tale", "Grand Tale"),
        ("lantern-keep", "Lantern Keep"),
        ("harbor-run", "Harbor Run"),
    ]
    .into_iter()
    .map(|(key, name)| Project {
        key: key.to_owned(),
        name: name.to_owned(),
    })
    .collect()
}

/// Sets the keys of the projects the person may create issues in (a member or an admin, not a viewer). The project list
/// sets them with the list (`wire_projects`), so the New issue entries follow the same answer.
pub fn set_editable(keys: Vec<String>) {
    EDITABLE.with(|editable| *editable.borrow_mut() = keys);
}

/// The projects the person may create issues in, in list order. Empty until the list loads.
pub fn editable() -> Vec<Project> {
    let keys = EDITABLE.with(|editable| editable.borrow().clone());
    list()
        .into_iter()
        .filter(|project| keys.contains(&project.key))
        .collect()
}

/// The current list.
pub fn list() -> Vec<Project> {
    LIST.with(|list| list.borrow().clone())
}

/// The key of the current project.
pub fn current_key() -> String {
    CURRENT.with(|current| current.borrow().clone())
}

/// The name of the current project, or an empty name when the list does not have it (yet).
pub fn current_name() -> String {
    name_of(&current_key())
}

/// The name of the project with `key`, or an empty name when the list has no such project.
pub fn name_of(key: &str) -> String {
    LIST.with(|list| {
        list.borrow()
            .iter()
            .find(|project| project.key == key)
            .map(|project| project.name.clone())
            .unwrap_or_default()
    })
}

/// Makes the project with `key` the current one. Returns false, and changes nothing, when the list has no such project.
pub fn set_current(key: &str) -> bool {
    let found = LIST.with(|list| list.borrow().iter().any(|project| project.key == key));
    if found {
        CURRENT.with(|current| *current.borrow_mut() = key.to_owned());
    }
    found
}

/// The projects whose name or key contains `query`, ignoring case, in list order. A blank query (after trimming) lists
/// every project. The switcher shows all of them (OQ9): there are no favourites or recents.
pub fn filter(query: &str) -> Vec<Project> {
    let needle = query.trim().to_lowercase();
    list()
        .into_iter()
        .filter(|project| {
            needle.is_empty()
                || project.name.to_lowercase().contains(&needle)
                || project.key.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The project key of a route `/p/:key/...`, or `None` for any other route (shared by the projects and management pages).
pub fn project_of_route(route: &str) -> Option<&str> {
    route
        .strip_prefix("/p/")
        .and_then(|rest| rest.split('/').next())
        .filter(|key| !key.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_replaced_and_the_current_project_follows_it() {
        set_list(fixture());
        assert!(set_current("harbor-run"));
        assert!(!set_current("nope"), "an unknown key changes nothing");
        assert_eq!(current_name(), "Harbor Run");

        // The current project is kept when the new list still has it.
        set_list(vec![
            Project {
                key: "ops".into(),
                name: "Ops".into(),
            },
            Project {
                key: "harbor-run".into(),
                name: "Harbor".into(),
            },
        ]);
        assert_eq!(current_key(), "harbor-run");
        assert_eq!(current_name(), "Harbor");

        // Otherwise the first project of the new list is current.
        set_list(vec![Project {
            key: "ops".into(),
            name: "Ops".into(),
        }]);
        assert_eq!(current_key(), "ops");
        assert_eq!(
            name_of("grand-tale"),
            "",
            "a project not in the list has no name"
        );
        assert_eq!(filter("OP").len(), 1);
        assert_eq!(filter("  ").len(), 1);
    }
}
