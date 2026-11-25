//! The window side of the canvas area, as traits. The generated Slint types never appear in the area's Rust: the page, the
//! pop-out and the list talk to the window through these traits, and `wire_canvas!` (in `lib.rs`) implements them for the
//! generated windows.
//!
//! - [`CanvasPageProps`] and [`CanvasPageEvents`]: one getter and setter per property, and one `on_*` per callback, of the
//!   page's `CanvasState` global. They are generated from one table ([`__canvas_state_table!`]), so a property is listed once
//!   and the implementation for the window is generated from the same list. Both the main window and the pop-out window have
//!   a `CanvasState`, so both implement them.
//! - [`CanvasPageState`]: the parts of `CanvasState` that are not plain properties (the models, the load state, the item rows).
//! - [`CanvasWindow`] (the main window) and [`PopoutWindow`] (the pop-out window): what only that window has.
//! - [`UiDyn`], [`PopoutDyn`] and [`Ui`]: the page keeps its windows type-erased (it is held in thread-locals and cannot be
//!   generic), so these are the object-safe views of the two traits above, and `Ui` is the main window's weak handle.
//! - [`CanvasListWindow`]: the canvas list page's window side.

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, Image, Model as _, ModelRc, VecModel, Weak};
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataSource};

// -- The table of the page's `CanvasState` properties and callbacks ---------------------------------------------------

/// Calls the macro `$mac` (an exported macro of this crate, by its name) with `$args` and then the table of `CanvasState`
/// properties and callbacks: `props { (setter, getter, type), ... }` and `events { (on_name, (arg: type, ...) -> ret), ... }`.
#[doc(hidden)]
#[macro_export]
macro_rules! __canvas_state_table {
    ($mac:ident [$($args:tt)*]) => {
        $crate::$mac! {
            $($args)*
            props {
                (set_title, get_title, $crate::__slint::SharedString),
                (set_error, get_error, $crate::__slint::SharedString),
                (set_read_only, get_read_only, bool),
                (set_connection, get_connection, $crate::__slint::SharedString),
                (set_zoom, get_zoom, f32),
                (set_pan_x, get_pan_x, f32),
                (set_pan_y, get_pan_y, f32),
                (set_zoom_label, get_zoom_label, $crate::__slint::SharedString),
                (set_tool, get_tool, $crate::__slint::SharedString),
                (set_has_selection, get_has_selection, bool),
                (set_sel_x, get_sel_x, f32),
                (set_sel_y, get_sel_y, f32),
                (set_sel_w, get_sel_w, f32),
                (set_sel_h, get_sel_h, f32),
                (set_sel_rotation, get_sel_rotation, f32),
                (set_sel_locked, get_sel_locked, bool),
                (set_marquee_active, get_marquee_active, bool),
                (set_marquee_x, get_marquee_x, f32),
                (set_marquee_y, get_marquee_y, f32),
                (set_marquee_w, get_marquee_w, f32),
                (set_marquee_h, get_marquee_h, f32),
                (set_editing_id, get_editing_id, $crate::__slint::SharedString),
                (set_editing_text, get_editing_text, $crate::__slint::SharedString),
                (set_editing_field, get_editing_field, $crate::__slint::SharedString),
                (set_can_undo, get_can_undo, bool),
                (set_can_redo, get_can_redo, bool),
                (set_menu_open, get_menu_open, bool),
                (set_cursor, get_cursor, $crate::__slint::SharedString),
                (set_eraser_shown, get_eraser_shown, bool),
                (set_eraser_x, get_eraser_x, f32),
                (set_eraser_y, get_eraser_y, f32),
                (set_eraser_r, get_eraser_r, f32),
                (set_renaming, get_renaming, bool),
                (set_busy_text, get_busy_text, $crate::__slint::SharedString),
                (set_drop_hover, get_drop_hover, bool),
                (set_grid_mode, get_grid_mode, $crate::__slint::SharedString),
                (set_crop_active, get_crop_active, bool),
                (set_crop_full_x, get_crop_full_x, f32),
                (set_crop_full_y, get_crop_full_y, f32),
                (set_crop_full_w, get_crop_full_w, f32),
                (set_crop_full_h, get_crop_full_h, f32),
                (set_crop_x, get_crop_x, f32),
                (set_crop_y, get_crop_y, f32),
                (set_crop_w, get_crop_w, f32),
                (set_crop_h, get_crop_h, f32),
                (set_crop_rotation, get_crop_rotation, f32),
                (set_hierarchy_open, get_hierarchy_open, bool),
                (set_slideshow_open, get_slideshow_open, bool),
                (set_slideshow_image, get_slideshow_image, $crate::__slint::Image),
                (set_slideshow_caption, get_slideshow_caption, $crate::__slint::SharedString),
                (set_slideshow_playing, get_slideshow_playing, bool),
                (set_assets_open, get_assets_open, bool),
                (set_assets_busy, get_assets_busy, bool),
                (set_windows_open, get_windows_open, bool),
                (set_popped_out, get_popped_out, bool),
            }
            events {
                (on_opened, (route: $crate::__slint::SharedString)),
                (on_retry, ()),
                (on_pointer, (kind: $crate::__slint::SharedString, x: f32, y: f32, button: $crate::__slint::SharedString, shift: bool, ctrl: bool, alt: bool)),
                (on_double_click, (x: f32, y: f32)),
                (on_scroll, (x: f32, y: f32, dx: f32, dy: f32, ctrl: bool, shift: bool)),
                (on_key, (text: $crate::__slint::SharedString, shift: bool, ctrl: bool, alt: bool) -> bool),
                (on_viewport, (w: f32, h: f32)),
                (on_set_tool, (name: $crate::__slint::SharedString)),
                (on_zoom_in, ()),
                (on_zoom_out, ()),
                (on_zoom_reset, ()),
                (on_zoom_fit, ()),
                (on_undo, ()),
                (on_redo, ()),
                (on_add_image, ()),
                (on_menu_hover, (row: i32)),
                (on_menu_click, (row: i32, x: f32)),
                (on_menu_dismiss, ()),
                (on_text_commit, (text: $crate::__slint::SharedString)),
                (on_text_cancel, ()),
                (on_rename, (name: $crate::__slint::SharedString)),
                (on_follow, (conn: $crate::__slint::SharedString)),
                (on_hierarchy_pick, (id: $crate::__slint::SharedString, shift: bool)),
                (on_hierarchy_close, ()),
                (on_hierarchy_open_item, (id: $crate::__slint::SharedString)),
                (on_slideshow_step, (delta: i32)),
                (on_slideshow_toggle, ()),
                (on_slideshow_close, ()),
                (on_assets_delete_unused, ()),
                (on_assets_close, ()),
                (on_window_picked, (handle: $crate::__slint::SharedString)),
                (on_windows_close, ()),
                (on_bring_back, ()),
                (on_pop_out, ()),
                (on_video_toggle, (id: $crate::__slint::SharedString)),
                (on_video_seek, (id: $crate::__slint::SharedString, fraction: f32)),
                (on_video_mute, (id: $crate::__slint::SharedString)),
                (on_video_loop, (id: $crate::__slint::SharedString)),
                (on_video_together, (id: $crate::__slint::SharedString)),
            }
        }
    };
}

/// Generates the two traits from the table.
#[doc(hidden)]
#[macro_export]
macro_rules! __canvas_state_trait {
    (
        props { $( ($set:ident, $get:ident, $ty:ty) ),* $(,)? }
        events { $( ($on:ident, ($($an:ident : $at:ty),*) $(-> $ret:ty)?) ),* $(,)? }
    ) => {
        /// A getter and a setter for each property of the page's `CanvasState`.
        pub trait CanvasPageProps {
            $(
                fn $set(&self, value: $ty);
                fn $get(&self) -> $ty;
            )*
        }

        /// A registration for each callback of the page's `CanvasState`.
        pub trait CanvasPageEvents {
            $(
                fn $on(&self, f: impl Fn($($at),*) $(-> $ret)? + 'static);
            )*
        }
    };
}

/// Implements the two traits for a window, over the `CanvasState` global `$G`. Expanded by `wire_canvas!`.
#[doc(hidden)]
#[macro_export]
macro_rules! __canvas_state_impl {
    (
        $W:ty, $G:ty,
        props { $( ($set:ident, $get:ident, $ty:ty) ),* $(,)? }
        events { $( ($on:ident, ($($an:ident : $at:ty),*) $(-> $ret:ty)?) ),* $(,)? }
    ) => {
        impl $crate::window::CanvasPageProps for $W {
            $(
                fn $set(&self, value: $ty) {
                    $crate::__slint::ComponentHandle::global::<$G>(self).$set(value);
                }
                fn $get(&self) -> $ty {
                    $crate::__slint::ComponentHandle::global::<$G>(self).$get()
                }
            )*
        }

        impl $crate::window::CanvasPageEvents for $W {
            $(
                fn $on(&self, f: impl Fn($($at),*) $(-> $ret)? + 'static) {
                    $crate::__slint::ComponentHandle::global::<$G>(self)
                        .$on(move |$($an),*| f($($an),*));
                }
            )*
        }
    };
}

__canvas_state_table!(__canvas_state_trait []);

// -- Plain views of the generated structs -----------------------------------------------------------------------------

/// One item of the canvas as the window draws it (`CanvasItemView`).
#[derive(Clone, PartialEq, Default)]
pub struct ItemView {
    pub id: String,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    pub opacity: f32,
    pub image: Image,
    pub has_image: bool,
    pub pixelated: bool,
    pub text: String,
    pub color: String,
    pub fill: String,
    pub font_size: f32,
    pub align: String,
    pub locked: bool,
    pub selected: bool,
    pub editing: bool,
    pub crop_x: f32,
    pub crop_y: f32,
    pub crop_w: f32,
    pub crop_h: f32,
    pub path: String,
    pub shape: String,
    pub stroke_width: f32,
    pub stroke_color: String,
    pub fill_color: String,
    pub comment: String,
    pub gray: bool,
    pub is_video: bool,
    pub duration_text: String,
}

/// A snap guide line (`CanvasGuide`).
#[derive(Clone, PartialEq, Default)]
pub struct GuideView {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// One panel of the canvas menu (`CanvasMenuPanel`).
#[derive(Clone, PartialEq, Default)]
pub struct MenuPanelView {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One row of the canvas menu (`CanvasMenuRow`).
#[derive(Clone, PartialEq, Default)]
pub struct MenuRowView {
    pub panel: i32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: String,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: i32,
    pub highlighted: bool,
    pub toggle_left: String,
    pub toggle_right: String,
    pub toggle_right_on: bool,
}

/// One row of the hierarchy panel (`CanvasHierarchyRow`).
#[derive(Clone, PartialEq, Default)]
pub struct HierarchyRowView {
    pub id: String,
    pub depth: i32,
    pub label: String,
    pub kind: String,
    pub selected: bool,
    pub locked: bool,
}

/// A person in the room (`CanvasPeer`).
#[derive(Clone, PartialEq, Default)]
pub struct PeerView {
    pub conn_id: String,
    pub name: String,
    pub initials: String,
    pub color: slint::Color,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub has_cursor: bool,
    pub following: bool,
}

/// A peer's selection outline (`CanvasPeerBox`).
#[derive(Clone, PartialEq, Default)]
pub struct PeerBoxView {
    pub color: slint::Color,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rotation: f32,
}

/// The play controls of one video (`CanvasVideoControl`).
#[derive(Clone, PartialEq, Default)]
pub struct VideoControlView {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub playing: bool,
    pub progress: f32,
    pub time_text: String,
    pub muted: bool,
    pub looping: bool,
    pub loading: bool,
    pub selected: bool,
    pub shared: bool,
    pub following: bool,
}

/// One row of the manage-images dialog (`CanvasAssetRow`).
#[derive(Clone, PartialEq, Default)]
pub struct AssetRowView {
    pub id: String,
    pub name: String,
    pub size_text: String,
    pub pixels_text: String,
    pub uses: i32,
    pub thumb: Image,
    pub has_thumb: bool,
}

/// One open window in the "Always on top of..." picker (`CanvasWindowItem`).
#[derive(Clone, PartialEq, Default)]
pub struct WindowItemView {
    pub handle: String,
    pub title: String,
}

/// The page state that is set only when it changes. Popping out and bringing back copy it from one window to the other.
#[derive(Clone)]
pub struct PageCopy {
    pub state: LoadState,
    pub error: String,
    pub title: String,
    pub read_only: bool,
    pub connection: String,
    pub busy_text: String,
}

/// The item rows' model, shared by the main window and the pop-out. The rows change in place.
pub trait ItemsModel {
    fn row_count(&self) -> usize;
    fn set_row_data(&self, index: usize, view: ItemView);
    fn set_vec(&self, views: Vec<ItemView>);
    /// For the window glue, which gets its own model type back.
    fn as_any(&self) -> &dyn Any;
}

// -- The window traits ------------------------------------------------------------------------------------------------

/// The parts of the page's `CanvasState` that are not plain properties. Object safe.
pub trait CanvasPageState: CanvasPageProps {
    /// The page's load state (loading, loaded, error).
    fn set_load_state(&self, state: LoadState);
    /// The page state copied between windows.
    fn page_copy(&self) -> PageCopy;
    fn apply_page_copy(&self, copy: PageCopy);
    /// Makes the window's items draw the rows of `items`.
    fn attach_items(&self, items: &Rc<dyn ItemsModel>);
    /// The model rows, updated in place (see [`sync_model`]).
    fn sync_guides(&self, rows: Vec<GuideView>);
    fn sync_menu_panels(&self, rows: Vec<MenuPanelView>);
    fn sync_menu_rows(&self, rows: Vec<MenuRowView>);
    fn sync_grid_xs(&self, rows: Vec<f32>);
    fn sync_grid_ys(&self, rows: Vec<f32>);
    fn sync_hierarchy_rows(&self, rows: Vec<HierarchyRowView>);
    fn sync_peers(&self, rows: Vec<PeerView>);
    fn sync_peer_boxes(&self, rows: Vec<PeerBoxView>);
    fn sync_video_controls(&self, rows: Vec<VideoControlView>);
    fn sync_asset_rows(&self, rows: Vec<AssetRowView>);
    /// The window picker's rows.
    fn set_window_items(&self, items: Vec<WindowItemView>);
}

/// The main window's side of the canvas page.
pub trait CanvasWindow: ComponentHandle + CanvasPageState + CanvasPageEvents + 'static {
    /// The pop-out window this window opens.
    type Popout: PopoutWindow;
    /// Opens a canvas route in the focused tab (the window's `canvas-open-route`).
    fn canvas_open_route(&self, route: &str);
    /// Closes the focused group's active page tab (the window's `canvas-close-tab`).
    fn canvas_close_tab(&self);
    /// A new, empty item model of this window's item type.
    fn new_items(&self) -> Rc<dyn ItemsModel>;
}

/// The pop-out window's own properties (besides its `CanvasState`). Object safe.
pub trait PopoutProps: CanvasPageState {
    fn get_overlay_mode(&self) -> bool;
    fn set_overlay_mode(&self, value: bool);
    fn get_click_through(&self) -> bool;
    fn set_click_through(&self, value: bool);
    fn get_pinned(&self) -> bool;
    fn set_pinned(&self, value: bool);
    fn get_locked(&self) -> bool;
    fn set_locked(&self, value: bool);
    fn get_window_opacity(&self) -> f32;
    fn set_window_opacity(&self, value: f32);
}

/// The pop-out window: its bar's callbacks and its creation.
pub trait PopoutWindow: ComponentHandle + PopoutProps + CanvasPageEvents + 'static {
    /// Creates the (hidden) window.
    fn create() -> Result<Self, slint::PlatformError>
    where
        Self: Sized;
    fn on_bar_drag(&self, f: impl Fn() + 'static);
    fn on_bar_toggle_pin(&self, f: impl Fn() + 'static);
    fn on_bar_toggle_click_through(&self, f: impl Fn() + 'static);
    fn on_bar_minimize(&self, f: impl Fn() + 'static);
    fn on_bar_maximize(&self, f: impl Fn() + 'static);
    fn on_bar_close(&self, f: impl Fn() + 'static);
    fn on_bar_opacity(&self, f: impl Fn(f32) + 'static);
}

/// The object-safe view of a [`CanvasWindow`], which the page holds.
pub trait UiDyn: CanvasPageState {
    fn open_route(&self, route: &str);
    fn close_tab(&self);
}

impl<W: CanvasWindow> UiDyn for W {
    fn open_route(&self, route: &str) {
        self.canvas_open_route(route);
    }
    fn close_tab(&self) {
        self.canvas_close_tab();
    }
}

/// The object-safe view of a [`PopoutWindow`], which the pop-out module holds.
pub trait PopoutDyn: PopoutProps {
    fn slint_window(&self) -> &slint::Window;
    fn show_window(&self) -> Result<(), slint::PlatformError>;
    fn hide_window(&self) -> Result<(), slint::PlatformError>;
}

impl<P: PopoutWindow> PopoutDyn for P {
    fn slint_window(&self) -> &slint::Window {
        ComponentHandle::window(self)
    }
    fn show_window(&self) -> Result<(), slint::PlatformError> {
        ComponentHandle::show(self)
    }
    fn hide_window(&self) -> Result<(), slint::PlatformError> {
        ComponentHandle::hide(self)
    }
}

/// A weak handle to a pop-out window, whatever its type.
#[derive(Clone)]
pub struct WeakPopout(Rc<dyn Fn() -> Option<Rc<dyn PopoutDyn>>>);

impl WeakPopout {
    pub fn new<P: PopoutWindow>(window: &P) -> WeakPopout {
        let weak = window.as_weak();
        WeakPopout(Rc::new(move || {
            weak.upgrade()
                .map(|window| Rc::new(window) as Rc<dyn PopoutDyn>)
        }))
    }

    /// The window, if it is still open.
    pub fn upgrade(&self) -> Option<Rc<dyn PopoutDyn>> {
        (self.0)()
    }
}

// -- The main window's weak handle ------------------------------------------------------------------------------------

/// What a worker's result carries through [`Ui::fetch`]: the real type is put back by the typed wrapper.
type AnyOut = Box<dyn Any + Send>;
type AnyWork = Box<
    dyn FnOnce(Arc<ApiClient>, String) -> Pin<Box<dyn Future<Output = Result<AnyOut, ClientError>>>>
        + Send,
>;
type AnyDone = Box<dyn FnOnce(&dyn UiDyn, Result<AnyOut, ClientError>) + Send>;
/// A closure sent to the main thread to run against the type-erased window.
type PostFn = Box<dyn FnOnce(&dyn UiDyn) + Send>;

/// The typed half of [`Ui`]: one implementation per window type.
trait UiBackend: Send + Sync {
    fn upgrade(&self) -> Option<Rc<dyn UiDyn>>;
    fn fetch_any(&self, source: &Arc<DataSource>, work: AnyWork, done: AnyDone);
    fn post(&self, f: PostFn);
}

struct WeakWindow<W: CanvasWindow>(Mutex<Weak<W>>);

impl<W: CanvasWindow> WeakWindow<W> {
    fn weak(&self) -> Weak<W> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl<W: CanvasWindow> UiBackend for WeakWindow<W> {
    fn upgrade(&self) -> Option<Rc<dyn UiDyn>> {
        self.weak()
            .upgrade()
            .map(|window| Rc::new(window) as Rc<dyn UiDyn>)
    }

    fn fetch_any(&self, source: &Arc<DataSource>, work: AnyWork, done: AnyDone) {
        source.fetch(&self.weak(), work, move |ui: &W, result| done(ui, result));
    }

    fn post(&self, f: Box<dyn FnOnce(&dyn UiDyn) + Send>) {
        page_data::ui_thread(&self.weak(), move |ui: &W| f(ui));
    }
}

/// A weak handle to the main window, whatever its type. Clones share the window. It can be sent to a worker.
#[derive(Clone)]
pub struct Ui(Arc<dyn UiBackend>);

impl Ui {
    pub fn new<W: CanvasWindow>(window: Weak<W>) -> Ui {
        Ui(Arc::new(WeakWindow(Mutex::new(window))))
    }

    /// The window, if it is still open. Only on the UI thread.
    pub fn upgrade(&self) -> Option<Rc<dyn UiDyn>> {
        self.0.upgrade()
    }

    /// [`DataSource::fetch`] on this window: `work` runs on a worker with the API and the token, `done` on the UI thread with
    /// the window.
    pub fn fetch<T, F, Fut, D>(&self, source: &Arc<DataSource>, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(Arc<ApiClient>, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, ClientError>> + 'static,
        D: FnOnce(&dyn UiDyn, Result<T, ClientError>) + Send + 'static,
    {
        let work: AnyWork = Box::new(move |api, token| {
            let future = work(api, token);
            Box::pin(async move { future.await.map(|value| Box::new(value) as AnyOut) })
                as Pin<Box<dyn Future<Output = Result<AnyOut, ClientError>>>>
        });
        let done: AnyDone = Box::new(move |ui, result| {
            let result = result.map(|value| {
                *value
                    .downcast::<T>()
                    .unwrap_or_else(|_| unreachable!("the result type of a fetch"))
            });
            done(ui, result);
        });
        self.0.fetch_any(source, work, done);
    }

    /// Runs `f` on the UI thread with the window, if it is still open (see [`page_data::ui_thread`]).
    pub fn post(&self, f: impl FnOnce(&dyn UiDyn) + Send + 'static) {
        self.0.post(Box::new(f));
    }
}

// -- The canvas list page ----------------------------------------------------------------------------------------------

/// One column of the list's table.
#[derive(Clone, Debug)]
pub struct ListColumn {
    pub title: String,
    pub width: f32,
    pub sortable: bool,
}

/// One row of a table: its cells and whether it is selected.
#[derive(Clone, Debug, PartialEq)]
pub struct ListRow {
    pub cells: Vec<String>,
    pub selected: bool,
}

/// What the list shows besides its rows: the load state, the error and the sort.
#[derive(Clone, Debug)]
pub struct ListHeader {
    pub state: LoadState,
    pub error: String,
    pub can_create: bool,
    pub show_archived: bool,
    pub sort_column: i32,
    pub sort_descending: bool,
    pub columns: Vec<ListColumn>,
}

/// The selected canvas's properties.
#[derive(Clone, Debug)]
pub struct ListSelection {
    pub index: i32,
    pub id: String,
    pub can_manage: bool,
    pub archived: bool,
    pub can_share: bool,
    pub can_move: bool,
}

/// One choice of a select: its value and its label.
pub use space_page_kit::picker::Choice as ListChoice;

/// The window side of the canvas list page. `wire_canvas!` implements it for the generated window.
pub trait CanvasListWindow: ComponentHandle + 'static {
    /// Opens a canvas route in the focused tab (the window's `canvas-open-route`).
    fn canvas_open_route(&self, route: &str);

    /// The models the dialogs start with: the role options, no move options and no share rows or results.
    fn init_models(&self, role_options: Vec<ListChoice>);
    fn show_header(&self, header: ListHeader);
    /// Replaces the table's rows in place.
    fn set_table_rows(&self, rows: Vec<ListRow>);
    /// The `selected` flag of table row `index`, when the table has it.
    fn table_row_selected(&self, index: usize) -> Option<bool>;
    fn set_table_row_selected(&self, index: usize, selected: bool);
    fn show_selection(&self, selection: ListSelection);
    /// The quota line: its text and bar fraction, or `None` to hide it.
    fn show_quota(&self, quota: Option<(String, f32)>);
    fn set_creating(&self, on: bool);
    fn open_rename(&self, id: &str, title: &str);
    fn open_delete(&self, id: &str, title: &str);
    /// The Share dialog: the title and an empty query, open.
    fn open_share(&self, id: &str, title: &str);
    fn show_share_rows(&self, rows: Vec<Vec<String>>);
    fn show_share_results(&self, rows: Vec<Vec<String>>);
    fn share_query(&self) -> String;
    fn set_share_query(&self, text: &str);
    fn set_share_message(&self, text: &str);
    /// The Move to project dialog, open, with no options yet.
    fn open_move(&self, id: &str, title: &str);
    fn set_move_options(&self, options: Vec<ListChoice>);

    fn on_opened(&self, f: impl Fn(String) + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_create(&self, f: impl Fn() + 'static);
    fn on_create_named(&self, f: impl Fn(String) + 'static);
    fn on_toggle_archived(&self, f: impl Fn() + 'static);
    fn on_sort_by(&self, f: impl Fn(i32) + 'static);
    fn on_pick(&self, f: impl Fn(i32) + 'static);
    fn on_open_row(&self, f: impl Fn(i32) + 'static);
    fn on_resize_column(&self, f: impl Fn(i32, f32) + 'static);
    fn on_row_action(&self, f: impl Fn(String, String) + 'static);
    fn on_rename_confirmed(&self, f: impl Fn(String, String) + 'static);
    fn on_delete_confirmed(&self, f: impl Fn(String) + 'static);
    fn on_share_query_changed(&self, f: impl Fn(String) + 'static);
    fn on_share_add(&self, f: impl Fn(i32) + 'static);
    fn on_share_role_changed(&self, f: impl Fn(i32, String) + 'static);
    fn on_share_remove(&self, f: impl Fn(i32) + 'static);
    fn on_move_confirmed(&self, f: impl Fn(String, String) + 'static);
}

// -- Helpers for the window glue ---------------------------------------------------------------------------------------

/// Puts `rows` into the model a global property holds, in place: changed rows are set, extra rows pushed or removed. A
/// repeater over the model then keeps its elements (and their touch areas) instead of rebuilding them, so a press and its
/// release land on the same element. A model that is not a `VecModel` yet is replaced once through `set`.
pub fn sync_model<T: Clone + PartialEq + 'static>(
    current: &ModelRc<T>,
    rows: Vec<T>,
    set: impl FnOnce(ModelRc<T>),
) {
    let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() else {
        set(ModelRc::new(VecModel::from(rows)));
        return;
    };
    let old = model.row_count();
    let new = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        if index < old {
            if model.row_data(index).as_ref() != Some(&row) {
                model.set_row_data(index, row);
            }
        } else {
            model.push(row);
        }
    }
    for index in (new..old).rev() {
        model.remove(index);
    }
}
