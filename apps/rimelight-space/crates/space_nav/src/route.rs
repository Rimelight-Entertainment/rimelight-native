//! Route parsing and formatting (P0.20). A route pattern is a path whose `:name` segments capture one segment each.
//!
//! [`parse`] finds the page whose pattern matches a concrete path and returns its parameters. [`format_route`] goes
//! the other way: it fills a pattern from parameters. Parsing then formatting gives back the same path.

use crate::manifest::pages;
use crate::{Page, Template};

/// A concrete path that matched a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    pub page: &'static Page,
    /// Parameter names and values in pattern order.
    pub params: Vec<(String, String)>,
    /// The saved view id from `?view=<id>` (OQ7). Only a list route accepts it.
    pub view: Option<String>,
}

/// Splits a path or pattern into its non-empty segments. A trailing slash is ignored.
fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Reads a query string that is exactly `view=<id>`. The id is non-empty and uses only ASCII letters, digits, `-`
/// and `_`, so it round-trips unchanged. Any other query returns `None`.
fn parse_view(query: &str) -> Option<String> {
    let id = query.strip_prefix("view=")?;
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.to_string())
}

/// Finds the page for a concrete path such as `/p/grand-tale/issues`. Returns `None` for an unknown path, and for a
/// path with an empty parameter value. A path may end in `?view=<id>` only when it names a list route (OQ7).
pub fn parse(path: &str) -> Option<Matched> {
    let (path, view) = match path.split_once('?') {
        Some((path, query)) => (path, Some(parse_view(query)?)),
        None => (path, None),
    };
    let parts = segments(path);
    if parts.is_empty() {
        return None;
    }
    for page in pages() {
        let pattern = segments(page.route);
        if pattern.len() != parts.len() {
            continue;
        }
        let mut params = Vec::new();
        let mut ok = true;
        for (pat, actual) in pattern.iter().zip(parts.iter()) {
            match pat.strip_prefix(':') {
                Some(name) => params.push((name.to_string(), (*actual).to_string())),
                None if pat == actual => {}
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            if view.is_some() && page.template() != Template::List {
                continue;
            }
            return Some(Matched { page, params, view });
        }
    }
    None
}

/// Fills `pattern` with `params` (name and value pairs). A missing parameter is left as the `:name` text, so the
/// result is visibly wrong rather than silently empty.
pub fn format_route(pattern: &str, params: &[(String, String)]) -> String {
    let filled: Vec<String> = segments(pattern)
        .into_iter()
        .map(|seg| match seg.strip_prefix(':') {
            Some(name) => params
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| seg.to_string()),
            None => seg.to_string(),
        })
        .collect();
    format!("/{}", filled.join("/"))
}

/// Fills `pattern` like [`format_route`] and appends `?view=<id>` when a view is given. Parsing the result gives back
/// the same page, parameters and view.
pub fn format_view_route(pattern: &str, params: &[(String, String)], view: Option<&str>) -> String {
    let base = format_route(pattern, params);
    match view {
        Some(id) => format!("{base}?view={id}"),
        None => base,
    }
}

/// The pattern of a page filled with the given parameters, for a page the palette or sidebar opens.
pub fn fill(page: &Page, params: &[(String, String)]) -> String {
    format_route(page.route, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_static_route() {
        let m = parse("/home/todos").expect("matches");
        assert_eq!(m.page.route, "/home/todos");
        assert!(m.params.is_empty());
    }

    #[test]
    fn captures_parameters_in_order() {
        let m = parse("/p/grand-tale/issues/42").expect("matches");
        assert_eq!(m.page.route, "/p/:key/issues/:id");
        assert_eq!(
            m.params,
            vec![
                ("key".to_string(), "grand-tale".to_string()),
                ("id".to_string(), "42".to_string())
            ]
        );
    }

    #[test]
    fn formats_back_to_the_same_path() {
        let m = parse("/mail").expect("matches");
        assert_eq!(format_route(m.page.route, &m.params), "/mail");
    }

    #[test]
    fn missing_parameter_stays_visible() {
        assert_eq!(format_route("/p/:key/issues", &[]), "/p/:key/issues");
    }

    #[test]
    fn trailing_slash_is_ignored() {
        assert_eq!(parse("/finance/").expect("matches").page.route, "/finance");
    }

    #[test]
    fn wrong_length_does_not_match() {
        assert!(parse("/p/grand-tale/issues/1/extra").is_none());
    }

    #[test]
    fn list_route_takes_a_view_and_round_trips() {
        let m = parse("/p/grand-tale/issues?view=mine").expect("matches");
        assert_eq!(m.page.route, "/p/:key/issues");
        assert_eq!(m.view.as_deref(), Some("mine"));
        assert_eq!(
            format_view_route(m.page.route, &m.params, m.view.as_deref()),
            "/p/grand-tale/issues?view=mine"
        );
    }

    #[test]
    fn view_on_a_parameter_list_route_round_trips() {
        let m = parse("/p/grand-tale/issues?view=open-bugs_2").expect("matches");
        assert_eq!(m.page.route, "/p/:key/issues");
        assert_eq!(m.view.as_deref(), Some("open-bugs_2"));
        assert_eq!(
            format_view_route(m.page.route, &m.params, m.view.as_deref()),
            "/p/grand-tale/issues?view=open-bugs_2"
        );
    }

    #[test]
    fn a_path_without_a_query_has_no_view() {
        assert!(
            parse("/p/grand-tale/issues")
                .expect("matches")
                .view
                .is_none()
        );
    }

    #[test]
    fn view_on_a_non_list_route_does_not_parse() {
        assert!(parse("/home?view=x").is_none());
        assert!(parse("/p/grand-tale/issues/42?view=x").is_none());
    }

    #[test]
    fn bad_queries_do_not_parse() {
        assert!(parse("/p/grand-tale/issues?view=").is_none());
        assert!(parse("/p/grand-tale/issues?view=a/b").is_none());
        assert!(parse("/p/grand-tale/issues?view=a%20b").is_none());
        assert!(parse("/p/grand-tale/issues?sort=name").is_none());
        assert!(parse("/p/grand-tale/issues?view=a&view=b").is_none());
    }
}
