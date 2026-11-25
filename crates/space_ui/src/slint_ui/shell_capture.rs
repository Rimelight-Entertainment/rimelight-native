//! Whole-shell captures for visual review (review captures, not kept). The real `AppShell` is built with
//! [`app_shell::build`] on the headless testing backend (software renderer, no OS window). The window is then driven
//! into one named state with the same callbacks and clicks the UI tests use, and the frame is written as a PNG.
//!
//! Compiled only with the `headless` feature, so no production binary carries it. `space_gallery` (apps/gallery) calls it
//! for `--story shell/<state>` together with `--screenshot`; `cargo xtask screenshot shell/<state>` runs that.

use std::collections::HashSet;
use std::path::Path;

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle as _, LogicalSize, Model as _, ModelRc, SharedString, VecModel};
use space_nav::{Area, pages_in};

use crate::slint_ui::app_shell::{self, App};
use crate::slint_ui::palette::DEFAULT_PROJECT_KEY;
use crate::slint_ui::shell::{rail_badges, sidebar_entries};
use crate::slint_ui::{AppShell, ConnectionState, JobItem, TileGroup};
use crate::theme::ThemeMode;
use space_page_kit::projects;

/// The start page of the desktop and phone states.
const HOME: &str = "/home";

/// Every state name `shell/<name>` accepts, in the order the owner review lists them.
pub const STATES: &[&str] = &[
    "desktop-default",
    "desktop-offline",
    "desktop-badges-timer-jobs",
    "desktop-quick-action",
    "desktop-shortcuts",
    "desktop-tab-context-menu",
    "desktop-workspace-menu",
    "desktop-strip-focus",
    "desktop-projects-switcher",
    "desktop-empty",
    "desktop-profile-menu",
    "desktop-language-menu",
    "desktop-zoom-50",
    "desktop-zoom-150",
    "phone-default",
    "phone-more-drawer",
    "phone-workspace-sheet",
    "phone-page-back",
    "desktop-content-home",
    "desktop-content-new-page",
    "desktop-content-page",
];

/// A frame with fewer distinct colours than this is refused as blank (the same rule as the shared gallery's capture in rimelight-rs).
const MIN_DISTINCT_COLOURS: usize = 8;

/// Builds the real shell, drives it into `state`, and writes the frame at `size` to `out`. Returns a one-line summary
/// (the groups and their tab counts, so the layout can be checked from the log).
pub fn capture(
    state: &str,
    theme: ThemeMode,
    size: (u32, u32),
    out: &Path,
) -> Result<String, String> {
    if !STATES.contains(&state) {
        return Err(format!(
            "unknown shell state {state:?}; known: {}",
            STATES.join(", ")
        ));
    }
    let start = match state {
        "desktop-projects-switcher" => format!("/p/{DEFAULT_PROJECT_KEY}/issues"),
        "desktop-content-home" | "desktop-content-new-page" => "/company/knowledge".to_string(),
        "desktop-content-page" => "/company/knowledge/getting-started".to_string(),
        _ => HOME.to_string(),
    };
    if state == "desktop-projects-switcher" {
        projects::set_list(projects::fixture());
        projects::set_current(DEFAULT_PROJECT_KEY);
    }
    let app =
        app_shell::build(&start, theme).map_err(|e| format!("could not build the shell: {e}"))?;
    let ui = &app.ui;
    ui.show().map_err(|e| format!("show failed: {e}"))?;
    resize(ui, size);

    let routes = home_routes();
    if routes.len() < 6 {
        return Err("Home has too few pages to build the layout".to_string());
    }

    match state {
        "desktop-default" | "desktop-tab-context-menu" => {
            three_workspaces(&app);
            three_groups(&app, &routes);
            if state == "desktop-tab-context-menu" {
                right_click_tab(ui, "Mail")?;
            }
        }
        "desktop-workspace-menu" => {
            // The workspace right-click menu on the second workspace tab ("Sales"), opened by a real right press.
            three_workspaces(&app);
            three_groups(&app, &routes);
            let index = workspace_index(ui, "Sales")?;
            ui.invoke_test_open_workspace_menu(index);
        }
        "desktop-strip-focus" => {
            // Keyboard focus on the second page tab (the focus ring), and the pointer on the pinned workspace tab (its
            // tooltip). Only one strip holds the keyboard focus, so the workspace tab is shown by hover, not by focus.
            three_workspaces(&app);
            three_groups(&app, &routes);
            focus_second_page_tab(ui);
            ui.invoke_test_hover_pinned_workspace();
        }
        "desktop-offline" => ui.set_connection(ConnectionState::Offline),
        "desktop-badges-timer-jobs" => {
            // Counts in `Area::ALL` order: Home, Chat, Projects, Company, People, Finance, Customers, Operations,
            // Analytics, Admin. Only the queue areas draw a badge.
            ui.set_rail_badges(ModelRc::new(VecModel::from(rail_badges(&[
                0, 3, 0, 0, 1, 5, 0, 2, 0, 0,
            ]))));
            ui.set_timer_running(true);
            ui.set_timer_text("1:02:03".into());
            ui.set_jobs(ModelRc::new(VecModel::from(vec![
                job("Export payroll", "Queued"),
                job("Sync mailbox", "Running"),
            ])));
        }
        "desktop-quick-action" => ui.invoke_test_tap_quick_action(),
        "desktop-shortcuts" => ui.invoke_open_shortcuts(),
        "desktop-projects-switcher" => ui.invoke_test_open_project_switcher(),
        "desktop-empty" => ui.invoke_close_workspace_requested(),
        "desktop-profile-menu" => ui.invoke_test_open_profile(),
        "desktop-language-menu" => ui.invoke_test_click_language(),
        "desktop-zoom-50" => set_zoom(ui, 50.0),
        "desktop-zoom-150" => set_zoom(ui, 150.0),
        "desktop-content-home" => content_home(ui, false),
        "desktop-content-new-page" => content_home(ui, true),
        "desktop-content-page" => content_page(ui),
        "phone-default" => {}
        "phone-more-drawer" => {
            let last = ui.get_tab_items().row_count() as i32 - 1;
            ui.invoke_tab_activated(last);
        }
        "phone-workspace-sheet" => {
            // The second workspace opens another page than the start one, so the two rows of the sheet differ.
            ui.invoke_new_workspace_requested();
            ui.invoke_sidebar_activated(row_of(routes[0]));
            ui.invoke_test_open_workspace_sheet();
        }
        "phone-page-back" => {
            ui.invoke_sidebar_activated(row_of(routes[0]));
        }
        _ => {}
    }
    slint::platform::update_timers_and_animations();

    let summary = layout_summary(ui);
    snapshot(ui, out).map(|()| {
        format!(
            "{state} {}x{} {summary} -> {}",
            size.0,
            size.1,
            out.display()
        )
    })
}

/// Sets the window size as an OS resize would, then runs the update that sets `phone` from the width.
fn resize(ui: &AppShell, (width, height): (u32, u32)) {
    let size = LogicalSize::new(width as f32, height as f32);
    ui.window().set_size(size);
    ui.window().dispatch_event(WindowEvent::Resized { size });
    slint::platform::update_timers_and_animations();
}

/// Pages of Home that take no parameter, except the start page (the same list the tile tests use).
fn home_routes() -> Vec<&'static str> {
    pages_in(Area::Home)
        .filter(|page| !page.has_params())
        .map(|page| page.route)
        .filter(|route| *route != HOME)
        .collect()
}

/// The sidebar row of `route` in the Home area.
fn row_of(route: &str) -> i32 {
    sidebar_entries(Area::Home)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .unwrap_or_else(|| panic!("{route} is a Home sidebar row")) as i32
}

fn job(title: &str, state: &str) -> JobItem {
    JobItem {
        title: title.into(),
        state: state.into(),
    }
}

/// One key press and release, as an OS key event would deliver them.
fn key(ui: &AppShell, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Tab until a page-tab strip holds the keyboard focus, then Home and Right: the focus goes to the second tab of that group.
fn focus_second_page_tab(ui: &AppShell) {
    for _ in 0..200 {
        if ui.get_strip_focus_owner() >= 0 {
            break;
        }
        key(ui, Key::Tab);
    }
    key(ui, Key::Home);
    key(ui, Key::RightArrow);
}

/// The index of the workspace tab labelled `label`, read from the workspace model (the strip draws one tab per entry).
fn workspace_index(ui: &AppShell, label: &str) -> Result<i32, String> {
    ui.get_workspace_tabs()
        .iter()
        .position(|tab| tab.label.as_str() == label)
        .map(|index| index as i32)
        .ok_or_else(|| format!("no workspace tab labelled {label:?}"))
}

/// The tile group id and the tab index of the page tab labelled `label`, read from the tile group model.
fn page_tab_position(ui: &AppShell, label: &str) -> Result<(i32, i32), String> {
    for group in ui.get_tile_groups().iter() {
        if let Some(index) = group
            .tabs
            .iter()
            .position(|tab| tab.label.as_str() == label)
        {
            return Ok((group.id, index as i32));
        }
    }
    Err(format!("no page tab labelled {label:?}"))
}

/// The right press on the page tab `label`: the strip's context request, then its tab menu opens.
fn right_click_tab(ui: &AppShell, label: &str) -> Result<(), String> {
    let (group, index) = page_tab_position(ui, label)?;
    ui.invoke_test_open_tab_menu(group, index);
    slint::platform::update_timers_and_animations();
    Ok(())
}

/// Three workspaces: the start one (pinned), a second one renamed "Sales", and a third. The start one is active.
fn three_workspaces(app: &App) {
    let ui = &app.ui;
    ui.invoke_new_workspace_requested();
    ui.invoke_new_workspace_requested();
    ui.invoke_workspace_menu_picked(0, "pin".into());
    // The rename callback alone: the menu pick would open the rename dialog over the shell.
    ui.invoke_workspace_renamed(1, "Sales".into());
    ui.invoke_workspace_activated(0);
}

/// Three groups with one nested split, and several page tabs in each.
///
/// Left group: Home, route 0, route 2. Right group (route 1, route 3) split down: its last tab (route 5) and route 4 sit in the bottom group.
fn three_groups(app: &App, routes: &[&str]) {
    let ui = &app.ui;
    open_in_new_tab(app, routes[0]);
    open_in_new_tab(app, routes[1]);
    let left = focused_id(ui);
    // Route 1 moves to a new group on the right (tab index 2 of the left group).
    pick_on_tab(app, left, 2, "split_right");
    let right = groups(ui)
        .iter()
        .find(|group| group.id != left)
        .map(|group| group.id)
        .expect("the split made a second group");
    ui.invoke_tile_group_focused(left);
    open_in_new_tab(app, routes[2]);
    ui.invoke_tile_group_focused(right);
    open_in_new_tab(app, routes[3]);
    open_in_new_tab(app, routes[5]);
    let last = groups(ui)
        .iter()
        .find(|group| group.id == right)
        .map(|group| group.tabs.row_count() as i32 - 1)
        .unwrap_or(0);
    // The last tab of the right group goes down into a new group: the nested split.
    pick_on_tab(app, right, last, "split_down");
    let bottom = groups(ui)
        .iter()
        .find(|group| group.id != left && group.id != right)
        .map(|group| group.id)
        .expect("the nested split made a third group");
    ui.invoke_tile_group_focused(bottom);
    open_in_new_tab(app, routes[4]);
    ui.invoke_tile_group_focused(left);
    slint::platform::update_timers_and_animations();
}

fn open_in_new_tab(app: &App, route: &str) {
    app.ui.invoke_sidebar_opened_in_new_tab(row_of(route));
}

fn pick_on_tab(app: &App, id: i32, index: i32, item: &str) {
    app.ui
        .invoke_tile_tab_context_requested(id, index, 0.0, 0.0);
    app.ui.invoke_open_tab_menu_picked(index, item.into());
}

fn groups(ui: &AppShell) -> Vec<TileGroup> {
    ui.get_tile_groups().iter().collect()
}

fn focused_id(ui: &AppShell) -> i32 {
    groups(ui)
        .iter()
        .find(|group| group.focused)
        .map(|group| group.id)
        .unwrap_or(0)
}

/// Group ids with their tab counts, for the log line, for example `groups 3 [2 1 2]`.
fn layout_summary(ui: &AppShell) -> String {
    let counts: Vec<String> = groups(ui)
        .iter()
        .map(|group| group.tabs.row_count().to_string())
        .collect();
    format!(
        "workspaces {} groups {} [tabs {}] page {:?}",
        ui.get_workspace_tabs().row_count(),
        counts.len(),
        counts.join(" "),
        ui.get_page_title().as_str()
    )
}

/// Takes the frame, refuses a blank one, and writes the PNG.
fn snapshot(ui: &AppShell, out: &Path) -> Result<(), String> {
    // Mock time: a second lets slides and fades finish (the More drawer, the workspace sheet) before the frame is taken.
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    slint::platform::update_timers_and_animations();
    let buffer = ui
        .window()
        .take_snapshot()
        .map_err(|e| format!("take_snapshot failed: {e}"))?;
    let bytes = buffer.as_bytes();
    let (pixels, _) = bytes.as_chunks::<4>();
    let distinct = pixels.iter().copied().collect::<HashSet<[u8; 4]>>().len();
    if distinct < MIN_DISTINCT_COLOURS {
        return Err(format!(
            "captured image is blank ({distinct} distinct colours): {}",
            out.display()
        ));
    }
    let img = image::RgbaImage::from_raw(buffer.width(), buffer.height(), bytes.to_vec())
        .ok_or_else(|| "snapshot buffer does not match its size".to_string())?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    img.save(out)
        .map_err(|e| format!("could not write {}: {e}", out.display()))
}

/// Sets the interface zoom as Ctrl+plus and Ctrl+minus do: the slider value and the applied zoom at once.
fn set_zoom(ui: &AppShell, percent: f32) {
    ui.set_zoom_percent(percent);
    ui.set_zoom_applied(percent);
}

/// The content scope home (Company > Knowledge Base) with rows shaped like the live API's, and optionally the New page
/// dialog.
fn content_home(ui: &AppShell, new_dialog: bool) {
    use space_area_content::home::HomeWindow as _;
    use space_area_content::scope::Access;
    use space_area_content::view::{PageRow, TabView};
    use space_page_kit::load::LoadState;
    slint::platform::update_timers_and_animations();
    let tab = |slug: &str, label: &str, chip: (u8, u8, u8), count: i32| TabView {
        slug: slug.into(),
        label: label.into(),
        chip,
        count,
    };
    ui.show_scope("Company");
    ui.show_tabs(
        vec![
            tab("", "All", (120, 120, 120), 133),
            tab("doc", "Docs", (59, 130, 246), 193),
            tab("blog", "Posts", (234, 88, 12), 9),
            tab("legal", "Legal documents", (100, 116, 139), 6),
            tab("page", "Pages", (16, 185, 129), 4),
        ],
        0,
    );
    let row = |n: usize,
               title: &str,
               slug: &str,
               collection: &str,
               chip: (u8, u8, u8),
               status: &'static str| PageRow {
        id: format!("page-{n}"),
        title: title.into(),
        slug: slug.into(),
        collection: collection.into(),
        chip,
        status,
        updated: "2 days ago".into(),
        visibility: "Public",
        public: true,
        access: Access::Manage,
    };
    ui.show_rows(vec![
        row(
            1,
            "Introduction",
            "v1/api/overview/introduction",
            "Doc",
            (59, 130, 246),
            "published",
        ),
        row(
            2,
            "Authentication",
            "v1/api/overview/authentication",
            "Doc",
            (59, 130, 246),
            "published",
        ),
        row(
            3,
            "Rate limits",
            "v1/api/overview/rate-limits",
            "Doc",
            (59, 130, 246),
            "draft",
        ),
        row(
            4,
            "An agent-written sample post",
            "agent-written-sample-post",
            "Post",
            (234, 88, 12),
            "published",
        ),
        row(
            5,
            "Code of conduct",
            "code-of-conduct",
            "Legal document",
            (100, 116, 139),
            "published",
        ),
        row(
            6,
            "Privacy policy",
            "privacy-policy",
            "Legal document",
            (100, 116, 139),
            "published",
        ),
    ]);
    ui.show_state(LoadState::Loaded);
    if new_dialog {
        ui.show_new_dialog(
            true,
            vec![("doc".into(), "Doc".into()), ("blog".into(), "Post".into())],
            vec![("t1".into(), "API reference page".into())],
            "",
        );
    }
    slint::platform::update_timers_and_animations();
}

/// A content page in edit mode: the header and the shared block editor holding a realistic docs page.
fn content_page(ui: &AppShell) {
    use space_area_content::page::{PageHeader, PageWindow as _};
    use space_page_kit::load::LoadState;
    slint::platform::update_timers_and_animations();
    ui.show_header(PageHeader {
        title: "Getting started with the API".into(),
        collection: "Doc".into(),
        scope_label: "Company".into(),
        visibility: "Public",
        status: "published",
        can_edit: true,
        can_manage: true,
        has_draft: true,
    });
    ui.show_mode(true);
    ui.show_state(LoadState::Loaded);
    let page: serde_json::Value = serde_json::from_str(SAMPLE_PAGE).expect("sample page JSON");
    let doc = rimelight_content::import_page(&page).expect("sample page imports");
    let editor = rimelight_content::editor::EditorDoc::from_doc(doc);
    let controller = std::rc::Rc::new(std::cell::RefCell::new(
        rimelight_editor::EditorController::new(editor, "en"),
    ));
    let binding = crate::slint_ui::block_editor::bind_block_editor(ui, controller, |_| {});
    binding.refresh();
    std::mem::forget(binding);
    slint::platform::update_timers_and_animations();
}

/// A docs page with the blocks people use most: sections, rich paragraphs, a callout, a list, code, a quote.
const SAMPLE_PAGE: &str = r##"{
  "meta": {"slug": "getting-started", "type": "doc", "title": {"en": "Getting started with the API"}},
  "content": {"properties": {}, "blocks": [
    {"id": "00000000-0000-4000-8000-000000000001", "type": "SectionBlock", "props": {"title": [{"type": "text", "text": "Overview"}]}, "children": [
      {"id": "00000000-0000-4000-8000-000000000002", "type": "ParagraphBlock", "props": {"text": [{"type": "text", "text": "The Rimelight API gives "}, {"type": "text", "text": "read and write access", "marks": ["bold"]}, {"type": "text", "text": " to your game data. Every request is authenticated with a token, and responses are JSON."}]}},
      {"id": "00000000-0000-4000-8000-000000000003", "type": "CalloutBlock", "props": {"variant": "info"}, "children": [
        {"id": "00000000-0000-4000-8000-000000000004", "type": "ParagraphBlock", "props": {"text": [{"type": "text", "text": "Keep your token secret. Anyone with it can act as your account."}]}}
      ]},
      {"id": "00000000-0000-4000-8000-000000000005", "type": "UnorderedListBlock", "props": {}, "children": [
        {"id": "00000000-0000-4000-8000-000000000006", "type": "ListItemBlock", "props": {"content": [{"type": "text", "text": "Create a token in your account settings"}]}},
        {"id": "00000000-0000-4000-8000-000000000007", "type": "ListItemBlock", "props": {"content": [{"type": "text", "text": "Send it in the Authorization header"}]}}
      ]}
    ]},
    {"id": "00000000-0000-4000-8000-000000000008", "type": "SectionBlock", "props": {"title": [{"type": "text", "text": "Your first request"}]}, "children": [
      {"id": "00000000-0000-4000-8000-000000000009", "type": "CodeBlock", "props": {"language": "bash", "code": "curl https://rimelight.com/api/v1/me -H 'Authorization: Bearer <token>'"}},
      {"id": "00000000-0000-4000-8000-000000000010", "type": "QuoteBlock", "props": {"text": [{"type": "text", "text": "A 200 response with your profile means you are ready."}]}},
      {"id": "00000000-0000-4000-8000-000000000011", "type": "ParagraphBlock", "props": {"text": [{"type": "text", "text": "Next, read about "}, {"type": "text", "text": "rate limits", "marks": ["italic"]}, {"type": "text", "text": " before you send many requests."}]}}
    ]}
  ]}
}"##;
