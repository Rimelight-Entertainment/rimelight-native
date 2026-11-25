//! Navigation for the idantity app: the sidebar tree, as data.
//!
//! The tree is the egui shell's `NAV` table (`crates/app/src/shell.rs`), which mirrors the web app's
//! `config/navigation.ts`. A section groups items; an item is a link, and may carry sub-links that the sidebar shows as
//! indented rows when the item is expanded, and as a tab strip above the page. Each link names its Lucide icon.
//!
//! The functions here decide what the shell shows: which item is active for a route, the navbar title, and the sub-tabs.
//! No UI dependency, so the desktop shell and these tests read the same data.

mod tree;

pub use tree::{ASSETS, NAV};

/// One sidebar link: a label, the route it opens, and its Lucide icon name (kebab case, as in `lucide-static`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub label: &'static str,
    pub route: &'static str,
    pub icon: &'static str,
}

/// A sidebar item. `subs` is empty for a plain link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub link: Link,
    pub subs: &'static [Link],
}

/// A group of items. A section with no title is drawn without a heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub title: Option<&'static str>,
    pub items: &'static [Item],
}

/// Every sidebar section, in sidebar order.
pub fn sections() -> &'static [Section] {
    NAV
}

/// The item whose link, or one of whose sub-links, is `route`.
pub fn active_item(route: &str) -> Option<&'static Item> {
    NAV.iter()
        .flat_map(|section| section.items.iter())
        .find(|item| item.link.route == route || item.subs.iter().any(|sub| sub.route == route))
}

/// The navbar title for `route`: the matching sub-link's label, else the item's, else "Not ported yet".
/// `/storage` and `/inbox` keep their own titles, as the egui navbar showed them.
pub fn title_for(route: &str) -> &'static str {
    if route == ASSETS.route {
        return "Assets & Storage";
    }
    if route == "/inbox" {
        return "Inbox";
    }
    for section in NAV {
        for item in section.items {
            if let Some(sub) = item.subs.iter().find(|sub| sub.route == route) {
                return sub.label;
            }
            if item.link.route == route {
                return item.link.label;
            }
        }
    }
    "Not ported yet"
}

/// The sub-tab strip for `route`: the active item's sub-links, or none when the item has none. Watchlist's sections are
/// in the sidebar only, so its strip is never shown.
pub fn subtabs(route: &str) -> &'static [Link] {
    match active_item(route) {
        Some(item) if item.link.route != "/watchlist" => item.subs,
        _ => &[],
    }
}

/// The route for `path` when the app has a nav entry for it, with a trailing slash dropped (except on `/`). `None` for
/// any other path.
pub fn parse(path: &str) -> Option<&'static str> {
    let path = if path.len() == 1 {
        path
    } else {
        path.trim_end_matches('/')
    };
    NAV.iter()
        .flat_map(|section| section.items.iter())
        .flat_map(|item| std::iter::once(&item.link).chain(item.subs.iter()))
        .chain(std::iter::once(&ASSETS))
        .find(|link| link.route == path)
        .map(|link| link.route)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Every link and sub-link, footer included.
    fn all_links() -> Vec<&'static Link> {
        let mut links = vec![&ASSETS];
        for section in sections() {
            for item in section.items {
                links.push(&item.link);
                links.extend(item.subs.iter());
            }
        }
        links
    }

    #[test]
    fn routes_are_unique() {
        // A group's first sub-link ("Overview") shares its item's route on purpose, as in the egui tree. Every other
        // link must have a route of its own.
        let mut seen = HashSet::new();
        for section in sections() {
            for item in section.items {
                assert!(
                    seen.insert(item.link.route),
                    "{} appears twice",
                    item.link.route
                );
                for sub in item.subs.iter().filter(|sub| sub.route != item.link.route) {
                    assert!(seen.insert(sub.route), "{} appears twice", sub.route);
                }
            }
        }
        assert!(seen.insert(ASSETS.route));
    }

    #[test]
    fn every_link_names_an_icon() {
        for link in all_links() {
            assert!(!link.icon.is_empty(), "{} has no icon", link.route);
        }
    }

    #[test]
    fn tree_matches_the_egui_shell() {
        let titles: Vec<_> = sections().iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            [None, Some("Tracking"), Some("Management"), Some("Leisure")]
        );
        let finances = active_item("/finances").expect("finances");
        assert_eq!(finances.link.label, "Finances");
        let subs: Vec<_> = finances.subs.iter().map(|s| s.label).collect();
        assert_eq!(
            subs,
            [
                "Overview",
                "Transactions",
                "Accounts",
                "Incomes",
                "Bills",
                "Budgets",
                "Debts & Loans",
                "Wishlist"
            ]
        );
        assert_eq!(finances.subs[1].icon, "receipt");
        assert_eq!(finances.subs[1].route, "/finances/transactions");
        let annotations = active_item("/notes").expect("annotations");
        assert_eq!(annotations.link.label, "Annotations");
        assert_eq!(annotations.subs.len(), 2);
    }

    #[test]
    fn active_item_matches_a_sub_link_route() {
        assert_eq!(
            active_item("/finances/bills").map(|i| i.link.label),
            Some("Finances")
        );
        assert_eq!(
            active_item("/health/fasting").map(|i| i.link.label),
            Some("Health & Fitness")
        );
        assert!(active_item("/nope").is_none());
    }

    #[test]
    fn title_follows_the_sub_link_then_the_item() {
        assert_eq!(title_for("/finances/transactions"), "Transactions");
        assert_eq!(title_for("/todos"), "Todos");
        assert_eq!(title_for("/notebooks"), "Notebooks");
        assert_eq!(title_for("/inbox"), "Inbox");
        assert_eq!(title_for("/storage"), "Assets & Storage");
        assert_eq!(title_for("/watchlist"), "All Media");
        assert_eq!(title_for("/nope"), "Not ported yet");
    }

    #[test]
    fn subtabs_show_for_items_with_sub_links_except_watchlist() {
        let labels: Vec<_> = subtabs("/housing/pets").iter().map(|l| l.label).collect();
        assert_eq!(labels, ["Overview", "Pets", "Groceries", "Wishlist"]);
        assert!(subtabs("/todos").is_empty());
        assert!(subtabs("/watchlist/movies").is_empty());
        assert!(subtabs("/storage").is_empty());
    }

    #[test]
    fn parse_ignores_a_trailing_slash_and_rejects_unknown_routes() {
        assert_eq!(parse("/todos/"), Some("/todos"));
        assert_eq!(parse("/"), Some("/"));
        assert_eq!(parse("/finances/bills"), Some("/finances/bills"));
        assert_eq!(parse("/storage"), Some("/storage"));
        assert_eq!(parse("/finances/nope"), None);
        assert_eq!(parse("/nope"), None);
    }

    #[test]
    fn every_route_of_the_egui_client_is_in_the_tree() {
        // The routes dispatched in crates/app/src/lib.rs and tests/routes.rs.
        for route in [
            "/",
            "/todos",
            "/habits",
            "/notes",
            "/inbox",
            "/notebooks",
            "/watchlist",
            "/watchlist/movies",
            "/watchlist/series",
            "/watchlist/books",
            "/music",
            "/music/piano",
            "/music/guitar",
            "/music/dj",
            "/health",
            "/health/fasting",
            "/health/workouts",
            "/health/measurements",
            "/housing",
            "/housing/pets",
            "/housing/groceries",
            "/housing/wishlist",
            "/finances",
            "/finances/transactions",
            "/finances/accounts",
            "/finances/incomes",
            "/finances/bills",
            "/finances/budgets",
            "/finances/debts",
            "/finances/wishlist",
            "/storage",
        ] {
            assert_eq!(parse(route), Some(route), "missing {route}");
        }
    }
}
