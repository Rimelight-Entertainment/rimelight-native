//! Canvas page at `/home/canvases/{id}` and `/p/{KEY}/canvases/{id}` (CV.6, CV.7 to CV.13). The page's markup is
//! `areas/canvas/ui/canvas.slint` and its `CanvasState` global. Every rule of the editor (tools, selection, dragging, keys,
//! menus, undo) is in [`crate::controller`]; this file wires that to the window (or to the pop-out window, see
//! [`crate::popout`]), the live room, the pictures and the presence of the others.
//!
//! Route: [`canvas_id_of`] reads the canvas id from either route, lower-cased. The page opens (its `opened` callback):
//! the canvas snapshot is read with the REST client (it carries the assets, so their sizes and thumbnails are known at
//! once), then a live session connects to its room. The room's hello loads the whole document into the controller; each
//! later message (an ack of our own change, or someone else's) is applied to it. Our edits go out as `ClientOp`s through
//! the session. With no session (tests, or before the room connects) they are kept in `outbox` instead.
//!
//! Hosts: the page draws into the main window's `CanvasState`, or into the pop-out window's (`Host::Popout`) while the
//! canvas is popped out. Every state read and write goes through `Inner::with_state` on the active host. Toasts always go
//! to the main window.
//!
//! Pictures: an image item draws the picture of its level of detail (the 256 or 1024 thumbnail, or the original), chosen
//! from its on-screen size by [`images::level_for`]. Pictures load through [`DataSource::fetch`], at most [`MAX_LOADS`] at a
//! time, from the disk cache when there is one. A missing thumbnail is made from the original and uploaded once. Animated
//! GIF and WebP pictures tick through their frames on a timer while one is on screen.
//!
//! Test hooks ([`CanvasPage::deliver_live`], [`CanvasPage::sent_ops`], [`CanvasPage::set_image`]) act on the page's handle
//! directly. A test window has no [`DataSource`], so nothing is fetched and no session opens.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use std::path::{Path, PathBuf};

use space_canvas::geom::{Camera, Rect, RotRect, Vec2};
use space_canvas::grid::GridMode;
use space_canvas::model::{Item, ItemData, ItemKind};
use space_client::canvas_live::{EventSink, LiveEvent, LiveSession};
use space_client::error::ClientError;
use space_proto::canvases::{
    AssetUsage, AssetView, CanvasView, ClientOp, CreateCanvas, ItemState, MediaControl, MediaState,
    Peer, ServerMessage, Snapshot,
};

use crate::actions;
use crate::controller::{
    Button, Controller, EditField, Effect, ExportCommand, ItemRow, Mods, PointerKind,
    tool_from_name, tool_name,
};
use crate::images::{self, DiskCache};
use crate::popout;
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
use crate::video;
use crate::window::{
    AssetRowView, CanvasPageEvents, CanvasPageState, CanvasWindow, GuideView, HierarchyRowView,
    ItemView, ItemsModel, MenuPanelView, MenuRowView, PeerBoxView, PeerView, PopoutWindow, Ui,
    UiDyn, VideoControlView, WeakPopout,
};
use rimelight_ui_kit::toast::{self, Tone};
use space_page_kit::load::LoadState;
use space_page_kit::page_data::{self, DataCell};

/// How many pictures load at once.
const MAX_LOADS: usize = 4;

/// How many videos decode at once. The oldest one started is stopped when another starts.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
const MAX_PLAYERS: usize = 4;

/// The frame timer of the videos: it runs while a video plays.
const VIDEO_EVERY: Duration = Duration::from_millis(8);

/// Presence is sent at most this often while the pointer moves.
const PRESENCE_EVERY: Duration = Duration::from_millis(50);

/// The frame timer of animated pictures.
const FRAME_EVERY: Duration = Duration::from_millis(33);

/// The slideshow's step while it plays.
const SLIDE_EVERY: Duration = Duration::from_secs(3);

thread_local! {
    /// The page the UI thread's closures act on. Set by [`wire`]; a closure that finds the page gone does nothing.
    static PAGE: RefCell<Option<std::rc::Weak<RefCell<Inner>>>> = const { RefCell::new(None) };

    /// Backup items waiting for the room of their new canvas (see [`queue_import`]).
    static PENDING_IMPORT: RefCell<Option<(String, Vec<Item>)>> = const { RefCell::new(None) };
}

/// Runs `f` on the page, if it is still open. Only on the UI thread.
fn with_page(f: impl FnOnce(&mut Inner)) {
    if let Some(page) = current_page() {
        f(&mut page.inner.borrow_mut());
    }
}

/// The page's handle, if it is still open. Only on the UI thread.
fn current_page() -> Option<CanvasPage> {
    PAGE.with(|slot| slot.borrow().as_ref().and_then(std::rc::Weak::upgrade))
        .map(|inner| CanvasPage { inner })
}

/// Runs `f` with the page after the current call returns, so it may borrow the page itself (a window command can open
/// the pop-out, which takes the page).
fn defer(f: impl FnOnce(&CanvasPage) + 'static) {
    let mut once = Some(f);
    slint::Timer::single_shot(Duration::ZERO, move || {
        if let (Some(f), Some(page)) = (once.take(), current_page()) {
            f(&page);
        }
    });
}

/// Where the page draws: the main window, or the pop-out window while the canvas is popped out.
enum Host {
    Main,
    Popout(WeakPopout),
}

/// A decoded picture and its greyscale copy, made on the worker.
struct Loaded {
    decoded: images::Decoded,
    grey: images::Decoded,
}

/// A picture of one asset at one level, ready to draw: its frames in colour and in grey, with the delays and the first
/// frame's pixels (for the eyedropper and the export).
struct Picture {
    color: Vec<slint::Image>,
    grey: Vec<slint::Image>,
    delays_ms: Vec<u32>,
    width: u32,
    height: u32,
    first: images::Buffer,
}

impl Picture {
    /// The picture of a decoded file and its grey copy.
    fn new(decoded: &images::Decoded, grey: &images::Decoded) -> Picture {
        Picture {
            color: decoded.frames.iter().map(page_data::picture).collect(),
            grey: grey.frames.iter().map(page_data::picture).collect(),
            delays_ms: decoded.delays_ms.clone(),
            width: decoded.width,
            height: decoded.height,
            first: decoded.first().clone(),
        }
    }

    /// A still picture that is already a Slint image (the test hook).
    fn from_image(image: slint::Image) -> Picture {
        let size = image.size();
        Picture {
            color: vec![image.clone()],
            grey: vec![image],
            delays_ms: vec![100],
            width: size.width,
            height: size.height,
            first: images::Buffer::new(1, 1),
        }
    }
}

/// What an item draws from a picture: the level, the frame, the grey copy, and whether the picture is animated. Two equal
/// keys draw the same image, so the item is redrawn only when its key changes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PictureKey {
    level: u32,
    frame: usize,
    gray: bool,
    animated: bool,
}

/// One picture load: which asset, at which level (0 = original, 256 or 1024 = thumbnail), with what the snapshot knows.
/// A video's job never loads its original: it takes its thumbnail or nothing (`video`).
struct LoadJob {
    canvas: String,
    asset: String,
    sha: String,
    thumbs: Vec<u32>,
    level: u32,
    video: bool,
}

/// A video on this page being played: its player, the last frame drawn, when it started, and the size its output is set to.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
struct Playback {
    player: rimelight_media::Player,
    frame: Option<slint::Image>,
    started: Instant,
    size: (u32, u32),
}

/// What the page knows of a video's playback: whether it plays, its position and length (ms), and whether its file is
/// being downloaded to play.
#[derive(Default, Clone, Copy)]
struct VideoStatus {
    playing: bool,
    position_ms: u64,
    duration_ms: u64,
    loading: bool,
}

/// What a picture load gives back: the picture, and the asset when a missing thumbnail was made and uploaded.
type LoadResult = Result<(Loaded, Option<AssetView>), ClientError>;

/// Which export draws the picture of the items: the whole canvas or the selection.
#[derive(Clone, Copy)]
enum Want {
    Canvas,
    Selection,
}

/// The page's state. Shared by the callbacks of the windows and by the results of workers (through [`with_page`]).
struct Inner {
    ui: Ui,
    host: Host,
    data: DataCell,
    /// The route the page last opened, for Try again.
    route: String,
    /// The canvas area's last reported size. The area reports it once when it appears, which can be before the room's
    /// hello creates the controller, so it is kept here and handed over then.
    viewport: (f64, f64),
    /// The canvas on the page, once it is opened.
    canvas_id: Option<String>,
    /// Bumped by every open. A result that answers for an older open is dropped.
    open_generation: u64,
    /// The viewer may only look (the canvas is shared with view access).
    view_only: bool,
    /// The canvas is archived: it is read-only.
    archived: bool,
    /// The connection id of this socket, so our own presence is not drawn as a peer.
    self_conn: Option<String>,
    controller: Option<Controller>,
    session: Option<LiveSession>,
    /// The canvas's assets (from the snapshot, then uploads and thumbnails), by id.
    assets: HashMap<String, AssetView>,
    /// Loaded pictures by asset id, then by level (0 = original, 256 or 1024 = thumbnail).
    pictures: HashMap<String, HashMap<u32, Picture>>,
    /// Assets whose original could not be loaded (Relink lists them).
    missing: HashSet<String>,
    /// Videos whose thumbnail could not be loaded. They are not asked for again until a thumbnail arrives.
    no_thumb: HashSet<String>,
    /// Videos on this page being played, by item id (desktop only).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    players: HashMap<String, Playback>,
    /// Videos whose file is being downloaded to play, by item id (desktop only).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    loading_videos: HashSet<String>,
    /// The frame timer of the playing videos.
    video_timer: slint::Timer,
    /// Picture loads in flight, and the ones queued.
    loading: HashSet<(String, u32)>,
    load_queue: VecDeque<(String, u32)>,
    loads_running: usize,
    /// The level each image item wants, by item id (kept for the hysteresis of [`images::level_for`]).
    levels: HashMap<String, u32>,
    /// The rows last drawn, in draw order, and the picture key each one drew: a redraw only replaces what changed.
    last_rows: Vec<ItemRow>,
    last_keys: Vec<Option<PictureKey>>,
    items_model: Rc<dyn ItemsModel>,
    /// True while an animated picture is on screen: the frame timer runs.
    animating: bool,
    anim_timer: slint::Timer,
    slide_timer: slint::Timer,
    started: Instant,
    cache: Arc<DiskCache>,
    /// The other people in the room, by their last presence (or join).
    peers: Vec<Peer>,
    /// The peer whose viewport the camera follows.
    follow_id: Option<String>,
    /// What the text editor is for: "text", "comment" or "title".
    edit_field: &'static str,
    last_presence: Option<Instant>,
    presence_armed: bool,
    /// The manage-images dialog's rows (the usage the server gave), and whether the dialog is open.
    usage: Vec<AssetUsage>,
    assets_open: bool,
    /// Relink: the missing assets still to pick, each with the items that use it.
    relink: VecDeque<(String, Vec<String>)>,
    /// An eyedropper click waiting for the original picture: asset and the click's fractions.
    eyedrop: Option<(String, f64, f64)>,
    /// Operations made while there is no session. Kept for the tests; a live page sends them.
    outbox: Vec<ClientOp>,
    /// The shared video playbacks of the room, by item id (CV-V.5). Held in memory only.
    shared: HashMap<String, MediaState>,
    /// Items whose shared playback this person left (they do not follow it until they join back).
    left: HashSet<String>,
}

/// The canvas page's handle, held by the app. Clones share the page.
#[derive(Clone)]
pub struct CanvasPage {
    inner: Rc<RefCell<Inner>>,
}

/// Queues the items of a backup for the room of `canvas_id` (the canvas [`actions::open_backup`] just created). The page
/// imports them after the room's hello.
pub fn queue_import(canvas_id: String, items: Vec<Item>) {
    PENDING_IMPORT.with(|slot| *slot.borrow_mut() = Some((canvas_id.to_ascii_lowercase(), items)));
}

/// The items queued for `canvas`, when they were queued for it. Taking them clears the queue.
fn take_import(canvas: &str) -> Option<Vec<Item>> {
    PENDING_IMPORT.with(|slot| {
        let mut queued = slot.borrow_mut();
        match queued.take() {
            Some((id, items)) if id == canvas => Some(items),
            other => {
                *queued = other;
                None
            }
        }
    })
}

/// Wires the main window's `CanvasState` callbacks to the page. `data` is the slot the session driver fills; until it is
/// set, the page stays in its loading state and nothing is fetched.
pub fn wire<W: CanvasWindow>(ui: &W, data: DataCell) -> CanvasPage {
    let items = ui.new_items();
    let inner = Rc::new(RefCell::new(Inner::new(
        Ui::new(ui.as_weak()),
        data,
        items.clone(),
    )));
    PAGE.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&inner)));
    ui.attach_items(&items);
    let page = CanvasPage { inner };
    popout::register::<W::Popout>();
    wire_state(ui, &page);
    page
}

/// Wires every callback of one `CanvasState` to the page. Used for the main window and for the pop-out window.
fn wire_state<S: CanvasPageEvents + CanvasPageState>(state: &S, page: &CanvasPage) {
    let target = page.clone();
    state.on_opened(move |route| {
        // The pop-out's own page has no route: its open is ignored.
        if !route.is_empty() {
            target.inner.borrow_mut().open(route.as_str());
        }
    });

    let target = page.clone();
    state.on_retry(move || {
        let mut inner = target.inner.borrow_mut();
        // Forces a full open, even when the page still holds this canvas.
        inner.canvas_id = None;
        let route = inner.route.clone();
        inner.open(&route);
    });

    let target = page.clone();
    state.on_pointer(move |kind, x, y, button, shift, ctrl, alt| {
        target.inner.borrow_mut().pointer(
            kind.as_str(),
            Vec2 {
                x: f64::from(x),
                y: f64::from(y),
            },
            button.as_str(),
            Mods { shift, ctrl, alt },
        );
    });

    let target = page.clone();
    state.on_double_click(move |x, y| {
        target.inner.borrow_mut().double_click(Vec2 {
            x: f64::from(x),
            y: f64::from(y),
        });
    });

    let target = page.clone();
    state.on_scroll(move |x, y, dx, dy, ctrl, shift| {
        target.inner.borrow_mut().scroll(
            Vec2 {
                x: f64::from(x),
                y: f64::from(y),
            },
            Vec2 {
                x: f64::from(dx),
                y: f64::from(dy),
            },
            Mods {
                shift,
                ctrl,
                alt: false,
            },
        );
    });

    let target = page.clone();
    state.on_key(move |text, shift, ctrl, alt| {
        target
            .inner
            .borrow_mut()
            .key(text.as_str(), Mods { shift, ctrl, alt })
    });

    let target = page.clone();
    state.on_viewport(move |w, h| {
        target
            .inner
            .borrow_mut()
            .viewport(f64::from(w), f64::from(h));
    });

    let target = page.clone();
    state.on_set_tool(move |name| target.inner.borrow_mut().set_tool(name.as_str()));

    let target = page.clone();
    state.on_zoom_in(move || target.inner.borrow_mut().act(|c| c.zoom_in()));

    let target = page.clone();
    state.on_zoom_out(move || target.inner.borrow_mut().act(|c| c.zoom_out()));

    let target = page.clone();
    state.on_zoom_reset(move || target.inner.borrow_mut().act(|c| c.zoom_reset()));

    let target = page.clone();
    state.on_zoom_fit(move || target.inner.borrow_mut().act(|c| c.zoom_fit()));

    let target = page.clone();
    state.on_undo(move || target.inner.borrow_mut().act(|c| c.undo()));

    let target = page.clone();
    state.on_redo(move || target.inner.borrow_mut().act(|c| c.redo()));

    let target = page.clone();
    state.on_add_image(move || target.inner.borrow_mut().add_image());

    let target = page.clone();
    state.on_menu_hover(move |row| target.inner.borrow_mut().act(|c| c.menu_hover(row)));

    let target = page.clone();
    state.on_menu_click(move |row, x| {
        target
            .inner
            .borrow_mut()
            .act(|c| c.menu_click(row, f64::from(x)));
    });

    let target = page.clone();
    state.on_menu_dismiss(move || target.inner.borrow_mut().act(|c| c.menu_dismiss()));

    let target = page.clone();
    state.on_text_commit(move |text| {
        let text = text.to_string();
        target.inner.borrow_mut().act(|c| c.commit_text(&text));
    });

    let target = page.clone();
    state.on_text_cancel(move || target.inner.borrow_mut().act(|c| c.cancel_text()));

    let target = page.clone();
    state.on_rename(move |name| target.inner.borrow_mut().rename(name.as_str()));

    let target = page.clone();
    state.on_follow(move |conn| target.inner.borrow_mut().follow(conn.as_str()));

    let target = page.clone();
    state.on_hierarchy_pick(move |id, shift| {
        target
            .inner
            .borrow_mut()
            .act(|c| c.hierarchy_pick(id.as_str(), shift));
    });

    let target = page.clone();
    state.on_hierarchy_close(move || target.inner.borrow_mut().act(Controller::close_hierarchy));

    let target = page.clone();
    state.on_hierarchy_open_item(move |id| {
        target
            .inner
            .borrow_mut()
            .act(|c| c.hierarchy_open_item(id.as_str()));
    });

    let target = page.clone();
    state.on_slideshow_step(move |delta| {
        target.inner.borrow_mut().act(|c| c.slideshow_step(delta));
    });

    let target = page.clone();
    state.on_slideshow_toggle(move || target.inner.borrow_mut().act(|c| c.slideshow_toggle()));

    let target = page.clone();
    state.on_slideshow_close(move || target.inner.borrow_mut().act(|c| c.slideshow_close()));

    let target = page.clone();
    state.on_assets_delete_unused(move || target.inner.borrow_mut().delete_unused());

    let target = page.clone();
    state.on_assets_close(move || target.inner.borrow_mut().close_assets());

    state.on_window_picked(move |handle| popout::set_owner_from_picker(handle.as_str()));

    let target = page.clone();
    state.on_windows_close(move || {
        target
            .inner
            .borrow()
            .with_state(|state| state.set_windows_open(false));
    });

    let target = page.clone();
    state.on_bring_back(move || popout::close(&target));

    let target = page.clone();
    state.on_pop_out(move || popout::open(&target));

    let target = page.clone();
    state.on_video_toggle(move |id| target.inner.borrow_mut().toggle_video(id.as_str()));

    let target = page.clone();
    state.on_video_seek(move |id, fraction| {
        target
            .inner
            .borrow_mut()
            .seek_video(id.as_str(), f64::from(fraction));
    });

    let target = page.clone();
    state.on_video_mute(move |id| {
        target
            .inner
            .borrow_mut()
            .flip_video_flag(id.as_str(), "muted");
    });

    let target = page.clone();
    state.on_video_loop(move |id| {
        target
            .inner
            .borrow_mut()
            .flip_video_flag(id.as_str(), "loop")
    });

    let target = page.clone();
    state.on_video_together(move |id| target.inner.borrow_mut().video_together(id.as_str()));
}

impl CanvasPage {
    /// Moves the canvas into the pop-out window: the window's own `CanvasState` is wired, and the page draws there. The
    /// main window shows its placeholder. Called by [`popout::open`].
    pub fn pop_out<P: PopoutWindow>(&self, window: &P) {
        wire_state(window, self);
        if let Some(ui) = self.inner.borrow().ui.upgrade() {
            copy_page_state(&*ui, window);
        }
        let mut inner = self.inner.borrow_mut();
        inner.host = Host::Popout(WeakPopout::new(window));
        inner.set_main_popped_out(true);
        inner.attach_items();
        if let Some(controller) = inner.controller.as_mut() {
            controller.set_popout(true);
        }
        inner.refresh();
    }

    /// Brings the canvas back into the main window: the main page draws it again. Called when the pop-out closes.
    pub fn bring_back(&self) {
        let mut inner = self.inner.borrow_mut();
        let Host::Popout(window) = &inner.host else {
            return;
        };
        if let (Some(window), Some(ui)) = (window.upgrade(), inner.ui.upgrade()) {
            copy_page_state(&*window, &*ui);
        }
        inner.host = Host::Main;
        inner.set_main_popped_out(false);
        inner.attach_items();
        if let Some(controller) = inner.controller.as_mut() {
            controller.set_popout(false);
        }
        inner.refresh();
    }

    /// Redraws the canvas, for example after the pop-out's overlay selection mode changed which items show.
    pub fn redraw(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.refresh();
        }
    }

    /// Delivers a live room event, as the session would. For tests.
    pub fn deliver_live(&self, event: LiveEvent) {
        self.inner.borrow_mut().handle_live(event);
    }

    /// The operations sent while there is no session, in order. For tests.
    pub fn sent_ops(&self) -> Vec<ClientOp> {
        self.inner.borrow().outbox.clone()
    }

    /// Gives an image asset its picture, as a finished load would, and redraws. For tests.
    pub fn set_image(&self, asset_id: &str, image: slint::Image) {
        let mut inner = self.inner.borrow_mut();
        inner
            .pictures
            .entry(asset_id.to_string())
            .or_default()
            .insert(0, Picture::from_image(image));
        inner.refresh();
    }

    /// Adds images as the picker does: one block at the view's centre, with the effects run. For tests.
    pub fn add_assets_for_test(&self, assets: Vec<AssetView>) {
        let mut inner = self.inner.borrow_mut();
        for asset in &assets {
            inner.store_asset(asset.clone());
        }
        let (videos, images): (Vec<AssetView>, Vec<AssetView>) =
            assets.into_iter().partition(AssetView::is_video);
        inner.act(|c| {
            let at = c.view_center();
            c.add_images(&images, at)
        });
        inner.act(|c| {
            let at = c.view_center();
            c.add_videos(&videos, at)
        });
    }
}

/// Files are being dragged over the window: shows or hides the drop overlay. The overlay shows only while a canvas is open.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn drop_hover(active: bool) {
    with_page(|inner| inner.set_drop_hover(active));
}

/// Files were dropped on the window. `window_pos` (logical px from the window's top-left, or `None` when unknown) is not used
/// yet: the pictures go at the pointer's last position on the canvas, or at the view's centre. A folder adds the pictures and
/// videos in it, one level deep. The window sends one call per dropped file, so the paths are gathered for a moment and added
/// as one batch: the pictures are then laid out side by side instead of on top of each other.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub fn files_dropped(paths: Vec<PathBuf>, _window_pos: Option<(f64, f64)>) {
    thread_local! {
        static PENDING_DROP: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    }
    let first = PENDING_DROP.with(|pending| {
        let mut pending = pending.borrow_mut();
        let first = pending.is_empty();
        pending.extend(paths);
        first
    });
    if first {
        // One file event per dropped file arrives in the same burst; 100 ms gathers them all.
        slint::Timer::single_shot(std::time::Duration::from_millis(100), || {
            let paths = PENDING_DROP.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
            with_page(|inner| inner.files_dropped(&paths));
        });
    }
}

/// The event sink of a live session on native targets: the session thread posts each event to the UI thread.
#[cfg(not(target_arch = "wasm32"))]
fn event_sink() -> EventSink {
    std::sync::Arc::new(|event: LiveEvent| {
        let _ = slint::invoke_from_event_loop(move || on_live(event));
    })
}

/// The event sink of a live session in the browser: the session already runs on the page's only thread.
#[cfg(target_arch = "wasm32")]
fn event_sink() -> EventSink {
    std::rc::Rc::new(|event: LiveEvent| on_live(event))
}

/// Handles a live event on the UI thread.
fn on_live(event: LiveEvent) {
    with_page(|inner| inner.handle_live(event));
}

/// A new UUID (version 4) for an item the page creates. It only has to be unique, not secret: each call hashes a counter
/// with a fresh random hasher state.
fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The canvas id of a canvas route (`/home/canvases/{id}` or `/p/{KEY}/canvases/{id}`), lower-cased, or `None` for any other
/// route. The id must be a UUID; the project key is 2 to 10 ASCII letters or digits.
pub fn canvas_id_of(route: &str) -> Option<String> {
    let id = if let Some(id) = route.strip_prefix("/home/canvases/") {
        id
    } else {
        let rest = route.strip_prefix("/p/")?;
        let (key, id) = rest.split_once("/canvases/")?;
        if !(2..=10).contains(&key.len()) || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        id
    };
    is_uuid(id).then(|| id.to_ascii_lowercase())
}

/// True for the 36-character form of a UUID: hex digits with dashes after the 8th, 13th, 18th and 23rd characters.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// The pointer kind a Slint string names.
fn pointer_kind(name: &str) -> Option<PointerKind> {
    match name {
        "down" => Some(PointerKind::Down),
        "move" => Some(PointerKind::Move),
        "up" => Some(PointerKind::Up),
        _ => None,
    }
}

/// The mouse button a Slint string names. Anything else is no button.
fn button_of(name: &str) -> Button {
    match name {
        "left" => Button::Left,
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => Button::None,
    }
}

/// The word of an item kind, as `ItemView.kind` shows it.
fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Image => "Image",
        ItemKind::Text => "Text",
        ItemKind::Shape => "Shape",
        ItemKind::Stroke => "Stroke",
        ItemKind::Frame => "Frame",
        ItemKind::Video => "Video",
    }
}

/// A video's length or position as "m:ss", or "h:mm:ss" from one hour.
fn duration_text(ms: u64) -> String {
    let total = ms / 1000;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The size a video's output is decoded at on screen: the item's box at the zoom, at least 64 px and at most 1920 by 1080.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn output_size(rect: &Rect, zoom: f64) -> (u32, u32) {
    let w = (rect.w * zoom).round().clamp(64.0, 1920.0) as u32;
    let h = (rect.h * zoom).round().clamp(64.0, 1080.0) as u32;
    (w, h)
}

/// The name of the editor's field, as `CanvasState.editing-field` shows it.
fn field_name(field: &EditField) -> &'static str {
    match field {
        EditField::Text => "text",
        EditField::Comment => "comment",
        EditField::Title => "title",
    }
}

/// The message a closed room shows, for the reasons the reader can act on. `None` for the rest.
fn ended_message(reason: &str) -> Option<&'static str> {
    match reason {
        "deleted" => Some("This canvas was deleted."),
        "unauthorized" => Some("Sign in again to open this canvas."),
        "not_found" => Some("There is no such canvas."),
        "unsupported" => Some("Live canvases are not available in the browser yet."),
        _ => None,
    }
}

/// A colour from a peer's colour text ("#rrggbb" or "rrggbb"). Grey when it does not parse.
fn peer_color(text: &str) -> slint::Color {
    let hex = text.trim_start_matches('#');
    match u32::from_str_radix(hex, 16) {
        Ok(value) if hex.len() == 6 => {
            slint::Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8)
        }
        _ => slint::Color::from_rgb_u8(0x6b, 0x72, 0x80),
    }
}

/// Up to two initials of a name, upper-cased. "?" for an empty name.
fn initials_of(name: &str) -> String {
    let initials: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if initials.is_empty() {
        "?".to_string()
    } else {
        initials
    }
}

/// The frame of an animation that shows at `elapsed_ms` from its start. Frames play by their delays, then loop.
fn frame_index(delays_ms: &[u32], elapsed_ms: u64) -> usize {
    let total: u64 = delays_ms.iter().map(|d| u64::from(*d)).sum::<u64>().max(1);
    let mut left = elapsed_ms % total;
    for (index, delay) in delays_ms.iter().enumerate() {
        let delay = u64::from(*delay);
        if left < delay {
            return index;
        }
        left -= delay;
    }
    0
}

/// A size in bytes as "512 B", "3.4 KB", "12.0 MB" or "1.2 GB".
fn size_text(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else {
        format!("{:.1} GB", value / (KB * KB * KB))
    }
}

/// The picture file type of a file name, from its extension: PNG, JPEG, WebP or GIF. `None` for any other file.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn picture_mime(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else {
        None
    }
}

/// The decoded picture of some bytes with its grey copy, or `None` when they are not a picture.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn loaded_from_bytes(bytes: &[u8]) -> Option<Loaded> {
    let decoded = images::decode(bytes).ok()?;
    Some(loaded_of(decoded))
}

/// A decoded picture with its grey copy.
fn loaded_of(decoded: images::Decoded) -> Loaded {
    let grey = images::grayscale_all(&decoded);
    Loaded { decoded, grey }
}

/// Shows the native dialog for picture and video files. Runs on the worker. The pictures are read; a video is only kept as
/// its path, for its upload. A cancelled dialog gives none.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
async fn pick_files() -> (Vec<(String, Vec<u8>)>, Vec<PathBuf>) {
    let pictures = ["png", "jpg", "jpeg", "webp", "gif"];
    let mut everything = pictures.to_vec();
    everything.extend(video::VIDEO_EXTENSIONS);
    let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("Add images and videos")
        .add_filter("Images and videos", &everything[..])
        .add_filter("Images", &pictures[..])
        .add_filter("Videos", &video::VIDEO_EXTENSIONS[..])
        .pick_files()
        .await
    else {
        return (Vec::new(), Vec::new());
    };
    let mut read_pictures = Vec::new();
    let mut videos = Vec::new();
    for file in files {
        let path = file.path().to_path_buf();
        let name = file_name_of(&path);
        if picture_mime(&name).is_some() {
            let bytes = file.read().await;
            read_pictures.push((name, bytes));
        } else if video::video_mime(&name).is_some() {
            videos.push(path);
        }
    }
    (read_pictures, videos)
}

/// What an upload gave back: the asset, and its decoded picture (None when it does not decode).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
type Uploaded = (AssetView, Option<Loaded>);

/// What an upload run gave back: the pictures uploaded in order, and the error that stopped the run, if one did.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
type UploadBatch = (Vec<Uploaded>, Option<ClientError>);

/// Uploads each picture in order, decoding each one too so the page can show it at once. Runs on the worker. The run stops
/// at the first upload that fails: the pictures that went up before it are still returned, with the error.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn upload_files(
    api: Arc<space_client::api::ApiClient>,
    token: String,
    canvas: String,
    files: Vec<(String, Vec<u8>)>,
) -> Result<UploadBatch, ClientError> {
    let mut added = Vec::new();
    for (name, bytes) in files {
        let mime = picture_mime(&name).unwrap_or("application/octet-stream");
        let loaded = loaded_from_bytes(&bytes);
        match api
            .upload_canvas_asset(&token, &canvas, &name, mime, bytes)
            .await
        {
            Ok(asset) => added.push((asset, loaded)),
            Err(error) => return Ok((added, Some(error))),
        }
    }
    Ok((added, None))
}

/// The file name of a path, or an empty name when it has none.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A file's name and bytes, or `None` when it cannot be read.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn read_file(path: &Path) -> Option<(String, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    Some((file_name_of(path), bytes))
}

/// Sorts file paths into pictures, which are read now, and videos, which are kept as paths for their upload. Other files
/// are left out.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn split_paths(paths: &[PathBuf]) -> (Vec<(String, Vec<u8>)>, Vec<PathBuf>) {
    let mut pictures = Vec::new();
    let mut videos = Vec::new();
    for path in paths {
        let name = file_name_of(path);
        if picture_mime(&name).is_some() {
            pictures.extend(read_file(path));
        } else if video::video_mime(&name).is_some() {
            videos.push(path.clone());
        }
    }
    (pictures, videos)
}

/// The files of a drop, split into pictures (read) and videos (paths). A folder contributes the files in it (one level, by
/// name).
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn read_dropped(paths: &[PathBuf]) -> (Vec<(String, Vec<u8>)>, Vec<PathBuf>) {
    let mut expanded = Vec::new();
    for path in paths {
        if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else {
                continue;
            };
            let mut inside: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|item| item.is_file())
                .collect();
            inside.sort();
            expanded.extend(inside);
        } else {
            expanded.push(path.clone());
        }
    }
    split_paths(&expanded)
}

/// The busy line of a video upload: its name, its place in the run when there are several, and its percent.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn video_busy(name: &str, index: usize, total: usize, percent: u64) -> String {
    if total == 1 {
        format!("Uploading {name}: {percent} %")
    } else {
        format!("Uploading {name} ({} of {total}): {percent} %", index + 1)
    }
}

/// What a video upload run gave back: the assets uploaded in order, and the error that stopped the run, if one did.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
type VideoBatch = (Vec<AssetView>, Option<ClientError>);

/// Uploads each video in order on the worker. Each percent is posted to the busy line. The run stops at the first upload
/// that fails: the videos that went up before it are still returned, with the error.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
async fn upload_videos(
    api: Arc<space_client::api::ApiClient>,
    token: String,
    canvas: String,
    paths: Vec<PathBuf>,
    cache: Arc<DiskCache>,
) -> Result<VideoBatch, ClientError> {
    let total = paths.len();
    let mut added = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let name = file_name_of(path);
        let last_percent = std::sync::atomic::AtomicU64::new(u64::MAX);
        let progress = move |sent: u64, size: u64| {
            let percent = (sent * 100).checked_div(size).unwrap_or(100);
            let previous = last_percent.swap(percent, std::sync::atomic::Ordering::Relaxed);
            if previous != percent {
                let text = video_busy(&name, index, total, percent);
                let _ = slint::invoke_from_event_loop(move || {
                    with_page(|inner| inner.set_busy(&text));
                });
            }
        };
        match video::upload_video(&api, &token, &canvas, path, &cache, &progress).await {
            Ok(asset) => added.push(asset),
            Err(error) => return Ok((added, Some(error))),
        }
    }
    Ok((added, None))
}

/// The clipboard's RGBA pixels, as a PNG file.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn encode_png(image: arboard::ImageData<'_>) -> Result<Vec<u8>, String> {
    let (Ok(width), Ok(height)) = (u32::try_from(image.width), u32::try_from(image.height)) else {
        return Err("The clipboard image is too large.".to_string());
    };
    let rgba = image::RgbaImage::from_raw(width, height, image.bytes.into_owned())
        .ok_or_else(|| "The clipboard image is damaged.".to_string())?;
    let mut png = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("The clipboard image could not be saved: {error}"))?;
    Ok(png.into_inner())
}

/// Copies text to the clipboard. Desktop only.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| format!("The clipboard could not be written: {error}"))
}

/// Copying text is not built for the web, Android or iOS yet.
#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
fn copy_text(_text: &str) -> Result<(), String> {
    Err("Copying is not available here yet.".to_string())
}

/// The protocol-error of a message that is not a client error (a picture that does not decode).
fn protocol(message: String) -> ClientError {
    ClientError::Protocol(message)
}

/// The original picture of a job, from the disk cache when it is there, else from the server (and cached).
async fn load_original(
    api: &space_client::api::ApiClient,
    token: &str,
    job: &LoadJob,
    cache: &DiskCache,
) -> Result<images::Decoded, ClientError> {
    let bytes = match cache.get(&job.sha, "orig") {
        Some(bytes) => bytes,
        None => {
            let bytes = api
                .canvas_asset_bytes(token, &job.canvas, &job.asset)
                .await?;
            cache.put(&job.sha, "orig", &bytes);
            bytes
        }
    };
    images::decode(&bytes).map_err(protocol)
}

/// A thumbnail of the original at the job's level, lossless WebP, uploaded to the server (its answer is the asset with the
/// new thumbnail). The upload is best effort: the thumbnail shows either way.
async fn make_thumb(
    api: &space_client::api::ApiClient,
    token: &str,
    job: &LoadJob,
    original: images::Decoded,
    cache: &DiskCache,
) -> Result<(images::Decoded, Option<AssetView>), ClientError> {
    let webp = images::thumbnail_webp(&original, job.level).map_err(protocol)?;
    let decoded = images::decode(&webp).map_err(protocol)?;
    cache.put(&job.sha, &job.level.to_string(), &webp);
    let updated = api
        .upload_canvas_thumb(token, &job.canvas, &job.asset, job.level, webp)
        .await
        .ok();
    Ok((decoded, updated))
}

/// One picture of a job: its thumbnail when the snapshot has it (cache first), else the original, else a thumbnail made
/// from the original and uploaded. Runs on the worker.
async fn load_picture(
    api: Arc<space_client::api::ApiClient>,
    token: String,
    job: LoadJob,
    cache: Arc<DiskCache>,
) -> LoadResult {
    let thumb = if job.level != 0 && job.thumbs.contains(&job.level) {
        let variant = job.level.to_string();
        let bytes = match cache.get(&job.sha, &variant) {
            Some(bytes) => Some(bytes),
            None => match api
                .canvas_thumb_bytes(&token, &job.canvas, &job.asset, job.level)
                .await
            {
                Ok(bytes) => {
                    cache.put(&job.sha, &variant, &bytes);
                    Some(bytes)
                }
                Err(_) => None,
            },
        };
        bytes.and_then(|bytes| images::decode(&bytes).ok())
    } else {
        None
    };
    let (decoded, updated) = match thumb {
        Some(decoded) => (decoded, None),
        // A video's original is the video file: with no thumbnail the item keeps its placeholder.
        None if job.video => return Err(protocol("The video has no thumbnail.".into())),
        None => {
            let original = load_original(&api, &token, &job, &cache).await?;
            if job.level == 0 || job.thumbs.contains(&job.level) {
                (original, None)
            } else {
                make_thumb(&api, &token, &job, original, &cache).await?
            }
        }
    };
    Ok((loaded_of(decoded), updated))
}

/// The originals of several assets, for the export. Runs on the worker.
async fn fetch_originals(
    api: Arc<space_client::api::ApiClient>,
    token: String,
    jobs: Vec<LoadJob>,
    cache: Arc<DiskCache>,
) -> Result<Vec<(String, Loaded)>, ClientError> {
    let mut loaded = Vec::new();
    for job in jobs {
        let decoded = load_original(&api, &token, &job, &cache).await?;
        loaded.push((job.asset.clone(), loaded_of(decoded)));
    }
    Ok(loaded)
}

/// The answer to New canvas: the new canvas opens in this tab, in the same home as the open canvas (`project_key` is its
/// project, or `None` for a personal canvas). Runs on the UI thread without the page borrowed, so the navigation can open
/// the canvas page at once.
fn canvas_created(
    ui: &dyn UiDyn,
    project_key: Option<&str>,
    result: Result<CanvasView, ClientError>,
) {
    match result {
        Ok(view) => {
            let route = match project_key {
                Some(key) => format!("/p/{key}/canvases/{}", view.id),
                None => format!("/home/canvases/{}", view.id),
            };
            ui.open_route(&route);
        }
        Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
    }
}

impl Inner {
    fn new(ui: Ui, data: DataCell, items: Rc<dyn ItemsModel>) -> Inner {
        Inner {
            ui,
            host: Host::Main,
            data,
            route: String::new(),
            viewport: (0.0, 0.0),
            canvas_id: None,
            open_generation: 0,
            view_only: false,
            archived: false,
            self_conn: None,
            controller: None,
            session: None,
            assets: HashMap::new(),
            pictures: HashMap::new(),
            missing: HashSet::new(),
            no_thumb: HashSet::new(),
            #[cfg(all(
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            players: HashMap::new(),
            #[cfg(all(
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            loading_videos: HashSet::new(),
            video_timer: slint::Timer::default(),
            loading: HashSet::new(),
            load_queue: VecDeque::new(),
            loads_running: 0,
            levels: HashMap::new(),
            last_rows: Vec::new(),
            last_keys: Vec::new(),
            items_model: items,
            animating: false,
            anim_timer: slint::Timer::default(),
            slide_timer: slint::Timer::default(),
            started: Instant::now(),
            cache: Arc::new(DiskCache::open()),
            peers: Vec::new(),
            follow_id: None,
            edit_field: "text",
            last_presence: None,
            presence_armed: false,
            usage: Vec::new(),
            assets_open: false,
            relink: VecDeque::new(),
            eyedrop: None,
            outbox: Vec::new(),
            shared: HashMap::new(),
            left: HashSet::new(),
        }
    }

    /// Runs `f` on the active host's `CanvasState`, if that window is still open.
    fn with_state<R>(&self, f: impl FnOnce(&dyn CanvasPageState) -> R) -> Option<R> {
        match &self.host {
            Host::Main => self.ui.upgrade().map(|ui| f(&*ui)),
            Host::Popout(window) => window.upgrade().map(|w| f(&*w)),
        }
    }

    /// The main window's "canvas is in its own window" flag.
    fn set_main_popped_out(&self, on: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_popped_out(on);
        }
    }

    /// Gives the active host the item model, so its items draw the page's rows.
    fn attach_items(&self) {
        let items = self.items_model.clone();
        self.with_state(|state| state.attach_items(&items));
    }

    /// The title of the open canvas, for file names.
    fn title(&self) -> String {
        self.with_state(|state| state.get_title().to_string())
            .unwrap_or_default()
    }

    /// Milliseconds since the page opened: the clock of animations.
    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Opens `route`. The same canvas already on the page only redraws. Another canvas closes the old session, resets the
    /// page, reads the canvas snapshot, then connects its room (see [`Inner::opened`]).
    fn open(&mut self, route: &str) {
        self.route = route.to_string();
        let Some(id) = canvas_id_of(route) else {
            return;
        };
        if self.canvas_id.as_deref() == Some(id.as_str()) && self.controller.is_some() {
            self.refresh();
            return;
        }
        // Dropping the old session closes its room.
        self.session = None;
        self.canvas_id = Some(id.clone());
        self.controller = None;
        self.outbox.clear();
        self.view_only = false;
        self.archived = false;
        self.self_conn = None;
        self.open_generation += 1;
        let generation = self.open_generation;
        self.assets.clear();
        self.pictures.clear();
        self.missing.clear();
        self.no_thumb.clear();
        self.stop_videos();
        self.loading.clear();
        self.load_queue.clear();
        self.levels.clear();
        self.last_rows.clear();
        self.last_keys.clear();
        self.items_model.set_vec(Vec::new());
        self.peers.clear();
        self.shared.clear();
        self.left.clear();
        self.follow_id = None;
        self.usage.clear();
        self.assets_open = false;
        self.relink.clear();
        self.eyedrop = None;
        self.animating = false;
        self.edit_field = "text";
        self.with_state(|state| {
            state.set_load_state(LoadState::Loading);
            state.set_error("".into());
            state.set_title("".into());
            state.set_connection("connecting".into());
            state.set_read_only(false);
            state.set_assets_open(false);
        });
        self.refresh();
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.ui.fetch(
            &source,
            move |api, token| async move { api.canvas_snapshot(&token, &id).await },
            move |_ui, result| with_page(|inner| inner.opened(generation, result)),
        );
    }

    /// The snapshot read answered: the canvas title shows, the assets are known, and the room connects. A read for an
    /// older open is dropped.
    fn opened(&mut self, generation: u64, result: Result<Snapshot, ClientError>) {
        if generation != self.open_generation {
            return;
        }
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.with_state(|state| {
                    state.set_load_state(LoadState::Error);
                    state.set_error(page_data::error_text(&error).into());
                });
                return;
            }
        };
        let view = snapshot.canvas;
        self.assets = snapshot
            .assets
            .into_iter()
            .map(|asset| (asset.id.clone(), asset))
            .collect();
        self.sync_assets();
        self.with_state(|state| state.set_title(view.title.as_str().into()));
        self.view_only = view.my_access == "view";
        self.archived = view.archived;
        self.sync_read_only();

        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else {
            self.with_state(|state| {
                state.set_load_state(LoadState::Error);
                state.set_error("Sign in again to open this canvas.".into());
            });
            return;
        };
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        self.session = Some(LiveSession::connect(
            source.api(),
            token,
            canvas,
            event_sink(),
        ));
    }

    /// Applies one live room event. The room's hello loads the document; later changes are applied to it.
    fn handle_live(&mut self, event: LiveEvent) {
        match event {
            LiveEvent::Connected(hello) => {
                self.view_only |= hello.you.access == "view";
                self.archived = hello.canvas.archived;
                self.self_conn = Some(hello.you.conn_id.clone());
                self.peers = hello
                    .peers
                    .iter()
                    .filter(|peer| peer.conn_id != hello.you.conn_id)
                    .cloned()
                    .collect();
                let read_only = self.is_read_only();
                let popout = matches!(self.host, Host::Popout(_));
                let user = hello.you.user_id.clone();
                let viewport = self.viewport;
                let controller = self
                    .controller
                    .get_or_insert_with(|| Controller::new(user, Box::new(new_uuid)));
                controller.set_viewport(viewport.0, viewport.1);
                controller.set_read_only(read_only);
                controller.set_popout(popout);
                let effects = controller.load(&hello);
                self.sync_assets();
                self.with_state(|state| {
                    state.set_title(hello.canvas.title.as_str().into());
                    state.set_read_only(read_only);
                    state.set_load_state(LoadState::Loaded);
                    state.set_connection("live".into());
                });
                self.run(effects);
                if let Some(items) = self.canvas_id.clone().and_then(|id| take_import(&id)) {
                    self.act(|c| c.import_items(items));
                }
                self.shared = hello
                    .media
                    .iter()
                    .map(|media| (media.item_id.clone(), media.clone()))
                    .collect();
                self.left.retain(|id| self.shared.contains_key(id));
                self.refresh();
                self.follow_shared();
            }
            LiveEvent::Message(message) => match message {
                ServerMessage::Ack { op_id, item, .. } => {
                    self.act(|c| c.ack(&op_id, &item));
                }
                ServerMessage::Applied { item, .. } => self.apply_item(&item),
                ServerMessage::Reject {
                    op_id,
                    message,
                    item,
                    ..
                } => {
                    if let Some(item) = item {
                        self.act(|c| c.ack(&op_id, &item));
                    }
                    toast::announce(Tone::Warning, message);
                }
                ServerMessage::Presence(peer) | ServerMessage::Join { peer } => {
                    self.peer_moved(peer);
                }
                ServerMessage::Leave { conn_id } => {
                    self.peers.retain(|peer| peer.conn_id != conn_id);
                    if self.follow_id.as_deref() == Some(conn_id.as_str()) {
                        self.follow_id = None;
                    }
                    self.refresh();
                }
                ServerMessage::Canvas { title, archived } => {
                    if let Some(title) = title {
                        self.with_state(|state| state.set_title(title.as_str().into()));
                    }
                    if let Some(archived) = archived {
                        self.archived = archived;
                        self.sync_read_only();
                    }
                }
                ServerMessage::Error { message } => toast::announce(Tone::Warning, message),
                ServerMessage::Media(media) => self.media_changed(media),
                _ => {}
            },
            LiveEvent::Disconnected { .. } => {
                self.with_state(|state| state.set_connection("reconnecting".into()));
            }
            LiveEvent::Ended { reason } => {
                self.with_state(|state| {
                    state.set_connection("ended".into());
                    if let Some(message) = ended_message(&reason) {
                        state.set_load_state(LoadState::Error);
                        state.set_error(message.into());
                    }
                });
            }
        }
    }

    /// A peer joined or sent its presence. The camera follows it when it is the followed peer.
    fn peer_moved(&mut self, peer: Peer) {
        if self.self_conn.as_deref() == Some(peer.conn_id.as_str()) {
            return;
        }
        let followed = self.follow_id.as_deref() == Some(peer.conn_id.as_str());
        let viewport = peer.viewport.clone();
        match self.peers.iter().position(|p| p.conn_id == peer.conn_id) {
            Some(index) => self.peers[index] = peer,
            None => self.peers.push(peer),
        }
        if followed && let Some(viewport) = viewport {
            self.act(|c| c.follow_viewport(&viewport));
        }
        self.refresh();
    }

    /// Applies one item the room reports.
    fn apply_item(&mut self, item: &ItemState) {
        self.act(|c| c.apply(item));
    }

    /// Follows a peer's viewport (`conn` empty stops following).
    fn follow(&mut self, conn: &str) {
        self.follow_id = (!conn.is_empty()).then(|| conn.to_string());
        if let Some(id) = self.follow_id.clone()
            && let Some(viewport) = self
                .peers
                .iter()
                .find(|peer| peer.conn_id == id)
                .and_then(|peer| peer.viewport.clone())
        {
            self.act(|c| c.follow_viewport(&viewport));
        }
        self.refresh();
    }

    /// True while the viewer may only look: a view-only share, or an archived canvas.
    fn is_read_only(&self) -> bool {
        self.view_only || self.archived
    }

    /// Pushes the read-only flag to the controller and the window.
    fn sync_read_only(&mut self) {
        let read_only = self.is_read_only();
        if let Some(controller) = self.controller.as_mut() {
            controller.set_read_only(read_only);
        }
        self.with_state(|state| state.set_read_only(read_only));
    }

    /// Stores an asset of the canvas: its size and source go to the controller.
    fn store_asset(&mut self, asset: AssetView) {
        if !asset.thumbs.is_empty() {
            self.no_thumb.remove(&asset.id);
        }
        if let Some(controller) = self.controller.as_mut() {
            controller.set_asset_size(&asset.id, f64::from(asset.width), f64::from(asset.height));
            controller.set_asset_source(&asset.id, &asset.source_url, &asset.file_name);
        }
        self.assets.insert(asset.id.clone(), asset);
    }

    /// Gives the controller the size and source of every known asset (once the controller exists).
    fn sync_assets(&mut self) {
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        for asset in self.assets.values() {
            controller.set_asset_size(&asset.id, f64::from(asset.width), f64::from(asset.height));
            controller.set_asset_source(&asset.id, &asset.source_url, &asset.file_name);
        }
    }

    /// Records a new asset with its picture (when one was decoded) at the original level.
    fn adopt_asset(&mut self, asset: AssetView, loaded: Option<Loaded>) {
        if let Some(loaded) = loaded {
            self.pictures
                .entry(asset.id.clone())
                .or_default()
                .insert(0, Picture::new(&loaded.decoded, &loaded.grey));
        }
        self.missing.remove(&asset.id);
        self.store_asset(asset);
    }

    /// Tells the controller which assets have no picture (Relink lists them, and the menu checks it).
    fn sync_missing(&mut self) {
        let ids: Vec<String> = self.missing.iter().cloned().collect();
        if let Some(controller) = self.controller.as_mut() {
            controller.set_missing_assets(ids);
        }
    }

    /// Runs one controller call and performs its effects. Nothing happens while there is no controller.
    fn act(&mut self, f: impl FnOnce(&mut Controller) -> Vec<Effect>) {
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let effects = f(controller);
        self.run(effects);
    }

    /// Performs what the controller asked, then drops the follow when the controller cancelled it.
    fn run(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Send(op) => match self.session.as_ref() {
                    Some(session) => session.send_op(op),
                    None => self.outbox.push(op),
                },
                Effect::Redraw | Effect::Camera => self.refresh(),
                Effect::Presence => self.presence_changed(),
                Effect::NewCanvas => self.new_canvas(),
                Effect::CloseCanvas => {
                    self.stop_videos();
                    // Deferred: closing the tab can show another page, which may open a canvas and re-enter this page.
                    self.ui.post(|ui| ui.close_tab());
                }
                Effect::VideoToggle(id) => self.toggle_video(&id),
                Effect::CopyImages(ids) => self.copy_images(&ids),
                Effect::CopyFiles(ids) => self.copy_files(&ids),
                Effect::OpenSource(url) => actions::open_source(&url),
                Effect::Window(command) => defer(move |page| {
                    popout::command(page, command);
                    page.redraw();
                }),
                Effect::Export(command) => self.export(command),
                Effect::ManageImages => self.manage_images(),
                Effect::ReplaceImage(item) => self.replace_image(item),
                Effect::RelinkMissing => self.relink_start(),
                Effect::Eyedropper {
                    asset,
                    x_frac,
                    y_frac,
                } => self.eyedrop(asset, x_frac, y_frac),
                Effect::Paste => {
                    if self.can_paste() {
                        self.paste();
                    }
                }
                // The menu's "Add images…" opens the same file picker as the toolbar button.
                Effect::AddImages => self.add_image(),
                Effect::EditText { id, text, field } => {
                    self.edit_field = field_name(&field);
                    self.with_state(|state| {
                        state.set_editing_id(id.as_str().into());
                        state.set_editing_text(text.as_str().into());
                        state.set_editing_field(self.edit_field.into());
                    });
                }
                Effect::EndEdit => {
                    self.edit_field = "text";
                    self.with_state(|state| {
                        state.set_editing_id("".into());
                        state.set_editing_text("".into());
                        state.set_editing_field("text".into());
                    });
                }
                Effect::Cursor(cursor) => {
                    self.with_state(|state| state.set_cursor(cursor.into()));
                }
            }
        }
        self.sync_follow();
    }

    /// Drops the follow when the controller cancelled it (the user panned or zoomed).
    fn sync_follow(&mut self) {
        let cancelled = self
            .controller
            .as_mut()
            .is_some_and(Controller::take_follow_cancelled);
        if cancelled {
            self.follow_id = None;
            self.refresh();
        }
    }

    /// Sends the presence now, or at most every [`PRESENCE_EVERY`]: a change inside the window is sent when it closes.
    fn presence_changed(&mut self) {
        if self.session.is_none() {
            return;
        }
        match self.last_presence {
            Some(at) if at.elapsed() < PRESENCE_EVERY => {
                if !self.presence_armed {
                    self.presence_armed = true;
                    let wait = PRESENCE_EVERY.saturating_sub(at.elapsed());
                    slint::Timer::single_shot(wait, || with_page(|inner| inner.flush_presence()));
                }
            }
            _ => self.flush_presence(),
        }
    }

    /// Sends the controller's presence (cursor, selection, viewport) to the room.
    fn flush_presence(&mut self) {
        self.presence_armed = false;
        if let (Some(controller), Some(session)) = (self.controller.as_ref(), self.session.as_ref())
        {
            session.send_presence(controller.presence());
            self.last_presence = Some(Instant::now());
        }
    }

    /// New canvas: a canvas in the same home as the open one (its project, or personal). The new canvas opens in this tab.
    fn new_canvas(&self) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let project_key = self.current_project_key();
        let key_for_route = project_key.clone();
        self.ui.fetch(
            &source,
            move |api, token| async move {
                let body = CreateCanvas {
                    title: None,
                    project_key,
                };
                api.create_canvas(&token, &body).await
            },
            move |ui, result| canvas_created(ui, key_for_route.as_deref(), result),
        );
    }

    /// The project key of the open canvas, when it is a project canvas (`/p/{KEY}/canvases/{id}`). `None` for a personal
    /// canvas or any other route.
    fn current_project_key(&self) -> Option<String> {
        canvas_id_of(&self.route)?;
        let rest = self.route.strip_prefix("/p/")?;
        let (key, _) = rest.split_once("/canvases/")?;
        Some(key.to_string())
    }

    /// The context the actions run in, or `None` while no canvas is open.
    fn ctx(&self) -> Option<actions::Ctx> {
        let source = self.data.borrow().clone()?;
        let canvas_id = self.canvas_id.clone()?;
        Some(actions::Ctx {
            ui: self.ui.clone(),
            source,
            canvas_id,
            project_key: self.current_project_key(),
        })
    }

    /// The assets of some ids: each id is an asset id, or an item id whose image is that asset. Each asset once.
    fn assets_of(&self, ids: &[String]) -> Vec<AssetView> {
        let image_items = self
            .controller
            .as_ref()
            .map(Controller::image_items)
            .unwrap_or_default();
        let mut out: Vec<AssetView> = Vec::new();
        for id in ids {
            let asset_id = if self.assets.contains_key(id) {
                Some(id.clone())
            } else {
                image_items
                    .iter()
                    .find(|(item, _)| item == id)
                    .map(|(_, asset)| asset.clone())
            };
            if let Some(asset) = asset_id.and_then(|asset| self.assets.get(&asset).cloned())
                && !out.iter().any(|known| known.id == asset.id)
            {
                out.push(asset);
            }
        }
        out
    }

    /// The images of the selected items, each asset once.
    fn selected_images(&self) -> Vec<AssetView> {
        let Some(controller) = self.controller.as_ref() else {
            return Vec::new();
        };
        let selected = controller.selected_ids();
        let ids: Vec<String> = controller
            .image_items()
            .into_iter()
            .filter(|(item, _)| selected.contains(item))
            .map(|(_, asset)| asset)
            .collect();
        self.assets_of(&ids)
    }

    /// Copy image: the first picture among `ids` goes to the clipboard at its original size.
    fn copy_images(&self, ids: &[String]) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        match self.assets_of(ids).into_iter().next() {
            Some(asset) => actions::copy_original_image(&ctx, asset),
            None => toast::announce(Tone::Warning, "No picture is selected."),
        }
    }

    /// Copy as file: the original files of `ids` go to the clipboard as files.
    fn copy_files(&self, ids: &[String]) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        let assets = self.assets_of(ids);
        if assets.is_empty() {
            toast::announce(Tone::Warning, "No picture is selected.");
        } else {
            actions::copy_files(&ctx, assets);
        }
    }

    /// Chooses picture and video files with the native dialog, then adds them at the view's centre.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn add_image(&mut self) {
        if self.is_read_only() {
            toast::announce(Tone::Warning, "This canvas is view only.");
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.ui.fetch(
            &source,
            |_api, _token| async { Ok::<_, ClientError>(pick_files().await) },
            move |_ui, result| {
                with_page(|inner| match result {
                    Ok((pictures, videos)) => inner.add_picked(pictures, videos, None),
                    Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
                });
            },
        );
    }

    /// Adding images is not built for the web, Android or iOS yet.
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn add_image(&mut self) {
        toast::announce(Tone::Neutral, "Adding images is not available here yet.");
    }

    /// Changes the editor's tool. Unknown names change nothing.
    fn set_tool(&mut self, name: &str) {
        if let Some(tool) = tool_from_name(name) {
            self.act(|c| c.set_tool(tool));
        }
    }

    /// A pointer event on the canvas area. Unknown kinds are ignored.
    fn pointer(&mut self, kind: &str, at: Vec2, button: &str, mods: Mods) {
        // The pointer left the canvas: no cursor for the others, no eraser circle.
        if kind == "leave" {
            if let Some(controller) = self.controller.as_mut() {
                controller.pointer_left();
            }
            self.presence_changed();
            self.refresh();
            return;
        }
        let Some(kind) = pointer_kind(kind) else {
            return;
        };
        let button = button_of(button);
        self.act(|c| c.pointer(kind, at, button, mods));
        // The eraser circle and the faded items follow the pointer, so every move redraws while the eraser is out.
        if self
            .controller
            .as_ref()
            .is_some_and(|c| c.eraser_view().is_some())
        {
            self.refresh();
        }
        self.presence_changed();
    }

    fn double_click(&mut self, at: Vec2) {
        self.act(|c| c.double_click(at));
    }

    fn scroll(&mut self, at: Vec2, delta: Vec2, mods: Mods) {
        self.act(|c| c.scroll(at, delta, mods));
    }

    /// A key on the canvas. True when the page or the controller used it. Ctrl+V pastes, unless a text edit has the keys.
    fn key(&mut self, text: &str, mods: Mods) -> bool {
        let Some(controller) = self.controller.as_mut() else {
            return false;
        };
        let (used, effects) = controller.key(text, mods);
        self.run(effects);
        used
    }

    /// True when Ctrl+V may paste: a canvas is open, and no item text or rename field is being edited.
    fn can_paste(&self) -> bool {
        let Some(controller) = self.controller.as_ref() else {
            return false;
        };
        let renaming = self
            .with_state(|state| state.get_renaming())
            .unwrap_or(false);
        controller.editing().is_none() && !renaming
    }

    /// Ctrl+V: an image on the clipboard, copied files, or a web link to an image. True when the paste was handled.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn paste(&mut self) -> bool {
        let at = self.controller.as_ref().and_then(Controller::pointer_world);
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(_) => {
                toast::announce(Tone::Warning, "The clipboard could not be read.");
                return true;
            }
        };
        if let Ok(image) = clipboard.get_image() {
            match encode_png(image) {
                Ok(png) => self.add_files(vec![("pasted.png".to_string(), png)], at),
                Err(error) => toast::announce(Tone::Danger, error),
            }
            return true;
        }
        if let Ok(paths) = clipboard.get().file_list()
            && !paths.is_empty()
        {
            let (pictures, videos) = split_paths(&paths);
            self.add_picked(pictures, videos, at);
            return true;
        }
        if let Ok(text) = clipboard.get_text() {
            let link = text.trim();
            if link.starts_with("https://") && !link.contains(char::is_whitespace) {
                self.import_link(link.to_string(), at);
                return true;
            }
        }
        toast::announce(Tone::Warning, "The clipboard has no image.");
        true
    }

    /// Paste is not built for the web, Android or iOS: the key goes on to the controller.
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn paste(&mut self) -> bool {
        false
    }

    /// The canvas area's size, which the controller fits the camera to.
    fn viewport(&mut self, w: f64, h: f64) {
        self.viewport = (w, h);
        if let Some(controller) = self.controller.as_mut() {
            controller.set_viewport(w, h);
        }
    }

    /// Adds picture files to the open canvas, placed together at `at` (world), or at the view's centre when `at` is `None`.
    /// Each file is uploaded and decoded in order on the worker while "Adding N images…" shows. Files that are not pictures
    /// are skipped, with one toast when all of them were skipped.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn add_files(&mut self, files: Vec<(String, Vec<u8>)>, at: Option<Vec2>) {
        if self.is_read_only() {
            toast::announce(Tone::Warning, "This canvas is view only.");
            return;
        }
        let total = files.len();
        let files: Vec<(String, Vec<u8>)> = files
            .into_iter()
            .filter(|(name, _)| picture_mime(name).is_some())
            .collect();
        if files.is_empty() {
            if total > 0 {
                toast::announce(
                    Tone::Warning,
                    "Only PNG, JPEG, WebP and GIF images can be added.",
                );
            }
            return;
        }
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        if self.controller.is_none() {
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let label = if files.len() == 1 {
            "Adding 1 image…".to_string()
        } else {
            format!("Adding {} images…", files.len())
        };
        self.set_busy(&label);
        let done_canvas = canvas.clone();
        self.ui.fetch(
            &source,
            move |api, token| upload_files(api, token, canvas, files),
            move |_ui, result| with_page(|inner| inner.uploads_done(&done_canvas, result, at)),
        );
    }

    /// Imports the picture at `url` into the canvas (the server fetches it), then places it like an upload.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn import_link(&mut self, url: String, at: Option<Vec2>) {
        if self.is_read_only() {
            toast::announce(Tone::Warning, "This canvas is view only.");
            return;
        }
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        if self.controller.is_none() {
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.set_busy("Adding image…");
        let done_canvas = canvas.clone();
        self.ui.fetch(
            &source,
            move |api, token| async move {
                let asset = api.import_canvas_asset(&token, &canvas, &url).await?;
                let bytes = api.canvas_asset_bytes(&token, &canvas, &asset.id).await?;
                let loaded = loaded_from_bytes(&bytes);
                Ok::<UploadBatch, ClientError>((vec![(asset, loaded)], None))
            },
            move |_ui, result| with_page(|inner| inner.uploads_done(&done_canvas, result, at)),
        );
    }

    /// An upload run finished (or a link import did). The busy line clears, the pictures are cached and placed together at
    /// `at`, or at the view's centre. Pictures for a canvas that is no longer open are not placed.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn uploads_done(
        &mut self,
        canvas: &str,
        result: Result<UploadBatch, ClientError>,
        at: Option<Vec2>,
    ) {
        self.set_busy("");
        let (added, error) = match result {
            Ok(batch) => batch,
            Err(error) => {
                toast::announce(Tone::Danger, page_data::error_text(&error));
                return;
            }
        };
        if self.canvas_id.as_deref() == Some(canvas) {
            let mut assets = Vec::new();
            for (asset, loaded) in added {
                self.adopt_asset(asset.clone(), loaded);
                assets.push(asset);
            }
            let effects = match self.controller.as_mut() {
                Some(controller) => {
                    let at = at.unwrap_or_else(|| controller.view_center());
                    controller.add_images(&assets, at)
                }
                None => Vec::new(),
            };
            self.run(effects);
        }
        if let Some(error) = error {
            toast::announce(Tone::Danger, page_data::error_text(&error));
        }
    }

    /// Adds what a drop, a pick or a paste gave: the pictures at `at` (or the view's centre) and the videos to upload. When
    /// there is neither, one toast says which files can be added.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn add_picked(
        &mut self,
        pictures: Vec<(String, Vec<u8>)>,
        videos: Vec<PathBuf>,
        at: Option<Vec2>,
    ) {
        if pictures.is_empty() && videos.is_empty() {
            toast::announce(
                Tone::Warning,
                "Only PNG, JPEG, WebP and GIF images and MP4, MOV, WebM and MKV videos can be added.",
            );
            return;
        }
        self.add_files(pictures, at);
        self.add_videos(videos, at);
    }

    /// Uploads videos to the open canvas, one after another on the worker, with the busy line showing each percent. The
    /// uploaded videos are then placed at `at` (or the view's centre).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn add_videos(&mut self, paths: Vec<PathBuf>, at: Option<Vec2>) {
        let Some(first) = paths.first() else {
            return;
        };
        if self.is_read_only() {
            toast::announce(Tone::Warning, "This canvas is view only.");
            return;
        }
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        if self.controller.is_none() {
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.set_busy(&video_busy(&file_name_of(first), 0, paths.len(), 0));
        let done_canvas = canvas.clone();
        let cache = self.cache.clone();
        self.ui.fetch(
            &source,
            move |api, token| upload_videos(api, token, canvas, paths, cache),
            move |_ui, result| with_page(|inner| inner.videos_done(&done_canvas, result, at)),
        );
    }

    /// A video upload run finished. The busy line clears. The uploaded videos are stored for the canvas when it is still
    /// open and placed on it at `at` (or the view's centre), and the count is announced. A failure shows its toast.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn videos_done(
        &mut self,
        canvas: &str,
        result: Result<VideoBatch, ClientError>,
        at: Option<Vec2>,
    ) {
        self.set_busy("");
        let (assets, error) = match result {
            Ok(batch) => batch,
            Err(error) => {
                toast::announce(Tone::Danger, page_data::error_text(&error));
                return;
            }
        };
        if self.canvas_id.as_deref() == Some(canvas) && !assets.is_empty() {
            let count = assets.len();
            for asset in &assets {
                self.store_asset(asset.clone());
            }
            let effects = match self.controller.as_mut() {
                Some(controller) => {
                    let at = at.unwrap_or_else(|| controller.view_center());
                    controller.add_videos(&assets, at)
                }
                None => Vec::new(),
            };
            self.run(effects);
            toast::announce(Tone::Success, format!("Uploaded {count} video(s)."));
        }
        if let Some(error) = error {
            toast::announce(Tone::Danger, page_data::error_text(&error));
        }
    }

    /// Shows `text` as the busy line of the page. An empty text hides it.
    fn set_busy(&self, text: &str) {
        self.with_state(|state| state.set_busy_text(text.into()));
    }

    /// Files are being dragged over the window: shows the overlay, or hides it.
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    fn set_drop_hover(&self, active: bool) {
        let shown = active && self.controller.is_some();
        self.with_state(|state| state.set_drop_hover(shown));
    }

    /// Files were dropped: the overlay clears, and the pictures among `paths` are added at the pointer's last position.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn files_dropped(&mut self, paths: &[PathBuf]) {
        self.set_drop_hover(false);
        if self.controller.is_none() {
            return;
        }
        let (pictures, videos) = read_dropped(paths);
        let at = self.controller.as_ref().and_then(Controller::pointer_world);
        self.add_picked(pictures, videos, at);
    }

    /// Renames the open canvas. The new title shows at once and is saved; a refused save brings the old title back.
    fn rename(&mut self, raw: &str) {
        let title = raw.trim().to_string();
        let Some(id) = self.canvas_id.clone() else {
            return;
        };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let old = self.title();
        if title.is_empty() || title == old {
            return;
        }
        self.with_state(|state| state.set_title(title.as_str().into()));
        let done_id = id.clone();
        self.ui.fetch(
            &source,
            move |api, token| async move { api.rename_canvas(&token, &id, &title).await },
            move |_ui, result| with_page(|inner| inner.renamed(&done_id, old, result)),
        );
    }

    /// The rename's answer: the server's title shows, or the old title comes back with the error. An answer for a canvas
    /// that is no longer open is dropped.
    fn renamed(&mut self, canvas: &str, old: String, result: Result<CanvasView, ClientError>) {
        if self.canvas_id.as_deref() != Some(canvas) {
            return;
        }
        match result {
            Ok(view) => {
                self.with_state(|state| state.set_title(view.title.as_str().into()));
            }
            Err(error) => {
                self.with_state(|state| state.set_title(old.as_str().into()));
                toast::announce(Tone::Danger, page_data::error_text(&error));
            }
        }
    }

    /// Asks for the picture of an asset at a level, unless it is loaded, loading, queued or missing.
    fn request(&mut self, asset: &str, level: u32) {
        if self.missing.contains(asset)
            || self.no_thumb.contains(asset)
            || self
                .pictures
                .get(asset)
                .is_some_and(|levels| levels.contains_key(&level))
        {
            return;
        }
        let key = (asset.to_string(), level);
        if self.loading.contains(&key) || self.load_queue.contains(&key) {
            return;
        }
        self.load_queue.push_back(key);
    }

    /// Starts queued picture loads while fewer than [`MAX_LOADS`] run. Does nothing without a data source (tests).
    fn start_loads(&mut self) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        while self.loads_running < MAX_LOADS {
            let Some((asset, level)) = self.load_queue.pop_front() else {
                break;
            };
            if self
                .pictures
                .get(&asset)
                .is_some_and(|levels| levels.contains_key(&level))
            {
                continue;
            }
            let (sha, thumbs, video) = self
                .assets
                .get(&asset)
                .map(|known| (known.sha256.clone(), known.thumbs.clone(), known.is_video()))
                .unwrap_or_default();
            self.loading.insert((asset.clone(), level));
            self.loads_running += 1;
            let job = LoadJob {
                canvas: canvas.clone(),
                asset: asset.clone(),
                sha,
                thumbs,
                level,
                video,
            };
            let cache = self.cache.clone();
            let generation = self.open_generation;
            self.ui.fetch(
                &source,
                move |api, token| load_picture(api, token, job, cache),
                move |_ui, result| {
                    with_page(|inner| inner.picture_loaded(generation, asset, level, result));
                },
            );
        }
    }

    /// A picture load finished: its picture is kept, the items are redrawn, and the next queued load starts. A result for
    /// an earlier open is dropped.
    fn picture_loaded(&mut self, generation: u64, asset: String, level: u32, result: LoadResult) {
        self.loads_running = self.loads_running.saturating_sub(1);
        if generation != self.open_generation {
            self.start_loads();
            return;
        }
        self.loading.remove(&(asset.clone(), level));
        match result {
            Ok((loaded, updated)) => {
                if let Some(updated) = updated {
                    self.store_asset(updated);
                }
                let picture = Picture::new(&loaded.decoded, &loaded.grey);
                self.pictures
                    .entry(asset.clone())
                    .or_default()
                    .insert(level, picture);
                self.missing.remove(&asset);
                if let Some((wanted, x, y)) = self.eyedrop.clone()
                    && wanted == asset
                    && level == 0
                {
                    self.eyedrop = None;
                    self.sample_at(&asset, x, y);
                }
            }
            Err(_) => {
                // A video without a thumbnail is not missing: its placeholder stays, and it is not asked for again.
                if self.assets.get(&asset).is_some_and(AssetView::is_video) {
                    self.no_thumb.insert(asset.clone());
                } else if !self.pictures.contains_key(&asset) {
                    self.missing.insert(asset.clone());
                }
                if self
                    .eyedrop
                    .as_ref()
                    .is_some_and(|(wanted, _, _)| *wanted == asset)
                {
                    self.eyedrop = None;
                }
            }
        }
        self.sync_missing();
        if self.assets_open {
            self.push_asset_rows();
        }
        self.refresh();
        self.start_loads();
    }

    /// The picture an item draws for `asset` at `wanted`: that level, else the nearest one that is loaded.
    fn picture_for(&self, asset: &str, wanted: u32) -> Option<(u32, &Picture)> {
        let levels = self.pictures.get(asset)?;
        [wanted, 1024, 256, 0]
            .into_iter()
            .find_map(|level| levels.get(&level).map(|picture| (level, picture)))
    }

    /// True when the original of `asset` is loaded.
    fn has_original(&self, asset: &str) -> bool {
        self.pictures
            .get(asset)
            .is_some_and(|levels| levels.contains_key(&0))
    }

    /// The key of the picture an item draws now, or `None` for an item with no picture (or a picture not loaded yet).
    fn key_of(&self, row: &ItemRow, now_ms: u64) -> Option<PictureKey> {
        if !matches!(row.kind, ItemKind::Image | ItemKind::Video) || row.asset.is_empty() {
            return None;
        }
        let wanted = self.levels.get(&row.id).copied().unwrap_or(1024);
        let (level, picture) = self.picture_for(&row.asset, wanted)?;
        if picture.color.is_empty() {
            return None;
        }
        let frame = frame_index(&picture.delays_ms, now_ms).min(picture.color.len() - 1);
        Some(PictureKey {
            level,
            frame,
            gray: row.gray,
            animated: picture.color.len() > 1,
        })
    }

    /// Chooses the level of each image item from its on-screen size (with hysteresis) and queues what it needs. The
    /// slideshow's picture is queued too.
    fn plan_pictures(&mut self, rows: &[ItemRow], zoom: f64) {
        let mut wanted: Vec<(String, u32)> = Vec::new();
        for row in rows {
            if !matches!(row.kind, ItemKind::Image | ItemKind::Video) || row.asset.is_empty() {
                continue;
            }
            let long_side = row.rect.w.max(row.rect.h) * zoom;
            let current = self.levels.get(&row.id).copied().unwrap_or(u32::MAX);
            let mut level = images::level_for(long_side, current);
            if matches!(row.kind, ItemKind::Video) {
                // A video never loads its original (the file): the thumbnail of its level, else the one the asset has.
                let wanted_level = if level == 0 { 1024 } else { level };
                let thumbs = self
                    .assets
                    .get(&row.asset)
                    .map(|known| known.thumbs.clone())
                    .unwrap_or_default();
                level = [wanted_level, 1024, 256]
                    .into_iter()
                    .find(|thumb| thumbs.contains(thumb))
                    .unwrap_or(wanted_level);
                self.levels.insert(row.id.clone(), level);
                if thumbs.contains(&level) {
                    wanted.push((row.asset.clone(), level));
                }
                continue;
            }
            self.levels.insert(row.id.clone(), level);
            wanted.push((row.asset.clone(), level));
        }
        if let Some(view) = self.controller.as_ref().and_then(Controller::slideshow) {
            wanted.push((view.asset, 1024));
        }
        for (asset, level) in wanted {
            self.request(&asset, level);
        }
        self.start_loads();
    }

    /// The view of one row: its geometry, its look, and its picture (the frame and level of `key`) when it has one.
    fn view_of(&self, row: &ItemRow, key: Option<PictureKey>) -> ItemView {
        let picture = key.and_then(|key| {
            self.picture_for(&row.asset, key.level)
                .map(|(_, picture)| (picture, key))
        });
        let (image, has_image) = match picture {
            Some((picture, key)) => {
                let frames = if key.gray {
                    &picture.grey
                } else {
                    &picture.color
                };
                (frames.get(key.frame).cloned().unwrap_or_default(), true)
            }
            None => (slint::Image::default(), false),
        };
        // A playing video draws its latest frame over its thumbnail.
        let (image, has_image) = match self.video_frame(&row.id) {
            Some(frame) => (frame, true),
            None => (image, has_image),
        };
        // The crop, in pixels of the picture drawn (the fractions of the crop times the picture's size).
        let crop: [f32; 4] = match (row.crop.as_ref(), picture) {
            (Some(crop), Some((picture, _))) => {
                let w = f64::from(picture.width);
                let h = f64::from(picture.height);
                [
                    (crop.x * w) as f32,
                    (crop.y * h) as f32,
                    (crop.w * w) as f32,
                    (crop.h * h) as f32,
                ]
            }
            _ => [0.0; 4],
        };
        ItemView {
            id: row.id.as_str().into(),
            kind: kind_name(row.kind).into(),
            x: row.rect.x as f32,
            y: row.rect.y as f32,
            w: row.rect.w as f32,
            h: row.rect.h as f32,
            rotation: row.rotation as f32,
            flip_x: row.flip_x,
            flip_y: row.flip_y,
            // The items the eraser would delete show faded.
            opacity: if row.erase_hover {
                (row.opacity * 0.3) as f32
            } else {
                row.opacity as f32
            },
            image,
            has_image,
            pixelated: row.pixelated,
            text: row.text.as_str().into(),
            color: row.color.as_str().into(),
            fill: row.fill.as_str().into(),
            font_size: row.font_size as f32,
            align: row.align.into(),
            locked: row.locked,
            selected: row.selected,
            editing: row.editing,
            crop_x: crop[0],
            crop_y: crop[1],
            crop_w: crop[2],
            crop_h: crop[3],
            path: row.path.as_str().into(),
            shape: row.shape.into(),
            stroke_width: row.stroke_width as f32,
            stroke_color: row.stroke_color.as_str().into(),
            fill_color: row.fill_color.as_str().into(),
            comment: row.comment.as_str().into(),
            gray: row.gray,
            is_video: matches!(row.kind, ItemKind::Video),
            duration_text: self
                .assets
                .get(&row.asset)
                .and_then(|asset| asset.duration_ms)
                .filter(|_| matches!(row.kind, ItemKind::Video))
                .map(duration_text)
                .unwrap_or_default(),
        }
    }

    /// Replaces the item rows that changed (their look, or the picture they draw). A change of the row list rebuilds the model.
    fn push_items(&mut self, rows: Vec<ItemRow>) {
        let now = self.now_ms();
        let keys: Vec<Option<PictureKey>> = rows.iter().map(|row| self.key_of(row, now)).collect();
        let same_rows = self.last_rows.len() == rows.len()
            && self.items_model.row_count() == rows.len()
            && self
                .last_rows
                .iter()
                .zip(&rows)
                .all(|(old, new)| old.id == new.id);
        if same_rows {
            for (index, row) in rows.iter().enumerate() {
                if self.last_rows[index] != *row || self.last_keys[index] != keys[index] {
                    let view = self.view_of(row, keys[index]);
                    self.items_model.set_row_data(index, view);
                }
            }
        } else {
            let views: Vec<ItemView> = rows
                .iter()
                .zip(&keys)
                .map(|(row, key)| self.view_of(row, *key))
                .collect();
            self.items_model.set_vec(views);
        }
        self.last_rows = rows;
        self.last_keys = keys;
    }

    /// The frame timer: redraws the items whose animated picture moved on to its next frame.
    fn tick_animations(&mut self) {
        let now = self.now_ms();
        let changed: Vec<(usize, Option<PictureKey>)> = self
            .last_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let key = self.key_of(row, now);
                (key != self.last_keys[index]).then_some((index, key))
            })
            .collect();
        for (index, key) in changed {
            let view = self.view_of(&self.last_rows[index], key);
            self.items_model.set_row_data(index, view);
            self.last_keys[index] = key;
        }
    }

    /// Starts or stops the frame timer by whether an animated picture is on screen.
    fn sync_anim_timer(&self) {
        let running = self.anim_timer.running();
        if self.animating && !running {
            self.anim_timer
                .start(slint::TimerMode::Repeated, FRAME_EVERY, || {
                    with_page(|inner| inner.tick_animations())
                });
        } else if !self.animating && running {
            self.anim_timer.stop();
        }
    }

    /// Starts or stops the slideshow timer by whether the slideshow plays.
    fn sync_slide_timer(&self) {
        let playing = self
            .controller
            .as_ref()
            .and_then(Controller::slideshow)
            .is_some_and(|view| view.playing);
        let running = self.slide_timer.running();
        if playing && !running {
            self.slide_timer
                .start(slint::TimerMode::Repeated, SLIDE_EVERY, || {
                    with_page(|inner| inner.act(|c| c.slideshow_tick()))
                });
        } else if !playing && running {
            self.slide_timer.stop();
        }
    }

    /// Playback of the video `id` (see [`VideoStatus`]).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn video_status(&self, id: &str) -> VideoStatus {
        match self.players.get(id) {
            Some(playback) => VideoStatus {
                playing: playback.player.is_playing() && !playback.player.ended(),
                position_ms: playback.player.position_ms(),
                duration_ms: playback.player.duration_ms(),
                loading: false,
            },
            None => VideoStatus {
                loading: self.loading_videos.contains(id),
                ..VideoStatus::default()
            },
        }
    }

    /// Playback of the video `id`: none here, the players are desktop only.
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn video_status(&self, _id: &str) -> VideoStatus {
        VideoStatus::default()
    }

    /// The newest frame of the video `id`, while its player has one.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn video_frame(&self, id: &str) -> Option<slint::Image> {
        self.players
            .get(id)
            .and_then(|playback| playback.frame.clone())
    }

    /// No video frames without the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn video_frame(&self, _id: &str) -> Option<slint::Image> {
        None
    }

    /// Takes the newest frame of each playing video. Returns the ids of the videos that got one.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn take_video_frames(&mut self) -> Vec<String> {
        let mut changed = Vec::new();
        for (id, playback) in self.players.iter_mut() {
            if let Some(frame) = playback.player.take_frame() {
                playback.frame = Some(frame.picture.into_slint_image());
                changed.push(id.clone());
            }
        }
        changed
    }

    /// No frames without the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn take_video_frames(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Applies each playing video's muted and loop props (they can change remotely) and its output size (when the item's
    /// size on screen changed by more than a quarter).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn sync_players(&mut self) {
        let zoom = self.controller.as_ref().map_or(1.0, |c| c.camera().zoom);
        for row in self
            .last_rows
            .iter()
            .filter(|row| matches!(row.kind, ItemKind::Video))
        {
            let Some(playback) = self.players.get_mut(&row.id) else {
                continue;
            };
            playback.player.set_muted(row.muted);
            playback.player.set_looping(row.looping);
            let (w, h) = output_size(&row.rect, zoom);
            let moved = |old: u32, new: u32| {
                (f64::from(new) - f64::from(old)).abs() > 0.25 * f64::from(old)
            };
            if moved(playback.size.0, w) || moved(playback.size.1, h) {
                playback.player.set_output_size(w, h);
                playback.size = (w, h);
            }
        }
    }

    /// Nothing to apply without the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn sync_players(&mut self) {}

    /// Drops the players and loads of videos that are no longer on the canvas (deleted).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn drop_players(&mut self, videos: &HashSet<String>) {
        self.players.retain(|id, _| videos.contains(id));
        self.loading_videos.retain(|id| videos.contains(id));
    }

    /// Nothing to drop without the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn drop_players(&mut self, _videos: &HashSet<String>) {}

    /// Drops every player and load (the canvas closes or another one opens).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn clear_players(&mut self) {
        self.players.clear();
        self.loading_videos.clear();
    }

    /// Nothing to clear without the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn clear_players(&mut self) {}

    /// Stops every video on this page: the frame timer stops and the players are dropped.
    fn stop_videos(&mut self) {
        self.video_timer.stop();
        self.clear_players();
    }

    /// Seeks a playing video to a fraction (0..1) of its length.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn seek_video(&mut self, id: &str, fraction: f64) {
        let seeked = self.players.get(id).map(|playback| {
            let ms = fraction.clamp(0.0, 1.0) * playback.player.duration_ms() as f64;
            let ms = ms.round() as u64;
            playback.player.seek(ms);
            ms
        });
        if let Some(position_ms) = seeked {
            let playing = self.video_status(id).playing;
            self.report_video(id, playing, position_ms);
        }
        self.push_video_controls();
    }

    /// Seeking needs the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn seek_video(&mut self, _id: &str, _fraction: f64) {}

    /// Plays or pauses the video `id`. A video with no player yet is opened: from the disk cache, or after its file is
    /// downloaded.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn toggle_video(&mut self, id: &str) {
        let status = self.video_status(id);
        let restart = self.players.get(id).is_some_and(|p| p.player.ended());
        if let Some(playback) = self.players.get(id) {
            if playback.player.is_playing() {
                playback.player.pause();
            } else {
                if playback.player.ended() {
                    playback.player.seek(0);
                }
                playback.player.play();
            }
        } else if !self.loading_videos.contains(id) {
            self.start_video(id);
        }
        self.sync_video_timer();
        self.push_video_controls();
        if !status.loading {
            let position_ms = if restart { 0 } else { status.position_ms };
            self.report_video(id, !status.playing, position_ms);
        }
    }

    /// Playing needs the desktop app: elsewhere a press says so.
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn toggle_video(&mut self, _id: &str) {
        toast::announce(Tone::Neutral, "Video plays in the desktop app.");
    }

    /// Starts a video with no player: its file opens from the disk cache, or it is downloaded first (busy line shows it).
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn start_video(&mut self, id: &str) {
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        let Some(asset) = self
            .last_rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| self.assets.get(&row.asset).cloned())
        else {
            return;
        };
        if let Some(path) = self.cache.file(&asset.sha256, "orig") {
            self.open_video(id, &path);
            return;
        }
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        self.loading_videos.insert(id.to_string());
        self.set_busy(&format!("Downloading {}…", asset.file_name));
        let cache = self.cache.clone();
        let (sha, asset_id) = (asset.sha256.clone(), asset.id.clone());
        let video = id.to_string();
        let generation = self.open_generation;
        self.ui.fetch(
            &source,
            move |api, token| async move {
                let bytes = api.canvas_asset_bytes(&token, &canvas, &asset_id).await?;
                cache.put(&sha, "orig", &bytes);
                Ok::<Option<PathBuf>, ClientError>(cache.file(&sha, "orig"))
            },
            move |_ui, result| {
                with_page(|inner| inner.video_downloaded(generation, &video, result));
            },
        );
        self.push_video_controls();
    }

    /// A video's file download finished: it opens when this canvas is still open.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn video_downloaded(
        &mut self,
        generation: u64,
        video: &str,
        result: Result<Option<PathBuf>, ClientError>,
    ) {
        if generation != self.open_generation {
            return;
        }
        self.loading_videos.remove(video);
        self.set_busy("");
        match result {
            Ok(Some(path)) => self.open_video(video, &path),
            Ok(None) => toast::announce(Tone::Danger, "The video could not be saved for playing."),
            Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
        }
        self.push_video_controls();
    }

    /// Opens a player on the file of video `id`, sized to its box on screen, and starts it. At most [`MAX_PLAYERS`] stay open:
    /// the one started longest ago is stopped first.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn open_video(&mut self, id: &str, path: &Path) {
        let zoom = self.controller.as_ref().map_or(1.0, |c| c.camera().zoom);
        let Some((muted, looping, (w, h))) = self
            .last_rows
            .iter()
            .find(|row| row.id == id)
            .map(|row| (row.muted, row.looping, output_size(&row.rect, zoom)))
        else {
            return;
        };
        if self.players.len() >= MAX_PLAYERS
            && let Some(oldest) = self
                .players
                .iter()
                .min_by_key(|(_, playback)| playback.started)
                .map(|(id, _)| id.clone())
        {
            self.players.remove(&oldest);
        }
        let options = rimelight_media::PlayerOptions {
            max_width: w,
            max_height: h,
            audio: true,
        };
        match rimelight_media::Player::open(path, options) {
            Ok(player) => {
                player.set_muted(muted);
                player.set_looping(looping);
                player.play();
                self.players.insert(
                    id.to_string(),
                    Playback {
                        player,
                        frame: None,
                        started: Instant::now(),
                        size: (w, h),
                    },
                );
            }
            Err(error) => toast::announce(
                Tone::Danger,
                format!("The video could not be opened: {error}"),
            ),
        }
        self.sync_video_timer();
        self.push_video_controls();
    }

    /// Sets the shared `muted` or `loop` prop of the video `id` to its opposite, as its overlay button asks.
    fn flip_video_flag(&mut self, id: &str, flag: &str) {
        let on = self
            .last_rows
            .iter()
            .find(|row| row.id == id)
            .is_some_and(|row| {
                if flag == "muted" {
                    !row.muted
                } else {
                    !row.looping
                }
            });
        self.act(|c| c.set_video_flag(id, flag, on));
    }

    /// The play controls of the videos on screen: one per video row, boxed at its screen rect (rotation ignored).
    fn video_controls(&self) -> Vec<VideoControlView> {
        let camera = self
            .controller
            .as_ref()
            .map(Controller::camera)
            .unwrap_or_default();
        self.last_rows
            .iter()
            .filter(|row| matches!(row.kind, ItemKind::Video))
            .map(|row| {
                let status = self.video_status(&row.id);
                let asset_ms = self
                    .assets
                    .get(&row.asset)
                    .and_then(|asset| asset.duration_ms)
                    .unwrap_or(0);
                let total = if status.duration_ms > 0 {
                    status.duration_ms
                } else {
                    asset_ms
                };
                let progress = if total > 0 {
                    (status.position_ms as f64 / total as f64).min(1.0)
                } else {
                    0.0
                };
                VideoControlView {
                    id: row.id.as_str().into(),
                    x: (row.rect.x * camera.zoom + camera.pan.x) as f32,
                    y: (row.rect.y * camera.zoom + camera.pan.y) as f32,
                    w: (row.rect.w * camera.zoom) as f32,
                    h: (row.rect.h * camera.zoom) as f32,
                    playing: status.playing,
                    progress: progress as f32,
                    time_text: format!(
                        "{} / {}",
                        duration_text(status.position_ms),
                        duration_text(total)
                    ),
                    muted: row.muted,
                    looping: row.looping,
                    loading: status.loading,
                    selected: row.selected,
                    shared: self.shared.contains_key(row.id.as_str()),
                    following: self.is_following(&row.id),
                }
            })
            .collect()
    }

    /// Puts the video controls into the active window, in place.
    fn push_video_controls(&self) {
        let controls = self.video_controls();
        self.with_state(|state| {
            state.sync_video_controls(controls);
        });
    }

    /// True while this person follows the shared playback of video `id`: there is one and they have not left it.
    fn is_following(&self, id: &str) -> bool {
        self.shared.contains_key(id) && !self.left.contains(id)
    }

    /// Sends a shared video control of this person to the room (when connected).
    fn send_media(&self, id: &str, playing: bool, position_ms: u64, ended: bool) {
        if let Some(session) = self.session.as_ref() {
            session.send_media(MediaControl {
                item_id: id.to_string(),
                playing,
                position_ms,
                ended,
            });
        }
    }

    /// Sends a play, pause or seek of video `id` to the room, when this person follows its shared playback and may
    /// control it. A viewer sends nothing: the room would refuse it.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn report_video(&self, id: &str, playing: bool, position_ms: u64) {
        if self.is_following(id) && !self.is_read_only() {
            self.send_media(id, playing, position_ms, false);
        }
    }

    /// The "Play for everyone" button of video `id` (CV-V.5). With no shared playback, an editor starts one from this
    /// person's position. With one, a follower who may control it ends it when it is paused, else leaves it; someone who
    /// left joins back. A viewer only follows and leaves.
    fn video_together(&mut self, id: &str) {
        let editable = !self.is_read_only();
        let Some(media) = self.shared.get(id).cloned() else {
            if editable {
                let position_ms = self.video_status(id).position_ms;
                self.send_media(id, true, position_ms, false);
            }
            return;
        };
        if self.left.remove(id) {
            self.follow_shared();
        } else if editable && !media.playing {
            self.send_media(id, false, media.position_ms, true);
        } else {
            self.left.insert(id.to_string());
        }
        self.refresh();
    }

    /// A shared playback changed in the room. It is stored (or dropped when it ended), and the people following it follow.
    fn media_changed(&mut self, media: MediaState) {
        if media.ended {
            self.shared.remove(&media.item_id);
            self.left.remove(&media.item_id);
        } else {
            self.shared.insert(media.item_id.clone(), media);
        }
        self.follow_shared();
        self.refresh();
    }

    /// Brings the players of the shared playbacks this person follows to them.
    fn follow_shared(&mut self) {
        let followed: Vec<MediaState> = self
            .shared
            .values()
            .filter(|media| !self.left.contains(media.item_id.as_str()))
            .cloned()
            .collect();
        for media in &followed {
            self.apply_media(media);
        }
    }

    /// Where a shared playback is now, in ms: its position, moved on by the time since the room stamped it while it plays.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn shared_position_ms(&self, media: &MediaState) -> u64 {
        let Some(session) = self.session.as_ref() else {
            return media.position_ms;
        };
        if !media.playing {
            return media.position_ms;
        }
        let behind = session.server_now_ms().saturating_sub(media.at);
        media
            .position_ms
            .saturating_add(u64::try_from(behind).unwrap_or(0))
    }

    /// Brings the player of video `media.item_id` to a shared playback: the player opens when there is none (its file is
    /// downloaded first if needed), then plays or pauses with the shared state, and seeks when it is more than 300 ms away.
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    fn apply_media(&mut self, media: &MediaState) {
        const DRIFT_MS: u64 = 300;
        let id = media.item_id.as_str();
        if !self.players.contains_key(id) && !self.loading_videos.contains(id) {
            self.start_video(id);
        }
        let target = self.shared_position_ms(media);
        let Some(playback) = self.players.get(id) else {
            return;
        };
        let player = &playback.player;
        let duration = player.duration_ms();
        let target = if duration > 0 {
            target.min(duration)
        } else {
            target
        };
        if media.playing {
            if !player.is_playing() && !player.ended() {
                player.play();
            }
        } else if player.is_playing() {
            player.pause();
        }
        if player.position_ms().abs_diff(target) > DRIFT_MS {
            player.seek(target);
        }
        self.sync_video_timer();
        self.push_video_controls();
    }

    /// Following needs the players (desktop only).
    #[cfg(not(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    fn apply_media(&mut self, _media: &MediaState) {}

    /// Redraws one item row from its current state (a video's frame changed).
    fn redraw_video(&self, id: &str) {
        let Some(index) = self.last_rows.iter().position(|row| row.id == id) else {
            return;
        };
        let view = self.view_of(&self.last_rows[index], self.last_keys[index]);
        self.items_model.set_row_data(index, view);
    }

    /// Starts the frame timer while a video plays, and stops it when none does.
    fn sync_video_timer(&self) {
        let playing = self
            .last_rows
            .iter()
            .any(|row| matches!(row.kind, ItemKind::Video) && self.video_status(&row.id).playing);
        let running = self.video_timer.running();
        if playing && !running {
            self.video_timer
                .start(slint::TimerMode::Repeated, VIDEO_EVERY, || {
                    with_page(|inner| inner.tick_video())
                });
        } else if !playing && running {
            self.video_timer.stop();
        }
    }

    /// The frame timer: draws each video's new frame, keeps the controls current, and stops when nothing plays.
    fn tick_video(&mut self) {
        self.follow_shared();
        for id in self.take_video_frames() {
            self.redraw_video(&id);
        }
        self.push_video_controls();
        self.sync_video_timer();
    }

    /// Redraws the items, the selection, the marquee, the guides, the view settings and the toolbar state from the controller.
    fn refresh(&mut self) {
        let controller = self.controller.as_ref();
        let camera = controller.map(Controller::camera).unwrap_or_default();
        let all_rows = controller.map(Controller::rows).unwrap_or_default();
        if controller.is_some() {
            // A shared playback of a deleted item is dropped.
            let ids: HashSet<String> = all_rows.iter().map(|row| row.id.clone()).collect();
            self.shared.retain(|id, _| ids.contains(id));
            self.left.retain(|id| ids.contains(id));
        }
        self.plan_pictures(&all_rows, camera.zoom);
        let videos: HashSet<String> = all_rows
            .iter()
            .filter(|row| matches!(row.kind, ItemKind::Video))
            .map(|row| row.id.clone())
            .collect();
        self.drop_players(&videos);
        let (peers, boxes) = self.peer_views(&all_rows, camera);
        let overlay = matches!(self.host, Host::Popout(_)) && popout::overlay_active();
        let rows: Vec<ItemRow> = if overlay {
            all_rows.into_iter().filter(|row| row.selected).collect()
        } else {
            all_rows
        };
        self.push_items(rows);
        self.sync_players();
        self.animating = self
            .last_keys
            .iter()
            .any(|key| key.is_some_and(|key| key.animated));
        self.sync_anim_timer();
        self.sync_slide_timer();
        self.sync_video_timer();
        self.push_state(camera, peers, boxes);
    }

    /// The other people in the room, with their cursors and selection outlines on screen.
    fn peer_views(&self, rows: &[ItemRow], camera: Camera) -> (Vec<PeerView>, Vec<PeerBoxView>) {
        let to_screen = |x: f64, y: f64| {
            (
                x * camera.zoom + camera.pan.x,
                y * camera.zoom + camera.pan.y,
            )
        };
        let mut peers = Vec::new();
        let mut boxes = Vec::new();
        for peer in &self.peers {
            let color = peer_color(&peer.color);
            let cursor = peer
                .cursor
                .as_ref()
                .map(|point| to_screen(point.x, point.y));
            peers.push(PeerView {
                conn_id: peer.conn_id.as_str().into(),
                name: peer.name.as_str().into(),
                initials: initials_of(&peer.name),
                color,
                cursor_x: cursor.map_or(0.0, |(x, _)| x as f32),
                cursor_y: cursor.map_or(0.0, |(_, y)| y as f32),
                has_cursor: cursor.is_some(),
                following: self.follow_id.as_deref() == Some(peer.conn_id.as_str()),
            });
            for id in &peer.selection {
                if let Some(row) = rows.iter().find(|row| &row.id == id) {
                    let (x, y) = to_screen(row.rect.x, row.rect.y);
                    boxes.push(PeerBoxView {
                        color,
                        x: x as f32,
                        y: y as f32,
                        w: (row.rect.w * camera.zoom) as f32,
                        h: (row.rect.h * camera.zoom) as f32,
                        rotation: row.rotation as f32,
                    });
                }
            }
        }
        (peers, boxes)
    }

    /// The images of the selection or the canvas, as the export draws them: each asset's picture must be loaded first.
    fn export(&mut self, command: ExportCommand) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        match command {
            ExportCommand::CanvasImage => self.export_image(Want::Canvas, true),
            ExportCommand::SelectionImage => self.export_image(Want::Selection, true),
            ExportCommand::AllToFolder => {
                let files: Vec<(String, AssetView)> = self
                    .assets
                    .values()
                    .map(|asset| (asset.file_name.clone(), asset.clone()))
                    .collect();
                actions::save_images_to_folder(&ctx, files);
            }
            ExportCommand::SelectedToFolder => {
                let files: Vec<(String, AssetView)> = self
                    .selected_images()
                    .into_iter()
                    .map(|asset| (asset.file_name.clone(), asset))
                    .collect();
                if files.is_empty() {
                    toast::announce(Tone::Warning, "No image is selected.");
                } else {
                    actions::save_images_to_folder(&ctx, files);
                }
            }
            ExportCommand::Backup => self.save_backup(&ctx),
            ExportCommand::OpenBackup => actions::open_backup(&ctx),
        }
    }

    /// PNG of the canvas or the selection. The originals are loaded first when they are not on the page yet.
    fn export_image(&mut self, want: Want, fetch: bool) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        let (items, bounds) = match want {
            Want::Canvas => {
                let ids: Vec<String> = controller
                    .doc_ref()
                    .ordered()
                    .iter()
                    .map(|item| item.id.clone())
                    .collect();
                (
                    controller.items_for_export(false),
                    controller.doc_ref().bounds(&ids),
                )
            }
            Want::Selection => (
                controller.items_for_export(true),
                controller.selection_bounds(),
            ),
        };
        let Some(bounds) = bounds else {
            toast::announce(Tone::Warning, "There is nothing to export.");
            return;
        };
        let mut needed: Vec<String> = Vec::new();
        for item in &items {
            if let ItemData::Image { asset, .. } = &item.data
                && !needed.contains(asset)
                && !self.has_original(asset)
            {
                needed.push(asset.clone());
            }
        }
        if fetch && !needed.is_empty() {
            self.load_originals(needed, want);
            return;
        }
        let pictures: HashMap<String, images::Buffer> = items
            .iter()
            .filter_map(|item| match &item.data {
                ItemData::Image { asset, .. } => self
                    .pictures
                    .get(asset)
                    .and_then(|levels| levels.get(&0))
                    .map(|picture| (asset.clone(), picture.first.clone())),
                // A video is drawn from its thumbnail, never from the video file.
                ItemData::Video { asset, .. } => self
                    .picture_for(asset, 1024)
                    .map(|(_, picture)| (asset.clone(), picture.first.clone())),
                _ => None,
            })
            .collect();
        let stem = self.title();
        actions::export_image(&ctx, items, pictures, bounds, stem);
    }

    /// Downloads the originals of `assets` for an export, then exports again.
    fn load_originals(&mut self, assets: Vec<String>, want: Want) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(canvas) = self.canvas_id.clone() else {
            return;
        };
        let jobs: Vec<LoadJob> = assets
            .into_iter()
            .map(|asset| {
                let sha = self
                    .assets
                    .get(&asset)
                    .map(|known| known.sha256.clone())
                    .unwrap_or_default();
                LoadJob {
                    canvas: canvas.clone(),
                    asset,
                    sha,
                    thumbs: Vec::new(),
                    level: 0,
                    video: false,
                }
            })
            .collect();
        let cache = self.cache.clone();
        self.set_busy("Preparing the export…");
        self.ui.fetch(
            &source,
            move |api, token| fetch_originals(api, token, jobs, cache),
            move |_ui, result| with_page(|inner| inner.originals_ready(want, result)),
        );
    }

    /// The originals of an export arrived: they are kept, and the export runs.
    fn originals_ready(&mut self, want: Want, result: Result<Vec<(String, Loaded)>, ClientError>) {
        self.set_busy("");
        match result {
            Ok(loaded) => {
                for (asset, picture) in loaded {
                    self.pictures
                        .entry(asset)
                        .or_default()
                        .insert(0, Picture::new(&picture.decoded, &picture.grey));
                }
                self.export_image(want, false);
            }
            Err(error) => toast::announce(Tone::Danger, page_data::error_text(&error)),
        }
    }

    /// The backup file of the canvas: its items as the room states them, and the images they use.
    fn save_backup(&self, ctx: &actions::Ctx) {
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        let items: Vec<ItemState> = controller
            .doc_ref()
            .ordered()
            .into_iter()
            .map(item_state)
            .collect();
        let title = self.title();
        let json = serde_json::json!({ "title": title, "items": items });
        let image_ids: Vec<String> = controller
            .image_items()
            .into_iter()
            .map(|(_, asset)| asset)
            .collect();
        let assets = self.assets_of(&image_ids);
        actions::save_backup(ctx, json, assets, title);
    }

    /// Manage images (Ctrl+Shift+J): the dialog opens at once, and its rows come with the usage.
    fn manage_images(&mut self) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        self.assets_open = true;
        self.with_state(|state| {
            state.set_assets_open(true);
            state.set_assets_busy(true);
        });
        actions::load_asset_usage(&ctx, move |result| {
            with_page(|inner| inner.assets_loaded(result));
        });
    }

    /// The usage of the canvas's images arrived: the dialog's rows show it.
    fn assets_loaded(&mut self, result: Result<Vec<AssetUsage>, ClientError>) {
        self.with_state(|state| state.set_assets_busy(false));
        match result {
            Ok(list) => {
                let assets: Vec<AssetView> = list.iter().map(|usage| usage.asset.clone()).collect();
                self.usage = list;
                for asset in assets {
                    self.store_asset(asset);
                }
                self.push_asset_rows();
            }
            Err(error) => {
                self.assets_open = false;
                self.with_state(|state| state.set_assets_open(false));
                toast::announce(Tone::Danger, page_data::error_text(&error));
            }
        }
    }

    /// The manage-images rows from the usage, with the 256 thumbnail where its picture is loaded.
    fn push_asset_rows(&self) {
        let rows: Vec<AssetRowView> = self
            .usage
            .iter()
            .map(|usage| {
                let thumb = self
                    .pictures
                    .get(&usage.asset.id)
                    .and_then(|levels| levels.get(&256))
                    .and_then(|picture| picture.color.first().cloned());
                AssetRowView {
                    id: usage.asset.id.as_str().into(),
                    name: usage.asset.file_name.as_str().into(),
                    size_text: size_text(usage.asset.bytes),
                    pixels_text: format!("{} × {} px", usage.asset.width, usage.asset.height),
                    uses: i32::try_from(usage.uses).unwrap_or(i32::MAX),
                    has_thumb: thumb.is_some(),
                    thumb: thumb.unwrap_or_default(),
                }
            })
            .collect();
        self.with_state(|state| state.sync_asset_rows(rows));
    }

    /// Delete unused images: the assets no live item uses are deleted, then the dialog reloads.
    fn delete_unused(&mut self) {
        let ids: Vec<String> = self
            .usage
            .iter()
            .filter(|usage| usage.uses == 0)
            .map(|usage| usage.asset.id.clone())
            .collect();
        if ids.is_empty() {
            toast::announce(Tone::Neutral, "No image is unused.");
            return;
        }
        let Some(ctx) = self.ctx() else {
            return;
        };
        self.with_state(|state| state.set_assets_busy(true));
        actions::delete_unused_assets(&ctx, ids, move |deleted| {
            with_page(|inner| inner.unused_deleted(deleted));
        });
    }

    /// The unused images are gone: the count is told and the dialog reloads.
    fn unused_deleted(&mut self, deleted: usize) {
        toast::announce(Tone::Success, format!("Deleted {deleted} unused images."));
        self.manage_images();
    }

    /// Closes the manage-images dialog.
    fn close_assets(&mut self) {
        self.assets_open = false;
        self.with_state(|state| state.set_assets_open(false));
    }

    /// Replace image: a file is picked and uploaded, and the selected image item uses it.
    fn replace_image(&mut self, item: String) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        actions::pick_and_upload_one(&ctx, move |picked| {
            with_page(|inner| inner.replaced(&item, picked));
        });
    }

    /// The replacement picture arrived: it is kept and the item uses it (centre and longer side kept).
    fn replaced(&mut self, item: &str, picked: Option<(AssetView, images::Decoded)>) {
        let Some((asset, decoded)) = picked else {
            return;
        };
        self.adopt_asset(asset.clone(), Some(loaded_of(decoded)));
        let ids = [item.to_string()];
        let effects = match self.controller.as_mut() {
            Some(controller) => controller.replace_asset(&ids, &asset),
            None => Vec::new(),
        };
        self.run(effects);
    }

    /// Relink missing images: every missing asset is listed with the items that use it; a file is picked for each.
    fn relink_start(&mut self) {
        let image_items = self
            .controller
            .as_ref()
            .map(Controller::image_items)
            .unwrap_or_default();
        let mut missing: Vec<String> = self.missing.iter().cloned().collect();
        missing.sort();
        self.relink = missing
            .into_iter()
            .map(|asset| {
                let items: Vec<String> = image_items
                    .iter()
                    .filter(|(_, used)| *used == asset)
                    .map(|(item, _)| item.clone())
                    .collect();
                (asset, items)
            })
            .collect();
        if self.relink.is_empty() {
            toast::announce(Tone::Neutral, "No image is missing.");
            return;
        }
        self.relink_next();
    }

    /// Picks the file for the next missing asset.
    fn relink_next(&mut self) {
        let Some((asset, items)) = self.relink.pop_front() else {
            return;
        };
        let Some(ctx) = self.ctx() else {
            return;
        };
        actions::pick_and_upload_one(&ctx, move |picked| {
            with_page(|inner| inner.relinked(asset, items, picked));
        });
    }

    /// A relink file arrived: the items that used the missing asset now use the new one, then the next one is asked for.
    /// A cancelled dialog ends the relink.
    fn relinked(
        &mut self,
        asset: String,
        items: Vec<String>,
        picked: Option<(AssetView, images::Decoded)>,
    ) {
        let Some((new, decoded)) = picked else {
            self.relink.clear();
            return;
        };
        self.adopt_asset(new.clone(), Some(loaded_of(decoded)));
        self.missing.remove(&asset);
        self.sync_missing();
        let effects = match self.controller.as_mut() {
            Some(controller) => controller.replace_asset(&items, &new),
            None => Vec::new(),
        };
        self.run(effects);
        self.relink_next();
    }

    /// The eyedropper: the colour of the original picture under the click, copied as #rrggbb. The original loads first when
    /// it is not on the page.
    fn eyedrop(&mut self, asset: String, x_frac: f64, y_frac: f64) {
        if self.has_original(&asset) {
            self.sample_at(&asset, x_frac, y_frac);
            return;
        }
        self.eyedrop = Some((asset.clone(), x_frac, y_frac));
        self.request(&asset, 0);
        self.start_loads();
    }

    /// Reads the colour at a fraction of the original picture and reports it.
    fn sample_at(&self, asset: &str, x_frac: f64, y_frac: f64) {
        let Some(picture) = self.pictures.get(asset).and_then(|levels| levels.get(&0)) else {
            return;
        };
        let rgba = images::sample(&picture.first, x_frac, y_frac);
        let hex = images::hex(rgba);
        match copy_text(&hex) {
            Ok(()) => toast::announce(Tone::Success, format!("Copied {hex}")),
            Err(error) => toast::announce(Tone::Warning, error),
        }
    }

    /// Pushes the controller's view state to the active window: camera, selection, marquee, guides, menu, grid, crop,
    /// hierarchy, slideshow, peers and the tool.
    fn push_state(&self, camera: Camera, peers: Vec<PeerView>, boxes: Vec<PeerBoxView>) {
        let controller = self.controller.as_ref();
        let guides: Vec<GuideView> = controller
            .map(Controller::guides)
            .unwrap_or_default()
            .into_iter()
            .map(|(a, b)| GuideView {
                x1: a.x as f32,
                y1: a.y as f32,
                x2: b.x as f32,
                y2: b.y as f32,
            })
            .collect();
        let (panels, menu_rows) = controller.map(Controller::menu_view).unwrap_or_default();
        let panels: Vec<MenuPanelView> = panels
            .into_iter()
            .map(|panel| MenuPanelView {
                x: panel.x as f32,
                y: panel.y as f32,
                w: panel.w as f32,
                h: panel.h as f32,
            })
            .collect();
        let menu_rows: Vec<MenuRowView> = menu_rows
            .into_iter()
            .map(|row| MenuRowView {
                panel: row.panel as i32,
                x: row.x as f32,
                y: row.y as f32,
                w: row.w as f32,
                h: row.h as f32,
                kind: row.kind.into(),
                label: row.label.into(),
                shortcut: row.shortcut.as_str().into(),
                enabled: row.enabled,
                checked: row.checked,
                highlighted: row.highlighted,
                toggle_left: row.toggle_left.into(),
                toggle_right: row.toggle_right.into(),
                toggle_right_on: row.toggle_right_on,
            })
            .collect();
        let grid_mode = match controller.map(Controller::grid).unwrap_or_default() {
            GridMode::None => "none",
            GridMode::Lines => "lines",
            GridMode::Dots => "dots",
        };
        let (grid_x, grid_y) = controller.map(Controller::grid_lines).unwrap_or_default();
        let grid_x: Vec<f32> = grid_x.into_iter().map(|v| v as f32).collect();
        let grid_y: Vec<f32> = grid_y.into_iter().map(|v| v as f32).collect();
        let crop = controller.and_then(Controller::crop_view);
        let hierarchy_rows: Vec<HierarchyRowView> = controller
            .map(Controller::hierarchy_rows)
            .unwrap_or_default()
            .into_iter()
            .map(|row| HierarchyRowView {
                id: row.id,
                depth: row.depth,
                label: row.label,
                kind: row.kind.into(),
                selected: row.selected,
                locked: row.locked,
            })
            .collect();
        let slide = controller.and_then(Controller::slideshow);
        let slide_image = slide
            .as_ref()
            .and_then(|view| self.picture_for(&view.asset, 1024))
            .and_then(|(_, picture)| picture.color.first().cloned())
            .unwrap_or_default();
        let slide_caption = slide
            .as_ref()
            .map_or_else(String::new, |view| view.caption.clone());
        let slide_playing = slide.as_ref().is_some_and(|view| view.playing);
        let tool = controller.map_or("select", |c| tool_name(c.tool()));
        let selection = controller.and_then(Controller::selection_box);
        let marquee = controller.and_then(Controller::marquee);
        let can_undo = controller.is_some_and(Controller::can_undo);
        let can_redo = controller.is_some_and(Controller::can_redo);
        let menu_open = controller.is_some_and(Controller::menu_open);
        let hierarchy_open = controller.is_some_and(Controller::hierarchy_open);
        let zoom_label = controller.map(Controller::zoom_label).unwrap_or_default();
        let read_only = self.is_read_only();
        let edit_field = self.edit_field;
        let eraser = controller.and_then(Controller::eraser_view);

        self.with_state(|state| {
            state.set_zoom(camera.zoom as f32);
            state.set_pan_x(camera.pan.x as f32);
            state.set_pan_y(camera.pan.y as f32);
            state.set_zoom_label(zoom_label.into());
            state.set_tool(tool.into());
            state.set_read_only(read_only);
            state.set_can_undo(can_undo);
            state.set_can_redo(can_redo);
            match selection {
                Some((bounds, locked)) => {
                    state.set_has_selection(true);
                    set_bounds(state, &bounds);
                    state.set_sel_locked(locked);
                }
                None => {
                    state.set_has_selection(false);
                    state.set_sel_locked(false);
                }
            }
            match marquee {
                Some(rect) => {
                    state.set_marquee_active(true);
                    state.set_marquee_x(rect.x as f32);
                    state.set_marquee_y(rect.y as f32);
                    state.set_marquee_w(rect.w as f32);
                    state.set_marquee_h(rect.h as f32);
                }
                None => state.set_marquee_active(false),
            }
            state.sync_guides(guides);
            state.sync_menu_panels(panels);
            state.sync_menu_rows(menu_rows);
            state.set_menu_open(menu_open);
            state.set_grid_mode(grid_mode.into());
            state.sync_grid_xs(grid_x);
            state.sync_grid_ys(grid_y);
            match &crop {
                Some((full, window)) => {
                    state.set_crop_active(true);
                    state.set_crop_full_x(full.rect.x as f32);
                    state.set_crop_full_y(full.rect.y as f32);
                    state.set_crop_full_w(full.rect.w as f32);
                    state.set_crop_full_h(full.rect.h as f32);
                    state.set_crop_rotation(full.rotation as f32);
                    state.set_crop_x(window.x as f32);
                    state.set_crop_y(window.y as f32);
                    state.set_crop_w(window.w as f32);
                    state.set_crop_h(window.h as f32);
                }
                None => state.set_crop_active(false),
            }
            state.set_hierarchy_open(hierarchy_open);
            state.sync_hierarchy_rows(hierarchy_rows);
            state.set_slideshow_open(slide.is_some());
            state.set_slideshow_image(slide_image);
            state.set_slideshow_caption(slide_caption.into());
            state.set_slideshow_playing(slide_playing);
            state.set_editing_field(edit_field.into());
            match eraser {
                Some((x, y, r)) => {
                    state.set_eraser_shown(true);
                    state.set_eraser_x(x as f32);
                    state.set_eraser_y(y as f32);
                    state.set_eraser_r(r as f32);
                }
                None => state.set_eraser_shown(false),
            }
            state.sync_peers(peers);
            state.sync_peer_boxes(boxes);
            state.sync_video_controls(self.video_controls());
        });
    }
}

/// Copies the page's state that is set only when it changes (load state, error, title, read-only, connection, busy text)
/// from one window's `CanvasState` to another's. Popping out and bringing back move the canvas between windows; without
/// this the new window would keep its defaults (the pop-out stayed on "Loading canvas").
fn copy_page_state(from: &dyn CanvasPageState, to: &dyn CanvasPageState) {
    to.apply_page_copy(from.page_copy());
}

/// Sets the selection's handles from its box (world units) and rotation.
fn set_bounds(state: &dyn CanvasPageState, bounds: &RotRect) {
    let Rect { x, y, w, h } = bounds.rect;
    state.set_sel_x(x as f32);
    state.set_sel_y(y as f32);
    state.set_sel_w(w as f32);
    state.set_sel_h(h as f32);
    state.set_sel_rotation(bounds.rotation as f32);
}

/// The item of the room a backup stores (its props as the room has them).
fn item_state(item: &Item) -> ItemState {
    ItemState {
        id: item.id.clone(),
        kind: item.kind().as_str().to_string(),
        z: item.z.clone(),
        version: item.version,
        props: item.to_props(),
        deleted: false,
        created_by: String::new(),
        updated_by: String::new(),
        updated_at: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::duration_text;

    #[test]
    fn durations_read_as_minutes_and_hours() {
        assert_eq!(duration_text(0), "0:00");
        assert_eq!(duration_text(65_000), "1:05");
        assert_eq!(duration_text(3_725_000), "1:02:05");
    }
}
