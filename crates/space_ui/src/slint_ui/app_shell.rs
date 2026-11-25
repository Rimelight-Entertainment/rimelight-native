//! App shell wiring (S9.4; PLAN 6.1). The window `apps/desktop` opens: rail and sidebar from the
//! `space_nav` manifest, a placeholder page per route, the shell screens (About, Report Issue, Settings), the command
//! palette (Ctrl+K), the theme switch, the toast stack and the status bar. The markup is `ui/app-shell.slint`; this module
//! fills its models and answers its callbacks.
//!
//! Page header and panes (SF.4; IA section 6): the breadcrumbs are Area, Section, Page. Each navigation opens or activates a
//! tab ([`Tabs`]); the tabs can be closed (the last one stays), and the split view shows the next open page in a second pane.
//!
//! The toast queue is ticked from one `slint::Timer` (every [`TICK_MS`]), and the stack is kept in step with the queue.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use slint::{
    CloseRequestResponse, ComponentHandle as _, Model as _, ModelRc, SharedString, Timer,
    TimerMode, VecModel,
};
use space_nav::{Area, Page, Template, format_view_route, pages_in, parse, quick};

use crate::slint_ui::chat_sidebar::{self, Action};
use crate::slint_ui::clipboard;
use crate::slint_ui::content;
use crate::slint_ui::glue::theme as slint_theme;
use crate::slint_ui::history;
use crate::slint_ui::palette::{self, Entry};
use crate::slint_ui::shell::{icon, listed_route, sidebar_entries, sidebar_rows, toggle_heading};
use crate::slint_ui::tile_drop::{self, Hit, Plan, Zone};
use crate::slint_ui::tiles::{
    Edge, GROUP_CAP, GroupId, GroupRect, HandleRect, Orientation, TileLayout,
};
use crate::slint_ui::webviews::WebHost;
use crate::slint_ui::workspace::{MemoryStore, Saver, Store, Workspaces, startup};
use crate::slint_ui::{
    AppShell, HomeState, Icons, MenuChecked, MenuItem, MenuKind, PaletteRow, ProjectChoice,
    QuickCategory, QuickItem, RailItem, TabItem, TileGroup, TileHandle, ToastEntry, ToastTone,
};
use crate::theme::ThemeMode;
use rimelight_shell::tabs::{PageTab, StripMenu, TabMenuRow, action, reopen_shortcut, rule};
use rimelight_ui_kit::toast::{Record, Toast, ToastQueue, Tone, sync_model, take_announced};
use space_area_admin::{organisation, roles, types_tags, users};
use space_area_canvas::list::{self as canvas_list, CanvasList};
use space_area_canvas::page::{self as canvas_page, CanvasPage};
use space_area_company::departments::{self, Departments};
use space_area_company::employees::{self, Employees};
use space_area_company::locations::{self, Locations};
use space_area_company::profile::{self, PHOTO_NATIVE, Profile};
use space_area_company::team::{self, TeamPage};
use space_area_company::teams::{self, Teams};
use space_area_home::dashboard::{self as home_dashboard, Home};
use space_area_projects::create_project;
use space_area_projects::issue::{self, IssueDetail};
use space_area_projects::issues::{self, Issues, can_create};
use space_area_projects::new_issue::{self, NewIssue};
use space_area_projects::new_sprint;
use space_area_projects::overview::{self, Overview};
use space_page_kit::page_data::{self, DataCell, DataSource};
use space_page_kit::projects;

/// The page tabs of one tile group in Space (`rimelight_shell::tabs::PageTabs` over Space's routes).
pub(crate) type PageTabs = rimelight_shell::tabs::PageTabs<crate::slint_ui::routes::SpaceRoutes>;

/// The toast countdown step, in milliseconds. One timer drives every toast.
pub const TICK_MS: u64 = 100;

/// The route shown when the start route is not a known page.
const FALLBACK_ROUTE: &str = "/home";

/// The areas with a tab on a phone, in tab order (PLAN 6.2; IA section 8). The tab after them is More, which opens the drawer.
pub const PHONE_TABS: [Area; 4] = [Area::Home, Area::Chat, Area::Projects, Area::Company];

/// The areas with no phone tab, in nav order: they are listed in the More drawer (OQ24). The tabs are [`PHONE_TABS`].
pub fn more_areas() -> Vec<Area> {
    Area::ALL
        .iter()
        .copied()
        .filter(|area| !PHONE_TABS.contains(area))
        .collect()
}

/// The label and icon of the More tab.
const MORE_LABEL: &str = "More";
const MORE_ICON: &str = "ellipsis";

/// The width of a bar between two tile groups, in logical pixels (the bar is drawn at this thickness).
const TILE_HANDLE: f32 = 6.0;

/// The window and the timer that ticks its toasts. Keep it alive while the window runs: dropping the timer stops the ticks.
pub struct App {
    pub ui: AppShell,
    /// The id of the last quick action item picked (`space_nav::quick`). Nothing acts on it yet (no create dialogs).
    pub last_quick_pick: Rc<RefCell<Option<String>>>,
    /// The page data slot (SP.3b): the session driver fills it when the window is attached to a session.
    pub data: DataCell,
    /// Home > Dashboard (S2): the Assigned to me and Recent canvases cards.
    pub home: Home<AppShell>,
    /// Company > Employees (SP.3b).
    pub employees: Employees<AppShell>,
    /// Company > Employees > one person's profile (SP.3c).
    pub profile: Profile<AppShell>,
    /// Company > Teams: the team list and the org chart (SP.3d).
    pub teams: Teams<AppShell>,
    /// Company > Teams > one team (SP.3d).
    pub team: TeamPage<AppShell>,
    /// Company > Departments (SP.3d).
    pub departments: Departments<AppShell>,
    /// Company > Locations.
    pub locations: Locations<AppShell>,
    /// Canvas page at `/home/canvases/{id}` and `/p/{KEY}/canvases/{id}` (CV.6): the editor and its live room.
    pub canvas: CanvasPage,
    /// Canvas lists at `/home/canvases` and `/p/{KEY}/canvases` (CV.6).
    pub canvas_list: CanvasList<AppShell>,
    /// Admin > Users (SP.3e).
    pub admin_users: space_area_admin::users::Page<AppShell>,
    /// Admin > Organisation (SP.3e).
    pub admin_org: space_area_admin::organisation::Page<AppShell>,
    /// Admin > Roles (SP.3e).
    pub admin_roles: space_area_admin::roles::Page<AppShell>,
    /// Projects > Overview: the summary of one project at `/p/:key`.
    pub overview: Overview<AppShell>,
    /// Projects > Issues: the issue list of one project (S1.3).
    pub issues: Issues<AppShell>,
    /// Projects > Issue: one issue at `/p/:key/issues/:id` (S1.3, part 3).
    pub issue: IssueDetail<AppShell>,
    /// Projects > New issue (S1.5c): the dialog, opened by the issues page and the quick action.
    pub new_issue: NewIssue<AppShell>,
    tabs: Rc<Tabs>,
    _timer: Timer,
    /// Returns the content web views after a popup closes (CMS.6). Dropping it stops the ticks.
    _web_timer: Timer,
    /// Feeds the status bar clock once a second. Dropping it stops the clock.
    _clock: Timer,
}

/// The local time for the status bar clock, as `HH:MM` (24 hours).
fn clock_text() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

impl App {
    /// The browser moved to `location` (its pathname with the `?view=` query): Back or Forward in the browser (planned
    /// feature A3). The active tab lands on that route without a new address bar entry. Call it from the popstate event.
    pub fn location_changed(&self, location: &str) {
        self.tabs.land(&self.ui, location);
    }

    /// Saves the open workspaces now, when they changed since the last save (planned feature B3). Called on exit: the desktop
    /// window's close request does it, and so do the web page's hide events and the Android pause and stop events.
    pub fn flush(&self) {
        self.tabs.flush();
    }

    /// The saved text of the open workspaces as they are now (see [`Workspaces::encode`]).
    pub fn state_text(&self) -> Result<String, String> {
        self.tabs.workspaces.borrow().encode()
    }

    /// Shows or hides the in-app workspace strip (planned feature B3b). The web hides it, since the browser's tabs do that job
    /// (`PLAN.md section 6.6` part B, P5). The workspaces, their saving and the shortcuts are the same either way. Call it
    /// before the event loop runs.
    pub fn show_workspace_strip(&self, show: bool) {
        self.ui.set_show_workspace_strip(show);
    }
}

/// How long after the last change the workspaces are saved (planned feature B3): one second, so a burst of changes writes once.
pub const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The strip tab menu rows as the window shows them (`AppShell.open-tab-menu` or `open-workspace-menu`).
fn tab_menu_items(rows: &[TabMenuRow]) -> ModelRc<MenuItem> {
    let items: Vec<MenuItem> = rows
        .iter()
        .map(|row| MenuItem {
            id: row.id.into(),
            kind: if row.separator {
                MenuKind::Separator
            } else {
                MenuKind::Item
            },
            label: row.label.as_str().into(),
            icon: Default::default(),
            shortcut: row.shortcut.as_str().into(),
            disabled: !row.enabled,
            danger: false,
            checked: MenuChecked::None,
            parent: Default::default(),
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

/// The strip item of a page tab.
fn tab_item(tab: &PageTab) -> TabItem {
    TabItem {
        label: tab.title.as_str().into(),
        short_label: short_label(&tab.title),
        pinned: tab.pinned,
    }
}

/// Puts `rows` into a strip's model. A model of the same length is updated row by row, so the strip elements that draw its
/// rows stay alive and keep the keyboard focus they hold (gaps D3). A different length resets the model.
fn replace_rows<T: Clone + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() == rows.len() {
        for (at, row) in rows.into_iter().enumerate() {
            model.set_row_data(at, row);
        }
    } else {
        model.set_vec(rows);
    }
}

/// The letter a pinned tab draws: the first character of its title, in capitals.
fn short_label(title: &str) -> SharedString {
    title
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default()
        .into()
}

/// Where the active tab's route is shown in the browser address bar (planned feature A3). The web app writes the address
/// bar; the desktop and Android builds pass [`NoLocation`].
pub trait Location {
    /// A new step: the address bar gets a new history entry for `route` (a location from [`history::location_from_route`]).
    fn push(&self, route: &str);
    /// The current entry changes to `route`, with no new entry.
    fn replace(&self, route: &str);
}

/// A [`Location`] that writes nothing, for the builds without an address bar.
pub struct NoLocation;

impl Location for NoLocation {
    fn push(&self, _route: &str) {}
    fn replace(&self, _route: &str) {}
}

/// How a route change reaches the address bar: a new entry, or the current entry replaced (Back and Forward inside the app).
#[derive(Clone, Copy)]
enum UrlStep {
    Push,
    Replace,
}

/// A page tab drag in progress (T2b): the dragged tab, its label, the pointer in page area px, and what a drop there would do.
struct TileDrag {
    source: GroupId,
    index: usize,
    label: String,
    x: f32,
    y: f32,
    hit: Option<Hit>,
    plan: Plan,
}

/// The drop overlay of group `group` while a tile drag is over it: `(zone, strip mark, refused)`. The zone codes are those of
/// `TileGroup.drop-zone` (0 none, 1 left, 2 right, 3 top, 4 bottom, 5 centre).
fn drop_view(drag: Option<&TileDrag>, group: GroupId) -> (i32, i32, bool) {
    let Some(drag) = drag else {
        return (0, -1, false);
    };
    let Some(hit) = drag.hit else {
        return (0, -1, false);
    };
    if hit.group != group || drag.plan == Plan::Nothing {
        return (0, -1, false);
    }
    let refused = drag.plan == Plan::Refused;
    match hit.zone {
        Zone::Edge(edge) => (edge_code(edge), -1, refused),
        Zone::Centre => (5, -1, refused),
        Zone::Strip(_) => match drag.plan {
            Plan::Move {
                mark: Some(mark), ..
            } => (0, mark as i32, false),
            _ => (0, -1, false),
        },
    }
}

/// The `TileGroup.drop-zone` code of an edge.
fn edge_code(edge: Edge) -> i32 {
    match edge {
        Edge::Left => 1,
        Edge::Right => 2,
        Edge::Top => 3,
        Edge::Bottom => 4,
    }
}

/// The open page tabs (SF.4; IA section 6). The tab strip model is kept in step with them by [`Tabs::sync`].
struct Tabs {
    /// The top-level workspaces. The page tabs shown are the active workspace's (planned feature B; see [`Tabs::view`]).
    workspaces: RefCell<Workspaces>,
    /// The focused group's page tab strip, mirrored for the window's `open-tabs` (the window tests read it).
    model: Rc<VecModel<TabItem>>,
    /// The tile groups of the active workspace, drawn by the window (`tile-groups`).
    groups: Rc<VecModel<TileGroup>>,
    /// The page tab model of each group, by group id (see `group_tab_model`).
    group_tabs: RefCell<HashMap<i32, Rc<VecModel<TabItem>>>>,
    /// The breadcrumbs model of each group, by group id (see `group_crumb_model`).
    group_crumbs: RefCell<HashMap<i32, Rc<VecModel<SharedString>>>>,
    /// The bars between the groups (`tile-handles`).
    bars: Rc<VecModel<TileHandle>>,
    /// The bars of the last layout, in the order of `bars`: a drag finds its split from the path.
    handles: RefCell<Vec<HandleRect>>,
    /// The page area's size in pixels, from the window (`tile-area-resized`). Zero until the window reports it.
    area: Cell<(f32, f32)>,
    /// The workspace strip model (planned feature B2, desktop only).
    workspace_model: Rc<VecModel<TabItem>>,
    location: Rc<dyn Location>,
    /// The route the address bar shows last, so a change is written once.
    last_url: RefCell<String>,
    /// True while the browser lands the tab (a Back or Forward, or the start): the address bar already shows that location,
    /// so no new entry is written for it.
    landing: Cell<bool>,
    /// The page tab drag in progress (T2b), if any.
    drag: RefCell<Option<TileDrag>>,
    /// The workspace tab dragged in the strip (its index), if any (T2b).
    workspace_drag: Cell<Option<usize>>,
    /// The insertion mark of the workspace drag (`-1` for none).
    workspace_mark: Cell<i32>,
    /// Writes the workspaces to the store (planned feature B3). The debounced save and the exit save both go through it.
    saver: Saver,
    /// The debounce timer: each change restarts it, and it saves [`SAVE_DELAY`] after the last change.
    save_timer: Timer,
    /// This tab list, for the timer's callback (set by `Rc::new_cyclic`, so the timer does not keep it alive).
    me: Weak<Tabs>,
    /// The window, read for the rail area and sidebar selection when the workspaces are saved.
    window: slint::Weak<AppShell>,
}

impl Tabs {
    /// The tab list for `workspaces`, bound to the window's tab strip, that writes the active route to `location` and saves
    /// through `saver`.
    fn new(
        ui: &AppShell,
        location: Rc<dyn Location>,
        workspaces: Workspaces,
        saver: Saver,
    ) -> Rc<Self> {
        let model = Rc::new(VecModel::<TabItem>::default());
        ui.set_open_tabs(ModelRc::new(model.clone()));
        let workspace_model = Rc::new(VecModel::<TabItem>::default());
        ui.set_workspace_tabs(ModelRc::new(workspace_model.clone()));
        let groups = Rc::new(VecModel::<TileGroup>::default());
        ui.set_tile_groups(ModelRc::new(groups.clone()));
        let bars = Rc::new(VecModel::<TileHandle>::default());
        ui.set_tile_handles(ModelRc::new(bars.clone()));
        Rc::new_cyclic(|me| Self {
            workspaces: RefCell::new(workspaces),
            model,
            groups,
            group_tabs: RefCell::new(HashMap::new()),
            group_crumbs: RefCell::new(HashMap::new()),
            bars,
            handles: RefCell::new(Vec::new()),
            area: Cell::new((0.0, 0.0)),
            workspace_model,
            location,
            last_url: RefCell::new(String::new()),
            landing: Cell::new(false),
            drag: RefCell::new(None),
            workspace_drag: Cell::new(None),
            workspace_mark: Cell::new(-1),
            saver,
            save_timer: Timer::default(),
            me: me.clone(),
            window: ui.as_weak(),
        })
    }

    /// A change to the workspaces: the debounce timer restarts, so the save runs [`SAVE_DELAY`] after the last change.
    fn schedule_save(&self) {
        let me = self.me.clone();
        self.save_timer
            .start(TimerMode::SingleShot, SAVE_DELAY, move || {
                if let Some(tabs) = me.upgrade() {
                    tabs.save_now();
                }
            });
    }

    /// Saves the open workspaces now, if their text changed since the last save. The window's rail area and sidebar selection
    /// are read into the active workspace first (they are not kept anywhere else). A failed write is reported on stderr, and
    /// the app keeps running.
    fn save_now(&self) {
        if let Some(ui) = self.window.upgrade() {
            save_shell(&ui, self);
        }
        if let Err(error) = self.saver.save(&self.workspaces.borrow()) {
            eprintln!("rimelight: the workspaces were not saved: {error}");
        }
    }

    /// Stops the pending save and saves now. Called on exit.
    fn flush(&self) {
        self.save_timer.stop();
        self.save_now();
    }

    /// Runs `f` on the active workspace's page tabs. `None` when no workspace is open (the empty state).
    fn view<R>(&self, f: impl FnOnce(&PageTabs) -> R) -> Option<R> {
        self.workspaces.borrow().active_pages().map(f)
    }

    /// Runs `f` on the active workspace's page tabs, for changing. `None` when no workspace is open.
    fn edit<R>(&self, f: impl FnOnce(&mut PageTabs) -> R) -> Option<R> {
        let mut workspaces = self.workspaces.borrow_mut();
        let result = workspaces.active_pages_mut().map(f);
        workspaces.tidy_active();
        result
    }

    /// Opens a blank workspace when none is open, so that a page can open (a rail, sidebar or palette pick in the empty state).
    fn ensure_workspace(&self) {
        let mut workspaces = self.workspaces.borrow_mut();
        if workspaces.is_empty() {
            workspaces.new_blank_workspace();
        }
    }

    /// The workspace menu rows for workspace `index` (see [`StripMenu`]). `compact` is the phone sheet's shorter menu.
    fn workspace_menu_rows(&self, index: usize, compact: bool) -> Vec<TabMenuRow> {
        let workspaces = self.workspaces.borrow();
        let pinned = workspaces.pinned_flags();
        StripMenu {
            noun: "workspace",
            pinned: &pinned,
            index,
            reopen_shortcut: reopen_shortcut("W"),
            reopen_enabled: workspaces.closed_len() > 0,
            rename: true,
            split: None,
            compact,
        }
        .rows()
    }

    /// The index of the active workspace, or `None` when none is open.
    fn active_workspace(&self) -> Option<usize> {
        self.workspaces.borrow().active()
    }

    /// Lands the active tab on the route for a browser `location` (a popstate). The address bar is not written for it, since
    /// it already shows the location. An unknown location shows Home, and the address bar is replaced with that route.
    fn land(&self, ui: &AppShell, location: &str) {
        let route = history::route_from_location(location);
        let Some(matched) = parse(&route) else {
            return;
        };
        self.landing.set(true);
        self.ensure_workspace();
        if show_route(ui, &route) {
            self.edit(|pages| pages.land(&route, matched.page.title));
            self.sync_with(ui, UrlStep::Replace);
        }
        self.landing.set(false);
        if route != location {
            self.location.replace(&route);
        }
    }

    /// Writes the active route to the address bar as the current entry, with no new entry. The start writes nothing through
    /// [`Tabs::record_url`] (it is a landing), so the build calls this once when it is done.
    fn write_location(&self) {
        let Some(route) = self.route(self.active()) else {
            return;
        };
        *self.last_url.borrow_mut() = route.clone();
        if let Some(location) = history::location_from_route(&route) {
            self.location.replace(&location);
        }
    }

    /// Writes the active route to the address bar when it changed since the last write. A landing writes nothing.
    fn record_url(&self, step: UrlStep) {
        let Some(route) = self.route(self.active()) else {
            return;
        };
        if *self.last_url.borrow() == route {
            return;
        }
        *self.last_url.borrow_mut() = route.clone();
        if self.landing.get() {
            return;
        }
        let Some(location) = history::location_from_route(&route) else {
            return;
        };
        match step {
            UrlStep::Push => self.location.push(&location),
            UrlStep::Replace => self.location.replace(&location),
        }
    }

    /// Activates the tab for `route`, opening it first when it is not open.
    fn open(&self, route: &str, title: &str) {
        self.ensure_workspace();
        self.edit(|pages| pages.open(route, title));
    }

    /// Replaces the active tab's route (a sidebar click), pushing it onto that tab's history.
    fn navigate(&self, route: &str, title: &str) {
        self.ensure_workspace();
        self.edit(|pages| pages.navigate(route, title));
    }

    /// Opens `route` in a new tab (Ctrl+click or middle-click).
    fn open_new_tab(&self, route: &str, title: &str) {
        self.ensure_workspace();
        self.edit(|pages| pages.open_new_tab(route, title));
    }

    /// Goes back in the active tab's history. Returns the route to show.
    fn back(&self) -> Option<String> {
        self.edit(|pages| pages.back()).flatten()
    }

    /// Goes forward in the active tab's history. Returns the route to show.
    fn forward(&self) -> Option<String> {
        self.edit(|pages| pages.forward()).flatten()
    }

    /// The current route of tab `index`, if it is open.
    fn route(&self, index: usize) -> Option<String> {
        self.view(|pages| pages.route(index)).flatten()
    }

    /// The route of the active page tab in workspace `index` (Copy link on a workspace menu), or `None` when that workspace
    /// is not open.
    fn workspace_route(&self, index: usize) -> Option<String> {
        self.workspaces
            .borrow()
            .get(index)
            .and_then(|workspace| workspace.active_route())
    }

    /// The index of the active tab (0 when no workspace or tab is open).
    fn active(&self) -> usize {
        self.view(|pages| pages.active).unwrap_or(0)
    }

    /// Makes tab `index` active. Returns false when there is no such tab.
    fn select(&self, index: usize) -> bool {
        self.edit(|pages| pages.select(index)).unwrap_or(false)
    }

    /// Closes tab `index`. Returns false when there is no tab `index`.
    fn close(&self, index: usize) -> bool {
        self.edit(|pages| pages.close(index)).unwrap_or(false)
    }

    /// True when no page is open (or no workspace is). The page area then shows the empty state.
    fn is_empty(&self) -> bool {
        self.view(|pages| pages.pages.is_empty()).unwrap_or(true)
    }

    /// Brings the tab strip, the active tab and the split pane's second page in line with the open pages, and pushes a new
    /// active route to the address bar.
    fn sync(&self, ui: &AppShell) {
        self.sync_with(ui, UrlStep::Push);
    }

    /// [`Tabs::sync`] with the address bar step given: Back and Forward inside the app replace the entry instead.
    fn sync_with(&self, ui: &AppShell, step: UrlStep) {
        self.sync_strip(ui);
        self.record_url(step);
    }

    /// The tile groups, the focused group's strip mirror, the window's tab state and the back and forward flags (no address
    /// bar). With no workspace open, the groups are empty and the workspace strip is shown alone.
    /// The page tab model of group `id`. It is the same instance for the life of the group, so a sync only replaces its rows:
    /// the window's repeater keeps the tab elements, and with them the pointer grab of a drag in progress. A fresh model on
    /// every sync rebuilt the strip's tabs on the first drag move, and the drag ended there.
    fn group_tab_model(&self, id: i32) -> Rc<VecModel<TabItem>> {
        self.group_tabs
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(VecModel::default()))
            .clone()
    }

    /// The breadcrumbs model of group `id`, kept for the life of the group like `group_tab_model`: a sync only replaces its rows.
    fn group_crumb_model(&self, id: i32) -> Rc<VecModel<SharedString>> {
        self.group_crumbs
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(VecModel::default()))
            .clone()
    }

    /// The drag preview (ghost, overlay, strip mark) as root properties. A pointer move during a drag calls only this, never
    /// `sync_strip`: the `tile-groups` model stays untouched, so no repeater item is rebuilt under the pointer.
    fn sync_drag(&self, ui: &AppShell) {
        let drag = self.drag.borrow();
        let (group, zone, mark, refused) = drag
            .as_ref()
            .and_then(|d| d.hit.map(|hit| (hit.group, drop_view(Some(d), hit.group))))
            .map_or((-1, 0, -1, false), |(group, (zone, mark, refused))| {
                (group.0 as i32, zone, mark, refused)
            });
        ui.set_drag_group(group);
        ui.set_drag_zone(zone);
        ui.set_drag_mark(mark);
        ui.set_drag_refused(refused);
        ui.set_tile_ghost_visible(drag.is_some());
        ui.set_tile_ghost_label(drag.as_ref().map_or("", |d| d.label.as_str()).into());
        ui.set_tile_ghost_x(drag.as_ref().map_or(0.0, |d| d.x));
        ui.set_tile_ghost_y(drag.as_ref().map_or(0.0, |d| d.y));
        ui.set_tile_ghost_refused(drag.as_ref().is_some_and(|d| d.plan == Plan::Refused));
    }

    fn sync_strip(&self, ui: &AppShell) {
        // Every change to the page tabs, the groups or the active workspace ends here, so it is the save's choke point.
        self.schedule_save();
        let phone = ui.get_phone();
        let (width, height) = self.area.get();
        let icons = ui.global::<Icons>();
        let mut groups = Vec::new();
        let mut bars = Vec::new();
        let mut handles = Vec::new();
        let mut focused_rows: Vec<TabItem> = Vec::new();
        let (mut open_active, mut open_pinned) = (0, 0);
        let drag = self.drag.borrow();
        {
            let workspaces = self.workspaces.borrow();
            if let Some(tiles) = workspaces.active_tiles() {
                let focused = tiles.focused_group();
                // A phone keeps one group: the focused one fills the page area, and there are no bars.
                let (rects, found) = if phone {
                    let whole = GroupRect {
                        id: focused,
                        x: 0.0,
                        y: 0.0,
                        w: width.max(0.0),
                        h: height.max(0.0),
                    };
                    (vec![whole], Vec::new())
                } else {
                    tiles.layout_rects(width, height, TILE_HANDLE)
                };
                for rect in rects {
                    let Some(group) = tiles.group(rect.id) else {
                        continue;
                    };
                    let pages = group.tabs();
                    let is_focused = rect.id == focused;
                    let items: Vec<TabItem> = pages.pages.iter().map(tab_item).collect();
                    // The group's own tab model, kept across syncs: a new model would rebuild the strip's tabs, and the
                    // pointer grab of a drag in progress would go with the old tab (see `group_tab_model`).
                    let tabs_model = self.group_tab_model(rect.id.0 as i32);
                    replace_rows(&tabs_model, items.clone());
                    let pinned = pages.pinned_count();
                    let image = pages
                        .route(pages.active)
                        .and_then(|route| parse(&route))
                        .map(|matched| icon(&icons, matched.page.icon()))
                        .unwrap_or_default();
                    // The group's own page header, from its active page (the same values `show_route` sets for the focused page).
                    let active_route = pages
                        .route(pages.active)
                        .map(|route| route.to_string())
                        .unwrap_or_default();
                    let header = page_header(&active_route);
                    let crumbs_model = self.group_crumb_model(rect.id.0 as i32);
                    replace_rows(
                        &crumbs_model,
                        header.as_ref().map_or_else(Vec::new, |h| h.crumbs.clone()),
                    );
                    let (title, inspector) = header.as_ref().map_or_else(
                        || (SharedString::new(), false),
                        |h| (h.title.clone(), h.inspector),
                    );
                    if is_focused {
                        focused_rows = items.clone();
                        open_active = pages.active;
                        open_pinned = pinned;
                    }
                    groups.push(TileGroup {
                        id: rect.id.0 as i32,
                        x: rect.x,
                        y: rect.y,
                        w: rect.w,
                        h: rect.h,
                        focused: is_focused,
                        icon: image,
                        tabs: ModelRc::new(tabs_model),
                        active: pages.active as i32,
                        pinned_count: pinned as i32,
                        title,
                        route: SharedString::from(active_route.as_str()),
                        crumbs: ModelRc::new(crumbs_model),
                        can_share: header.is_some(),
                        inspector_available: inspector,
                    });
                }
                for bar in found {
                    bars.push(TileHandle {
                        x: bar.x,
                        y: bar.y,
                        w: bar.w,
                        h: bar.h,
                        vertical_bar: bar.orientation == Orientation::Horizontal,
                    });
                    handles.push(bar);
                }
            }
        }
        replace_rows(&self.groups, groups);
        // In place when the count is the same: a new model would rebuild the handle under the pointer and drop its grab.
        replace_rows(&self.bars, bars);
        *self.handles.borrow_mut() = handles;
        drop(drag);
        self.sync_drag(ui);
        self.model.set_vec(focused_rows);
        ui.set_open_active(open_active as i32);
        ui.set_open_pinned_count(open_pinned as i32);
        ui.set_can_back(self.view(|p| p.can_back()).unwrap_or(false));
        ui.set_can_forward(self.view(|p| p.can_forward()).unwrap_or(false));
        self.sync_workspaces(ui);
    }

    /// Focuses tile group `id` of the active workspace. Returns false when there is no such group.
    fn focus(&self, id: u32) -> bool {
        self.workspaces
            .borrow_mut()
            .active_tiles_mut()
            .is_some_and(|tiles| tiles.focus(GroupId(id)))
    }

    /// Splits the focused group on `edge`, moving tab `index` of it into the new group. Returns false when the split is
    /// refused (the group has one tab, or the layout is at its cap).
    fn split(&self, index: usize, edge: Edge) -> bool {
        let mut workspaces = self.workspaces.borrow_mut();
        let done = workspaces.active_tiles_mut().is_some_and(|tiles| {
            let group = tiles.focused_group();
            tiles.split(group, edge, Some((group, index))).is_ok()
        });
        workspaces.tidy_active();
        done
    }

    /// Moves the split behind handle `index` of the last layout so that its bar sits at the pointer (`x`, `y` in page area
    /// coordinates). Returns false when there is no such handle. The caller syncs the window.
    fn drag_handle(&self, index: usize, x: f32, y: f32) -> bool {
        let Some(handle) = self.handles.borrow().get(index).cloned() else {
            return false;
        };
        let (width, height) = self.area.get();
        let mut workspaces = self.workspaces.borrow_mut();
        let Some(tiles) = workspaces.active_tiles_mut() else {
            return false;
        };
        let Some((orientation, origin, length)) =
            tiles.split_extent(&handle.path, width, height, TILE_HANDLE)
        else {
            return false;
        };
        let usable = length - TILE_HANDLE;
        if usable <= 0.0 {
            return false;
        }
        let pointer = match orientation {
            Orientation::Horizontal => x,
            Orientation::Vertical => y,
        };
        let ratio = (pointer - origin - TILE_HANDLE / 2.0) / usable;
        tiles.set_ratio(&handle.path, ratio)
    }

    /// A page tab of group `source` was dragged to page area point (`x`, `y`) (T2b). The first move starts the drag, and every
    /// move works out what a drop there would do (the overlay and the ghost show it). Returns false when there is no such tab.
    /// The caller syncs the window.
    fn tile_drag_moved(&self, source: u32, index: usize, x: f32, y: f32) -> bool {
        let source = GroupId(source);
        let (label, hit, plan) = {
            let workspaces = self.workspaces.borrow();
            let Some(tiles) = workspaces.active_tiles() else {
                return false;
            };
            let Some(label) = tiles
                .group(source)
                .and_then(|group| group.tabs().pages.get(index))
                .map(|page| page.title.clone())
            else {
                return false;
            };
            let (width, height) = self.area.get();
            let groups = tile_drop::drop_groups(tiles, width, height, TILE_HANDLE);
            let hit = tile_drop::hit_test(&groups, x, y);
            (label, hit, tile_drop::plan(tiles, source, index, hit))
        };
        let mut drag = self.drag.borrow_mut();
        let label = drag.as_ref().map_or(label, |running| running.label.clone());
        *drag = Some(TileDrag {
            source,
            index,
            label,
            x,
            y,
            hit,
            plan,
        });
        true
    }

    /// The tile drag ends at page area point (`x`, `y`) (T2b): the drop runs its plan (a split, or a move into a strip or the
    /// centre of a group, which then takes focus), and an emptied source group closes. Released outside every group, on the
    /// source's own centre, or refused, nothing moves. Returns whether a tab moved. The caller shows the active tab and syncs.
    fn tile_drag_dropped(&self, x: f32, y: f32) -> bool {
        let Some(drag) = self.drag.borrow_mut().take() else {
            return false;
        };
        let mut workspaces = self.workspaces.borrow_mut();
        let (width, height) = self.area.get();
        let Some(tiles) = workspaces.active_tiles_mut() else {
            return false;
        };
        let groups = tile_drop::drop_groups(tiles, width, height, TILE_HANDLE);
        let hit = tile_drop::hit_test(&groups, x, y);
        let done = match tile_drop::plan(tiles, drag.source, drag.index, hit) {
            Plan::Split { group, edge } => tiles
                .split(group, edge, Some((drag.source, drag.index)))
                .is_ok(),
            Plan::Move { group, to, .. } => {
                tiles.move_tab(drag.source, drag.index, group, to).is_ok()
            }
            Plan::Nothing | Plan::Refused => false,
        };
        workspaces.tidy_active();
        done
    }

    /// Ends a tile drag without a drop (Escape). Returns whether a drag was running.
    fn tile_drag_cancelled(&self) -> bool {
        self.drag.borrow_mut().take().is_some()
    }

    /// A workspace tab was dragged in the strip to strip x `x` (T2b). Returns false when `index` is not a workspace. The
    /// caller syncs the window.
    fn workspace_drag_moved(&self, index: usize, x: f32) -> bool {
        let pinned = self.workspaces.borrow().pinned_flags();
        let slot = tile_drop::insertion_slot(&pinned, x);
        let Some((_, mark)) = tile_drop::reorder(&pinned, index, slot) else {
            return false;
        };
        self.workspace_drag.set(Some(index));
        self.workspace_mark.set(mark.map_or(-1, |at| at as i32));
        true
    }

    /// Drops the dragged workspace tab at strip x `x`, in the pinned rules (T2b). Returns whether the order changed. The
    /// caller syncs the window.
    fn workspace_drag_dropped(&self, x: f32) -> bool {
        let Some(from) = self.workspace_drag.take() else {
            return false;
        };
        self.workspace_mark.set(-1);
        let mut workspaces = self.workspaces.borrow_mut();
        let pinned = workspaces.pinned_flags();
        let slot = tile_drop::insertion_slot(&pinned, x);
        match tile_drop::reorder(&pinned, from, slot) {
            Some((to, Some(_))) => workspaces.move_workspace(from, to),
            _ => false,
        }
    }

    /// Records the page area's size (see `area`). The caller syncs the window.
    fn set_area(&self, width: f32, height: f32) {
        self.area.set((width, height));
    }

    /// The split rows of a page tab menu: enabled when the focused group has two tabs (so one can move out) and the layout
    /// is below its cap. A phone has no split rows (it keeps one group).
    fn split_rows(&self, phone: bool) -> Option<bool> {
        if phone {
            return None;
        }
        let workspaces = self.workspaces.borrow();
        let groups = workspaces.active_tiles().map_or(0, TileLayout::group_count);
        let tabs = workspaces
            .active_pages()
            .map_or(0, |pages| pages.pages.len());
        Some(groups < GROUP_CAP && tabs >= 2)
    }

    /// The page tab menu rows for tab `index` of the focused group.
    fn page_menu_rows(&self, ui: &AppShell, index: usize) -> Vec<TabMenuRow> {
        let split = self.split_rows(ui.get_phone());
        self.view(|pages| pages.tab_menu_rows(index, split))
            .unwrap_or_default()
    }

    /// The workspace strip: one tab per workspace (its display name, pinned state and the "+" limit). Named after the active
    /// page, unless the user named it (see [`Workspaces::display_name`]).
    fn sync_workspaces(&self, ui: &AppShell) {
        self.schedule_save();
        let workspaces = self.workspaces.borrow();
        let rows: Vec<TabItem> = (0..workspaces.len())
            .map(|at| {
                let name = workspaces.display_name(at).unwrap_or_default();
                TabItem {
                    label: name.as_str().into(),
                    short_label: short_label(&name),
                    pinned: workspaces.get(at).is_some_and(|tab| tab.pinned()),
                }
            })
            .collect();
        replace_rows(&self.workspace_model, rows);
        ui.set_workspace_drop_mark(self.workspace_mark.get());
        ui.set_workspace_active(workspaces.active().map_or(-1, |at| at as i32));
        ui.set_workspace_pinned_count(workspaces.pinned_count() as i32);
        ui.set_workspace_can_add(!workspaces.is_full());
        ui.set_workspace_can_reopen(workspaces.closed_len() > 0);
    }
}

/// The rail area on screen (the index in `shell-area`).
fn shell_area_of(ui: &AppShell) -> Area {
    Area::ALL
        .get(ui.get_shell_area().max(0) as usize)
        .copied()
        .unwrap_or(Area::Home)
}

/// Keeps the window's rail area and sidebar selection in the active workspace, before the window leaves it (planned feature
/// B2: switching a workspace saves its area and selection, and [`restore_shell`] brings them back).
fn save_shell(ui: &AppShell, tabs: &Tabs) {
    let area = shell_area_of(ui);
    let selected = ui.get_sidebar_selected();
    let mut workspaces = tabs.workspaces.borrow_mut();
    if let Some(at) = workspaces.active()
        && let Some(tab) = workspaces.get_mut(at)
    {
        tab.set_area(area);
        tab.set_sidebar_selected(selected);
    }
}

/// Shows the active workspace on screen: its active page, or, when it has no page, its rail area with no row picked. The
/// sidebar selection the workspace kept is used when its page is in that area. With no workspace open the page area is
/// cleared (the window shows the empty state, see `AppShell.no-workspace`). The URL is replaced, not pushed.
fn restore_shell(ui: &AppShell, tabs: &Tabs) {
    let kept = {
        let workspaces = tabs.workspaces.borrow();
        workspaces
            .active()
            .and_then(|at| workspaces.get(at))
            .map(|tab| (tab.area(), tab.sidebar_selected()))
    };
    let Some((area, selected)) = kept else {
        clear_page(ui);
        tabs.sync_with(ui, UrlStep::Replace);
        return;
    };
    match tabs.route(tabs.active()) {
        Some(route) if show_route(ui, &route) => {
            let page_area = parse(&route).map(|matched| matched.page.area);
            if page_area == Some(area) {
                ui.set_sidebar_selected(selected);
            }
        }
        _ => {
            clear_page(ui);
            if area == Area::Chat {
                show_chat_area(ui);
            } else {
                show_area_tree(ui, area);
            }
        }
    }
    tabs.sync_with(ui, UrlStep::Replace);
}

/// Runs a change to the workspaces (a switch, a new or closed workspace, Reopen, Duplicate, pin, a bulk close) and brings
/// the window to the active workspace afterwards. Returns what `change` returned.
fn change_workspaces(
    ui: &AppShell,
    tabs: &Tabs,
    change: impl FnOnce(&mut Workspaces) -> bool,
) -> bool {
    save_shell(ui, tabs);
    let done = change(&mut tabs.workspaces.borrow_mut());
    restore_shell(ui, tabs);
    done
}

/// Opens a workspace on Home and makes it active (the "+" button, the empty state's button and Ctrl+Alt+T). Nothing happens at
/// the cap of 10.
fn new_workspace(ui: &AppShell, tabs: &Tabs) {
    change_workspaces(ui, tabs, |ws| ws.new_workspace().is_some());
}

/// Makes workspace `index` active. Returns false when there is no such workspace.
fn switch_workspace(ui: &AppShell, tabs: &Tabs, index: usize) -> bool {
    change_workspaces(ui, tabs, |ws| ws.activate(index))
}

/// Closes workspace `index` (pinned or not, no confirmation). Its contents go on the closed stack.
fn close_workspace(ui: &AppShell, tabs: &Tabs, index: usize) {
    change_workspaces(ui, tabs, |ws| ws.close(index));
}

/// Reopens the last closed workspace (the menu's Reopen, and Ctrl+Alt+Shift+W). Does nothing when the closed stack is empty.
fn reopen_workspace(ui: &AppShell, tabs: &Tabs) {
    change_workspaces(ui, tabs, |ws| ws.reopen().is_some());
}

/// Runs a workspace menu pick on workspace `index`. "Rename" opens the rename dialog; "Copy link" copies the link of the
/// workspace's active page.
fn workspace_menu_pick(ui: &AppShell, tabs: &Tabs, copier: &LinkCopier, index: usize, id: &str) {
    match id {
        "copy_link" => {
            if let Some(route) = tabs.workspace_route(index) {
                copier.copy(ui, &route);
            }
        }
        "close" => close_workspace(ui, tabs, index),
        "close_others" => {
            change_workspaces(ui, tabs, |ws| ws.close_others(index));
        }
        "close_left" => {
            change_workspaces(ui, tabs, |ws| ws.close_left(index));
        }
        "close_right" => {
            change_workspaces(ui, tabs, |ws| ws.close_right(index));
        }
        "close_all" => {
            change_workspaces(ui, tabs, |ws| {
                ws.close_all();
                true
            });
        }
        "duplicate" => {
            change_workspaces(ui, tabs, |ws| ws.duplicate(index));
        }
        "rename" => {
            let name = tabs
                .workspaces
                .borrow()
                .get(index)
                .and_then(|tab| tab.name())
                .unwrap_or("")
                .to_owned();
            ui.invoke_open_workspace_rename(index as i32, name.as_str().into());
        }
        "pin" => {
            change_workspaces(ui, tabs, |ws| {
                let pinned = ws.get(index).is_some_and(|tab| tab.pinned());
                ws.set_pinned(index, !pinned)
            });
        }
        "reopen" => reopen_workspace(ui, tabs),
        _ => {}
    }
}

/// Sets the name of workspace `index` from the rename dialog. An empty name clears it (the name becomes automatic).
fn rename_workspace(ui: &AppShell, tabs: &Tabs, index: usize, name: &str) {
    let renamed = tabs
        .workspaces
        .borrow_mut()
        .rename(index, Some(name.to_owned()));
    if renamed {
        tabs.sync_workspaces(ui);
    }
}

/// Runs the event loop of a window built with [`build_with_store`], until the window closes. The desktop app uses it after it
/// has attached the session driver (SA.4).
pub fn run_built(app: &App) -> Result<(), slint::PlatformError> {
    app.ui.run()
}

/// Opens the shell with `mode`, the workspaces loaded from `store`, and runs the event loop until the window closes. `start` is
/// the deep link to open (a concrete route, for example `/home/todos`), or `None` for no deep link.
pub fn run(
    start: Option<&str>,
    mode: ThemeMode,
    store: Rc<dyn Store>,
) -> Result<(), slint::PlatformError> {
    let app = build_with_store(start, mode, Rc::new(NoLocation), store)?;
    app.ui.run()
}

/// Builds the window with `start` as the start route, nothing saved (an in-memory store), and wires every callback. Does not
/// show the window or run the loop. The active route is not written to any address bar. For tests.
pub fn build(start: &str, mode: ThemeMode) -> Result<App, slint::PlatformError> {
    build_with_store(
        Some(start),
        mode,
        Rc::new(NoLocation),
        Rc::new(MemoryStore::default()),
    )
}

/// Builds the window, wires every callback and starts the toast timer. Does not show the window or run the loop.
///
/// The workspaces come from `store` (see [`startup`]). When the store holds a saved state, it comes back as it was, and a
/// deep link `start` that is not the active page opens as a new page tab in the focused group of the active workspace. When
/// nothing loads (missing, corrupt or unknown version), one workspace opens `start`, or Home when there is no `start`.
///
/// The active route is written to `location` (the web address bar, planned feature A3). A browser Back or Forward then
/// calls [`App::location_changed`]. Changes are saved about a second later (see [`SAVE_DELAY`]), and the window's close
/// request saves at once.
pub fn build_with_store(
    start: Option<&str>,
    mode: ThemeMode,
    location: Rc<dyn Location>,
    store: Rc<dyn Store>,
) -> Result<App, slint::PlatformError> {
    let ui = AppShell::new()?;
    // The page data slot and the Employees page. Wired before the start route opens, so its first load is not missed.
    let data: DataCell = Rc::default();
    let home = home_dashboard::wire(&ui, data.clone());
    let employees = employees::wire(&ui, data.clone());
    let profile = profile::wire(&ui, data.clone());
    let teams_page = teams::wire(&ui, data.clone());
    let team_page = team::wire(&ui, data.clone());
    let departments_page = departments::wire(&ui, data.clone());
    let locations_page = locations::wire(&ui, data.clone());
    let canvas = canvas_page::wire(&ui, data.clone());
    let canvas_lists = canvas_list::wire(&ui, data.clone());
    let admin_users = users::wire(&ui, data.clone());
    let admin_org = organisation::wire(&ui, data.clone());
    let admin_roles = roles::wire(&ui, data.clone());
    types_tags::wire(&ui, data.clone());
    space_area_management::settings::wire(&ui, data.clone());
    space_area_management::members::wire(&ui, data.clone());
    let overview = overview::wire(&ui, data.clone());
    let issues = issues::wire(&ui, data.clone());
    let new_issue = new_issue::wire(&ui, data.clone());
    let issue = issue::wire(&ui, data.clone(), issues.keys(), new_issue.clone());
    new_sprint::wire(&ui, data.clone());
    wire_projects(&ui, data.clone());
    wire_create_project(&ui, data.clone());
    // The page tab menu is a ContextMenu, which reads its rows through the MenuNav callbacks.
    crate::slint_ui::glue::menu::wire_nav(&ui.global::<crate::slint_ui::MenuNav>());
    // Every Select (pickers on the admin pages and forms) reads its label and its keyboard search through SelectNav.
    crate::slint_ui::glue::select::wire_nav(&ui.global::<crate::slint_ui::SelectNav>());
    // The ColorPicker's Hex field parses its text through ColorHex (every ColorPicker and ColorField in the shell).
    crate::slint_ui::glue::color::wire_hex(&ui.global::<crate::slint_ui::ColorHex>());
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    slint_theme::set_mode(&ui, mode.into());
    ui.set_theme_label(theme_label(mode).into());
    ui.set_theme_icon(theme_icon(&ui.global::<Icons>(), mode));

    let rail: Vec<RailItem> = Area::ALL
        .iter()
        .map(|area| RailItem {
            label: area.title().into(),
            icon: icon(&ui.global::<Icons>(), area.icon()),
        })
        .collect();
    ui.set_rail_items(ModelRc::new(VecModel::from(rail)));

    let mut tabs: Vec<RailItem> = PHONE_TABS
        .iter()
        .map(|area| RailItem {
            label: area.title().into(),
            icon: icon(&ui.global::<Icons>(), area.icon()),
        })
        .collect();
    tabs.push(RailItem {
        label: MORE_LABEL.into(),
        icon: icon(&ui.global::<Icons>(), MORE_ICON),
    });
    ui.set_tab_items(ModelRc::new(VecModel::from(tabs)));

    let more: Vec<RailItem> = more_areas()
        .iter()
        .map(|area| RailItem {
            label: area.title().into(),
            icon: icon(&ui.global::<Icons>(), area.icon()),
        })
        .collect();
    ui.set_more_areas(ModelRc::new(VecModel::from(more)));

    ui.set_mac_keys(cfg!(target_os = "macos"));
    ui.set_page_menu_items(tab_menu_items(&page_menu_rows()));
    ui.set_project_choices(project_choices(""));
    refresh_quick_categories(&ui);

    let (workspaces, saver, loaded) = startup(store);
    let open = Tabs::new(&ui, location, workspaces, saver);
    // The start is a landing: the address bar is written once, at the end (see `write_location`), not per step.
    open.landing.set(true);
    if loaded {
        // The saved workspaces come back as they were. A deep link opens as a new page tab, unless it is the active page.
        restore_shell(&ui, &open);
        if let Some(route) = start
            && open.route(open.active()).as_deref() != Some(route)
        {
            open_in_new_tab(&ui, &open, route);
        }
    } else if !start.is_some_and(|route| open_tab(&ui, &open, route)) {
        open_tab(&ui, &open, FALLBACK_ROUTE);
    }
    open.landing.set(false);
    open.write_location();

    let mode = Rc::new(Cell::new(mode));
    let queue = Rc::new(RefCell::new(ToastQueue::new()));
    let toasts = Rc::new(VecModel::<ToastEntry>::default());
    ui.set_toasts(ModelRc::new(toasts.clone()));

    let copier = LinkCopier {
        queue: queue.clone(),
        toasts: toasts.clone(),
    };
    wire_navigation(&ui, open.clone(), copier.clone());
    wire_workspaces(&ui, open.clone(), copier.clone());
    wire_palette(&ui, open.clone());
    wire_theme(&ui, mode);
    wire_language(&ui);
    wire_toasts(&ui, queue.clone(), toasts.clone());
    let last_quick_pick = wire_quick_action(&ui);

    // The exit save: closing the window saves the workspaces at once, then the window hides as it did before.
    let exit_tabs = Rc::downgrade(&open);
    ui.window().on_close_requested(move || {
        if let Some(tabs) = exit_tabs.upgrade() {
            tabs.flush();
        }
        CloseRequestResponse::HideWindow
    });

    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        Duration::from_millis(TICK_MS),
        move || {
            let mut queue = queue.borrow_mut();
            // Toasts that pages announced since the last tick (toast::announce) join the queue first.
            for (tone, title) in take_announced() {
                queue.show(Toast::new(tone, title, "Close"));
            }
            queue.tick(TICK_MS as u32);
            sync_toasts(&queue, &toasts);
        },
    );

    // The status bar clock (bottom right): local time as HH:MM, set now and then once a second.
    let clock_weak = ui.as_weak();
    ui.set_clock_text(clock_text().into());
    let clock = Timer::default();
    clock.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        if let Some(ui) = clock_weak.upgrade() {
            ui.set_clock_text(clock_text().into());
        }
    });

    // Content (CMS.6): the content pages wire for their route when they open. `web` holds the native web views of the editor's
    // web-only blocks, which no app supplies yet (UNSURE: the attach needs the desktop window handle, see webviews.rs).
    let web = Rc::new(RefCell::new(WebHost::new(ui.as_weak())));
    let content_cx = Rc::new(content::Context {
        data: data.clone(),
        web: web.clone(),
    });
    ui.on_content_kind(|route| content::kind(route.as_str()).into());
    let content_weak = ui.as_weak();
    let content_open = content_cx.clone();
    ui.on_content_opened(move |route| {
        if let Some(ui) = content_weak.upgrade() {
            content::open(&ui, &content_open, route.as_str());
        }
    });
    let web_tick = web.clone();
    let web_timer = Timer::default();
    web_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        web_tick.borrow_mut().tick();
    });

    // Interface zoom: every slider step and every shortcut applies at once. `zoom-applied` is what the token layer reads.
    let zoom_weak = ui.as_weak();
    ui.on_zoom_changed(move |value| {
        if let Some(ui) = zoom_weak.upgrade() {
            ui.set_zoom_applied(value);
        }
    });

    Ok(App {
        ui,
        last_quick_pick,
        data,
        home,
        employees,
        profile,
        teams: teams_page,
        team: team_page,
        departments: departments_page,
        locations: locations_page,
        canvas,
        canvas_list: canvas_lists,
        admin_users,
        admin_org,
        admin_roles,
        overview,
        issues,
        issue,
        new_issue,
        tabs: open,
        _timer: timer,
        _web_timer: web_timer,
        _clock: clock,
    })
}

/// The breadcrumbs of `page` at `route`: its area, then its section (a project page's middle crumb is the project's name,
/// `Projects > Grand Tale > Issues`), then the page itself.
fn page_crumbs(page: &Page, route: &str) -> Vec<SharedString> {
    let mut crumbs: Vec<SharedString> = vec![page.area.title().into()];
    match project_name_crumb(page.route, route) {
        Some(name) => crumbs.push(name.into()),
        None => {
            if let Some(section) = page.section_info() {
                crumbs.push(section.title.into());
            }
        }
    }
    crumbs.push(page.title.into());
    crumbs
}

/// The page header of one tile group: the breadcrumbs, title and inspector flag of its active page, the values
/// `show_route` sets for the focused page. `None` when `route` is not a known page.
struct PageHeader {
    crumbs: Vec<SharedString>,
    title: SharedString,
    inspector: bool,
}

fn page_header(route: &str) -> Option<PageHeader> {
    let matched = parse(route)?;
    let page = matched.page;
    Some(PageHeader {
        crumbs: page_crumbs(page, route),
        title: page.title.into(),
        inspector: matches!(
            page.template(),
            Template::Detail | Template::Split | Template::Board
        ),
    })
}

/// Shows the page at `route`: the rail area, the sidebar of that area with its row selected, the breadcrumbs, and the
/// placeholder. Returns false when `route` is not a known page, and then changes nothing.
pub fn show_route(ui: &AppShell, route: &str) -> bool {
    let Some(matched) = parse(route) else {
        return false;
    };
    let page = matched.page;
    let area_index = Area::ALL.iter().position(|a| *a == page.area).unwrap_or(0);
    let area = Area::ALL[area_index];
    // The Chat area has the Teams-style tree (SF.4 scaffold); the other areas keep the flat manifest rows.
    let (rows, selected) = if area == Area::Chat {
        let tree = chat_sidebar::rows(&ui.global::<Icons>(), route);
        (tree.items, tree.selected)
    } else {
        // `sidebar_rows` fills each page row's icon; `sidebar_entries` alone leaves every icon empty.
        (
            sidebar_rows(&ui.global::<Icons>(), area),
            shell_selected(area, route),
        )
    };

    let crumbs = page_crumbs(page, route);

    ui.set_shell_area(area_index as i32);
    ui.set_tab_selected(
        PHONE_TABS
            .iter()
            .position(|a| *a == area)
            .map_or(-1, |index| index as i32),
    );
    set_sidebar_header(ui, area);
    ui.set_sidebar_icon(icon(&ui.global::<Icons>(), area.icon()));
    ui.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_sidebar_selected(selected);
    ui.set_page_crumbs(ModelRc::new(VecModel::from(crumbs)));
    ui.set_page_title(page.title.into());
    ui.set_page_route(route.into());
    ui.set_page_icon(icon(&ui.global::<Icons>(), page.icon()));
    ui.set_share_items(tab_menu_items(&share_rows(page.route)));
    ui.set_primary_action_label(primary_action_label(page).unwrap_or_default().into());
    ui.set_inspector_available(matches!(
        page.template(),
        Template::Detail | Template::Split | Template::Board
    ));
    set_overlay(ui, None);
    true
}

/// The key of a project route (`/p/<key>/...`), or `None` for any other route.
fn project_key(route: &str) -> Option<&str> {
    route
        .strip_prefix("/p/")?
        .split('/')
        .next()
        .filter(|key| !key.is_empty())
}

/// The middle breadcrumb of a project page: the name of the project the route is for. `None` for a page that is not a
/// project page, or whose project is not in the list (the section's title is shown then).
fn project_name_crumb(page_route: &str, route: &str) -> Option<String> {
    if !page_route.starts_with("/p/") {
        return None;
    }
    let name = projects::name_of(project_key(route)?);
    (!name.is_empty()).then_some(name)
}

/// The label of a page's primary action (OQ21): the quick action item whose id is the page's `primary_action`, so the
/// button opens the same dialog as the quick action. `None` when the page has no primary action (every page today).
pub fn primary_action_label(page: &Page) -> Option<String> {
    let id = page.primary_action?;
    let label = quick::categories()
        .iter()
        .flat_map(|category| category.items.iter())
        .find(|item| item.id == id)
        .map_or(id, |item| item.label);
    Some(label.to_owned())
}

/// The public link pages (OQ20): docs, files and roadmaps (canvases are not public, A7). Their Share menu has the public link row.
const PUBLIC_LINK_PAGES: [&str; 4] = [
    "/p/:key/docs",
    "/p/:key/docs/:page",
    "/p/:key/files",
    "/p/:key/roadmap",
];

/// The rows of the Share menu for the page whose manifest route is `pattern` (OQ20). "Copy link" is on every page and
/// enabled (gaps D2). The public link row is on the public link pages, still disabled (not in scope).
fn share_rows(pattern: &str) -> Vec<TabMenuRow> {
    let mut rows = vec![action("copy_link", "Copy link", true)];
    if PUBLIC_LINK_PAGES.contains(&pattern) {
        rows.push(rule("share-rule"));
        rows.push(reason(
            "public_link",
            "Create public link",
            "Not available yet",
        ));
    }
    rows
}

/// The rows of the page overflow menu (IA section 6). They reuse the tab operations: a new tab on the same page, and the
/// tab duplicate.
fn page_menu_rows() -> Vec<TabMenuRow> {
    vec![
        action("open_new_tab", "Open in new tab", true),
        action("duplicate", "Duplicate tab", true),
    ]
}

/// A disabled row of a menu that shows its reason in the shortcut column (the Share menu's rows that are not built yet).
fn reason(id: &'static str, label: &str, why: &str) -> TabMenuRow {
    TabMenuRow {
        id,
        label: label.to_owned(),
        shortcut: why.to_owned(),
        enabled: false,
        separator: false,
    }
}

/// A shell screen that replaces the page: About, Report Issue or Settings.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    About,
    Feedback,
    Report,
    Settings,
}

/// Shows one shell screen over the page, or none (`None` returns to the page). Only one is open at a time.
fn set_overlay(ui: &AppShell, overlay: Option<Overlay>) {
    ui.set_about_open(overlay == Some(Overlay::About));
    ui.set_feedback_open(overlay == Some(Overlay::Feedback));
    ui.set_report_open(overlay == Some(Overlay::Report));
    ui.set_settings_open(overlay == Some(Overlay::Settings));
}

/// True while a shell screen (About, Give Feedback, Report Issue or Settings) is open over the page.
fn overlay_screen_open(ui: &AppShell) -> bool {
    ui.get_about_open() || ui.get_feedback_open() || ui.get_report_open() || ui.get_settings_open()
}

/// Shows `route` in the active page tab: the route replaces the tab's route and is pushed onto its history (IA section 6,
/// P1). Used by a plain pick: a sidebar row, a rail area, the palette, a breadcrumb and the phone tabs. Returns false when
/// `route` is not a known page, and then changes nothing.
fn navigate_to(ui: &AppShell, tabs: &Tabs, route: &str) -> bool {
    let Some(matched) = parse(route) else {
        return false;
    };
    if !show_route(ui, route) {
        return false;
    }
    tabs.navigate(route, matched.page.title);
    tabs.sync(ui);
    true
}

/// Shows `route` in a new page tab, which becomes the active one (Ctrl+click, Cmd+click or a middle-button click). Returns
/// false when `route` is not a known page, and then changes nothing.
fn open_in_new_tab(ui: &AppShell, tabs: &Tabs, route: &str) -> bool {
    let Some(matched) = parse(route) else {
        return false;
    };
    if !show_route(ui, route) {
        return false;
    }
    tabs.open_new_tab(route, matched.page.title);
    tabs.sync(ui);
    true
}

/// Opens the start route (or the fallback) as the first page tab, or activates the tab that already shows it. Returns false
/// when `route` is not a known page, and then changes nothing.
fn open_tab(ui: &AppShell, tabs: &Tabs, route: &str) -> bool {
    let Some(matched) = parse(route) else {
        return false;
    };
    if !show_route(ui, route) {
        return false;
    }
    tabs.open(route, matched.page.title);
    tabs.sync(ui);
    true
}

/// Shows the active tab after a close or a bulk change: its page, or the empty state when no tab is left. Then the strip is
/// brought in line.
fn show_active(ui: &AppShell, tabs: &Tabs) {
    if tabs.is_empty() {
        clear_page(ui);
    } else {
        show_tab(ui, tabs, tabs.active());
    }
    tabs.sync(ui);
}

/// The Copy link action (gaps D2): writes a page's web link to the clipboard, then shows the toast for the outcome ("Link
/// copied", or "Could not copy the link" when the browser refused). It holds the toast queue and its model, the same ones the
/// toast timer ticks.
#[derive(Clone)]
pub(crate) struct LinkCopier {
    queue: Rc<RefCell<ToastQueue>>,
    toasts: Rc<VecModel<ToastEntry>>,
}

impl LinkCopier {
    /// Copies the web link of `route` (`history::link_for`). The toast follows the outcome: at once on desktop and Android,
    /// and when the browser answers on the web.
    fn copy(&self, ui: &AppShell, route: &str) {
        let copier = self.clone();
        clipboard::write_link(ui, &history::link_for(route), move |copied| {
            copier.report(copied);
        });
    }

    /// Shows the toast for a copy outcome. No queue borrow is held by the caller.
    fn report(&self, copied: bool) {
        let (tone, title) = clipboard::copy_toast(copied);
        let mut queue = self.queue.borrow_mut();
        queue.show(Toast::new(tone, title, "Close"));
        sync_toasts(&queue, &self.toasts);
    }
}

/// Runs a page tab menu pick on tab `index`. "Copy link" copies that tab's route as a web link.
fn tab_menu_pick(ui: &AppShell, tabs: &Tabs, copier: &LinkCopier, index: usize, id: &str) {
    match id {
        "copy_link" => {
            if let Some(route) = tabs.route(index) {
                copier.copy(ui, &route);
            }
        }
        "close" => {
            tabs.close(index);
            show_active(ui, tabs);
        }
        "close_others" => {
            tabs.edit(|pages| pages.close_others(index));
            show_active(ui, tabs);
        }
        "close_left" => {
            tabs.edit(|pages| pages.close_left(index));
            show_active(ui, tabs);
        }
        "close_right" => {
            tabs.edit(|pages| pages.close_right(index));
            show_active(ui, tabs);
        }
        "close_all" => {
            tabs.edit(|pages| pages.close_all());
            show_active(ui, tabs);
        }
        "duplicate" => {
            let done = tabs.edit(|pages| pages.duplicate(index)) == Some(true);
            if done {
                show_active(ui, tabs);
            }
        }
        "split_right" => {
            if tabs.split(index, Edge::Right) {
                show_active(ui, tabs);
            }
        }
        "split_down" => {
            if tabs.split(index, Edge::Bottom) {
                show_active(ui, tabs);
            }
        }
        "pin" => {
            tabs.edit(|pages| {
                let pinned = pages.pages.get(index).is_some_and(|tab| tab.pinned);
                pages.set_pinned(index, !pinned)
            });
            tabs.sync(ui);
        }
        "reopen" => reopen_tab(ui, tabs),
        _ => {}
    }
}

/// Reopens the last closed tab (the menu's Reopen, and Ctrl+Alt+Shift+T). Does nothing when the closed stack is empty.
fn reopen_tab(ui: &AppShell, tabs: &Tabs) {
    let reopened = tabs.edit(|pages| pages.reopen()).flatten();
    if reopened.is_some() {
        show_active(ui, tabs);
    }
}

/// Goes back one step in the active page tab's history and shows that route (the top bar button, Alt+Left, Ctrl+[ and
/// mouse button 4). Does nothing when there is no step back.
fn go_back(ui: &AppShell, tabs: &Tabs) {
    if let Some(route) = tabs.back() {
        show_route(ui, &route);
        tabs.sync_with(ui, UrlStep::Replace);
    }
}

/// Goes forward one step in the active page tab's history and shows that route (the top bar button, Alt+Right, Ctrl+] and
/// mouse button 5). Does nothing when there is no step forward.
fn go_forward(ui: &AppShell, tabs: &Tabs) {
    if let Some(route) = tabs.forward() {
        show_route(ui, &route);
        tabs.sync_with(ui, UrlStep::Replace);
    }
}

/// A sidebar row pick at `index` in the active area. A plain pick replaces the active tab's route; `new_tab` opens the row in
/// a new tab. A Chat row takes its route from the Chat tree.
fn sidebar_pick(ui: &AppShell, tabs: &Tabs, index: i32, new_tab: bool) {
    let area = Area::ALL
        .get(ui.get_shell_area().max(0) as usize)
        .copied()
        .unwrap_or(Area::Home);
    let row = usize::try_from(index).unwrap_or(0);
    let route = if area == Area::Chat {
        match chat_sidebar::action(row) {
            Some(Action::Open(route)) => Some(route),
            _ => None,
        }
    } else {
        sidebar_entries(area)
            .into_iter()
            .nth(row)
            .and_then(|entry| entry.route)
    };
    let Some(route) = route else {
        return;
    };
    if new_tab {
        open_in_new_tab(ui, tabs, &route);
    } else {
        navigate_to(ui, tabs, &route);
    }
}

/// Activates tab `index` and shows its page.
fn show_tab(ui: &AppShell, tabs: &Tabs, index: usize) {
    let Some(route) = tabs.route(index) else {
        return;
    };
    if show_route(ui, &route) {
        tabs.select(index);
        tabs.sync(ui);
    }
}

/// Focuses tile group `group` (a click in it, or a press or a right press on one of its tabs). When the focus moves, the page
/// header, the sidebar and the address bar follow the group's active page, since the shared controls act on the focused
/// group. Returns false when there is no such group.
fn focus_group(ui: &AppShell, tabs: &Tabs, group: i32) -> bool {
    let Ok(id) = u32::try_from(group) else {
        return false;
    };
    if !tabs.focus(id) {
        return false;
    }
    show_active(ui, tabs);
    true
}

/// Clears the page header for the empty state (no open tab). The rail and sidebar stay as they are,
/// with no sidebar row selected.
fn clear_page(ui: &AppShell) {
    ui.set_page_crumbs(ModelRc::new(VecModel::<SharedString>::default()));
    ui.set_page_title("".into());
    ui.set_page_route("".into());
    ui.set_share_items(ModelRc::new(VecModel::<MenuItem>::default()));
    ui.set_primary_action_label("".into());
    ui.set_inspector_available(false);
    ui.set_sidebar_selected(-1);
    set_overlay(ui, None);
}

/// Shows the Chat area with its tree and no page (used while no Chat route parses). The rail and the sidebar change; the
/// page area clears.
fn show_chat_area(ui: &AppShell) {
    let area_index = Area::ALL.iter().position(|a| *a == Area::Chat).unwrap_or(0);
    let tree = chat_sidebar::rows(&ui.global::<Icons>(), "");
    ui.set_shell_area(area_index as i32);
    set_sidebar_header(ui, Area::Chat);
    ui.set_sidebar_icon(icon(&ui.global::<Icons>(), Area::Chat.icon()));
    ui.set_sidebar_rows(ModelRc::new(VecModel::from(tree.items)));
    clear_page(ui);
    ui.set_sidebar_selected(-1);
}

/// Shows `area`'s tree in the sidebar (the phone drawer, or the desktop sidebar) with no row selected and no page change.
/// The area becomes the active one, so a row pick opens its page.
fn show_area_tree(ui: &AppShell, area: Area) {
    let area_index = Area::ALL.iter().position(|a| *a == area).unwrap_or(0);
    ui.set_shell_area(area_index as i32);
    ui.set_tab_selected(-1);
    set_sidebar_header(ui, area);
    ui.set_sidebar_icon(icon(&ui.global::<Icons>(), area.icon()));
    ui.set_sidebar_rows(ModelRc::new(VecModel::from(sidebar_rows(
        &ui.global::<Icons>(),
        area,
    ))));
    ui.set_sidebar_selected(-1);
}

/// The sidebar's header for `area` (tail C3; IA section 3): the area's title, or on Projects the current project's name with
/// the project switcher on. The current project's key goes with it, for the switcher's mark.
fn set_sidebar_header(ui: &AppShell, area: Area) {
    let switcher = area == Area::Projects;
    ui.set_sidebar_switcher(switcher);
    let title = if switcher {
        projects::current_name()
    } else {
        area.title().to_owned()
    };
    ui.set_sidebar_title(title.into());
    ui.set_project_key(projects::current_key().into());
    set_current_project_picture(ui);
}

/// The header's picture: the current project's, drawn left of its name in the switcher (empty when it has none).
fn set_current_project_picture(ui: &AppShell) {
    let picture = projects::picture(&projects::current_key());
    ui.set_project_has_image(picture.is_some());
    ui.set_project_image(picture.unwrap_or_default());
    ui.set_project_initials(space_page_kit::person::initials(&projects::current_name()).into());
}

/// Draws the pictures that just arrived: each switcher row takes its picture (the rows keep the filter they have), and the
/// header takes the current project's.
fn refresh_project_pictures(ui: &AppShell) {
    let rows: Vec<ProjectChoice> = ui
        .get_project_choices()
        .iter()
        .map(|row| {
            let picture = projects::picture(&row.key);
            ProjectChoice {
                key: row.key.clone(),
                name: row.name.clone(),
                has_image: picture.is_some(),
                image: picture.unwrap_or_default(),
                initials: space_page_kit::person::initials(&row.name).into(),
            }
        })
        .collect();
    ui.set_project_choices(ModelRc::new(VecModel::from(rows)));
    set_current_project_picture(ui);
}

/// Starts the loads of the project pictures that are not cached yet. Each one, when it arrives, draws the switcher again.
fn fetch_project_pictures(source: &Arc<DataSource>, ui: &AppShell) {
    for path in projects::take_pictures_to_fetch() {
        let key = path.clone();
        source.fetch(
            &ui.as_weak(),
            move |api, token| async move {
                let bytes = api.avatar(&token, &path).await?;
                page_data::decode_picture(&bytes)
            },
            move |ui, result| {
                projects::picture_loaded(&key, result.ok());
                refresh_project_pictures(ui);
            },
        );
    }
}

/// The project list (S1.3, decision 0030): `GET /v1/projects` replaces the list the Projects sidebar and the switcher show.
/// The session driver asks for it when the person signs in. A failed load keeps the list the window has.
/// The Create New Project dialog: the switcher's row opens it empty, Save validates and creates the project, and a
/// successful create closes it and refreshes the project list.
fn wire_create_project(ui: &AppShell, data: DataCell) {
    let weak = ui.as_weak();
    let open = weak.clone();
    ui.set_create_project_picture_supported(PHOTO_NATIVE);
    ui.on_project_create_requested(move || {
        if let Some(ui) = open.upgrade() {
            ui.set_create_project_key("".into());
            ui.set_create_project_name("".into());
            ui.set_create_project_description("".into());
            ui.set_create_project_error("".into());
            clear_picture(&ui);
            ui.set_create_project_show(true);
        }
    });
    let choose = weak.clone();
    let pick_data = data.clone();
    ui.on_create_project_choose_picture(move || {
        create_project::pick_picture(&pick_data, &choose, |ui, result| match result {
            Ok(create_project::PickOutcome::Cancelled) => {}
            Ok(create_project::PickOutcome::Rejected(message)) => {
                ui.set_create_project_error(message.into());
            }
            Ok(create_project::PickOutcome::Chosen(picked)) => {
                ui.set_create_project_error("".into());
                ui.set_create_project_picture_name(picked.file_name.as_str().into());
                ui.set_create_project_picture_preview(page_data::picture(&picked.preview));
                ui.set_create_project_picture_has_preview(true);
                create_project::set_picked(Some(*picked));
            }
            Err(error) => ui.set_create_project_error(create_project::error_text(&error).into()),
        });
    });
    let save = weak.clone();
    ui.on_create_project_save(move |key, name, description| {
        let Some(ui) = save.upgrade() else {
            return;
        };
        match create_project::validate(&key, &name, &description) {
            Err(message) => ui.set_create_project_error(message.into()),
            Ok(body) => {
                ui.set_create_project_error("".into());
                ui.set_create_project_creating(true);
                let picture = create_project::take_picked();
                create_project::save(&data, &save, body, picture, |ui, result| {
                    ui.set_create_project_creating(false);
                    match result {
                        Ok(created) => {
                            ui.invoke_projects_refresh();
                            match created.picture_error {
                                None => ui.set_create_project_show(false),
                                // The project exists, so the dialog stays open with the picture's error.
                                Some(message) => {
                                    clear_picture(ui);
                                    ui.set_create_project_error(
                                        format!(
                                            "The project {} was created, but its picture did not upload: {message}",
                                            created.project.key
                                        )
                                        .into(),
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            ui.set_create_project_error(create_project::error_text(&error).into());
                        }
                    }
                });
            }
        }
    });
    let cancel = weak;
    ui.on_create_project_cancel(move || {
        if let Some(ui) = cancel.upgrade() {
            clear_picture(&ui);
            ui.set_create_project_show(false);
        }
    });
}

/// Forgets the picture chosen in the create dialog and clears its row.
fn clear_picture(ui: &AppShell) {
    create_project::set_picked(None);
    ui.set_create_project_picture_name("".into());
    ui.set_create_project_picture_has_preview(false);
}

fn wire_projects(ui: &AppShell, data: DataCell) {
    let weak = ui.as_weak();
    ui.on_projects_refresh(move || {
        let Some(source) = data.borrow().clone() else {
            return;
        };
        let pictures_source = source.clone();
        source.fetch(
            &weak,
            |api, token| async move { api.projects(&token).await },
            move |ui, result| {
                if let Ok(summaries) = result {
                    let editable: Vec<String> = summaries
                        .iter()
                        .filter(|project| can_create(project.my_role.as_deref()))
                        .map(|project| project.key.clone())
                        .collect();
                    projects::set_picture_paths(&summaries);
                    projects::set_list(projects::from_summaries(summaries));
                    projects::set_editable(editable);
                    ui.set_project_choices(project_choices(""));
                    refresh_project_pictures(ui);
                    fetch_project_pictures(&pictures_source, ui);
                    refresh_projects_sidebar(ui);
                    refresh_quick_categories(ui);
                }
            },
        );
    });
}

/// Draws the Projects sidebar again when it is the active area, so its rows and header follow the list.
fn refresh_projects_sidebar(ui: &AppShell) {
    let active = usize::try_from(ui.get_shell_area())
        .ok()
        .and_then(|index| Area::ALL.get(index).copied());
    if active == Some(Area::Projects) {
        set_sidebar_header(ui, Area::Projects);
        ui.set_sidebar_rows(ModelRc::new(VecModel::from(sidebar_rows(
            &ui.global::<Icons>(),
            Area::Projects,
        ))));
    }
}

/// The rows of the project switcher: every project whose name or key matches `query` (tail C3, OQ9).
fn project_choices(query: &str) -> ModelRc<ProjectChoice> {
    let rows: Vec<ProjectChoice> = projects::filter(query)
        .into_iter()
        .map(|project| {
            let picture = projects::picture(&project.key);
            ProjectChoice {
                key: project.key.into(),
                initials: space_page_kit::person::initials(&project.name).into(),
                name: project.name.into(),
                has_image: picture.is_some(),
                image: picture.unwrap_or_default(),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// Makes project `key` the current one (tail C3). Every Projects row takes the new key. An active project page moves to the
/// same page of the new project, as a new step in the history. Any other active page keeps its route, and the sidebar is
/// drawn again for the new key. An unknown key changes nothing.
fn switch_project(ui: &AppShell, tabs: &Tabs, key: &str) {
    if !projects::set_current(key) {
        return;
    }
    let route = tabs.route(tabs.active());
    let moved = route.as_deref().and_then(parse).and_then(|matched| {
        if !matched.page.route.starts_with("/p/") {
            return None;
        }
        let params: Vec<(String, String)> = matched
            .params
            .iter()
            .map(|(name, value)| {
                if name == "key" {
                    (name.clone(), key.to_owned())
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect();
        Some(format_view_route(
            matched.page.route,
            &params,
            matched.view.as_deref(),
        ))
    });
    match (moved, route) {
        (Some(moved), _) => {
            navigate_to(ui, tabs, &moved);
        }
        (None, Some(route)) => {
            show_route(ui, &route);
        }
        (None, None) => {}
    }
    tabs.sync(ui);
}

/// The index of the sidebar row of `route` in `area`, or -1 when no row shows it (its page is missing or its section is
/// collapsed).
fn shell_selected(area: Area, route: &str) -> i32 {
    sidebar_entries(area)
        .iter()
        .position(|entry| entry.route.as_deref() == Some(route))
        .map_or(-1, |index| index as i32)
}

/// The first page of `area` that has a route, or `None` when the area has no listed page.
fn first_route(area: Area) -> Option<String> {
    sidebar_entries(area)
        .into_iter()
        .find_map(|entry| entry.route)
}

/// The first listed page of section `section` in `area`, for a breadcrumb click on the section.
fn section_route(area: Area, section: &str) -> Option<String> {
    pages_in(area)
        .find(|page| page.section == section && (!page.has_params() || page.only_key_param()))
        .map(listed_route)
}

fn wire_navigation(ui: &AppShell, tabs: Rc<Tabs>, copier: LinkCopier) {
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_rail_activated(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let area = Area::ALL
            .get(index.max(0) as usize)
            .copied()
            .unwrap_or(Area::Home);
        if let Some(route) = first_route(area) {
            navigate_to(&ui, &open, &route);
        } else if area == Area::Chat {
            // No Chat route parses yet, so show the Chat tree without a page.
            show_chat_area(&ui);
        }
    });

    // Canvas routes (CV.6): the window asks which routes are canvases, and a canvas (a list row, or a new one) opens in the
    // same page tab.
    ui.on_is_canvas_route(|route| canvas_page::canvas_id_of(route.as_str()).is_some());
    ui.on_is_canvas_list_route(|route| canvas_list::list_scope_of(route.as_str()).is_some());
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_canvas_open_route(move |route| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, route.as_str());
        }
    });

    // Closes the focused group's active page tab, as its close button does (CV.6b, the canvas menu's Close canvas).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_canvas_close_tab(move || {
        let Some(ui) = weak.upgrade() else { return };
        if open.close(open.active()) {
            show_active(&ui, &open);
        }
    });

    // An issue (a row of Projects > Issues, or a new one) opens in the same page tab (S1.3).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_issues_open(move |route| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, route.as_str());
        }
    });

    // A row of the Dashboard (an issue or a canvas) opens in the same page tab, and the canvases card's View all opens the
    // canvases list (S2).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.global::<HomeState>().on_open(move |route| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, route.as_str());
        }
    });
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.global::<HomeState>().on_view_all_canvases(move || {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, home_dashboard::CANVASES_ROUTE);
        }
    });

    // A row of Company > Employees opens that person's profile in the same page tab (SP.3b).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_employees_open(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &employees::person_route(&id));
        }
    });

    // A manager or direct report on a profile opens that person's profile in the same page tab (SP.3c).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_profile_open_person(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &employees::person_route(&id));
        }
    });

    // The avatar menu's Profile row opens the signed-in person's own profile (SP.3c). Nothing opens before sign-in.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_profile_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            let id = ui.get_user_id();
            if !id.is_empty() {
                navigate_to(&ui, &open, &employees::person_route(&id));
            }
        }
    });

    // Content pages (CMS.6) ask the shell to open another route (a link to a page, a review row). It opens in the active page tab.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_navigate_requested(move |route| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, route.as_str());
        }
    });

    // Company > Teams (SP.3d): a team row or a department's team opens the team's page; a person opens their profile. Each opens in
    // the same page tab, like the employees list.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_teams_open_team(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &teams::team_route(&id));
        }
    });
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_dept_open_team(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &teams::team_route(&id));
        }
    });
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_team_open_person(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &employees::person_route(&id));
        }
    });
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_teams_open_person(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &employees::person_route(&id));
        }
    });
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_dept_open_person(move |id| {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, &employees::person_route(&id));
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_sidebar_activated(move |index| {
        if let Some(ui) = weak.upgrade() {
            sidebar_pick(&ui, &open, index, false);
        }
    });

    // Ctrl+click, Cmd+click or a middle-button click on a row: the row opens in a new tab (P1).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_sidebar_opened_in_new_tab(move |index| {
        if let Some(ui) = weak.upgrade() {
            sidebar_pick(&ui, &open, index, true);
        }
    });

    // Back and forward in the active page tab: the top bar buttons, the shortcuts and mouse buttons 4 and 5.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_back_requested(move || {
        if let Some(ui) = weak.upgrade() {
            go_back(&ui, &open);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_forward_requested(move || {
        if let Some(ui) = weak.upgrade() {
            go_forward(&ui, &open);
        }
    });

    // The page overflow menu (IA section 6) acts on the focused group's active tab: a new tab on its page, or its duplicate.
    let weak = ui.as_weak();
    let open = tabs.clone();
    let menu_copier = copier.clone();
    ui.on_page_menu_picked(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let index = open.active();
        match id.as_str() {
            "open_new_tab" => {
                if let Some(route) = open.route(index) {
                    open_in_new_tab(&ui, &open, &route);
                }
            }
            "duplicate" => tab_menu_pick(&ui, &open, &menu_copier, index, "duplicate"),
            _ => {}
        }
    });

    // The Share menu (OQ20, gaps D2): "Copy link" copies the focused tab's page link. Other rows are disabled.
    let weak = ui.as_weak();
    let open = tabs.clone();
    let share_copier = copier.clone();
    ui.on_share_menu_picked(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        if id.as_str() == "copy_link"
            && let Some(route) = open.route(open.active())
        {
            share_copier.copy(&ui, &route);
        }
    });

    // The Android system Back key (A3, gaps D7). The Slint key handler first closes the topmost Slint overlay
    // (`close-topmost-overlay`). Then this closes a shell screen (About, Report Issue, Settings), and otherwise steps the
    // active tab's history back. With none of those left it returns false, so the key goes to the system and the activity
    // finishes (the Slint Android backend does that for an unaccepted Back key).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_system_back(move || {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        if overlay_screen_open(&ui) {
            set_overlay(&ui, None);
            return true;
        }
        let can_back = open.view(|pages| pages.can_back()).unwrap_or(false);
        match history::back_key(can_back) {
            history::BackKey::Step => {
                go_back(&ui, &open);
                true
            }
            history::BackKey::System => false,
        }
    });

    // A collapsible heading or sub-row: flip its state and rebuild the rows. Chat has its own tree (its keys); every other
    // area collapses by section.
    let weak = ui.as_weak();
    ui.on_sidebar_toggled(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let area = Area::ALL
            .get(ui.get_shell_area().max(0) as usize)
            .copied()
            .unwrap_or(Area::Home);
        if area == Area::Chat {
            if let Some(Action::Toggle(key)) = chat_sidebar::action(index.max(0) as usize) {
                chat_sidebar::toggle(&key);
                let tree = chat_sidebar::rows(&ui.global::<Icons>(), ui.get_page_route().as_str());
                ui.set_sidebar_rows(ModelRc::new(VecModel::from(tree.items)));
                ui.set_sidebar_selected(tree.selected);
            }
            return;
        }
        if let Some(entry) = sidebar_entries(area).into_iter().nth(index.max(0) as usize)
            && entry.item.header
        {
            toggle_heading(area, entry.section);
            ui.set_sidebar_rows(ModelRc::new(VecModel::from(sidebar_rows(
                &ui.global::<Icons>(),
                area,
            ))));
            ui.set_sidebar_selected(shell_selected(area, ui.get_page_route().as_str()));
        }
    });

    // Phone tabs: an area tab shows that area's first page; the More tab opens the drawer.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tab_activated(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        match usize::try_from(index).ok().and_then(|i| PHONE_TABS.get(i)) {
            Some(area) => {
                if let Some(route) = first_route(*area) {
                    navigate_to(&ui, &open, &route);
                }
            }
            None => {
                ui.set_more_view(true);
                ui.set_drawer_open(true);
            }
        }
    });

    // A More drawer pick: that area's tree shows in the drawer, and the drawer stays open on it.
    let weak = ui.as_weak();
    ui.on_more_area_activated(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(area) = usize::try_from(index)
            .ok()
            .and_then(|i| more_areas().get(i).copied())
        {
            show_area_tree(&ui, area);
            ui.set_more_view(false);
        }
    });

    // Breadcrumbs: the area (index 0) opens its first page, the section (index 1) its first page. The page itself is the
    // last crumb and does not report clicks.
    // The project switcher (tail C3): the filter narrows its list, and a pick makes that project the current one.
    let weak = ui.as_weak();
    ui.on_project_filter_changed(move |query| {
        if let Some(ui) = weak.upgrade() {
            ui.set_project_choices(project_choices(&query));
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_project_picked(move |key| {
        if let Some(ui) = weak.upgrade() {
            switch_project(&ui, &open, &key);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_crumb_activated(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(matched) = parse(ui.get_page_route().as_str()) else {
            return;
        };
        let page = matched.page;
        let current = ui.get_page_route();
        let target = if index == 0 {
            first_route(page.area)
        } else if let Some(key) = project_name_crumb(page.route, current.as_str())
            .and_then(|_| project_key(current.as_str()))
        {
            // The project crumb opens the project's overview page.
            Some(format!("/p/{key}"))
        } else {
            section_route(page.area, page.section)
        };
        if let Some(route) = target {
            navigate_to(&ui, &open, &route);
        }
    });

    // Tile groups (T2a). A click in a group focuses it. A tab press in a group focuses it and shows the tab; the close control
    // closes the tab (closing the last tab of a group collapses the split, and the last tab of the window clears the page
    // area to its empty state).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_group_focused(move |group| {
        if let Some(ui) = weak.upgrade() {
            focus_group(&ui, &open, group);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_tab_activated(move |group, index| {
        let Some(ui) = weak.upgrade() else { return };
        if focus_group(&ui, &open, group) {
            show_tab(&ui, &open, index.max(0) as usize);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_tab_closed(move |group, index| {
        let Some(ui) = weak.upgrade() else { return };
        if focus_group(&ui, &open, group) && open.close(index.max(0) as usize) {
            show_active(&ui, &open);
        }
    });

    // Right click on a page tab: its group is focused, and the tab menu is filled for that tab (the markup opens it at the
    // pointer). A group that does not exist gives an empty menu.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_tab_context_requested(move |group, index, _x, _y| {
        let Some(ui) = weak.upgrade() else { return };
        let rows = if focus_group(&ui, &open, group) {
            let at = usize::try_from(index).unwrap_or(usize::MAX);
            open.page_menu_rows(&ui, at)
        } else {
            Vec::new()
        };
        ui.set_open_tab_menu(tab_menu_items(&rows));
    });

    // A drag of a bar between groups resizes the split behind it. The layout is synced after each move.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_handle_dragged(move |index, x, y| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(at) = usize::try_from(index)
            && open.drag_handle(at, x, y)
        {
            open.sync_strip(&ui);
        }
    });

    // The page area's size, on init and on every window resize: the groups are laid out again.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_area_resized(move |width, height| {
        let Some(ui) = weak.upgrade() else { return };
        open.set_area(width, height);
        open.sync_strip(&ui);
    });

    // A page tab dragged in a strip (T2b). The first move starts the drag; the overlay and the ghost follow the pointer. A drop
    // runs its plan, and the group it lands in takes focus; the shared controls then follow its active tab. Escape cancels.
    // Phone has one group and no drag.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_drag_moved(move |group, index, x, y| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_phone() {
            return;
        }
        if let (Ok(source), Ok(at)) = (u32::try_from(group), usize::try_from(index))
            && open.tile_drag_moved(source, at, x, y)
        {
            open.sync_drag(&ui);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_drag_dropped(move |_group, _index, x, y| {
        let Some(ui) = weak.upgrade() else { return };
        if open.tile_drag_dropped(x, y) {
            show_active(&ui, &open);
        } else {
            open.sync_strip(&ui);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_tile_drag_escaped(move || {
        let Some(ui) = weak.upgrade() else { return };
        if open.tile_drag_cancelled() {
            open.sync_strip(&ui);
        }
    });

    // A tab menu pick on the tab it was opened on. A target of -1 (no tab) does nothing.
    let weak = ui.as_weak();
    let open = tabs.clone();
    let tab_copier = copier.clone();
    ui.on_open_tab_menu_picked(move |index, id| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(at) = usize::try_from(index) {
            tab_menu_pick(&ui, &open, &tab_copier, at, id.as_str());
        }
    });

    // Ctrl+Alt+Shift+T (Cmd+Option+Shift+T on macOS) and the menu's Reopen.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_reopen_tab_requested(move || {
        if let Some(ui) = weak.upgrade() {
            reopen_tab(&ui, &open);
        }
    });

    // The empty state's button opens the Dashboard (the home route).
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_open_home_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            navigate_to(&ui, &open, FALLBACK_ROUTE);
        }
    });

    // Profile menu (and the phone More drawer): About, Report Issue and Settings each open their shell screen.
    let weak = ui.as_weak();
    ui.on_about_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            set_overlay(&ui, Some(Overlay::About));
        }
    });

    let weak = ui.as_weak();
    ui.on_submit_feedback_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            set_overlay(&ui, Some(Overlay::Feedback));
        }
    });

    let weak = ui.as_weak();
    ui.on_report_issue_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            set_overlay(&ui, Some(Overlay::Report));
        }
    });

    let weak = ui.as_weak();
    ui.on_settings_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            set_overlay(&ui, Some(Overlay::Settings));
        }
    });

    // Status bar Record: an empty mock. No recording runs yet (PLAN.md section 6.6, "Screen recording (Clips)").
    ui.on_record_clicked(|| {});

    // Status bar Fullscreen: toggles the window. The other top bar and status bar controls (notifications, profile,
    // shortcuts) only report their clicks for now.
    let weak = ui.as_weak();
    ui.on_fullscreen_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            toggle_fullscreen(&ui);
        }
    });
}

/// The workspace strip (planned feature B2, desktop): a click switches, the middle button or the X closes, a right press opens
/// the workspace menu, "+" and the empty state open a workspace on Home, and the shortcuts (Ctrl+Alt+T, W, Shift+W and 1 to 9)
/// do the same. Rename is a dialog (`AppShell.open-workspace-rename`), answered by `workspace-renamed`.
fn wire_workspaces(ui: &AppShell, tabs: Rc<Tabs>, copier: LinkCopier) {
    // A workspace tab dragged in the strip (T2b): the insertion bar follows the pointer, and the drop moves the workspace in
    // the pinned rules. Strip x is the strip's own px.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_drag_moved(move |index, x, _y| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(at) = usize::try_from(index)
            && open.workspace_drag_moved(at, x)
        {
            open.sync_strip(&ui);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_drag_dropped(move |_index, x, _y| {
        let Some(ui) = weak.upgrade() else { return };
        open.workspace_drag_dropped(x);
        open.sync_strip(&ui);
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_new_workspace_requested(move || {
        if let Some(ui) = weak.upgrade() {
            new_workspace(&ui, &open);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_activated(move |index| {
        if let Some(ui) = weak.upgrade()
            && let Ok(at) = usize::try_from(index)
        {
            switch_workspace(&ui, &open, at);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_closed(move |index| {
        if let Some(ui) = weak.upgrade()
            && let Ok(at) = usize::try_from(index)
        {
            close_workspace(&ui, &open, at);
        }
    });

    // A right press on a workspace: the menu is filled for that workspace, and the markup opens it at the pointer.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_context_requested(move |index, _x, _y| {
        let Some(ui) = weak.upgrade() else { return };
        let at = usize::try_from(index).unwrap_or(usize::MAX);
        let rows = open.workspace_menu_rows(at, false);
        ui.set_open_workspace_menu(tab_menu_items(&rows));
    });

    // The phone sheet's row "more" button (D12): the same menu, in its compact form, for that workspace.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_sheet_menu_requested(move |index, _x, _y| {
        let Some(ui) = weak.upgrade() else { return };
        let at = usize::try_from(index).unwrap_or(usize::MAX);
        let rows = open.workspace_menu_rows(at, true);
        ui.set_open_workspace_menu(tab_menu_items(&rows));
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    let workspace_copier = copier.clone();
    ui.on_workspace_menu_picked(move |index, id| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(at) = usize::try_from(index) {
            workspace_menu_pick(&ui, &open, &workspace_copier, at, id.as_str());
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_workspace_renamed(move |index, name| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(at) = usize::try_from(index) {
            rename_workspace(&ui, &open, at, name.as_str());
        }
    });

    // Ctrl+Alt+W closes the active workspace; Ctrl+Alt+Shift+W reopens the last closed one.
    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_close_workspace_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(at) = open.active_workspace() {
            close_workspace(&ui, &open, at);
        }
    });

    let weak = ui.as_weak();
    let open = tabs.clone();
    ui.on_reopen_workspace_requested(move || {
        if let Some(ui) = weak.upgrade() {
            reopen_workspace(&ui, &open);
        }
    });

    // Ctrl+Alt+1 to 9: the digit arrives as text, and the workspace is N - 1. A number past the open workspaces does nothing.
    let weak = ui.as_weak();
    let open = tabs;
    ui.on_switch_workspace_requested(move |digit| {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(number) = digit.as_str().parse::<usize>()
            && number >= 1
        {
            switch_workspace(&ui, &open, number - 1);
        }
    });
}

/// The palette: Ctrl+K opens it with an empty query and every page. Typing filters, Up and Down move the highlight, Enter
/// or a click opens the highlighted page, and Escape closes it.
fn wire_palette(ui: &AppShell, tabs: Rc<Tabs>) {
    // The entries follow the current project (tail C3), so they are read again each time the palette opens.
    let all: Rc<RefCell<Vec<Entry>>> = Rc::new(RefCell::new(palette::entries()));

    let weak = ui.as_weak();
    let entries = all.clone();
    ui.on_palette_requested(move || {
        if let Some(ui) = weak.upgrade() {
            *entries.borrow_mut() = palette::entries();
            ui.set_palette_query("".into());
            ui.set_palette_rows(ModelRc::new(VecModel::from(palette::rows_for(
                "",
                &entries.borrow(),
            ))));
            ui.set_palette_highlight(0);
            ui.set_palette_open(true);
        }
    });

    let weak = ui.as_weak();
    let entries = all.clone();
    ui.on_palette_query_changed(move |query| {
        if let Some(ui) = weak.upgrade() {
            let rows = palette::rows_for(&query, &entries.borrow());
            let highlight = if rows.is_empty() { -1 } else { 0 };
            ui.set_palette_rows(ModelRc::new(VecModel::from(rows)));
            ui.set_palette_highlight(highlight);
        }
    });

    let weak = ui.as_weak();
    ui.on_palette_move(move |delta| {
        if let Some(ui) = weak.upgrade() {
            let rows = ui.get_palette_rows().row_count();
            ui.set_palette_highlight(palette::step(ui.get_palette_highlight(), rows, delta));
        }
    });

    let weak = ui.as_weak();
    ui.on_palette_picked(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        // A pick on no row (an empty result, or a stale index) keeps the palette open.
        let row: Option<PaletteRow> = usize::try_from(index)
            .ok()
            .and_then(|i| ui.get_palette_rows().row_data(i));
        if let Some(row) = row {
            ui.set_palette_open(false);
            navigate_to(&ui, &tabs, row.route.as_str());
        }
    });

    let weak = ui.as_weak();
    ui.on_palette_closed(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_palette_open(false);
        }
    });
}

/// The theme switch cycles system, light, dark. The mode is applied at once; no restart and no toast.
fn wire_theme(ui: &AppShell, mode: Rc<Cell<ThemeMode>>) {
    let weak = ui.as_weak();
    ui.on_theme_clicked(move || {
        let Some(ui) = weak.upgrade() else { return };
        let next = next_mode(mode.get());
        mode.set(next);
        slint_theme::set_mode(&ui, next.into());
        ui.set_theme_label(theme_label(next).into());
        ui.set_theme_icon(theme_icon(&ui.global::<Icons>(), next));
    });
}

/// The toast stack: the close control, the action and the exit animation drive the queue, and hover or focus pauses a
/// toast's countdown.
fn wire_toasts(ui: &AppShell, queue: Rc<RefCell<ToastQueue>>, toasts: Rc<VecModel<ToastEntry>>) {
    let (q, m) = (queue.clone(), toasts.clone());
    ui.on_toast_action(move |id| {
        q.borrow_mut().action(id);
        sync_toasts(&q.borrow(), &m);
    });

    let (q, m) = (queue.clone(), toasts.clone());
    ui.on_toast_close(move |id| {
        q.borrow_mut().dismiss(id);
        sync_toasts(&q.borrow(), &m);
    });

    let (q, m) = (queue.clone(), toasts.clone());
    ui.on_toast_exit(move |id| {
        q.borrow_mut().remove(id);
        sync_toasts(&q.borrow(), &m);
    });

    let q = queue;
    ui.on_toast_paused(move |id, paused| {
        q.borrow_mut().set_paused(id, paused);
    });
}

/// Brings the toast model in line with the queue's visible toasts, newest first.
fn sync_toasts(queue: &ToastQueue, model: &VecModel<ToastEntry>) {
    let rows: Vec<ToastEntry> = queue.visible().into_iter().map(toast_entry).collect();
    sync_model(model, &rows, |row| row.id);
}

fn toast_entry(record: &Record) -> ToastEntry {
    ToastEntry {
        id: record.id,
        tone: tone(record.tone),
        title: record.title.as_str().into(),
        description: record.description.as_str().into(),
        action_label: record.action_label.as_deref().unwrap_or_default().into(),
        close_label: record.close_label.as_str().into(),
        closing: record.closing,
    }
}

fn tone(tone: Tone) -> ToastTone {
    match tone {
        Tone::Neutral => ToastTone::Neutral,
        Tone::Success => ToastTone::Success,
        Tone::Warning => ToastTone::Warning,
        Tone::Danger => ToastTone::Danger,
        Tone::Info => ToastTone::Info,
    }
}

fn theme_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "Theme: light",
        ThemeMode::Dark => "Theme: dark",
        ThemeMode::System => "Theme: system",
    }
}

/// The status bar icon for a mode: a monitor for system, a sun for light, a moon for dark.
fn theme_icon(icons: &Icons<'_>, mode: ThemeMode) -> slint::Image {
    match mode {
        ThemeMode::System => icons.get_monitor(),
        ThemeMode::Light => icons.get_sun(),
        ThemeMode::Dark => icons.get_moon(),
    }
}

/// Draws the quick action menu again, and its item count. Called at start and when the project list loads, since New issue
/// is disabled while the person may create issues in no project.
fn refresh_quick_categories(ui: &AppShell) {
    let categories = quick_categories(&ui.global::<Icons>(), !projects::editable().is_empty());
    let item_count: usize = categories.iter().map(|c| c.items.row_count()).sum();
    ui.set_quick_item_count(item_count as i32);
    ui.set_quick_categories(ModelRc::new(VecModel::from(categories)));
}

/// The quick action menu (IA section 4): one section per `space_nav::quick` category, Personal first, then Chat. The
/// categories and their item ids and labels come from that model; this function only adds the icon for each item. New
/// issue is disabled (with a hint) unless `can_new_issue` says the person may create issues in some project.
fn quick_categories(icons: &Icons<'_>, can_new_issue: bool) -> Vec<QuickCategory> {
    space_nav::quick::categories()
        .iter()
        .map(|category| {
            let items: Vec<QuickItem> = category
                .items
                .iter()
                .map(|item| {
                    let enabled = item.id != "new_issue" || can_new_issue;
                    QuickItem {
                        id: item.id.into(),
                        label: item.label.into(),
                        icon: icon(icons, quick_icon(item.id)),
                        enabled,
                        hint: if enabled {
                            "".into()
                        } else {
                            "Only a member of a project can create issues in it.".into()
                        },
                    }
                })
                .collect();
            QuickCategory {
                title: category.title.into(),
                items: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect()
}

/// The Lucide icon name of a quick action item id (`space_nav::quick`). The icon is a UI choice, so it lives here.
fn quick_icon(id: &str) -> &'static str {
    match id {
        "new_mail" => "mail",
        "new_event" => "calendar-plus",
        "new_task" => "square-check",
        "new_note" => "notebook-pen",
        "new_channel" => "hash",
        "new_chat" => "message-square",
        "new_room" => "users-round",
        "new_group" => "users",
        "new_issue" => "circle-dot",
        _ => "plus",
    }
}

/// The quick action pick: records the item id (`space_nav::quick`). The menu closes in the markup. New issue opens its
/// dialog (src/pages/projects/new_issue.rs); the other items have no dialog yet, so nothing else happens.
fn wire_quick_action(ui: &AppShell) -> Rc<RefCell<Option<String>>> {
    let last = Rc::new(RefCell::new(None));
    let record = last.clone();
    let weak = ui.as_weak();
    ui.on_quick_item_picked(move |id| {
        if id == "new_issue"
            && let Some(ui) = weak.upgrade()
        {
            ui.invoke_new_issue_requested();
        }
        *record.borrow_mut() = Some(id.to_string());
    });
    // The page's primary action (OQ21) opens the dialog of its id, as a quick action pick does.
    let weak = ui.as_weak();
    ui.on_primary_action_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(id) = parse(ui.get_page_route().as_str()).and_then(|m| m.page.primary_action) {
            ui.invoke_quick_item_picked(id.into());
        }
    });
    last
}

/// The Language menu (status bar). A pick records the checked locale in `language-index`. Switching the Slint translation
/// is not wired: the project has no `@tr` strings or bundled translations yet, so the choice is only recorded.
fn wire_language(ui: &AppShell) {
    let weak = ui.as_weak();
    ui.on_language_picked(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_language_index(index);
        }
    });
}

/// The order of the theme button (SF.4, IA section 5): system, then light, then dark, then system again.
fn next_mode(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::System => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
        ThemeMode::Dark => ThemeMode::System,
    }
}

/// Toggles the window between fullscreen and windowed (the status bar Fullscreen button).
fn toggle_fullscreen(ui: &AppShell) {
    let window = ui.window();
    window.set_fullscreen(!window.is_fullscreen());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_cycles_system_light_dark() {
        assert_eq!(next_mode(ThemeMode::System), ThemeMode::Light);
        assert_eq!(next_mode(ThemeMode::Light), ThemeMode::Dark);
        assert_eq!(next_mode(ThemeMode::Dark), ThemeMode::System);
    }

    #[test]
    fn every_area_with_pages_has_a_first_route() {
        for area in Area::ALL {
            let has_pages = sidebar_entries(area)
                .iter()
                .any(|entry| entry.route.is_some());
            assert_eq!(first_route(area).is_some(), has_pages, "{area:?}");
        }
    }
}
