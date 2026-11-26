//! Navigation manifest (PLAN 6.1, 6.3, 6.4; P0.20). Data only: areas, sidebar sections, pages with their routes,
//! templates, icons and owning phases, and the route parser and formatter. No UI dependency, so the backend and
//! the tests use the same data (PLAN 7.2).
//!
//! The data lives in [`manifest`]. Every route in PLAN 6.4 is one [`Page`]. `:name` segments are parameters.
//!
//! `PROJECT.md section 6.5` is generated from [`ia_doc`] by `cargo xtask docs ia`. Titles are plain English strings with no i18n
//! keys (TODO: P0.22).

pub mod ia_doc;
pub mod manifest;
pub mod quick;
pub mod route;

use std::fmt;

pub use manifest::{pages, sections};
pub use route::{Matched, format_route, format_view_route, parse};

/// The ten areas of the rail (PLAN 5, 6.1). Mail and Calendar sit under Home (owner direction 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    Home,
    Chat,
    Projects,
    Company,
    People,
    Finance,
    Customers,
    Operations,
    Analytics,
    Admin,
}

impl Area {
    /// The areas in rail order.
    pub const ALL: [Area; 10] = [
        Area::Home,
        Area::Chat,
        Area::Projects,
        Area::Company,
        Area::People,
        Area::Finance,
        Area::Customers,
        Area::Operations,
        Area::Analytics,
        Area::Admin,
    ];

    /// The name shown in the rail tooltip and the sidebar header. TODO(P0.22): move to i18n keys.
    pub fn title(self) -> &'static str {
        match self {
            Area::Home => "Home",
            Area::Chat => "Chat",
            Area::Projects => "Projects",
            Area::Company => "Company",
            Area::People => "People",
            Area::Finance => "Finance",
            Area::Customers => "Customers",
            Area::Operations => "Operations",
            Area::Analytics => "Analytics",
            Area::Admin => "Admin",
        }
    }

    /// The first route segment of the area's pages. Projects also owns `/p/:key`, whose first segment is `p`.
    pub fn root(self) -> &'static str {
        match self {
            Area::Home => "home",
            Area::Chat => "chat",
            Area::Projects => "projects",
            Area::Company => "company",
            Area::People => "people",
            Area::Finance => "finance",
            Area::Customers => "customers",
            Area::Operations => "ops",
            Area::Analytics => "analytics",
            Area::Admin => "admin",
        }
    }

    /// The Lucide icon name for the rail (matches `space_ui::IconName::lucide`).
    pub fn icon(self) -> &'static str {
        match self {
            Area::Home => "house",
            Area::Chat => "message-square",
            Area::Projects => "folder-kanban",
            Area::Company => "building-2",
            Area::People => "users",
            Area::Finance => "landmark",
            Area::Customers => "headset",
            Area::Operations => "boxes",
            Area::Analytics => "chart-column",
            Area::Admin => "shield",
        }
    }
}

/// A group of pages in the contextual sidebar of one area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// Stable id, unique across the manifest.
    pub id: &'static str,
    pub area: Area,
    pub title: &'static str,
}

/// The templates of PLAN 6.3. Each page names one or two of them (the first is the primary template).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Template {
    Dash,
    List,
    Split,
    Board,
    Timeline,
    Cal,
    Chat,
    Doc,
    Canvas,
    Detail,
    Tree,
    Review,
    Grid,
    Form,
    Call,
}

impl Template {
    /// The PLAN 6.3 description of the template, used as the placeholder's one-line description.
    pub fn summary(self) -> &'static str {
        match self {
            Template::Dash => "Grid of stat cards, charts and short lists",
            Template::List => "Toolbar, filter bar, virtual table, bulk actions",
            Template::Split => "List on the left, detail on the right",
            Template::Board => "Columns of cards with drag and drop",
            Template::Timeline => "Rows with bars on a time axis",
            Template::Cal => "Day, week, month and agenda views",
            Template::Chat => "Message list, composer, thread side panel",
            Template::Doc => "Page tree and block document",
            Template::Canvas => "Infinite canvas with toolbar",
            Template::Detail => "Record header, tab strip, sections, activity feed",
            Template::Tree => "Hierarchy diagram",
            Template::Review => "File tree and diff view with inline comments",
            Template::Grid => "Week grid of editable cells",
            Template::Form => "Sectioned settings form",
            Template::Call => "Video tiles and call controls",
        }
    }

    /// The Lucide icon name for the template. Every page gets its icon from its primary template.
    pub fn icon(self) -> &'static str {
        match self {
            Template::Dash => "layout-dashboard",
            Template::List => "list",
            Template::Split => "columns-2",
            Template::Board => "kanban",
            Template::Timeline => "ruler",
            Template::Cal => "calendar-days",
            Template::Chat => "message-square",
            Template::Doc => "file-text",
            Template::Canvas => "pen-tool",
            Template::Detail => "panel-right",
            Template::Tree => "network",
            Template::Review => "git-pull-request",
            Template::Grid => "table",
            Template::Form => "settings",
            Template::Call => "video",
        }
    }
}

/// One addressable page: one route of PLAN 6.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Route pattern, for example `/p/:key/issues`.
    pub route: &'static str,
    /// Plain English title. TODO(P0.22): i18n key.
    pub title: &'static str,
    pub area: Area,
    pub section: &'static str,
    /// Primary template first.
    pub templates: &'static [Template],
    /// The PLAN 12 phase that builds the page for real (1 to 13).
    pub phase: u8,
    /// The Lucide icon name of the page's sidebar row. Set per row in the manifest, not taken from the template.
    pub icon: &'static str,
    /// The id of the page's primary action (OQ21), for example a create dialog. `None` for most pages. The dialog UI
    /// is not built yet, so every row is `None` for now.
    pub primary_action: Option<&'static str>,
    /// Command palette search aliases (OQ6): other words that find the page, for example "Builds" and "Jobs" for
    /// Pipelines. Empty for most pages. The palette matches them like the title.
    pub aliases: &'static [&'static str],
}

impl Page {
    /// The primary template.
    pub fn template(&self) -> Template {
        self.templates[0]
    }

    /// The Lucide icon name of the sidebar row (the `icon` field), for the UI to draw.
    pub fn icon(&self) -> &'static str {
        self.icon
    }

    /// The one-line description used by the placeholder: the template's PLAN 6.3 description.
    pub fn description(&self) -> &'static str {
        self.template().summary()
    }

    /// Stable id derived from the route pattern: `/p/:key/issues` becomes `p.key.issues`.
    pub fn key(&self) -> String {
        self.route
            .trim_matches('/')
            .split('/')
            .map(|seg| seg.trim_start_matches(':'))
            .collect::<Vec<_>>()
            .join(".")
    }

    /// True when the route has `:params`. Such pages are not sidebar rows.
    pub fn has_params(&self) -> bool {
        self.route.split('/').any(|seg| seg.starts_with(':'))
    }

    /// True when the route's only parameter is `:key`, the project key (`/p/:key/...`, IA 2.6). Such a page is listed for
    /// the current project, with its key filled in. A page with any other parameter needs an id, so it is not listed.
    pub fn only_key_param(&self) -> bool {
        self.has_params()
            && self
                .route
                .split('/')
                .filter_map(|seg| seg.strip_prefix(':'))
                .all(|name| name == "key")
    }

    /// True for the HR-only pages (OQ34): People > HR records. The manifest has no role field, so this route match is
    /// the only marker. The UI must hide these pages from users who lack an HR role.
    pub fn hr_only(&self) -> bool {
        matches!(
            self.route,
            "/people/employee-records" | "/people/employee-records/:employee"
        )
    }

    /// The section record for this page.
    pub fn section_info(&self) -> Option<Section> {
        sections().iter().copied().find(|s| s.id == self.section)
    }
}

impl fmt::Display for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.title, self.route)
    }
}

/// The status dot of a live record (OQ12). Each state has a word label, so colour is never the only cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Ok,
    Attention,
    Failed,
    Idle,
}

impl Status {
    /// The four states in display order.
    pub const ALL: [Status; 4] = [Status::Ok, Status::Attention, Status::Failed, Status::Idle];

    /// The word shown with the dot.
    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Attention => "Attention",
            Status::Failed => "Failed",
            Status::Idle => "Idle",
        }
    }
}

/// The pages of one area, in manifest order.
pub fn pages_in(area: Area) -> impl Iterator<Item = &'static Page> {
    pages().iter().filter(move |p| p.area == area)
}

/// The sections of one area, in manifest order.
pub fn sections_in(area: Area) -> impl Iterator<Item = &'static Section> {
    sections().iter().filter(move |s| s.area == area)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Parameter names used in the manifest, with a sample value each.
    const SAMPLES: [(&str, &str); 9] = [
        ("key", "grand-tale"),
        ("id", "1"),
        ("user", "1"),
        ("team", "1"),
        ("course", "1"),
        ("page", "home"),
        ("location", "1"),
        ("employee", "1"),
        ("dept", "1"),
    ];

    /// The route with every parameter filled from [`SAMPLES`], and the (name, value) pairs used.
    fn instantiate(route: &str) -> (String, Vec<(String, String)>) {
        let mut params = Vec::new();
        let concrete: Vec<String> = route
            .split('/')
            .map(|seg| match seg.strip_prefix(':') {
                Some(name) => {
                    let (_, value) = SAMPLES
                        .iter()
                        .find(|(n, _)| *n == name)
                        .unwrap_or_else(|| panic!("no sample value for :{name}"));
                    params.push((name.to_string(), value.to_string()));
                    value.to_string()
                }
                None => seg.to_string(),
            })
            .collect();
        (concrete.join("/"), params)
    }

    fn find(route: &str) -> &'static Page {
        pages()
            .iter()
            .find(|p| p.route == route)
            .expect("page exists")
    }

    #[test]
    fn every_route_is_unique() {
        let mut seen = HashSet::new();
        for page in pages() {
            assert!(seen.insert(page.route), "duplicate route {}", page.route);
        }
    }

    #[test]
    fn every_page_parses_and_round_trips() {
        for page in pages() {
            let (path, params) = instantiate(page.route);
            let matched = parse(&path)
                .unwrap_or_else(|| panic!("{path} does not parse (page {})", page.route));
            assert!(
                std::ptr::eq(matched.page, find(page.route)),
                "{path} matched {} instead of {}",
                matched.page.route,
                page.route
            );
            assert_eq!(matched.params, params, "params of {path}");
            assert_eq!(
                format_route(page.route, &matched.params),
                path,
                "round trip of {path}"
            );
        }
    }

    #[test]
    fn unknown_paths_do_not_parse() {
        assert!(parse("/nope").is_none());
        assert!(parse("/home/nope/nope/nope").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn every_page_has_area_template_icon_and_phase() {
        for page in pages() {
            assert!(!page.title.is_empty(), "{} has no title", page.route);
            assert!(!page.templates.is_empty(), "{} has no template", page.route);
            assert!(!page.icon().is_empty(), "{} has no icon", page.route);
            assert!(
                (1..=13).contains(&page.phase),
                "{} phase {} out of range",
                page.route,
                page.phase
            );
            let first = page
                .route
                .trim_matches('/')
                .split('/')
                .next()
                .unwrap_or_default();
            let expected = if page.area == Area::Projects && first == "p" {
                "p"
            } else {
                page.area.root()
            };
            // Home also owns the Mail, Calendar and Payslip routes, which keep their existing paths (owner direction 5).
            let home_owned =
                page.area == Area::Home && ["mail", "calendar", "people"].contains(&first);
            assert!(
                first == expected || home_owned,
                "{} is not under its area {:?}",
                page.route,
                page.area
            );
        }
    }

    #[test]
    fn every_sidebar_row_has_its_own_icon() {
        let rows: Vec<&Page> = pages().iter().filter(|p| !p.has_params()).collect();
        assert!(
            rows.len() >= 100,
            "expected the sidebar rows, got {}",
            rows.len()
        );
        for page in &rows {
            assert!(!page.icon().is_empty(), "{} has no icon", page.route);
        }
        for section in sections() {
            let mut seen = HashSet::new();
            for page in rows.iter().filter(|p| p.section == section.id) {
                assert!(
                    seen.insert(page.icon()),
                    "{} shares icon {} with another row in section {}",
                    page.route,
                    page.icon(),
                    section.id
                );
            }
        }
    }

    #[test]
    fn every_page_has_a_known_section_of_its_area() {
        for page in pages() {
            let section = page
                .section_info()
                .unwrap_or_else(|| panic!("{} names unknown section {}", page.route, page.section));
            assert_eq!(section.area, page.area, "{} section area", page.route);
        }
    }

    #[test]
    fn section_ids_are_unique() {
        let mut seen = HashSet::new();
        for section in sections() {
            assert!(seen.insert(section.id), "duplicate section {}", section.id);
        }
    }

    #[test]
    fn all_ten_areas_are_present() {
        assert_eq!(Area::ALL.len(), 10);
        for area in Area::ALL {
            assert!(pages_in(area).count() > 0, "{area:?} has no pages");
            assert!(sections_in(area).count() > 0, "{area:?} has no sections");
        }
    }

    #[test]
    fn page_keys_are_unique() {
        let mut seen = HashSet::new();
        for page in pages() {
            assert!(seen.insert(page.key()), "duplicate key {}", page.key());
        }
    }

    #[test]
    fn status_has_four_states_with_distinct_word_labels() {
        assert_eq!(Status::ALL.len(), 4);
        let labels: HashSet<&str> = Status::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels.len(), 4, "labels must be distinct");
        assert!(labels.iter().all(|l| !l.is_empty()));
        assert_eq!(Status::Ok.label(), "OK");
        assert_eq!(Status::Attention.label(), "Attention");
        assert_eq!(Status::Failed.label(), "Failed");
        assert_eq!(Status::Idle.label(), "Idle");
    }

    #[test]
    fn primary_action_defaults_to_none_until_dialogs_exist() {
        // SF.3: the field exists, but no row names an action yet because the create dialog is not built.
        assert!(pages().iter().all(|p| p.primary_action.is_none()));
    }

    #[test]
    fn pipelines_has_the_palette_aliases_builds_and_jobs() {
        // OQ6: the label is Pipelines and the route is unchanged; "Builds" and "Jobs" are search aliases.
        let pipelines = find("/p/:key/code/builds");
        assert_eq!(pipelines.title, "Pipelines");
        assert_eq!(pipelines.aliases, &["Builds", "Jobs"]);
        let with_aliases: Vec<&str> = pages()
            .iter()
            .filter(|p| !p.aliases.is_empty())
            .map(|p| p.route)
            .collect();
        assert_eq!(with_aliases, ["/p/:key/code/builds"]);
    }

    #[test]
    fn no_route_is_a_create_route() {
        // OQ16: create actions open a dialog, so no route names one.
        const CREATE_WORDS: [&str; 8] = [
            "new", "add", "create", "upload", "invite", "import", "connect", "link",
        ];
        for page in pages() {
            for seg in page.route.split('/') {
                assert!(
                    !CREATE_WORDS.contains(&seg),
                    "{} is a create route (segment {seg})",
                    page.route
                );
            }
        }
    }

    #[test]
    fn hr_only_pages_are_the_employee_records_and_nothing_else() {
        let hr: Vec<&str> = pages()
            .iter()
            .filter(|p| p.hr_only())
            .map(|p| p.route)
            .collect();
        assert_eq!(
            hr,
            [
                "/people/employee-records",
                "/people/employee-records/:employee"
            ]
        );
        for page in pages().iter().filter(|p| p.hr_only()) {
            assert_eq!(page.section, "people.records", "{}", page.route);
        }
    }

    #[test]
    fn employee_records_have_their_own_icon_in_people() {
        let company = find("/company/employees").icon();
        let records = find("/people/employee-records").icon();
        assert_ne!(
            company, records,
            "HR records must not reuse Company > Employees' icon"
        );
    }

    #[test]
    fn print_counts() {
        let total = pages().len();
        println!(
            "manifest: {total} pages in {} sections, 10 areas",
            sections().len()
        );
        for area in Area::ALL {
            println!("  {:<11} {:>3} pages", area.title(), pages_in(area).count());
        }
        assert!(total >= 150, "expected the full PLAN 6.4 list, got {total}");
    }
}
