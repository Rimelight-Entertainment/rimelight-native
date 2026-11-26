//! Route rules for the back and forward history of one page tab (planned feature A, `PROJECT.md section 6.6`). The
//! [`History`] type is in `rimelight_shell::history`. This module keeps the rules that read `space_nav` (the location
//! and deep-link checks, the Back key) and the Home route the Space app opens on.

use space_nav::parse;

pub use rimelight_shell::history::History;

/// The route a location opens when no page matches it, and the route the phone and desktop start on.
pub const HOME_ROUTE: &str = "/home";

/// The route a browser location (`location.pathname` with its `?view=` query) opens. A location that no page matches
/// opens Home. Pure, so the web start route and the popstate landing use the same rule.
pub fn route_from_location(location: &str) -> String {
    if parse(location).is_some() {
        location.to_owned()
    } else {
        HOME_ROUTE.to_owned()
    }
}

/// The deep link a browser location is, when it names a known page (planned feature A3 and B3). `None` for the bare root
/// or an unknown path: the start then keeps the saved workspaces as they are.
pub fn deep_link(location: &str) -> Option<String> {
    parse(location).map(|_| location.to_owned())
}

/// What the Android system Back key does (planned feature A3): a step back in the active page tab, or nothing, which hands
/// the key to the system so the activity finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackKey {
    Step,
    System,
}

/// The Back key decision for a tab whose history can go back (`can_back`, [`History::can_back`]).
pub fn back_key(can_back: bool) -> BackKey {
    if can_back {
        BackKey::Step
    } else {
        BackKey::System
    }
}

/// The browser location for `route`. A route is already a path with an optional `?view=` query, so the location is the
/// route itself. `None` when `route` is not a known page, so nothing is written to the address bar for it.
pub fn location_from_route(route: &str) -> Option<String> {
    parse(route).map(|_| route.to_owned())
}

/// The web address of the app. Copy link writes `APP_BASE_URL` followed by a page's route (`PROJECT.md section 6.6`).
pub const APP_BASE_URL: &str = "https://space.rimelight.com";

/// The web link of a page route: `APP_BASE_URL` followed by the route, with its `?view=<id>` query when it has one. A route
/// is already a path that starts with `/`, so the link is the base and the route joined as they are. Pure.
pub fn link_for(route: &str) -> String {
    format!("{APP_BASE_URL}{route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_key_steps_back_only_while_there_is_history() {
        let mut h = History::new("/home");
        assert_eq!(
            back_key(h.can_back()),
            BackKey::System,
            "no history: the system handles Back"
        );
        h.push("/home/todos");
        assert_eq!(back_key(h.can_back()), BackKey::Step);
        assert_eq!(h.back(), Some("/home"));
        assert_eq!(
            back_key(h.can_back()),
            BackKey::System,
            "at the first page: the system handles Back"
        );
    }

    #[test]
    fn route_from_location_keeps_known_pages_and_their_view_query() {
        assert_eq!(route_from_location("/home/todos"), "/home/todos");
        assert_eq!(
            route_from_location("/home/todos?view=mine"),
            "/home/todos?view=mine"
        );
    }

    #[test]
    fn route_from_location_falls_back_to_home_for_unknown_locations() {
        for location in [
            "",
            "/",
            "/no-such-page",
            "/home/todos?bad",
            "/home/todos?view=",
        ] {
            assert_eq!(route_from_location(location), HOME_ROUTE, "{location:?}");
        }
    }

    #[test]
    fn location_from_route_is_the_route_for_known_pages_only() {
        assert_eq!(
            location_from_route("/home/todos?view=mine").as_deref(),
            Some("/home/todos?view=mine")
        );
        assert_eq!(location_from_route("/no-such-page"), None);
    }

    #[test]
    fn link_for_joins_the_base_and_the_route() {
        assert_eq!(APP_BASE_URL, "https://space.rimelight.com");
        assert_eq!(link_for("/home"), "https://space.rimelight.com/home");
        assert_eq!(
            link_for("/home/todos?view=mine"),
            "https://space.rimelight.com/home/todos?view=mine"
        );
        assert_eq!(link_for("/"), "https://space.rimelight.com/");
        assert_eq!(
            link_for("/p/ABC/docs/intro"),
            "https://space.rimelight.com/p/ABC/docs/intro"
        );
    }
}
