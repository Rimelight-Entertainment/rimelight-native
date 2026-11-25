//! Routes of the content area (CMS.6). Four scope homes and their page routes, plus the admin and review pages.
//!
//! - `/p/:key/docs`, `/company/knowledge`, `/company/teams/:id/docs`, `/company/departments/:id/docs` (the scope home)
//! - the same with `/:page` (one page; `?mode=edit` opens it in edit mode)
//! - `/admin/content/collections`, `/admin/content/media`, `/home/reviews`

use crate::scope::Scope;

/// The collections admin page.
pub const COLLECTIONS_ROUTE: &str = "/admin/content/collections";
/// The media library page.
pub const MEDIA_ROUTE: &str = "/admin/content/media";
/// The reviews page (every pending version the caller can review).
pub const REVIEWS_ROUTE: &str = "/home/reviews";

/// A parsed content route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    ScopeHome(Scope),
    Page {
        scope: Scope,
        page: String,
        edit: bool,
    },
    Collections,
    Media,
    Reviews,
}

/// The scope home's route.
pub fn home_route(scope: &Scope) -> String {
    match scope {
        Scope::Company => "/company/knowledge".to_string(),
        Scope::Project(key) => format!("/p/{key}/docs"),
        Scope::Team(id) => format!("/company/teams/{id}/docs"),
        Scope::Department(id) => format!("/company/departments/{id}/docs"),
    }
}

/// One page's route. `edit` adds `?mode=edit`.
pub fn page_route(scope: &Scope, page: &str, edit: bool) -> String {
    let base = format!("{}/{page}", home_route(scope));
    if edit {
        format!("{base}?mode=edit")
    } else {
        base
    }
}

/// The full public address of a page: the API base the client uses, then the page's route. A trailing slash on the base is
/// dropped, so the address never has `//` at the join.
pub fn public_address(api_base: &str, route: &str) -> String {
    format!("{}{route}", api_base.trim_end_matches('/'))
}

/// Parses a path (a query string is allowed on pages). `None` for anything else, including empty segments.
pub fn parse(path: &str) -> Option<Route> {
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path, ""),
    };
    let edit = query.split('&').any(|kv| kv == "mode=edit");
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return None;
    }
    match segments.as_slice() {
        ["admin", "content", "collections"] => Some(Route::Collections),
        ["admin", "content", "media"] => Some(Route::Media),
        ["home", "reviews"] => Some(Route::Reviews),
        ["company", "knowledge"] => Some(Route::ScopeHome(Scope::Company)),
        ["company", "knowledge", page] => Some(Route::Page {
            scope: Scope::Company,
            page: page.to_string(),
            edit,
        }),
        ["p", key, "docs"] => Some(Route::ScopeHome(Scope::Project(key.to_string()))),
        ["p", key, "docs", page] => Some(Route::Page {
            scope: Scope::Project(key.to_string()),
            page: page.to_string(),
            edit,
        }),
        ["company", "teams", id, "docs"] => Some(Route::ScopeHome(Scope::Team(id.to_string()))),
        ["company", "teams", id, "docs", page] => Some(Route::Page {
            scope: Scope::Team(id.to_string()),
            page: page.to_string(),
            edit,
        }),
        ["company", "departments", id, "docs"] => {
            Some(Route::ScopeHome(Scope::Department(id.to_string())))
        }
        ["company", "departments", id, "docs", page] => Some(Route::Page {
            scope: Scope::Department(id.to_string()),
            page: page.to_string(),
            edit,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_address_joins_the_base_and_the_route() {
        let route = "/company/knowledge/handbook";
        assert_eq!(
            public_address("https://api.example.test", route),
            "https://api.example.test/company/knowledge/handbook"
        );
        assert_eq!(
            public_address("https://api.example.test/", route),
            "https://api.example.test/company/knowledge/handbook"
        );
    }

    #[test]
    fn scope_homes_parse() {
        assert_eq!(
            parse("/p/ATL/docs"),
            Some(Route::ScopeHome(Scope::Project("ATL".into())))
        );
        assert_eq!(
            parse("/company/knowledge"),
            Some(Route::ScopeHome(Scope::Company))
        );
        assert_eq!(
            parse("/company/teams/t1/docs"),
            Some(Route::ScopeHome(Scope::Team("t1".into())))
        );
        assert_eq!(
            parse("/company/departments/d2/docs"),
            Some(Route::ScopeHome(Scope::Department("d2".into())))
        );
    }

    #[test]
    fn pages_parse_with_edit_mode() {
        assert_eq!(
            parse("/company/knowledge/pg1"),
            Some(Route::Page {
                scope: Scope::Company,
                page: "pg1".into(),
                edit: false
            })
        );
        assert_eq!(
            parse("/p/ATL/docs/pg2?mode=edit"),
            Some(Route::Page {
                scope: Scope::Project("ATL".into()),
                page: "pg2".into(),
                edit: true
            })
        );
    }

    #[test]
    fn admin_and_review_routes_parse() {
        assert_eq!(parse(COLLECTIONS_ROUTE), Some(Route::Collections));
        assert_eq!(parse(MEDIA_ROUTE), Some(Route::Media));
        assert_eq!(parse(REVIEWS_ROUTE), Some(Route::Reviews));
    }

    #[test]
    fn bad_paths_are_refused() {
        assert_eq!(parse("/p//docs"), None);
        assert_eq!(parse("/p/ATL/docs/a/b"), None);
        assert_eq!(parse("/company/teams/t1"), None);
        assert_eq!(parse("/"), None);
    }

    #[test]
    fn builders_round_trip_through_parse() {
        for scope in [
            Scope::Company,
            Scope::Project("ATL".into()),
            Scope::Team("t1".into()),
            Scope::Department("d1".into()),
        ] {
            assert_eq!(
                parse(&home_route(&scope)),
                Some(Route::ScopeHome(scope.clone()))
            );
            assert_eq!(
                parse(&page_route(&scope, "x1", true)),
                Some(Route::Page {
                    scope: scope.clone(),
                    page: "x1".into(),
                    edit: true
                })
            );
        }
    }
}
