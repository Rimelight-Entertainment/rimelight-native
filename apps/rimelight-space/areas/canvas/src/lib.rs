//! Canvas area (CV.6 to CV.13, decision 0045): the canvas editor. Plain Rust, with no generated Slint types:
//!
//! - [`controller`] (tools, selection, dragging, keys, menu actions, undo), [`export`] (PNG and backup zips), [`images`]
//!   (pictures, thumbnails and the disk cache), [`video`] (uploads, desktop only) and [`popout_platform`] (the pop-out
//!   window's platform hooks, installed by the desktop app);
//! - the pages, which wire the windows to that logic: [`page`] (the canvas at `/home/canvases/{id}` and
//!   `/p/{KEY}/canvases/{id}`), [`list`] (the canvas lists), [`popout`] (the pop-out window) and [`actions`] (clipboard, export
//!   files, backups, asset cleanup);
//! - [`window`]: the window side as traits (the page's `CanvasState` as property and callback traits, the main window, the
//!   pop-out window and the list window), which [`wire_canvas!`] implements for the generated windows.
//!
//! The markup is in [`UI_DIR`]; the app compiles it through the `@space_area_canvas` library path.

pub use space_page_kit;

/// Re-exported so the macros of this crate name slint through `$crate` in the app crate.
#[doc(hidden)]
pub use slint as __slint;

pub mod actions;
pub mod controller;
pub mod export;
pub mod images;
pub mod list;
pub mod page;
pub mod popout;
pub mod popout_platform;
/// Canvas video uploads (CV-V.3). Desktop only.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub mod video;
pub mod window;

/// The canvas markup: `@space_area_canvas/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Implements the model-bearing part of the page window traits for one window with a `CanvasState` global. Expanded by
/// [`wire_canvas!`] for the main window and the pop-out window.
#[doc(hidden)]
#[macro_export]
macro_rules! __canvas_state_models {
    (
        $W:ty, $G:ty, $Guide:ident, $Panel:ident, $MenuRow:ident, $Hierarchy:ident,
        $Peer:ident, $PeerBox:ident, $VideoControl:ident, $AssetRow:ident, $WindowItem:ident
    ) => {
        impl $crate::window::CanvasPageState for $W {
            fn set_load_state(&self, state: $crate::space_page_kit::load::LoadState) {
                $crate::__slint::ComponentHandle::global::<$G>(self)
                    .set_state(canvas_glue::load_state(state));
            }

            fn page_copy(&self) -> $crate::window::PageCopy {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                $crate::window::PageCopy {
                    state: canvas_glue::plain_state(g.get_state()),
                    error: g.get_error().to_string(),
                    title: g.get_title().to_string(),
                    read_only: g.get_read_only(),
                    connection: g.get_connection().to_string(),
                    busy_text: g.get_busy_text().to_string(),
                }
            }

            fn apply_page_copy(&self, copy: $crate::window::PageCopy) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                g.set_state(canvas_glue::load_state(copy.state));
                g.set_error(copy.error.as_str().into());
                g.set_title(copy.title.as_str().into());
                g.set_read_only(copy.read_only);
                g.set_connection(copy.connection.as_str().into());
                g.set_busy_text(copy.busy_text.as_str().into());
            }

            fn attach_items(&self, items: &::std::rc::Rc<dyn $crate::window::ItemsModel>) {
                if let Some(items) = items.as_any().downcast_ref::<canvas_glue::Items>() {
                    $crate::__slint::ComponentHandle::global::<$G>(self)
                        .set_items($crate::__slint::ModelRc::new(items.0.clone()));
                }
            }

            fn sync_guides(&self, rows: Vec<$crate::window::GuideView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$Guide> = rows
                    .into_iter()
                    .map(|r| $Guide {
                        x1: r.x1,
                        y1: r.y1,
                        x2: r.x2,
                        y2: r.y2,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_guides(), rows, |m| g.set_guides(m));
            }

            fn sync_menu_panels(&self, rows: Vec<$crate::window::MenuPanelView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$Panel> = rows
                    .into_iter()
                    .map(|r| $Panel {
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_menu_panels(), rows, |m| g.set_menu_panels(m));
            }

            fn sync_menu_rows(&self, rows: Vec<$crate::window::MenuRowView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$MenuRow> = rows
                    .into_iter()
                    .map(|r| $MenuRow {
                        panel: r.panel,
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                        kind: r.kind.as_str().into(),
                        label: r.label.as_str().into(),
                        shortcut: r.shortcut.as_str().into(),
                        enabled: r.enabled,
                        checked: r.checked,
                        highlighted: r.highlighted,
                        toggle_left: r.toggle_left.as_str().into(),
                        toggle_right: r.toggle_right.as_str().into(),
                        toggle_right_on: r.toggle_right_on,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_menu_rows(), rows, |m| g.set_menu_rows(m));
            }

            fn sync_grid_xs(&self, rows: Vec<f32>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                $crate::window::sync_model(&g.get_grid_xs(), rows, |m| g.set_grid_xs(m));
            }

            fn sync_grid_ys(&self, rows: Vec<f32>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                $crate::window::sync_model(&g.get_grid_ys(), rows, |m| g.set_grid_ys(m));
            }

            fn sync_hierarchy_rows(&self, rows: Vec<$crate::window::HierarchyRowView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$Hierarchy> = rows
                    .into_iter()
                    .map(|r| $Hierarchy {
                        id: r.id.as_str().into(),
                        depth: r.depth,
                        label: r.label.as_str().into(),
                        kind: r.kind.as_str().into(),
                        selected: r.selected,
                        locked: r.locked,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_hierarchy_rows(), rows, |m| {
                    g.set_hierarchy_rows(m)
                });
            }

            fn sync_peers(&self, rows: Vec<$crate::window::PeerView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$Peer> = rows
                    .into_iter()
                    .map(|r| $Peer {
                        conn_id: r.conn_id.as_str().into(),
                        name: r.name.as_str().into(),
                        initials: r.initials.as_str().into(),
                        color: r.color,
                        cursor_x: r.cursor_x,
                        cursor_y: r.cursor_y,
                        has_cursor: r.has_cursor,
                        following: r.following,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_peers(), rows, |m| g.set_peers(m));
            }

            fn sync_peer_boxes(&self, rows: Vec<$crate::window::PeerBoxView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$PeerBox> = rows
                    .into_iter()
                    .map(|r| $PeerBox {
                        color: r.color,
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                        rotation: r.rotation,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_peer_boxes(), rows, |m| g.set_peer_boxes(m));
            }

            fn sync_video_controls(&self, rows: Vec<$crate::window::VideoControlView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$VideoControl> = rows
                    .into_iter()
                    .map(|r| $VideoControl {
                        id: r.id.as_str().into(),
                        x: r.x,
                        y: r.y,
                        w: r.w,
                        h: r.h,
                        playing: r.playing,
                        progress: r.progress,
                        time_text: r.time_text.as_str().into(),
                        muted: r.muted,
                        looping: r.looping,
                        loading: r.loading,
                        selected: r.selected,
                        shared: r.shared,
                        following: r.following,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_video_controls(), rows, |m| {
                    g.set_video_controls(m)
                });
            }

            fn sync_asset_rows(&self, rows: Vec<$crate::window::AssetRowView>) {
                let g = $crate::__slint::ComponentHandle::global::<$G>(self);
                let rows: Vec<$AssetRow> = rows
                    .into_iter()
                    .map(|r| $AssetRow {
                        id: r.id.as_str().into(),
                        name: r.name.as_str().into(),
                        size_text: r.size_text.as_str().into(),
                        pixels_text: r.pixels_text.as_str().into(),
                        uses: r.uses,
                        thumb: r.thumb,
                        has_thumb: r.has_thumb,
                    })
                    .collect();
                $crate::window::sync_model(&g.get_asset_rows(), rows, |m| g.set_asset_rows(m));
            }

            fn set_window_items(&self, items: Vec<$crate::window::WindowItemView>) {
                let items: Vec<$WindowItem> = items
                    .into_iter()
                    .map(|i| $WindowItem {
                        handle: i.handle.as_str().into(),
                        title: i.title.as_str().into(),
                    })
                    .collect();
                $crate::__slint::ComponentHandle::global::<$G>(self).set_window_items(
                    $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(items)),
                );
            }
        }
    };
}

/// Expands, in the app crate, the canvas window glue: the trait implementations of the page, the pop-out and the list for the
/// generated windows, and the `canvas_glue` module (the item rows' model and the conversions the traits use).
///
/// The arguments are the generated types, in this order: the main window, `CanvasPopout`, `PageLoadState`, `CanvasState`,
/// `CanvasItemView`, `CanvasGuide`, `CanvasMenuPanel`, `CanvasMenuRow`, `CanvasHierarchyRow`, `CanvasPeer`, `CanvasPeerBox`,
/// `CanvasVideoControl`, `CanvasAssetRow`, `CanvasWindowItem`, `CanvasListState`, `TableColumn`, `TableRow` and `SelectOption`.
#[macro_export]
macro_rules! wire_canvas {
    (
        $AppWindow:ident, $CanvasPopout:ident, $PageLoadState:ident, $CanvasState:ident, $CanvasItemView:ident,
        $CanvasGuide:ident, $CanvasMenuPanel:ident, $CanvasMenuRow:ident, $CanvasHierarchyRow:ident,
        $CanvasPeer:ident, $CanvasPeerBox:ident, $CanvasVideoControl:ident, $CanvasAssetRow:ident,
        $CanvasWindowItem:ident, $CanvasListState:ident, $TableColumn:ident, $TableRow:ident,
        $SelectOption:ident $(,)?
    ) => {
        /// The item rows' model, the load-state mapping and the row conversions of the canvas glue.
        pub mod canvas_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            use $crate::__slint::{Model as _, ModelRc, SharedString, VecModel};

            $crate::space_page_kit::load_glue!($PageLoadState);

            /// The plain load state of a generated one.
            pub fn plain_state(state: $PageLoadState) -> $crate::space_page_kit::load::LoadState {
                match state {
                    $PageLoadState::Loading => $crate::space_page_kit::load::LoadState::Loading,
                    $PageLoadState::Loaded => $crate::space_page_kit::load::LoadState::Loaded,
                    $PageLoadState::Empty => $crate::space_page_kit::load::LoadState::Empty,
                    $PageLoadState::Error => $crate::space_page_kit::load::LoadState::Error,
                }
            }

            /// The generated item row of a plain one.
            pub fn item(v: $crate::window::ItemView) -> $CanvasItemView {
                $CanvasItemView {
                    id: v.id.as_str().into(),
                    kind: v.kind.as_str().into(),
                    x: v.x,
                    y: v.y,
                    w: v.w,
                    h: v.h,
                    rotation: v.rotation,
                    flip_x: v.flip_x,
                    flip_y: v.flip_y,
                    opacity: v.opacity,
                    image: v.image,
                    has_image: v.has_image,
                    pixelated: v.pixelated,
                    text: v.text.as_str().into(),
                    color: v.color.as_str().into(),
                    fill: v.fill.as_str().into(),
                    font_size: v.font_size,
                    align: v.align.as_str().into(),
                    locked: v.locked,
                    selected: v.selected,
                    editing: v.editing,
                    crop_x: v.crop_x,
                    crop_y: v.crop_y,
                    crop_w: v.crop_w,
                    crop_h: v.crop_h,
                    path: v.path.as_str().into(),
                    shape: v.shape.as_str().into(),
                    stroke_width: v.stroke_width,
                    stroke_color: v.stroke_color.as_str().into(),
                    fill_color: v.fill_color.as_str().into(),
                    comment: v.comment.as_str().into(),
                    gray: v.gray,
                    is_video: v.is_video,
                    duration_text: v.duration_text.as_str().into(),
                }
            }

            /// The one rows model of the canvas items, shared by the main window and the pop-out.
            pub struct Items(pub ::std::rc::Rc<VecModel<$CanvasItemView>>);

            impl $crate::window::ItemsModel for Items {
                fn row_count(&self) -> usize {
                    self.0.row_count()
                }
                fn set_row_data(&self, index: usize, view: $crate::window::ItemView) {
                    self.0.set_row_data(index, item(view));
                }
                fn set_vec(&self, views: Vec<$crate::window::ItemView>) {
                    self.0
                        .set_vec(views.into_iter().map(item).collect::<Vec<_>>());
                }
                fn as_any(&self) -> &dyn ::std::any::Any {
                    self
                }
            }

            /// A choice as the generated `SelectOption` (no icon, badge or group).
            pub fn select_option(choice: &$crate::space_page_kit::picker::Choice) -> $SelectOption {
                $SelectOption {
                    value: choice.value.as_str().into(),
                    label: choice.label.as_str().into(),
                    icon: Default::default(),
                    badge: Default::default(),
                    disabled: false,
                    group: Default::default(),
                }
            }

            /// The generated row of plain cells and a selected flag.
            pub fn table_row(cells: Vec<String>, selected: bool) -> $TableRow {
                $TableRow {
                    cells: ModelRc::new(VecModel::from(
                        cells
                            .into_iter()
                            .map(SharedString::from)
                            .collect::<Vec<_>>(),
                    )),
                    selected,
                }
            }

            /// The Table model of plain rows (no row is selected).
            pub fn plain_rows(rows: Vec<Vec<String>>) -> ModelRc<$TableRow> {
                ModelRc::new(VecModel::from(
                    rows.into_iter()
                        .map(|cells| table_row(cells, false))
                        .collect::<Vec<_>>(),
                ))
            }
        }

        // The page's `CanvasState` properties and callbacks, for the main window and for the pop-out window.
        $crate::__canvas_state_table!(__canvas_state_impl [$AppWindow, $CanvasState,]);
        $crate::__canvas_state_table!(__canvas_state_impl [$CanvasPopout, $CanvasState,]);

        // The page's models, for both windows.
        $crate::__canvas_state_models!(
            $AppWindow, $CanvasState, $CanvasGuide, $CanvasMenuPanel, $CanvasMenuRow, $CanvasHierarchyRow,
            $CanvasPeer, $CanvasPeerBox, $CanvasVideoControl, $CanvasAssetRow, $CanvasWindowItem
        );
        $crate::__canvas_state_models!(
            $CanvasPopout, $CanvasState, $CanvasGuide, $CanvasMenuPanel, $CanvasMenuRow, $CanvasHierarchyRow,
            $CanvasPeer, $CanvasPeerBox, $CanvasVideoControl, $CanvasAssetRow, $CanvasWindowItem
        );

        impl $crate::window::CanvasWindow for $AppWindow {
            type Popout = $CanvasPopout;

            fn canvas_open_route(&self, route: &str) {
                <$AppWindow>::invoke_canvas_open_route(self, route.into());
            }

            fn canvas_close_tab(&self) {
                <$AppWindow>::invoke_canvas_close_tab(self);
            }

            fn new_items(&self) -> ::std::rc::Rc<dyn $crate::window::ItemsModel> {
                ::std::rc::Rc::new(canvas_glue::Items(::std::rc::Rc::new(
                    $crate::__slint::VecModel::default(),
                )))
            }
        }

        impl $crate::window::PopoutProps for $CanvasPopout {
            fn get_overlay_mode(&self) -> bool {
                <$CanvasPopout>::get_overlay_mode(self)
            }
            fn set_overlay_mode(&self, value: bool) {
                <$CanvasPopout>::set_overlay_mode(self, value);
            }
            fn get_click_through(&self) -> bool {
                <$CanvasPopout>::get_click_through(self)
            }
            fn set_click_through(&self, value: bool) {
                <$CanvasPopout>::set_click_through(self, value);
            }
            fn get_pinned(&self) -> bool {
                <$CanvasPopout>::get_pinned(self)
            }
            fn set_pinned(&self, value: bool) {
                <$CanvasPopout>::set_pinned(self, value);
            }
            fn get_locked(&self) -> bool {
                <$CanvasPopout>::get_locked(self)
            }
            fn set_locked(&self, value: bool) {
                <$CanvasPopout>::set_locked(self, value);
            }
            fn get_window_opacity(&self) -> f32 {
                <$CanvasPopout>::get_window_opacity(self)
            }
            fn set_window_opacity(&self, value: f32) {
                <$CanvasPopout>::set_window_opacity(self, value);
            }
        }

        impl $crate::window::PopoutWindow for $CanvasPopout {
            fn create() -> Result<Self, $crate::__slint::PlatformError> {
                <$CanvasPopout>::new()
            }
            fn on_bar_drag(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_drag(self, move || f());
            }
            fn on_bar_toggle_pin(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_toggle_pin(self, move || f());
            }
            fn on_bar_toggle_click_through(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_toggle_click_through(self, move || f());
            }
            fn on_bar_minimize(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_minimize(self, move || f());
            }
            fn on_bar_maximize(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_maximize(self, move || f());
            }
            fn on_bar_close(&self, f: impl Fn() + 'static) {
                <$CanvasPopout>::on_bar_close(self, move || f());
            }
            fn on_bar_opacity(&self, f: impl Fn(f32) + 'static) {
                <$CanvasPopout>::on_bar_opacity(self, move |v| f(v));
            }
        }

        impl $crate::window::CanvasListWindow for $AppWindow {
            fn canvas_open_route(&self, route: &str) {
                <$AppWindow>::invoke_canvas_open_route(self, route.into());
            }

            fn init_models(&self, role_options: Vec<$crate::window::ListChoice>) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                let options: Vec<$SelectOption> =
                    role_options.iter().map(canvas_glue::select_option).collect();
                g.set_share_role_options($crate::__slint::ModelRc::new(
                    $crate::__slint::VecModel::from(options),
                ));
                g.set_move_options($crate::__slint::ModelRc::new(
                    $crate::__slint::VecModel::from(Vec::<$SelectOption>::new()),
                ));
                g.set_share_rows(canvas_glue::plain_rows(Vec::new()));
                g.set_share_results(canvas_glue::plain_rows(Vec::new()));
            }

            fn show_header(&self, header: $crate::window::ListHeader) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_state(canvas_glue::load_state(header.state));
                g.set_error(header.error.as_str().into());
                g.set_can_create(header.can_create);
                g.set_show_archived(header.show_archived);
                g.set_sort_column(header.sort_column);
                g.set_sort_descending(header.sort_descending);
                let columns: Vec<$TableColumn> = header
                    .columns
                    .into_iter()
                    .map(|c| $TableColumn {
                        title: c.title.as_str().into(),
                        width: c.width,
                        sortable: c.sortable,
                    })
                    .collect();
                g.set_table_columns($crate::__slint::ModelRc::new(
                    $crate::__slint::VecModel::from(columns),
                ));
            }

            fn set_table_rows(&self, rows: Vec<$crate::window::ListRow>) {
                use $crate::__slint::Model as _;
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                let rows: Vec<$TableRow> = rows
                    .into_iter()
                    .map(|r| canvas_glue::table_row(r.cells, r.selected))
                    .collect();
                let model = g.get_table_rows();
                match model
                    .as_any()
                    .downcast_ref::<$crate::__slint::VecModel<$TableRow>>()
                {
                    Some(table) => table.set_vec(rows),
                    None => g.set_table_rows($crate::__slint::ModelRc::new(
                        $crate::__slint::VecModel::from(rows),
                    )),
                }
            }

            fn table_row_selected(&self, index: usize) -> Option<bool> {
                use $crate::__slint::Model as _;
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                let model = g.get_table_rows();
                model
                    .as_any()
                    .downcast_ref::<$crate::__slint::VecModel<$TableRow>>()
                    .and_then(|table| table.row_data(index))
                    .map(|row| row.selected)
            }

            fn set_table_row_selected(&self, index: usize, selected: bool) {
                use $crate::__slint::Model as _;
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                let model = g.get_table_rows();
                if let Some(table) = model
                    .as_any()
                    .downcast_ref::<$crate::__slint::VecModel<$TableRow>>()
                    && let Some(mut row) = table.row_data(index)
                {
                    row.selected = selected;
                    table.set_row_data(index, row);
                }
            }

            fn show_selection(&self, selection: $crate::window::ListSelection) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_selected_index(selection.index);
                g.set_selected_id(selection.id.as_str().into());
                g.set_selected_can_manage(selection.can_manage);
                g.set_selected_archived(selection.archived);
                g.set_selected_can_share(selection.can_share);
                g.set_selected_can_move(selection.can_move);
            }

            fn show_quota(&self, quota: Option<(String, f32)>) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                match quota {
                    Some((text, fraction)) => {
                        g.set_quota_shown(true);
                        g.set_quota_text(text.as_str().into());
                        g.set_quota_fraction(fraction);
                    }
                    None => g.set_quota_shown(false),
                }
            }

            fn set_creating(&self, on: bool) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_creating(on);
            }

            fn open_rename(&self, id: &str, title: &str) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_rename_id(id.into());
                g.set_rename_title(title.into());
                g.set_rename_open(true);
            }

            fn open_delete(&self, id: &str, title: &str) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_delete_id(id.into());
                g.set_delete_title(title.into());
                g.set_delete_open(true);
            }

            fn open_share(&self, id: &str, title: &str) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_share_id(id.into());
                g.set_share_title(title.into());
                g.set_share_query("".into());
                g.set_share_open(true);
            }

            fn show_share_rows(&self, rows: Vec<Vec<String>>) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_share_rows(canvas_glue::plain_rows(rows));
            }

            fn show_share_results(&self, rows: Vec<Vec<String>>) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_share_results(canvas_glue::plain_rows(rows));
            }

            fn share_query(&self) -> String {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .get_share_query()
                    .to_string()
            }

            fn set_share_query(&self, text: &str) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_share_query(text.into());
            }

            fn set_share_message(&self, text: &str) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_share_message(text.into());
            }

            fn open_move(&self, id: &str, title: &str) {
                let g = $crate::__slint::ComponentHandle::global::<$CanvasListState>(self);
                g.set_move_id(id.into());
                g.set_move_title(title.into());
                g.set_move_key("".into());
                g.set_move_options($crate::__slint::ModelRc::new(
                    $crate::__slint::VecModel::from(Vec::<$SelectOption>::new()),
                ));
                g.set_move_open(true);
            }

            fn set_move_options(&self, options: Vec<$crate::window::ListChoice>) {
                let options: Vec<$SelectOption> =
                    options.iter().map(canvas_glue::select_option).collect();
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .set_move_options($crate::__slint::ModelRc::new(
                        $crate::__slint::VecModel::from(options),
                    ));
            }

            fn on_opened(&self, f: impl Fn(String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_retry(move || f());
            }
            fn on_create(&self, f: impl Fn() + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_create(move || f());
            }
            fn on_create_named(&self, f: impl Fn(String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_create_named(move |title| f(title.to_string()));
            }
            fn on_toggle_archived(&self, f: impl Fn() + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_toggle_archived(move || f());
            }
            fn on_sort_by(&self, f: impl Fn(i32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_sort_by(move |column| f(column));
            }
            fn on_pick(&self, f: impl Fn(i32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_pick(move |index| f(index));
            }
            fn on_open_row(&self, f: impl Fn(i32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_open_row(move |index| f(index));
            }
            fn on_resize_column(&self, f: impl Fn(i32, f32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_resize_column(move |column, width| f(column, width));
            }
            fn on_row_action(&self, f: impl Fn(String, String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_row_action(move |id, action| f(id.to_string(), action.to_string()));
            }
            fn on_rename_confirmed(&self, f: impl Fn(String, String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_rename_confirmed(move |id, title| f(id.to_string(), title.to_string()));
            }
            fn on_delete_confirmed(&self, f: impl Fn(String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_delete_confirmed(move |id| f(id.to_string()));
            }
            fn on_share_query_changed(&self, f: impl Fn(String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_share_query_changed(move |query| f(query.to_string()));
            }
            fn on_share_add(&self, f: impl Fn(i32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_share_add(move |index| f(index));
            }
            fn on_share_role_changed(&self, f: impl Fn(i32, String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_share_role_changed(move |index, role| f(index, role.to_string()));
            }
            fn on_share_remove(&self, f: impl Fn(i32) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_share_remove(move |index| f(index));
            }
            fn on_move_confirmed(&self, f: impl Fn(String, String) + 'static) {
                $crate::__slint::ComponentHandle::global::<$CanvasListState>(self)
                    .on_move_confirmed(move |id, key| f(id.to_string(), key.to_string()));
            }
        }
    };
}
