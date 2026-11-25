//! Command palette data (S9.3; PLAN 6.1). The rows come from the `space_nav` manifest. The filter and the highlight
//! step are shared: they live in `rimelight_shell::palette`, which holds the tests of those rules. The markup
//! (`@rimelight_shell/components/command-palette.slint`) draws the rows and reports keys. Space's app windows fill the rows
//! and answer the callbacks from these rules; the shared gallery (rimelight-rs) has its own palette story.
//!
//! Pages whose only parameter is `:key` (the project pages, `/p/:key/...`) are listed for one project: the key is
//! filled in from [`DEFAULT_PROJECT_KEY`]. Pages with any other parameter (a record id, for example `/p/:key/issues/:id`)
//! are left out: a pick has no id to fill in, as in the sidebar (PLAN 6.4). Each page's `aliases` are searched too
//! (OQ6: Pipelines is also found by "Builds" and "Jobs").

use crate::slint_ui::PaletteRow;
use space_nav::{format_route, pages};
use space_page_kit::projects;

pub use rimelight_shell::palette::{Entry, filter, step};

/// The key the app starts on, until the project list loads (`projects::set_list`, decision 0030).
pub use space_page_kit::projects::DEFAULT_PROJECT_KEY;

/// The palette entries for the current project (the project switcher's choice, tail C3; the default is
/// [`DEFAULT_PROJECT_KEY`]).
pub fn entries() -> Vec<Entry> {
    entries_for(&projects::current_key())
}

/// Every page the palette can open, in manifest order: the parameter-free pages, and the pages whose only parameter is
/// `:key`, filled in with `project_key`. A page with another parameter is left out.
pub fn entries_for(project_key: &str) -> Vec<Entry> {
    pages()
        .iter()
        .filter_map(|page| {
            if page.has_params() && !page.only_key_param() {
                return None;
            }
            let route = if page.has_params() {
                format_route(page.route, &[("key".to_string(), project_key.to_string())])
            } else {
                page.route.to_string()
            };
            Some(Entry {
                title: page.title,
                route,
                aliases: page.aliases,
            })
        })
        .collect()
}

/// The palette rows that match `query`, as Slint rows.
pub fn rows_for(query: &str, all: &[Entry]) -> Vec<PaletteRow> {
    filter(query, all)
        .into_iter()
        .map(|index| PaletteRow {
            title: all[index].title.into(),
            route: all[index].route.as_str().into(),
            subtitle: "".into(),
            category: "".into(),
        })
        .collect()
}
