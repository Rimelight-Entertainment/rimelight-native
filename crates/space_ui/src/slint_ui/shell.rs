//! Rail and sidebar data (S9.1). Fills the models of `@rimelight_shell/components/rail.slint` and
//! `@rimelight_shell/components/sidebar.slint` from the `space_nav` manifest. The markup only draws and reports clicks; the rows and the icons come from here.
//!
//! The rail lists the areas in manifest order. The sidebar lists the sections of the active area; each section heading is
//! collapsible (SF.4, PLAN.md section 6.6 "Sidebar hierarchy rule"), and under it are its pages that have no
//! `:parameter` (a parameter page needs an id, so it is not a sidebar row, as in PLAN 6.4). A collapsed section keeps its
//! heading and hides its pages. The collapse state is kept here, per area and section, in memory.

use std::cell::RefCell;
use std::collections::HashSet;

use slint::Image;
use space_nav::{Area, Page, Status, format_route, pages_in, parse, sections_in};

use crate::slint_ui::{Icons, RailBadge, SidebarItem, SidebarKind, SidebarStatus};
use space_page_kit::projects;

/// One sidebar row of an area: the row, the route it opens (`None` for a heading) and the section it belongs to (its
/// heading's key, which the collapse state uses).
pub struct Entry {
    pub item: SidebarItem,
    /// The route the row opens, with the project key filled in on a `/p/:key` page (see [`listed_route`]).
    pub route: Option<String>,
    pub section: &'static str,
    /// True for a live child row under a project page (tail C3, IA 2.6): a record of that page. It opens the page.
    pub live: bool,
}

thread_local! {
    /// The collapsed section headings, as (area title, section id). Every heading starts expanded.
    static COLLAPSED: RefCell<HashSet<(&'static str, &'static str)>> = RefCell::new(HashSet::new());
}

/// Project pages built so far. The sidebar lists only these under a project; the rest are design placeholders.
const BUILT_PROJECT_PAGES: [&str; 6] = [
    "/p/:key",
    "/p/:key/issues",
    // The project's content: docs, wiki, blog and any other collection, as tabs (CMS.6).
    "/p/:key/docs",
    "/p/:key/canvases",
    "/p/:key/settings",
    "/p/:key/members",
];

/// Whether the sidebar lists `page`: parameter-free pages always; a project page only when a project is current and built.
fn listed(page: &Page) -> bool {
    if !page.has_params() {
        return true;
    }
    page.only_key_param()
        && !projects::current_key().is_empty()
        && BUILT_PROJECT_PAGES.contains(&page.route)
}

/// The route a sidebar row of `page` opens: its pattern, with the current project's key filled in for a `/p/:key` page
/// (IA 2.6, the Projects tree).
pub fn listed_route(page: &Page) -> String {
    if page.has_params() {
        format_route(page.route, &[("key".to_owned(), projects::current_key())])
    } else {
        page.route.to_owned()
    }
}

/// The sidebar rows of `area` in order, collapse-aware: a heading per section that has pages, then (when the heading is
/// expanded) those pages. A page is listed when it has no parameter, or its only parameter is the project key (the
/// Projects tree, tail C3). The page
/// rows carry no icon here; `sidebar_rows` fills them.
pub fn sidebar_entries(area: Area) -> Vec<Entry> {
    let collapsed = COLLAPSED.with(|set| set.borrow().clone());
    let mut rows = Vec::new();
    for section in sections_in(area) {
        let pages: Vec<&'static Page> = pages_in(area)
            .filter(|page| page.section == section.id && listed(page))
            .collect();
        if pages.is_empty() {
            continue;
        }
        let open = !collapsed.contains(&(area.title(), section.id));
        // A collapsed heading shows the sum of its pages' counts (the rolled-up count, IA 2.0).
        let rolled_up: u32 = pages.iter().map(|page| mock_count(page.route)).sum();
        rows.push(Entry {
            item: SidebarItem {
                // The one heading style of the rail: uppercase, muted, collapsible.
                label: section.title.to_uppercase().into(),
                header: true,
                icon: Image::default(),
                kind: SidebarKind::Heading,
                expanded: open,
                count: rolled_up as i32,
                ..Default::default()
            },
            route: None,
            section: section.id,
            live: false,
        });
        if !open {
            continue;
        }
        for page in pages {
            let route = listed_route(page);
            rows.push(Entry {
                item: SidebarItem {
                    label: page.title.into(),
                    header: false,
                    // The icon is set by `sidebar_rows` from the page's Lucide name.
                    icon: Image::default(),
                    kind: SidebarKind::Page,
                    count: mock_count(page.route) as i32,
                    ..Default::default()
                },
                route: Some(route.clone()),
                section: section.id,
                live: false,
            });
        }
    }
    rows
}

/// The count shown on a page row (IA 2.x "Count"): the number of a saved view or work queue. The IA lists these pages
/// with a number, but the record queries do not exist yet, so the values are fixed mock numbers. A page with no count in
/// the IA has 0 (no badge).
pub fn mock_count(route: &str) -> u32 {
    match route {
        "/people/time-off" => 3,
        "/people/timesheets" => 5,
        "/people/expenses" => 2,
        "/people/recruiting/interviews" => 1,
        "/finance/ledger/entries" => 14,
        "/finance/banking/reconcile" => 9,
        "/finance/receivables/invoices" => 4,
        "/finance/payables/requests" => 2,
        "/finance/payables/bills" => 6,
        // The project pages with a number (IA 2.6): issues views, saved boards, backlog, the active sprint, reviews, and
        // assets by pipeline step.
        _ => 0,
    }
}

/// The sidebar dot of a live record's status (OQ12): the same four states as `space_nav::Status`, with the word
/// drawn beside the dot.
pub fn sidebar_status(status: Option<Status>) -> SidebarStatus {
    match status {
        None => SidebarStatus::None,
        Some(Status::Ok) => SidebarStatus::Ok,
        Some(Status::Attention) => SidebarStatus::Attention,
        Some(Status::Failed) => SidebarStatus::Failed,
        Some(Status::Idle) => SidebarStatus::Idle,
    }
}

/// The sidebar rows of `area`, collapse-aware (see [`sidebar_entries`]). Each page row carries its Lucide icon as an
/// image (from `icon`); a heading has none.
pub fn sidebar_rows(icons: &Icons<'_>, area: Area) -> Vec<SidebarItem> {
    sidebar_rows_with(|name| icon(icons, name), area)
}

/// [`sidebar_rows`] with the icons from `icon_of` (a manifest icon name to its image). The interpreter gallery passes its
/// own `Icons` global this way.
pub fn sidebar_rows_with(icon_of: impl Fn(&str) -> Image, area: Area) -> Vec<SidebarItem> {
    sidebar_entries(area)
        .into_iter()
        .map(|entry| {
            let mut row = entry.item;
            // A live child has no icon of its own: it shows its status dot under its page.
            if !entry.live
                && let Some(page) = entry
                    .route
                    .as_deref()
                    .and_then(parse)
                    .map(|matched| matched.page)
            {
                row.icon = icon_of(page.icon());
            }
            row
        })
        .collect()
}

/// Flips the collapse state of section `section` of `area`. The caller rebuilds the rows with [`sidebar_rows`].
pub fn toggle_heading(area: Area, section: &'static str) {
    COLLAPSED.with(|set| {
        let mut set = set.borrow_mut();
        let key = (area.title(), section);
        if !set.remove(&key) {
            set.insert(key);
        }
    });
}

/// The Lucide icon of a manifest icon name, from the bundled `Icons` global. An unknown name draws nothing. Every name
/// of `crates/space_nav/ICONS.txt` is mapped (the sidebar rows), plus the rail and page icons.
pub fn icon(icons: &Icons<'_>, name: &str) -> Image {
    match name {
        "house" => icons.get_house(),
        "message-square" => icons.get_message_square(),
        "mail" => icons.get_mail(),
        "calendar" => icons.get_calendar(),
        "folder-kanban" => icons.get_folder_kanban(),
        "building-2" => icons.get_building_2(),
        "users" => icons.get_users(),
        "landmark" => icons.get_landmark(),
        "headset" => icons.get_headset(),
        "boxes" => icons.get_boxes(),
        "chart-column" => icons.get_chart_column(),
        "shield" => icons.get_shield(),
        "tag" => icons.get_tag(),
        "ellipsis" => icons.get_ellipsis(),
        "activity" => icons.get_activity(),
        "app-window" => icons.get_app_window(),
        "arrow-left-right" => icons.get_arrow_left_right(),
        "at-sign" => icons.get_at_sign(),
        "ban" => icons.get_ban(),
        "banknote" => icons.get_banknote(),
        "book-marked" => icons.get_book_marked(),
        "book-open" => icons.get_book_open(),
        "book-user" => icons.get_book_user(),
        "bookmark" => icons.get_bookmark(),
        "bot" => icons.get_bot(),
        "box" => icons.get_box(),
        "briefcase" => icons.get_briefcase(),
        "briefcase-business" => icons.get_briefcase_business(),
        "building" => icons.get_building(),
        "calendar-check" => icons.get_calendar_check(),
        "calendar-days" => icons.get_calendar_days(),
        "calendar-range" => icons.get_calendar_range(),
        "chart-pie" => icons.get_chart_pie(),
        "circle-dot" => icons.get_circle_dot(),
        "clipboard-check" => icons.get_clipboard_check(),
        "clipboard-list" => icons.get_clipboard_list(),
        "clock" => icons.get_clock(),
        "compass" => icons.get_compass(),
        "concierge-bell" => icons.get_concierge_bell(),
        "contact" => icons.get_contact(),
        "credit-card" => icons.get_credit_card(),
        "database" => icons.get_database(),
        "door-open" => icons.get_door_open(),
        "file-chart-column" => icons.get_file_chart_column(),
        "file-chart-line" => icons.get_file_chart_line(),
        "file-check" => icons.get_file_check(),
        "file-pen" => icons.get_file_pen(),
        "file-pen-line" => icons.get_file_pen_line(),
        "file-signature" => icons.get_file_signature(),
        "file-spreadsheet" => icons.get_file_spreadsheet(),
        "file-text" => icons.get_file_text(),
        "flag" => icons.get_flag(),
        "folder" => icons.get_folder(),
        "folder-open" => icons.get_folder_open(),
        "messages-square" => icons.get_messages_square(),
        "pin" => icons.get_pin(),
        "folders" => icons.get_folders(),
        "funnel" => icons.get_funnel(),
        "gamepad-2" => icons.get_gamepad_2(),
        "gantt-chart" => icons.get_gantt_chart(),
        "gauge" => icons.get_gauge(),
        "git-branch" => icons.get_git_branch(),
        "git-merge" => icons.get_git_merge(),
        "git-pull-request" => icons.get_git_pull_request(),
        "graduation-cap" => icons.get_graduation_cap(),
        "hand-coins" => icons.get_hand_coins(),
        "handshake" => icons.get_handshake(),
        "hash" => icons.get_hash(),
        "heart-handshake" => icons.get_heart_handshake(),
        "history" => icons.get_history(),
        "id-card" => icons.get_id_card(),
        "kanban" => icons.get_kanban(),
        "key-round" => icons.get_key_round(),
        "languages" => icons.get_languages(),
        "laptop" => icons.get_laptop(),
        "layers" => icons.get_layers(),
        "layout-dashboard" => icons.get_layout_dashboard(),
        "layout-panel-top" => icons.get_layout_panel_top(),
        "list" => icons.get_list(),
        "list-checks" => icons.get_list_checks(),
        "list-todo" => icons.get_list_todo(),
        "lock" => icons.get_lock(),
        "map" => icons.get_map(),
        "map-pin" => icons.get_map_pin(),
        "megaphone" => icons.get_megaphone(),
        "message-circle" => icons.get_message_circle(),
        "message-square-reply" => icons.get_message_square_reply(),
        "network" => icons.get_network(),
        "notebook" => icons.get_notebook(),
        "notebook-pen" => icons.get_notebook_pen(),
        "package" => icons.get_package(),
        "package-check" => icons.get_package_check(),
        "palm-tree" => icons.get_palm_tree(),
        "pen-tool" => icons.get_pen_tool(),
        "percent" => icons.get_percent(),
        "phone-call" => icons.get_phone_call(),
        "pie-chart" => icons.get_pie_chart(),
        "plane" => icons.get_plane(),
        "play" => icons.get_play(),
        "plug" => icons.get_plug(),
        "plug-zap" => icons.get_plug_zap(),
        "presentation" => icons.get_presentation(),
        "receipt" => icons.get_receipt(),
        "receipt-text" => icons.get_receipt_text(),
        "repeat" => icons.get_repeat(),
        "rocket" => icons.get_rocket(),
        "scale" => icons.get_scale(),
        "school" => icons.get_school(),
        "scroll-text" => icons.get_scroll_text(),
        "send" => icons.get_send(),
        "settings" => icons.get_settings(),
        "calendar-plus" => icons.get_calendar_plus(),
        "square-check" => icons.get_square_check(),
        "shield-check" => icons.get_shield_check(),
        "shopping-cart" => icons.get_shopping_cart(),
        "smartphone" => icons.get_smartphone(),
        "stamp" => icons.get_stamp(),
        "table-properties" => icons.get_table_properties(),
        "target" => icons.get_target(),
        "ticket" => icons.get_ticket(),
        "timer" => icons.get_timer(),
        "truck" => icons.get_truck(),
        "user-check" => icons.get_user_check(),
        "user-cog" => icons.get_user_cog(),
        "user-minus" => icons.get_user_minus(),
        "user-plus" => icons.get_user_plus(),
        "user-round" => icons.get_user_round(),
        "user-search" => icons.get_user_search(),
        "users-round" => icons.get_users_round(),
        "video" => icons.get_video(),
        "wallet" => icons.get_wallet(),
        "warehouse" => icons.get_warehouse(),
        "webhook" => icons.get_webhook(),
        "workflow" => icons.get_workflow(),
        "zap" => icons.get_zap(),
        _ => Image::default(),
    }
}

/// The word the rail's accessible label adds after a badge count (OQ3), or `None` when the area has no work queue.
/// Only the five queues show a badge: Chat, People, Finance, Customers and Operations.
pub fn queue_noun(area: Area) -> Option<&'static str> {
    match area {
        Area::Chat => Some("unread"),
        Area::People | Area::Operations => Some("waiting"),
        Area::Finance => Some("due"),
        Area::Customers => Some("unassigned"),
        _ => None,
    }
}

/// The rail badges for per-area counts, in `Area::ALL` order (a missing count is 0). A count for an area without a work
/// queue is ignored: its badge has count 0 and so is not shown. Counts above 99 show as "99+".
pub fn rail_badges(counts: &[u32]) -> Vec<RailBadge> {
    Area::ALL
        .iter()
        .enumerate()
        .map(|(index, area)| match queue_noun(*area) {
            Some(noun) => {
                let count = counts.get(index).copied().unwrap_or(0);
                RailBadge {
                    count: count as i32,
                    text: rimelight_shell::badge::badge_text(count).into(),
                    noun: noun.into(),
                }
            }
            None => RailBadge {
                count: 0,
                text: "".into(),
                noun: "".into(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_has_an_icon_name_the_markup_maps() {
        // Each manifest icon must be one of the names `icon` maps. Keep this list equal to the match above.
        let mapped = [
            "house",
            "message-square",
            "mail",
            "calendar",
            "folder-kanban",
            "building-2",
            "users",
            "landmark",
            "headset",
            "boxes",
            "chart-column",
            "shield",
        ];
        for area in Area::ALL {
            assert!(mapped.contains(&area.icon()), "{:?}", area);
        }
    }

    #[test]
    fn sidebar_rows_skip_parameter_pages_and_empty_sections() {
        let entries = sidebar_entries(Area::Projects);
        assert!(
            entries.first().is_some_and(|e| e.item.header),
            "starts with a heading"
        );
        let pages = entries.iter().filter(|e| !e.item.header && !e.live).count();
        let expected = pages_in(Area::Projects).filter(|p| listed(p)).count();
        assert_eq!(
            pages, expected,
            "every parameter-free page, and the built project pages when a project is current, is a row"
        );
    }

    #[test]
    fn sidebar_entries_route_each_page_row_and_no_heading() {
        for area in Area::ALL {
            for entry in sidebar_entries(area) {
                assert_eq!(
                    entry.item.header,
                    entry.route.is_none(),
                    "{:?}",
                    entry.item.label
                );
                if let Some(route) = entry.route.as_deref() {
                    assert!(space_nav::parse(route).is_some(), "{route} parses");
                }
            }
        }
    }

    #[test]
    fn status_dots_follow_the_four_nav_states() {
        assert_eq!(sidebar_status(None), SidebarStatus::None);
        assert_eq!(sidebar_status(Some(Status::Ok)), SidebarStatus::Ok);
        assert_eq!(
            sidebar_status(Some(Status::Attention)),
            SidebarStatus::Attention
        );
        assert_eq!(sidebar_status(Some(Status::Failed)), SidebarStatus::Failed);
        assert_eq!(sidebar_status(Some(Status::Idle)), SidebarStatus::Idle);
    }

    #[test]
    fn mock_counts_are_on_parameter_free_pages_only() {
        // Every route with a mock count is a sidebar row (no parameter, or only the project key), so the count is visible.
        for area in Area::ALL {
            for page in pages_in(area) {
                if mock_count(page.route) > 0 {
                    assert!(
                        !page.has_params() || page.only_key_param(),
                        "{} has a parameter",
                        page.route
                    );
                }
            }
        }
        let counted = space_nav::pages()
            .iter()
            .filter(|page| mock_count(page.route) > 0)
            .count();
        assert_eq!(
            counted, 9,
            "the nine IA pages with a number (the project pages have no count yet)"
        );
    }

    #[test]
    fn a_heading_rolls_up_the_counts_of_its_pages() {
        for area in Area::ALL {
            // (sum of the page rows under the heading, the heading's count, whether the heading is expanded).
            let mut open_heading: Option<(i32, i32)> = None;
            let check = |heading: Option<(i32, i32)>| {
                if let Some((sum, rolled)) = heading {
                    assert_eq!(sum, rolled, "{area:?} heading roll-up");
                }
            };
            for entry in sidebar_entries(area) {
                if entry.item.header {
                    check(open_heading.take());
                    // A collapsed heading has no rows under it, so only an expanded one is summed here.
                    open_heading = entry.item.expanded.then_some((0, entry.item.count));
                } else if let Some((sum, _)) = open_heading.as_mut() {
                    *sum += entry.item.count;
                }
            }
            check(open_heading.take());
        }
    }
}
