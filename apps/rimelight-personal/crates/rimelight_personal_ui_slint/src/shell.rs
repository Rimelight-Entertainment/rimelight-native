//! The app shell's Rust side: the sidebar rows from `rimelight_personal_nav`, the sub-tab strip, the command palette (Ctrl+K), the
//! floating quick-action menu with the quick to-do modal, the toast stack, the route switch, the page glue, and the
//! window's event loop.
//!
//! A row is a heading (a section title, no route) or a link (a route). Sub-links are indented rows under their item. The
//! footer link (Assets, `/storage`) is the last row. The navbar title comes from `rimelight_personal_nav::title_for`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rimelight_personal_core::{ApiClient, InstanceConfig};
use rimelight_personal_feature_focus::Stretch;
use rimelight_personal_feature_todos::{QuickTodoController, QuickTodoView};
use rimelight_personal_nav::{ASSETS, Link, parse, sections, subtabs, title_for};
use rimelight_ui_kit::theme::Mode;
use rimelight_ui_kit::toast::{Record, Toast, ToastQueue, Tone, sync_model, take_announced};
use slint::{ComponentHandle as _, Model as _, ModelRc, Timer, TimerMode, VecModel};

use crate::generated::{
    FocusState, Icons, OverviewState, PaletteRow, RimelightPersonalShell, SidebarItem, SidebarKind,
    SubTab, ToastEntry, ToastTone, TodosState,
};
use crate::pages::{
    finances, focus, habits, health, housing, inbox, music, notebooks, notes, overview, projects,
    storage, todos, watchlist,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::platform::FileDrops;
use crate::{session, theme};

/// The countdown step of the toast timer, in milliseconds. One timer drives every toast.
const TICK_MS: u64 = 100;

/// One sidebar row: a heading (no route) or a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavEntry {
    pub label: &'static str,
    pub route: Option<&'static str>,
    pub icon: &'static str,
    pub heading: bool,
    pub indented: bool,
}

/// One command of the palette: the web's GlobalCommandPalette entries. Each one is a link to a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub category: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub route: &'static str,
}

/// The palette's commands, in the web's order (its 21 entries).
pub const COMMANDS: &[Command] = &[
    Command {
        category: "Navigation",
        title: "Dashboard Overview",
        subtitle: "Hub stats and modules",
        route: "/",
    },
    Command {
        category: "Navigation",
        title: "Notes & Scratchpad",
        subtitle: "Personal markdown notes and tags",
        route: "/notes",
    },
    Command {
        category: "Navigation",
        title: "Notebooks",
        subtitle: "Topic threads for quick info dumping",
        route: "/notebooks",
    },
    Command {
        category: "Navigation",
        title: "Action Todos",
        subtitle: "Task checklists and recurring tasks",
        route: "/todos",
    },
    Command {
        category: "Navigation",
        title: "Projects & Kanban",
        subtitle: "Kanban boards and tasks",
        route: "/projects",
    },
    Command {
        category: "Navigation",
        title: "Habit Tracker",
        subtitle: "Streaks and 52-week grid",
        route: "/habits",
    },
    Command {
        category: "Navigation",
        title: "Assets & Storage",
        subtitle: "Cloudflare R2 file explorer",
        route: "/storage",
    },
    Command {
        category: "Navigation",
        title: "Inbox & Messages",
        subtitle: "Personal inbox messages",
        route: "/inbox",
    },
    Command {
        category: "Finances",
        title: "Finances Overview",
        subtitle: "Net cashflow and accounts",
        route: "/finances",
    },
    Command {
        category: "Finances",
        title: "Bills",
        subtitle: "Recurring bills and services",
        route: "/finances/bills",
    },
    Command {
        category: "Finances",
        title: "Incomes & Deposits",
        subtitle: "Income streams and earnings",
        route: "/finances/incomes",
    },
    Command {
        category: "Finances",
        title: "Financial Wishlist",
        subtitle: "Planned gear and hardware purchases",
        route: "/finances/wishlist",
    },
    Command {
        category: "Finances",
        title: "Bank Accounts",
        subtitle: "Checking, savings, and balances",
        route: "/finances/accounts",
    },
    Command {
        category: "Health",
        title: "Fasting Tracker",
        subtitle: "Live timer & metabolic progress",
        route: "/health/fasting",
    },
    Command {
        category: "Health",
        title: "Workouts & Stretches",
        subtitle: "Gym exercises and timers",
        route: "/health/workouts",
    },
    Command {
        category: "Health",
        title: "Body Measurements",
        subtitle: "Weight targets and body dimensions",
        route: "/health/measurements",
    },
    Command {
        category: "Media",
        title: "Piano Studio",
        subtitle: "Repertoire, routines, and metronome",
        route: "/music/piano",
    },
    Command {
        category: "Media",
        title: "Guitar Studio",
        subtitle: "Fretboard, exercises, and tabs",
        route: "/music/guitar",
    },
    Command {
        category: "Media",
        title: "DJ Mixes",
        subtitle: "Set transitions and mix archive",
        route: "/music/dj",
    },
    Command {
        category: "Media",
        title: "Watchlist Matrix",
        subtitle: "Movies, TV series, and books",
        route: "/watchlist",
    },
    Command {
        category: "Media",
        title: "Housing Groceries",
        subtitle: "Shopping lists and pantry stores",
        route: "/housing/groceries",
    },
];

/// The sidebar rows, in order: each section's heading, its items, and each item's sub-links. An item's first sub-link
/// shares the item's route (for example "Overview"), so it is not listed twice.
pub fn nav_entries() -> Vec<NavEntry> {
    let mut entries = Vec::new();
    for section in sections() {
        if let Some(title) = section.title {
            entries.push(NavEntry {
                label: title,
                route: None,
                icon: "",
                heading: true,
                indented: false,
            });
        }
        for item in section.items {
            entries.push(link_entry(&item.link, false));
            for sub in item.subs.iter().filter(|sub| sub.route != item.link.route) {
                entries.push(link_entry(sub, true));
            }
        }
    }
    entries.push(link_entry(&ASSETS, false));
    entries
}

fn link_entry(link: &Link, indented: bool) -> NavEntry {
    NavEntry {
        label: link.label,
        route: Some(link.route),
        icon: link.icon,
        heading: false,
        indented,
    }
}

fn to_item(ui: &RimelightPersonalShell, entry: &NavEntry) -> SidebarItem {
    SidebarItem {
        label: entry.label.into(),
        header: entry.heading,
        icon: if entry.heading {
            Default::default()
        } else {
            ui.global::<Icons>().invoke_icon_for(entry.icon.into())
        },
        kind: if entry.heading {
            SidebarKind::Heading
        } else {
            SidebarKind::Page
        },
        expanded: entry.heading,
        indented: entry.indented,
        ..Default::default()
    }
}

/// Shows `route` in the window: the route, its title, the sidebar row that is highlighted, and the sub-tab strip. It does
/// not fetch the page's data; [`Shell::navigate`] does both.
pub fn navigate(ui: &RimelightPersonalShell, route: &str) {
    let entries = nav_entries();
    let Some(route) = parse(route) else { return };
    ui.set_route(route.into());
    ui.set_page_title(title_for(route).into());
    let row = entries
        .iter()
        .position(|entry| entry.route == Some(route))
        .unwrap_or(0);
    ui.set_nav_selected(row as i32);
    let tabs: Vec<SubTab> = subtabs(route)
        .iter()
        .map(|link| SubTab {
            label: link.label.into(),
            active: link.route == route,
        })
        .collect();
    ui.set_subtabs(ModelRc::new(VecModel::from(tabs)));
}

/// Fills the palette's rows with the commands whose title, subtitle or category contains `query` (case-insensitive), and
/// records their routes in the same order, so a picked row index maps to its route.
fn fill_palette(ui: &RimelightPersonalShell, query: &str, routes: &RefCell<Vec<&'static str>>) {
    let needle = query.trim().to_lowercase();
    let matches: Vec<&Command> = COMMANDS
        .iter()
        .filter(|command| {
            needle.is_empty()
                || [command.title, command.subtitle, command.category]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&needle))
        })
        .collect();
    *routes.borrow_mut() = matches.iter().map(|command| command.route).collect();
    let rows: Vec<PaletteRow> = matches
        .iter()
        .map(|command| PaletteRow {
            title: command.title.into(),
            route: command.route.into(),
            subtitle: command.subtitle.into(),
            category: command.category.into(),
        })
        .collect();
    let highlight = if rows.is_empty() { -1 } else { 0 };
    ui.set_palette_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_palette_highlight(highlight);
}

/// The page glue of the shell: one value per page, built once when the window opens.
pub struct Shell {
    ui: RimelightPersonalShell,
    todos: todos::Todos,
    habits: habits::Habits,
    watchlist: watchlist::Watchlist,
    notes: notes::Notes,
    overview: overview::Overview,
    projects: projects::Projects,
    finances: finances::Finances,
    housing: housing::Housing,
    health: health::Health,
    inbox: inbox::Inbox,
    music: music::Music,
    notebooks: notebooks::Notebooks,
    storage: Rc<storage::Storage>,
    /// The Focus card's glue. Kept alive for the window; its timer runs while the window is open.
    _focus: focus::Focus,
    /// The toast countdown. Dropping the timer stops the ticks, so it lives as long as the shell.
    _toasts: Timer,
    /// The window's file drops. Storage files dropped on the window go to the storage page.
    #[cfg(not(target_arch = "wasm32"))]
    _drops: FileDrops,
}

impl Shell {
    /// Shows `route`, and fetches the page's data when the route opens it. Leaving the music pages flushes the music
    /// page's pending saves first.
    pub fn navigate(&self, route: &str) {
        let Some(route) = parse(route) else { return };
        if self.ui.get_route().starts_with("/music") && !route.starts_with("/music") {
            self.music.flush();
        }
        navigate(&self.ui, route);
        self.activate(route);
    }

    /// Opens the add form of a quick-action menu item on its page, once the page is shown. The to-do and the Focus items
    /// are handled by the menu itself.
    fn open_add_form(&self, item: &str) {
        match item {
            "note" => self.notes.open_quick(),
            "notebook" => self.notebooks.open_create(),
            "transaction" => self.finances.open_form("expense"),
            "income" => self.finances.open_form("income"),
            "bill" => self.finances.open_form("bill"),
            "wishlist" => self.finances.open_form("plan"),
            "account" => self.finances.open_form("account"),
            "watchlist" => self.watchlist.open_add(&self.ui),
            _ => {}
        }
    }

    fn activate(&self, route: &str) {
        match route {
            "/" => self.overview.activate(),
            "/todos" => self.todos.activate(),
            "/notes" => self.notes.activate(),
            "/habits" => self.habits.activate(),
            "/inbox" => self.inbox.activate(),
            "/projects" => self.projects.activate(),
            "/storage" => self.storage.activate(),
            "/notebooks" => self.notebooks.activate(),
            _ => {}
        }
        if route.starts_with("/watchlist") {
            self.watchlist.activate(route);
        }
        if route.starts_with("/finances") {
            self.finances.activate();
        }
        if route.starts_with("/housing") {
            self.housing.activate();
        }
        if route.starts_with("/health") {
            self.health.open(route);
            self.health.activate();
        }
        if route.starts_with("/music") {
            self.music.activate(route);
        }
    }
}

/// Brings the toast model in line with the queue's visible toasts, newest first.
fn sync_toasts(queue: &ToastQueue, model: &VecModel<ToastEntry>) {
    let rows: Vec<ToastEntry> = queue.visible().into_iter().map(toast_entry).collect();
    sync_model(model, &rows, |row| row.id);
}

fn toast_entry(record: &Record) -> ToastEntry {
    ToastEntry {
        id: record.id,
        tone: toast_tone(record.tone),
        title: record.title.as_str().into(),
        description: record.description.as_str().into(),
        action_label: record.action_label.as_deref().unwrap_or_default().into(),
        close_label: record.close_label.as_str().into(),
        closing: record.closing,
    }
}

fn toast_tone(tone: Tone) -> ToastTone {
    match tone {
        Tone::Neutral => ToastTone::Neutral,
        Tone::Success => ToastTone::Success,
        Tone::Warning => ToastTone::Warning,
        Tone::Danger => ToastTone::Danger,
        Tone::Info => ToastTone::Info,
    }
}

/// Opens the shell window at `start` (a concrete route, already checked with `rimelight_personal_nav::parse`) with the theme `mode`,
/// and runs the event loop until the window closes. Used by the desktop app and the web app. The API and the Access
/// application are `instance`; requests carry the signed-in session's bearer token.
/// What the About overlay shows: the app's version and, for installed copies, the update check. `check` runs on a worker
/// thread and returns the sentence the overlay shows.
pub struct About {
    pub version: String,
    pub check: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

pub fn run(
    start: &str,
    mode: Mode,
    instance: InstanceConfig,
    about: About,
) -> Result<(), slint::PlatformError> {
    let ui = RimelightPersonalShell::new()?;
    let entries = nav_entries();
    let items: Vec<SidebarItem> = entries.iter().map(|entry| to_item(&ui, entry)).collect();
    ui.set_nav_items(ModelRc::new(VecModel::from(items)));
    ui.set_mac_keys(cfg!(target_os = "macos"));
    wire_about(&ui, about);

    // Sign-in first: the pages' API client carries the bearer token of the session.
    let auth = session::wire(&ui, instance.clone());
    let client = ApiClient::new(instance.api_base.clone()).with_bearer(auth.session.clone());
    let api = Arc::new(client);
    // Todos and Projects make their own requests; they carry the same session header.
    let todo_api = rimelight_personal_feature_todos::http_api_signed(
        instance.api_base.clone(),
        auth.session.clone(),
    );
    let projects_api = rimelight_personal_feature_projects::http_api_signed(
        instance.api_base.clone(),
        auth.session.clone(),
    );

    theme::set_mode(&ui, mode);

    let focus = focus::Focus::new(&ui);
    // The Focus controller is cloned for the health hook: a routine's stretch steps start the Focus page's timer.
    let focus_controller = focus.controller().clone();
    let routine_ui = ui.as_weak();
    let storage = Rc::new(storage::Storage::new(&ui, api.clone()));
    #[cfg(not(target_arch = "wasm32"))]
    let storage_for_drops = storage.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let drops = FileDrops::install(ui.window());
    #[cfg(not(target_arch = "wasm32"))]
    let subscription = drops.subscribe(move |drop| {
        if let crate::platform::FileDrop::Dropped(path) = drop {
            storage_for_drops.files_dropped(&[path.to_string_lossy().into_owned()]);
        }
    });
    // The subscription lives as long as the shell; dropping it would stop the file drops.
    #[cfg(not(target_arch = "wasm32"))]
    std::mem::forget(subscription);

    // The toasts: one queue and one model. Pages announce through rimelight_ui_kit::toast::announce; the timer below moves
    // them into the queue and counts them down.
    let queue = Rc::new(RefCell::new(ToastQueue::new()));
    let toasts = Rc::new(VecModel::<ToastEntry>::default());
    ui.set_toasts(ModelRc::new(toasts.clone()));
    let toast_timer = Timer::default();
    {
        let queue = queue.clone();
        let toasts = toasts.clone();
        toast_timer.start(
            TimerMode::Repeated,
            Duration::from_millis(TICK_MS),
            move || {
                let mut queue = queue.borrow_mut();
                for (tone, title) in take_announced() {
                    queue.show(Toast::new(tone, title, "Close"));
                }
                queue.tick(TICK_MS as u32);
                sync_toasts(&queue, &toasts);
            },
        );
    }

    // The global quick to-do modal. Its state reaches the window's properties through `notify`. A save bumps `saved_seq`;
    // the notify sees the change, and the Todos page reloads when it is the open route.
    let quick_todo = {
        let weak = ui.as_weak();
        let seen_saved = Arc::new(AtomicU64::new(0));
        QuickTodoController::new(todo_api.clone(), move |view: QuickTodoView| {
            let saved = seen_saved.swap(view.saved_seq, Ordering::SeqCst) != view.saved_seq;
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_quick_todo_open(view.open);
                ui.set_quick_todo_busy(view.busy);
                ui.set_quick_todo_error(view.error.clone().unwrap_or_default().into());
                if saved && ui.get_route() == "/todos" {
                    ui.global::<TodosState>().invoke_refresh_requested();
                }
            });
        })
    };

    let shell = Rc::new(Shell {
        ui: ui.clone_strong(),
        todos: todos::Todos::new(&ui, todo_api.clone()),
        habits: habits::Habits::new(&ui, api.clone(), rimelight_personal_feature_habits::today()),
        watchlist: watchlist::Watchlist::new(&ui, api.clone()),
        notes: notes::Notes::new(&ui, api.clone()),
        overview: overview::Overview::new(&ui, api.clone()),
        projects: projects::Projects::new(&ui, projects_api.clone()),
        finances: finances::Finances::new(&ui, api.clone()),
        housing: housing::Housing::new(&ui, api.clone()),
        health: health::Health::new(
            &ui,
            api.clone(),
            Arc::new(rimelight_personal_feature_health::SystemFeedback),
            move |steps: Vec<Stretch>| {
                focus_controller.start_stretches(steps);
                if let Some(ui) = routine_ui.upgrade() {
                    ui.global::<FocusState>().set_open(true);
                }
            },
        ),
        inbox: inbox::Inbox::new(&ui, api.clone()),
        #[cfg(not(target_arch = "wasm32"))]
        music: music::Music::new(
            &ui,
            api.clone(),
            Rc::new(crate::platform::System),
            drops.clone(),
        ),
        #[cfg(target_arch = "wasm32")]
        music: music::Music::new(&ui, api.clone()),
        notebooks: notebooks::Notebooks::new(&ui, api.clone()),
        storage,
        _focus: focus,
        _toasts: toast_timer,
        #[cfg(not(target_arch = "wasm32"))]
        _drops: drops,
    });
    // After sign-in, the page the user is on loads its data again (the first load happened before the sign-in).
    {
        let shell = shell.clone();
        ui.global::<crate::generated::Session>()
            .on_reload_page(move || {
                let route = shell.ui.get_route().to_string();
                shell.navigate(&route);
            });
    }

    // The route opens at `start`, with its page fetched.
    let start = parse(start).unwrap_or("/");
    shell.navigate(start);

    let weak_shell = Rc::downgrade(&shell);
    ui.on_nav_activated(move |index| {
        let Some(shell) = weak_shell.upgrade() else {
            return;
        };
        let Some(entry) = usize::try_from(index).ok().and_then(|i| entries.get(i)) else {
            return;
        };
        if let Some(route) = entry.route {
            shell.navigate(route);
        }
    });

    // The sub-tab strip: a click opens the sub-link of the active item at that index.
    let weak_shell = Rc::downgrade(&shell);
    let weak_ui = ui.as_weak();
    ui.on_subtab_clicked(move |index| {
        let (Some(shell), Some(ui)) = (weak_shell.upgrade(), weak_ui.upgrade()) else {
            return;
        };
        let current = ui.get_route();
        let route = usize::try_from(index)
            .ok()
            .and_then(|i| subtabs(&current).get(i))
            .map(|link| link.route);
        if let Some(route) = route {
            shell.navigate(route);
        }
    });

    // The command palette (Ctrl+K): the web's commands. `palette_routes` holds the routes of the rows shown.
    let palette_routes: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let palette_ui = ui.as_weak();
    {
        let (ui_ref, routes) = (palette_ui.clone(), palette_routes.clone());
        ui.on_palette_requested(move || {
            if let Some(ui) = ui_ref.upgrade() {
                ui.set_palette_query("".into());
                fill_palette(&ui, "", &routes);
                ui.set_palette_open(true);
            }
        });
    }
    {
        let (ui_ref, routes) = (palette_ui.clone(), palette_routes.clone());
        ui.on_palette_query_changed(move |query| {
            if let Some(ui) = ui_ref.upgrade() {
                fill_palette(&ui, &query, &routes);
            }
        });
    }
    {
        let ui_ref = palette_ui.clone();
        ui.on_palette_move(move |delta| {
            let Some(ui) = ui_ref.upgrade() else { return };
            let rows = ui.get_palette_rows().row_count() as i32;
            if rows == 0 {
                return;
            }
            ui.set_palette_highlight((ui.get_palette_highlight() + delta).clamp(0, rows - 1));
        });
    }
    {
        let (ui_ref, routes, weak_shell) = (
            palette_ui.clone(),
            palette_routes.clone(),
            Rc::downgrade(&shell),
        );
        ui.on_palette_picked(move |index| {
            // A pick on no row (an empty result, or a stale index) keeps the palette open.
            let route = usize::try_from(index)
                .ok()
                .and_then(|i| routes.borrow().get(i).copied());
            let Some(route) = route else { return };
            if let Some(ui) = ui_ref.upgrade() {
                ui.set_palette_open(false);
            }
            if let Some(shell) = weak_shell.upgrade() {
                shell.navigate(route);
            }
        });
    }
    {
        let ui_ref = palette_ui.clone();
        ui.on_palette_closed(move || {
            if let Some(ui) = ui_ref.upgrade() {
                ui.set_palette_open(false);
            }
        });
    }

    // The quick-action menu (web FloatingActionsOverlay). The to-do opens the quick modal and the Focus items open the Focus
    // card. The others open their page and then the page's add form.
    {
        let weak_shell = Rc::downgrade(&shell);
        let weak_ui = ui.as_weak();
        let quick = quick_todo.clone();
        ui.on_fab_picked(move |id| {
            let route = match id.as_str() {
                "note" => "/notes",
                "notebook" => "/notebooks",
                "transaction" => "/finances/transactions",
                "income" => "/finances/incomes",
                "bill" => "/finances/bills",
                "wishlist" => "/finances/wishlist",
                "account" => "/finances/accounts",
                "watchlist" => "/watchlist",
                "todo" => {
                    quick.open();
                    return;
                }
                "focus" | "metronome" => {
                    if let Some(ui) = weak_ui.upgrade() {
                        ui.global::<FocusState>().set_open(true);
                    }
                    return;
                }
                _ => return,
            };
            if let Some(shell) = weak_shell.upgrade() {
                shell.navigate(route);
                shell.open_add_form(id.as_str());
            }
        });
    }
    {
        let q = quick_todo.clone();
        ui.on_quick_todo_save(move |title, category, priority| {
            q.save(&title, &category, &priority);
        });
    }
    {
        let q = quick_todo.clone();
        ui.on_quick_todo_cancel(move || q.close());
    }

    // The toast stack: the close control, the action and the exit animation drive the queue, and hover or focus pauses a
    // toast's countdown.
    {
        let (q, m) = (queue.clone(), toasts.clone());
        ui.on_toast_action(move |id| {
            q.borrow_mut().action(id);
            sync_toasts(&q.borrow(), &m);
        });
    }
    {
        let (q, m) = (queue.clone(), toasts.clone());
        ui.on_toast_close(move |id| {
            q.borrow_mut().dismiss(id);
            sync_toasts(&q.borrow(), &m);
        });
    }
    {
        let (q, m) = (queue.clone(), toasts.clone());
        ui.on_toast_exit(move |id| {
            q.borrow_mut().remove(id);
            sync_toasts(&q.borrow(), &m);
        });
    }
    {
        let q = queue.clone();
        ui.on_toast_paused(move |id, paused| {
            q.borrow_mut().set_paused(id, paused);
        });
    }

    // The Overview page's links move the shell too.
    let weak_shell = Rc::downgrade(&shell);
    ui.global::<OverviewState>()
        .on_navigate_requested(move |route| {
            if let Some(shell) = weak_shell.upgrade() {
                shell.navigate(&route);
            }
        });

    ui.run()
}

/// The About overlay: the version, and the update check run on a worker thread (the check blocks on the network).
fn wire_about(ui: &RimelightPersonalShell, about: About) {
    ui.set_about_version(about.version.into());
    ui.set_about_can_check(about.check.is_some());
    let Some(check) = about.check else { return };
    let weak = ui.as_weak();
    ui.on_about_check_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.set_about_checking(true);
        ui.set_about_status("Checking for updates...".into());
        let check = check.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let answer = check();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_about_checking(false);
                ui.set_about_status(answer.into());
            });
        });
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The sidebar rows: the Overview row comes first, and the Assets footer is last. The palette has the web's 21 entries.
    pub(crate) fn scenario() {
        let entries = nav_entries();
        assert_eq!(entries.first().and_then(|e| e.route), Some("/"));
        assert_eq!(entries.last().and_then(|e| e.route), Some("/storage"));
        assert_eq!(COMMANDS.len(), 21);
    }
}
