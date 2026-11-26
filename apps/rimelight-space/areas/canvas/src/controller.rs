//! Interaction logic of the canvas page (CV.6): tools, selection, dragging (move, resize, rotate, marquee, pan), keys, the
//! context menu, text editing and undo, over `space_canvas`. No Slint types: the page (page.rs) turns the returned
//! Effects into property writes and live-room ops. Every edit is applied to the local Doc at once (optimistic) and sent
//! as an op; the room's ack or a remote change comes back through `apply`.

use space_canvas::arrange::{self, Align, Axis, Entry, Normalize, SortKey};
use space_canvas::geom::{Camera, Rect, RotRect, Vec2};
use space_canvas::grid::{self, GridMode};
use space_canvas::hit::{self, Handle, HitItem, Marquee};
use space_canvas::model::{Doc, Item, ItemData, ItemKind, ShapeKind, TextAlign};
use space_canvas::ops::{create_op, delete_op, update_op};
use space_canvas::snap::{self, Guide};
use space_canvas::undo::{Change, Edit, History};
use space_canvas::{bind, crop, draw, zorder};
use space_proto::canvases::{AssetView, ClientOp, Hello, ItemState, Point, Presence, Viewport};
use std::collections::{HashMap, HashSet};

const HANDLE_RADIUS_PX: f64 = 8.0;
const ROTATE_OFFSET_PX: f64 = 24.0;
const HIT_TOLERANCE_PX: f64 = 3.0;
const SNAP_PX: f64 = 6.0;
const DRAG_START_PX: f64 = 3.0;
const MAX_IMAGE_SIDE: f64 = 600.0;
/// The space between the images of one add, in world units.
const IMAGE_GAP: f64 = 16.0;
const NUDGE: f64 = 1.0;
const NUDGE_BIG: f64 = 10.0;
const ZOOM_STEP: f64 = 1.25;
const WHEEL_STEP: f64 = 1.1;
const FIT_MARGIN_PX: f64 = 48.0;
/// Smallest width or height a resize may reach, in world units.
const MIN_SIZE: f64 = 4.0;
/// Most unselected items considered as snap targets during a move.
const SNAP_OTHERS: usize = 300;
const TEXT_WIDTH: f64 = 240.0;
const TEXT_HEIGHT: f64 = 40.0;
const TEXT_SIZE: f64 = 24.0;
const TEXT_MIN_HEIGHT: f64 = 40.0;

/// The tool in use.
/// A z-order function from `space_canvas::zorder`: every item as (id, z) and the selected ids, to the new (id, z).
type Reorder = fn(&[(String, String)], &[String]) -> Vec<(String, String)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Text,
    Hand,
    Rect,
    Ellipse,
    Diamond,
    Line,
    Arrow,
    Pen,
    Eraser,
    Frame,
    Eyedropper,
}

/// The name the page uses for a tool.
pub fn tool_name(t: Tool) -> &'static str {
    match t {
        Tool::Select => "select",
        Tool::Text => "text",
        Tool::Hand => "hand",
        Tool::Rect => "rect",
        Tool::Ellipse => "ellipse",
        Tool::Diamond => "diamond",
        Tool::Line => "line",
        Tool::Arrow => "arrow",
        Tool::Pen => "pen",
        Tool::Eraser => "eraser",
        Tool::Frame => "frame",
        Tool::Eyedropper => "eyedropper",
    }
}

/// The tool with the page's name, if any.
pub fn tool_from_name(s: &str) -> Option<Tool> {
    Some(match s {
        "select" => Tool::Select,
        "text" => Tool::Text,
        "hand" => Tool::Hand,
        "rect" => Tool::Rect,
        "ellipse" => Tool::Ellipse,
        "diamond" => Tool::Diamond,
        "line" => Tool::Line,
        "arrow" => Tool::Arrow,
        "pen" => Tool::Pen,
        "eraser" => Tool::Eraser,
        "frame" => Tool::Frame,
        "eyedropper" => Tool::Eyedropper,
        _ => return None,
    })
}

/// Which text the editor is writing: an item's text, an image's comment, or a frame's title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Text,
    Comment,
    Title,
}

/// A Mode or Window command, run on the pop-out window by popout.rs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowCommand {
    Transparent,
    OverlaySelection,
    OnTop,
    OnTopOf,
    OnBottom,
    Maximize,
    Minimize,
    /// Screen px at the current zoom.
    ResizeToSelection {
        w: f64,
        h: f64,
    },
    Lock,
    NewWindow,
    OpacityUp,
    OpacityDown,
}

/// An export or backup command, run by the page (actions.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportCommand {
    CanvasImage,
    SelectionImage,
    AllToFolder,
    SelectedToFolder,
    Backup,
    OpenBackup,
}

/// One slide of the slideshow, as the page draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideshowView {
    pub asset: String,
    pub caption: String,
    pub index: usize,
    pub count: usize,
    pub playing: bool,
}

/// One row of the hierarchy panel.
#[derive(Debug, Clone, PartialEq)]
pub struct HierarchyRow {
    pub id: String,
    pub depth: i32,
    pub label: String,
    pub kind: &'static str,
    pub selected: bool,
    pub locked: bool,
}

/// The kind of pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Down,
    Move,
    Up,
}

/// The pointer button of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    None,
}

/// Modifier keys held during an event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// What the page must do after an input.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send to the live room.
    Send(ClientOp),
    /// Items, selection, marquee or guides changed: rebuild the view.
    Redraw,
    /// Zoom or pan changed.
    Camera,
    /// Open the text editor on this item.
    EditText {
        id: String,
        text: String,
        field: EditField,
    },
    /// Close the editor.
    EndEdit,
    /// Set the pointer cursor.
    Cursor(&'static str),
    /// Create a new canvas (Ctrl+Shift+K).
    NewCanvas,
    /// Close this canvas (Ctrl+Q).
    CloseCanvas,
    /// Copy these image assets (asset ids, selection order) to the clipboard.
    CopyImages(Vec<String>),
    /// Copy these assets as files (asset ids).
    CopyFiles(Vec<String>),
    /// Run the page's paste (Ctrl+V, menu Paste).
    Paste,
    /// Open the page's file picker to add images (menu Add images).
    AddImages,
    /// Open this URL in the browser (the image's source).
    OpenSource(String),
    /// A Mode or Window command for the pop-out.
    Window(WindowCommand),
    /// An export or backup command.
    Export(ExportCommand),
    /// Open the manage-images dialog.
    ManageImages,
    /// Pick a file to replace this item's image.
    ReplaceImage(String),
    /// Pick files for the missing images.
    RelinkMissing,
    /// Sample this pixel of the asset (fractions 0..1 of the original picture).
    Eyedropper {
        asset: String,
        x_frac: f64,
        y_frac: f64,
    },
    /// Play or pause this video on this client (double-click, Space, menu Play or pause). Not an edit.
    VideoToggle(String),
    /// The selection or camera changed: the page sends presence (throttled).
    Presence,
}

/// A keyboard shortcut of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    /// A lower-case letter, or "up", "down", "left", "right", "del", "plus", "minus", "enter", "esc".
    pub key: &'static str,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// One command: its label, shortcut and availability. The key table and the menu both run it by `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: Option<Keys>,
    pub needs_selection: bool,
    pub popout_only: bool,
    pub built: bool,
}

/// A menu panel as the page draws it: screen px within the canvas area.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuPanelView {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A menu row as the page draws it: absolute screen px within the canvas area.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRowView {
    pub panel: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub kind: &'static str,
    pub label: &'static str,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: i32,
    pub highlighted: bool,
    pub toggle_left: &'static str,
    pub toggle_right: &'static str,
    pub toggle_right_on: bool,
}

/// One row of the view, in draw order (bottom first).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub id: String,
    pub kind: ItemKind,
    pub rect: Rect,
    pub rotation: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub opacity: f64,
    pub asset: String,
    /// Nearest-neighbour sampling (images only).
    pub pixelated: bool,
    pub text: String,
    pub color: String,
    pub fill: String,
    pub font_size: f64,
    pub align: &'static str,
    pub locked: bool,
    pub selected: bool,
    pub editing: bool,
    /// The crop window as fractions of the full image (images only). None when not cropped.
    pub crop: Option<Rect>,
    /// Drawn in greyscale: the item's own setting or the canvas's.
    pub gray: bool,
    /// Path commands in item-local units (shapes and pen strokes).
    pub path: String,
    /// The shape's name ("rect", "ellipse", ...) for shapes, empty otherwise.
    pub shape: &'static str,
    pub stroke_width: f64,
    pub stroke_color: String,
    pub fill_color: String,
    /// The image's caption (images only).
    pub comment: String,
    /// True when the eraser at the pointer would delete this item (Eraser tool only). The page draws it faded.
    pub erase_hover: bool,
    /// Video only: the sound is off for everyone.
    pub muted: bool,
    /// Video only: plays again from the start when it ends.
    pub looping: bool,
}

/// An in-progress pointer gesture.
#[derive(Debug, Clone, PartialEq)]
enum Drag {
    None,
    Pending {
        start_screen: Vec2,
        start_world: Vec2,
        on_item: Option<String>,
        was_selected: bool,
    },
    Move {
        start_world: Vec2,
        originals: Vec<Item>,
    },
    Resize {
        handle: Handle,
        start_world: Vec2,
        originals: Vec<Item>,
        start_box: RotRect,
    },
    Rotate {
        start_world: Vec2,
        center: Vec2,
        originals: Vec<Item>,
        start_rot: f64,
    },
    Marquee {
        start_screen: Vec2,
        base: Vec<String>,
    },
    Pan {
        last_screen: Vec2,
    },
    /// A shape, line, arrow or frame being drawn: the provisional item and where the drag started.
    Create {
        id: String,
        tool: Tool,
        start: Vec2,
    },
    /// A pen stroke being drawn: the provisional item and the world points so far.
    Pen {
        id: String,
        points: Vec<Vec2>,
    },
    /// The eraser: the items removed so far (one history batch on release).
    Erase {
        changes: Vec<Change>,
    },
    /// A crop handle or the crop window being dragged.
    Crop {
        handle: Option<Handle>,
        start_world: Vec2,
        start_crop: Rect,
    },
}

/// The image being cropped: its full picture rect (world), the crop window (fractions) and the item as it was.
#[derive(Debug, Clone, PartialEq)]
struct CropEdit {
    id: String,
    full: Rect,
    crop: Rect,
    original: Item,
}

/// The slideshow: the slides (asset, caption), the shown index and whether it advances by itself.
#[derive(Debug, Clone, PartialEq)]
struct Slideshow {
    slides: Vec<(String, String)>,
    index: usize,
    playing: bool,
}

/// The open canvas menu: the anchor in screen px, the index path of open submenus, and the highlighted (level, row).
#[derive(Debug, Clone, PartialEq)]
struct MenuState {
    anchor: Vec2,
    open: Vec<usize>,
    highlight: Option<(usize, usize)>,
}

/// The interaction state of one open canvas.
pub struct Controller {
    user_id: String,
    doc: Doc,
    camera: Camera,
    viewport: Vec2,
    fitted_once: bool,
    selection: Vec<String>,
    tool: Tool,
    drag: Drag,
    history: History,
    next_op: u64,
    new_id: Box<dyn FnMut() -> String>,
    read_only: bool,
    /// Lock canvas: nothing is selected, moved, created or edited by pointer or keys (zoom, pan, undo stay).
    canvas_locked: bool,
    /// True in a pop-out window: the Mode and Window items are enabled.
    popout: bool,
    /// The natural pixel size of each image asset, once decoded.
    asset_sizes: HashMap<String, (f64, f64)>,
    /// The open menu, if any.
    menu: Option<MenuState>,
    /// Normalize mode of the menu toggle: true for Average (the default), false for From first.
    normalize_average: bool,
    /// The item being edited and whether it is new (not yet in the room).
    editing: Option<(String, bool)>,
    editing_text: String,
    /// The world point of the last right-click (for Note from the menu).
    menu_world: Vec2,
    guides: Vec<(Vec2, Vec2)>,
    marquee: Option<Rect>,
    cursor: &'static str,
    /// The pointer's last position in screen px, from any pointer event.
    pointer_screen: Option<Vec2>,
    /// Grid view setting (per session, not stored on the canvas).
    grid: GridMode,
    /// Snap moves and resizes to the grid.
    snap_grid: bool,
    /// Canvas view setting: every image is drawn in greyscale.
    canvas_gray: bool,
    hierarchy_open: bool,
    slideshow: Option<Slideshow>,
    crop_edit: Option<CropEdit>,
    /// What the open text editor writes (see `editing`).
    editing_field: EditField,
    /// Ops we sent and have not had acknowledged, per item id (F4).
    pending: HashMap<String, Vec<String>>,
    /// Source url and file name of each asset, when known (Open source, By path).
    asset_sources: HashMap<String, (String, String)>,
    /// Assets whose picture failed to load (Relink missing images).
    missing_assets: Vec<String>,
    /// Set by any user pan or zoom; the page reads it with take_follow_cancelled.
    follow_cancelled: bool,
    /// Connectors bound to the items of the drag in progress, as they were before it. They re-route while it runs.
    drag_followers: Vec<Item>,
}

impl Controller {
    /// `user_id` is the signed-in person; `new_id` makes a fresh UUID for new items.
    pub fn new(user_id: String, new_id: Box<dyn FnMut() -> String>) -> Controller {
        Controller {
            user_id,
            doc: Doc::default(),
            camera: Camera::default(),
            viewport: Vec2::ZERO,
            fitted_once: false,
            selection: Vec::new(),
            tool: Tool::Select,
            drag: Drag::None,
            history: History::default(),
            next_op: 0,
            new_id,
            read_only: false,
            canvas_locked: false,
            popout: false,
            asset_sizes: HashMap::new(),
            menu: None,
            normalize_average: true,
            editing: None,
            editing_text: String::new(),
            menu_world: Vec2::ZERO,
            guides: Vec::new(),
            marquee: None,
            cursor: "default",
            pointer_screen: None,
            grid: GridMode::None,
            snap_grid: false,
            canvas_gray: false,
            hierarchy_open: false,
            slideshow: None,
            crop_edit: None,
            editing_field: EditField::Text,
            pending: HashMap::new(),
            asset_sources: HashMap::new(),
            missing_assets: Vec::new(),
            follow_cancelled: false,
            drag_followers: Vec::new(),
        }
    }

    /// The signed-in person this controller was made for.
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
        if read_only {
            self.drag = Drag::None;
        }
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Replaces the whole document with the room's hello. Returns [Redraw, Camera] and fits the camera to the content
    /// the first time (an empty canvas keeps zoom 1 with the origin at the centre of the viewport).
    pub fn load(&mut self, hello: &Hello) -> Vec<Effect> {
        self.doc = Doc::from_states(&hello.items);
        self.selection.clear();
        // A new room: ops sent to the old one will never be acknowledged here.
        self.pending.clear();
        let mut out = Vec::new();
        let editing_gone = self
            .editing
            .as_ref()
            .is_some_and(|(id, _)| self.doc.get(id).is_none());
        if editing_gone {
            self.editing = None;
            self.editing_text.clear();
            out.push(Effect::EndEdit);
        }
        if !self.fitted_once && self.viewport.x > 0.0 && self.viewport.y > 0.0 {
            match self.all_bounds() {
                Some(bounds) => self.fit_to(bounds),
                None => {
                    self.camera = Camera {
                        pan: self.viewport / 2.0,
                        zoom: 1.0,
                    }
                }
            }
            self.fitted_once = true;
        }
        out.push(Effect::Redraw);
        out.push(Effect::Camera);
        out
    }

    /// A remote change (applied). Ignored while local ops on this item are pending: ours will win.
    pub fn apply(&mut self, state: &ItemState) -> Vec<Effect> {
        if self.has_pending(&state.id) {
            return Vec::new();
        }
        self.apply_remote(state)
    }

    /// The room acknowledged our op `op_id` with this item (F4). The local item is replaced only when no later local op
    /// on it is pending; otherwise only its version is taken, so the next op uses the right base.
    pub fn ack(&mut self, op_id: &str, state: &ItemState) -> Vec<Effect> {
        if let Some(ops) = self.pending.get_mut(&state.id) {
            ops.retain(|o| o.as_str() != op_id);
            if ops.is_empty() {
                self.pending.remove(&state.id);
            }
        }
        if !self.has_pending(&state.id) {
            return self.apply_remote(state);
        }
        if let Some(mut item) = self.doc.get(&state.id).cloned()
            && item.version != state.version
        {
            item.version = state.version;
            self.doc.put(item);
        }
        Vec::new()
    }

    fn has_pending(&self, item_id: &str) -> bool {
        self.pending.get(item_id).is_some_and(|ops| !ops.is_empty())
    }

    fn apply_remote(&mut self, state: &ItemState) -> Vec<Effect> {
        if self.doc.apply_state(state) {
            self.prune_selection();
            vec![Effect::Redraw]
        } else {
            Vec::new()
        }
    }

    fn track(&mut self, op: &ClientOp) {
        self.pending
            .entry(op.item_id.clone())
            .or_default()
            .push(op.op_id.clone());
    }

    pub fn set_viewport(&mut self, w: f64, h: f64) {
        self.viewport = Vec2::new(w, h);
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// The zoom as a percentage label, rounded.
    pub fn zoom_label(&self) -> String {
        format!("{:.0}%", self.camera.zoom * 100.0)
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: Tool) -> Vec<Effect> {
        self.tool = tool;
        let mut out = Vec::new();
        let cursor = match tool {
            Tool::Select => "default",
            Tool::Text => "text",
            Tool::Hand => "grab",
            // The page draws the eraser circle instead of a cursor.
            Tool::Eraser => "none",
            _ => "crosshair",
        };
        self.set_cursor(cursor, &mut out);
        // The toolbar shows the tool from the view, so the page must redraw.
        out.push(Effect::Redraw);
        out
    }

    /// The view's selection and camera, before and after an input (for Presence and follow).
    fn view_state(&self) -> (Vec<String>, Camera) {
        (self.selection.clone(), self.camera)
    }

    /// Adds Presence when the selection or camera changed; a camera change by the user cancels following.
    fn finish(&mut self, before: (Vec<String>, Camera), mut out: Vec<Effect>) -> Vec<Effect> {
        let (selection, camera) = self.view_state();
        if camera != before.1 {
            self.follow_cancelled = true;
        }
        if selection != before.0 || camera != before.1 {
            out.push(Effect::Presence);
        }
        out
    }

    pub fn pointer(
        &mut self,
        kind: PointerKind,
        at: Vec2,
        button: Button,
        mods: Mods,
    ) -> Vec<Effect> {
        let before = self.view_state();
        self.pointer_screen = Some(at);
        let out = match kind {
            PointerKind::Down => self.pointer_down(at, button, mods),
            PointerKind::Move => self.pointer_move(at, mods),
            PointerKind::Up => self.pointer_up(at, mods),
        };
        self.finish(before, out)
    }

    /// The pointer left the canvas: presence then sends no cursor.
    pub fn pointer_left(&mut self) {
        self.pointer_screen = None;
    }

    pub fn double_click(&mut self, at: Vec2) -> Vec<Effect> {
        let before = self.view_state();
        let mut out = Vec::new();
        self.double_click_inner(at, &mut out);
        self.finish(before, out)
    }

    fn double_click_inner(&mut self, at: Vec2, out: &mut Vec<Effect>) {
        if self.slideshow.is_some() || self.crop_edit.is_some() {
            return;
        }
        let p = self.world(at);
        let items = self.hit_items();
        let hit = hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), false);
        // Playing is not an edit, so a double-click on a video toggles it even on a locked or read-only canvas.
        if let Some(id) = &hit
            && self
                .doc
                .get(id)
                .is_some_and(|i| i.kind() == ItemKind::Video)
        {
            out.push(Effect::VideoToggle(id.clone()));
            return;
        }
        if self.locked() {
            return;
        }
        if let Some((id, title)) = self.frame_title_at(p) {
            self.begin_edit_field(id, false, title, EditField::Title, out);
            return;
        }
        match hit {
            Some(id) => {
                let text = match self.doc.get(&id) {
                    Some(Item {
                        data: ItemData::Text { text, .. },
                        ..
                    }) => Some(text.clone()),
                    _ => None,
                };
                if let Some(text) = text {
                    self.begin_edit(id, false, text, out);
                }
            }
            None => {
                if self.tool == Tool::Select {
                    self.start_new_text(p, out);
                }
            }
        }
    }

    pub fn scroll(&mut self, at: Vec2, delta: Vec2, mods: Mods) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.scroll_inner(at, delta, mods);
        self.finish(before, out)
    }

    fn scroll_inner(&mut self, at: Vec2, delta: Vec2, mods: Mods) -> Vec<Effect> {
        if mods.shift {
            self.camera = self.camera.pan_by(Vec2::new(-delta.y, 0.0));
        } else {
            if delta.y != 0.0 {
                // Wheel up (positive delta-y from Slint) zooms in, as in PureRef and image viewers.
                let factor = if delta.y > 0.0 {
                    WHEEL_STEP
                } else {
                    1.0 / WHEEL_STEP
                };
                self.camera = self.camera.zoom_at(at, factor);
            }
            if delta.x != 0.0 {
                self.camera = self.camera.pan_by(Vec2::new(-delta.x, 0.0));
            }
        }
        vec![Effect::Camera]
    }

    /// Returns (used, effects). The slideshow, the crop mode and the open menu take their keys first.
    pub fn key(&mut self, text: &str, mods: Mods) -> (bool, Vec<Effect>) {
        let before = self.view_state();
        let (used, out) = self.key_inner(text, mods);
        (used, self.finish(before, out))
    }

    fn key_inner(&mut self, text: &str, mods: Mods) -> (bool, Vec<Effect>) {
        let name = key_name(text);
        if let Some(out) = self.overlay_key(&name) {
            return (true, out);
        }
        if self.menu.is_some()
            && let Some(out) = self.menu_key(&name)
        {
            return (true, out);
        }
        // Plus and minus ignore Shift (it is part of typing them on most layouts).
        let plain_sign = name == "plus" || name == "minus";
        let shift = mods.shift && !plain_sign;
        let found = COMMANDS.iter().find(|c| {
            c.keys.is_some_and(|k| {
                k.key == name && k.ctrl == mods.ctrl && k.shift == shift && k.alt == mods.alt
            })
        });
        match found {
            Some(cmd) if self.enabled(cmd) => (true, self.run_command_at(cmd.id, None)),
            _ => (false, Vec::new()),
        }
    }

    pub fn zoom_in(&mut self) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.zoom_about_centre(ZOOM_STEP);
        self.finish(before, out)
    }

    pub fn zoom_out(&mut self) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.zoom_about_centre(1.0 / ZOOM_STEP);
        self.finish(before, out)
    }

    /// Zoom 1 keeping the world point at the viewport centre fixed.
    pub fn zoom_reset(&mut self) -> Vec<Effect> {
        let before = self.view_state();
        let centre = self.viewport / 2.0;
        let anchor = self.camera.screen_to_world(centre);
        self.camera = Camera {
            pan: centre - anchor,
            zoom: 1.0,
        };
        self.finish(before, vec![Effect::Camera])
    }

    pub fn zoom_fit(&mut self) -> Vec<Effect> {
        let before = self.view_state();
        let out = match self.all_bounds() {
            Some(bounds) => {
                self.fit_to(bounds);
                vec![Effect::Camera]
            }
            None => Vec::new(),
        };
        self.finish(before, out)
    }

    pub fn undo(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.read_only {
            return out;
        }
        let edits = self.history.undo(&self.doc, &mut *self.new_id);
        self.apply_edits(edits, &mut out);
        self.prune_selection();
        out.push(Effect::Redraw);
        out
    }

    pub fn redo(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.read_only {
            return out;
        }
        let edits = self.history.redo(&self.doc, &mut *self.new_id);
        self.apply_edits(edits, &mut out);
        self.prune_selection();
        out.push(Effect::Redraw);
        out
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Runs a command by id (the key table and the menu both come here). Disabled commands do nothing.
    pub fn run_command(&mut self, id: &str) -> Vec<Effect> {
        let before = self.view_state();
        let out = match command(id) {
            Some(cmd) if self.enabled(cmd) => self.run_command_at(cmd.id, None),
            _ => Vec::new(),
        };
        self.finish(before, out)
    }

    /// Whether a command can run now: built, its selection (if any) present, its window (if any) right, and not an
    /// edit while the canvas is read-only or locked. Some commands also need one image, a source or missing pictures.
    fn enabled(&self, cmd: &Command) -> bool {
        if !cmd.built {
            return false;
        }
        if cmd.needs_selection && self.selection.is_empty() {
            return false;
        }
        if cmd.popout_only && !self.popout {
            return false;
        }
        if (self.read_only || self.canvas_locked) && edits(cmd.id) {
            return false;
        }
        match cmd.id {
            "crop" | "comment" | "replace_image" => self.one_image().is_some(),
            "open_source" => self.source_url().is_some(),
            "relink" => !self.missing_assets.is_empty(),
            "copy_as_file" | "export_selected" => !self.selected_media().is_empty(),
            // Space plays only while no text is being edited.
            "video_play" => self.editing.is_none() && self.selected_video().is_some(),
            "video_mute" | "video_loop" => !self.selected_videos().is_empty(),
            _ => true,
        }
    }

    /// The one selected image: its item id and asset.
    fn one_image(&self) -> Option<(String, String)> {
        let items = self.selected_items();
        match items.as_slice() {
            [item] => match &item.data {
                ItemData::Image { asset, .. } => Some((item.id.clone(), asset.clone())),
                _ => None,
            },
            _ => None,
        }
    }

    /// The id of the one selected item when it is a video.
    fn selected_video(&self) -> Option<String> {
        match self.selected_items().as_slice() {
            [item] if item.kind() == ItemKind::Video => Some(item.id.clone()),
            _ => None,
        }
    }

    /// The selected videos, in draw order.
    fn selected_videos(&self) -> Vec<Item> {
        self.selected_items()
            .into_iter()
            .filter(|i| i.kind() == ItemKind::Video)
            .collect()
    }

    /// The selected images and videos (the items that have a file), in draw order.
    fn selected_media(&self) -> Vec<Item> {
        self.selected_items()
            .into_iter()
            .filter(|i| matches!(i.kind(), ItemKind::Image | ItemKind::Video))
            .collect()
    }

    /// The source url of the one selected image, when it has one.
    fn source_url(&self) -> Option<String> {
        let (_, asset) = self.one_image()?;
        self.asset_sources
            .get(&asset)
            .map(|(url, _)| url.clone())
            .filter(|url| !url.is_empty())
    }

    /// The assets of the selected images, in draw order.
    fn selected_assets(&self) -> Vec<String> {
        self.selected_media()
            .iter()
            .filter_map(|i| match &i.data {
                ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                    Some(asset.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The command with `id`, run without a menu point (Note goes to the view centre).
    fn run_command_at(&mut self, id: &str, at: Option<Vec2>) -> Vec<Effect> {
        if let Some(name) = id.strip_prefix("stroke_") {
            return self.edit_selected(true, |i| set_stroke(i, name));
        }
        if let Some(name) = id.strip_prefix("fill_") {
            let fill = if name == "none" {
                None
            } else {
                Some(name.to_string())
            };
            return self.edit_selected(true, |i| set_fill(i, fill.clone()));
        }
        match id {
            "new_canvas" => vec![Effect::NewCanvas],
            "close" => vec![Effect::CloseCanvas],
            "paste" => vec![Effect::Paste],
            "add_images" => vec![Effect::AddImages],
            "hierarchy" => {
                self.hierarchy_open = !self.hierarchy_open;
                vec![Effect::Redraw]
            }
            "draw" => {
                let next = if self.tool == Tool::Pen {
                    Tool::Select
                } else {
                    Tool::Pen
                };
                self.set_tool(next)
            }
            "tool_select" => self.set_tool(Tool::Select),
            "tool_text" => self.set_tool(Tool::Text),
            "tool_hand" => self.set_tool(Tool::Hand),
            "tool_rect" => self.set_tool(Tool::Rect),
            "tool_ellipse" => self.set_tool(Tool::Ellipse),
            "tool_diamond" => self.set_tool(Tool::Diamond),
            "tool_line" => self.set_tool(Tool::Line),
            "tool_arrow" => self.set_tool(Tool::Arrow),
            "tool_pen" => self.set_tool(Tool::Pen),
            "tool_eraser" => self.set_tool(Tool::Eraser),
            "tool_frame" => self.set_tool(Tool::Frame),
            "eyedropper" => self.set_tool(Tool::Eyedropper),
            "grid_none" => self.set_grid(GridMode::None),
            "grid_lines" => self.set_grid(GridMode::Lines),
            "grid_dots" => self.set_grid(GridMode::Dots),
            "grid_snap" => {
                self.snap_grid = !self.snap_grid;
                vec![Effect::Redraw]
            }
            "canvas_grayscale" => {
                self.canvas_gray = !self.canvas_gray;
                vec![Effect::Redraw]
            }
            "toggle_grayscale" => {
                let all = self
                    .selected_images()
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Image { gray: true, .. }));
                self.edit_selected(true, move |i| {
                    if let ItemData::Image { gray, .. } = &mut i.data {
                        *gray = !all;
                    }
                })
            }
            "opacity_100" => self.set_opacity(1.0),
            "opacity_75" => self.set_opacity(0.75),
            "opacity_50" => self.set_opacity(0.5),
            "opacity_25" => self.set_opacity(0.25),
            "width_2" => self.set_width(2.0),
            "width_4" => self.set_width(4.0),
            "width_8" => self.set_width(8.0),
            "video_play" => match self.selected_video() {
                Some(id) => vec![Effect::VideoToggle(id)],
                None => Vec::new(),
            },
            "video_mute" => {
                let all = self
                    .selected_videos()
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Video { muted: true, .. }));
                self.edit_selected(true, move |i| {
                    if let ItemData::Video { muted, .. } = &mut i.data {
                        *muted = !all;
                    }
                })
            }
            "video_loop" => {
                let all = self
                    .selected_videos()
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Video { looping: true, .. }));
                self.edit_selected(true, move |i| {
                    if let ItemData::Video { looping, .. } = &mut i.data {
                        *looping = !all;
                    }
                })
            }
            "crop" => self.start_crop(),
            "reset_crop" => self.reset_crop(),
            "comment" => self.edit_comment(),
            "slideshow" => self.slideshow_start(),
            "arrange_path" => self.arrange_path(),
            "copy_as_file" => {
                let assets = self.selected_assets();
                if assets.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::CopyFiles(assets)]
                }
            }
            "open_source" => match self.source_url() {
                Some(url) => vec![Effect::OpenSource(url)],
                None => Vec::new(),
            },
            "manage_images" => vec![Effect::ManageImages],
            "replace_image" => match self.one_image() {
                Some((id, _)) => vec![Effect::ReplaceImage(id)],
                None => Vec::new(),
            },
            "relink" => vec![Effect::RelinkMissing],
            "export_canvas" => vec![Effect::Export(ExportCommand::CanvasImage)],
            "export_selection" => vec![Effect::Export(ExportCommand::SelectionImage)],
            "export_all" => vec![Effect::Export(ExportCommand::AllToFolder)],
            "export_selected" => vec![Effect::Export(ExportCommand::SelectedToFolder)],
            "export_backup" => vec![Effect::Export(ExportCommand::Backup)],
            "export_open_backup" => vec![Effect::Export(ExportCommand::OpenBackup)],
            "mode_transparent" => vec![Effect::Window(WindowCommand::Transparent)],
            "mode_overlay" => vec![Effect::Window(WindowCommand::OverlaySelection)],
            "mode_on_top" => vec![Effect::Window(WindowCommand::OnTop)],
            "mode_on_top_of" => vec![Effect::Window(WindowCommand::OnTopOf)],
            "mode_on_bottom" => vec![Effect::Window(WindowCommand::OnBottom)],
            "win_maximize" => vec![Effect::Window(WindowCommand::Maximize)],
            "win_minimize" => vec![Effect::Window(WindowCommand::Minimize)],
            "win_lock" => vec![Effect::Window(WindowCommand::Lock)],
            "new_window" => vec![Effect::Window(WindowCommand::NewWindow)],
            "opacity_up" => vec![Effect::Window(WindowCommand::OpacityUp)],
            "opacity_down" => vec![Effect::Window(WindowCommand::OpacityDown)],
            "win_resize_to_selection" => {
                match self.selection_bounds().or_else(|| self.all_bounds()) {
                    Some(b) => vec![Effect::Window(WindowCommand::ResizeToSelection {
                        w: b.w * self.camera.zoom,
                        h: b.h * self.camera.zoom,
                    })],
                    None => Vec::new(),
                }
            }
            "copy" => {
                let assets: Vec<String> = self
                    .selection
                    .iter()
                    .filter_map(|id| match self.doc.get(id).map(|i| &i.data) {
                        Some(ItemData::Image { asset, .. }) => Some(asset.clone()),
                        _ => None,
                    })
                    .collect();
                if assets.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::CopyImages(assets)]
                }
            }
            "undo" => self.undo(),
            "redo" => self.redo(),
            "select_all" => self.select_all(),
            "delete" => self.delete_selected(),
            "duplicate" => self.duplicate_selected(),
            "escape" => self.escape(),
            "edit_text" => self.edit_selected_text(),
            "note" => {
                let mut out = Vec::new();
                let p = at.unwrap_or_else(|| self.view_center());
                self.start_note(p, &mut out);
                out
            }
            "zoom_fit" => self.zoom_fit(),
            "zoom_selection" => self.zoom_selection(),
            "zoom_in" => self.zoom_in(),
            "zoom_out" => self.zoom_out(),
            "reset_camera" => {
                self.camera = Camera {
                    pan: self.viewport / 2.0,
                    zoom: 1.0,
                };
                vec![Effect::Camera]
            }
            "reset_camera_zoom" => self.zoom_reset(),
            "lock_canvas" => {
                self.canvas_locked = !self.canvas_locked;
                if self.canvas_locked {
                    self.drag = Drag::None;
                }
                vec![Effect::Redraw]
            }
            "flip_h" => self.edit_selected(true, |i| i.flip_x = !i.flip_x),
            "flip_v" => self.edit_selected(true, |i| i.flip_y = !i.flip_y),
            "send_front" => self.reorder(zorder::to_front),
            "send_back" => self.reorder(zorder::to_back),
            "forward" => self.reorder(zorder::forward),
            "backward" => self.reorder(zorder::backward),
            "toggle_locked" => self.toggle_lock(),
            "reset_transform" => self.reset_transform(),
            "toggle_bilinear" => self.toggle_bilinear(),
            "group" => self.group_selected(),
            "ungroup" => self.edit_selected(true, |i| i.group = None),
            "arrange_optimal" | "arrange_name" | "arrange_order" | "arrange_addition"
            | "arrange_random" => self.arrange(id),
            "align_left" => self.align_selected(Align::Left),
            "align_right" => self.align_selected(Align::Right),
            "align_top" => self.align_selected(Align::Top),
            "align_bottom" => self.align_selected(Align::Bottom),
            "align_center_x" => self.align_selected(Align::CenterX),
            "align_center_y" => self.align_selected(Align::CenterY),
            "align_stack" => {
                let entries = self.selected_entries();
                let placed = arrange::stack(&entries, Vec2::new(24.0, 24.0));
                self.set_rects(placed)
            }
            "distribute_x" => self.distribute_selected(Axis::X),
            "distribute_y" => self.distribute_selected(Axis::Y),
            "normalize_height" => self.normalize_selected(Normalize::Height),
            "normalize_width" => self.normalize_selected(Normalize::Width),
            "normalize_area" => self.normalize_selected(Normalize::Area),
            "normalize_size" => self.normalize_size(),
            "normalize_scale" => self.normalize_scale(),
            "nudge_up" => self.nudge(0.0, -NUDGE),
            "nudge_down" => self.nudge(0.0, NUDGE),
            "nudge_left" => self.nudge(-NUDGE, 0.0),
            "nudge_right" => self.nudge(NUDGE, 0.0),
            "nudge_big_up" => self.nudge(0.0, -NUDGE_BIG),
            "nudge_big_down" => self.nudge(0.0, NUDGE_BIG),
            "nudge_big_left" => self.nudge(-NUDGE_BIG, 0.0),
            "nudge_big_right" => self.nudge(NUDGE_BIG, 0.0),
            _ => Vec::new(),
        }
    }

    /// Whether the locked state or the read-only flag blocks pointer creation (text, notes).
    fn locked(&self) -> bool {
        self.read_only || self.canvas_locked
    }

    /// Sets each item's rectangle where it differs (one op each, one history batch). Locked items are skipped.
    fn set_rects(&mut self, rects: Vec<(String, Rect)>) -> Vec<Effect> {
        if self.locked() {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for (id, rect) in rects {
            let Some(before) = self.doc.get(&id).cloned() else {
                continue;
            };
            if before.locked || before.rect == rect {
                continue;
            }
            let mut after = before.clone();
            after.rect = rect;
            self.doc.put(after.clone());
            changes.push(Change {
                id,
                before: Some(before),
                after: Some(after),
            });
        }
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    /// The selected items in draw order, as arrange entries.
    fn selected_entries(&self) -> Vec<Entry> {
        self.selected_items()
            .iter()
            .map(|item| {
                let (name, added) = match &item.data {
                    ItemData::Image { name, added, .. } | ItemData::Video { name, added, .. } => {
                        (name.clone(), *added)
                    }
                    ItemData::Text { text, .. } => (text.clone(), 0),
                    _ => (String::new(), 0),
                };
                Entry {
                    id: item.id.clone(),
                    rect: item.rect,
                    name,
                    added,
                    hue: None,
                }
            })
            .collect()
    }

    /// The natural pixel size of an image item, when its asset size is known.
    fn natural_size(&self, item: &Item) -> Option<(f64, f64)> {
        match &item.data {
            ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                self.asset_sizes.get(asset).copied()
            }
            _ => None,
        }
    }

    /// The aspect the layouts aim for: the viewport's, or square.
    fn view_aspect(&self) -> f64 {
        if self.viewport.x > 0.0 && self.viewport.y > 0.0 {
            self.viewport.x / self.viewport.y
        } else {
            1.0
        }
    }

    fn selected_images(&self) -> Vec<Item> {
        self.selected_items()
            .into_iter()
            .filter(|i| i.kind() == ItemKind::Image)
            .collect()
    }

    fn reset_transform(&mut self) -> Vec<Effect> {
        let sizes = std::mem::take(&mut self.asset_sizes);
        let fx = self.edit_selected(true, |i| {
            i.rot = 0.0;
            i.flip_x = false;
            i.flip_y = false;
            let natural = match &i.data {
                ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                    sizes.get(asset).copied()
                }
                _ => None,
            };
            if let Some((w, h)) = natural {
                let c = i.rect.center();
                i.rect = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
            }
        });
        self.asset_sizes = sizes;
        fx
    }

    /// Smooth to pixelated when any selected image is smooth; otherwise back to smooth.
    fn toggle_bilinear(&mut self) -> Vec<Effect> {
        let any_smooth = self.selected_images().iter().any(|i| {
            matches!(
                i.data,
                ItemData::Image {
                    pixelated: false,
                    ..
                }
            )
        });
        self.edit_selected(true, move |i| {
            if let ItemData::Image { pixelated, .. } = &mut i.data {
                *pixelated = any_smooth;
            }
        })
    }

    fn group_selected(&mut self) -> Vec<Effect> {
        if self.selection.len() < 2 {
            return Vec::new();
        }
        let group = (self.new_id)();
        self.edit_selected(true, |i| i.group = Some(group.clone()))
    }

    /// Arranges the selection (two or more items) by the given command.
    fn arrange(&mut self, id: &str) -> Vec<Effect> {
        let entries = self.selected_entries();
        if entries.len() < 2 {
            return Vec::new();
        }
        let aspect = self.view_aspect();
        let gap = IMAGE_GAP;
        let placed = match id {
            "arrange_optimal" => arrange::pack_optimal(&entries, gap, aspect),
            "arrange_name" => arrange::arrange_sorted(&entries, SortKey::Name, gap, aspect),
            "arrange_addition" => arrange::arrange_sorted(&entries, SortKey::Added, gap, aspect),
            "arrange_order" => {
                // Draw order (the selection is already in it): the index stands in for the added time.
                let by_order: Vec<Entry> = entries
                    .iter()
                    .enumerate()
                    .map(|(k, e)| Entry {
                        added: i64::try_from(k).unwrap_or(i64::MAX),
                        ..e.clone()
                    })
                    .collect();
                arrange::arrange_sorted(&by_order, SortKey::Added, gap, aspect)
            }
            _ => {
                let seed = (self.new_id)()
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .take(16)
                    .collect::<String>();
                let seed = u64::from_str_radix(&seed, 16).unwrap_or(self.next_op + 1);
                arrange::arrange_sorted(&entries, SortKey::Random(seed), gap, aspect)
            }
        };
        self.set_rects(placed)
    }

    fn align_selected(&mut self, how: Align) -> Vec<Effect> {
        let entries = self.selected_entries();
        let placed = arrange::align(&entries, how);
        self.set_rects(placed)
    }

    fn distribute_selected(&mut self, axis: Axis) -> Vec<Effect> {
        let entries = self.selected_entries();
        let placed = arrange::distribute(&entries, axis);
        self.set_rects(placed)
    }

    fn normalize_selected(&mut self, mode: Normalize) -> Vec<Effect> {
        let entries = self.selected_entries();
        let reference = if self.normalize_average {
            None
        } else {
            entries.first().map(|e| e.id.clone())
        };
        let placed = arrange::normalize(&entries, mode, reference.as_deref());
        self.set_rects(placed)
    }

    /// Every selected item gets the same longest side (the first's, or the mean).
    fn normalize_size(&mut self) -> Vec<Effect> {
        let items = self.selected_items();
        let values: Vec<Option<f64>> = items.iter().map(|i| Some(i.rect.w.max(i.rect.h))).collect();
        let Some(target) = pick_target(&values, self.normalize_average) else {
            return Vec::new();
        };
        let rects: Vec<(String, Rect)> = items
            .iter()
            .filter_map(|i| {
                let longest = i.rect.w.max(i.rect.h);
                if longest <= 0.0 {
                    return None;
                }
                let s = target / longest;
                let r = i.rect;
                Some((i.id.clone(), Rect::new(r.x, r.y, r.w * s, r.h * s)))
            })
            .collect();
        self.set_rects(rects)
    }

    /// Every selected image gets the same scale against its natural size (the first's, or the mean). Items with no
    /// known natural size are left as they are.
    fn normalize_scale(&mut self) -> Vec<Effect> {
        let items = self.selected_items();
        let known: Vec<Option<(f64, f64)>> = items.iter().map(|i| self.natural_size(i)).collect();
        let values: Vec<Option<f64>> = items
            .iter()
            .zip(&known)
            .map(|(i, n)| n.map(|(nw, _)| i.rect.w / nw))
            .collect();
        let Some(target) = pick_target(&values, self.normalize_average) else {
            return Vec::new();
        };
        let rects: Vec<(String, Rect)> = items
            .iter()
            .zip(&known)
            .filter_map(|(i, n)| {
                let (nw, nh) = (*n)?;
                let r = i.rect;
                Some((i.id.clone(), Rect::new(r.x, r.y, nw * target, nh * target)))
            })
            .collect();
        self.set_rects(rects)
    }

    /// Sets the natural pixel size of an image asset, reported by the page when the picture is decoded.
    pub fn set_asset_size(&mut self, asset: &str, w: f64, h: f64) {
        if w > 0.0 && h > 0.0 {
            self.asset_sizes.insert(asset.to_string(), (w, h));
        }
    }

    /// True in a pop-out window (Mode and Window items enabled). False in the main window.
    pub fn set_popout(&mut self, popout: bool) {
        self.popout = popout;
    }

    /// The pictures that failed to load: Relink missing images is enabled while there are any.
    pub fn set_missing_assets(&mut self, ids: Vec<String>) {
        self.missing_assets = ids;
    }

    /// The source url (may be empty) and file name of an asset.
    pub fn set_asset_source(&mut self, asset: &str, source_url: &str, file_name: &str) {
        self.asset_sources.insert(
            asset.to_string(),
            (source_url.to_string(), file_name.to_string()),
        );
    }

    fn set_grid(&mut self, mode: GridMode) -> Vec<Effect> {
        self.grid = mode;
        vec![Effect::Redraw]
    }

    fn set_opacity(&mut self, value: f64) -> Vec<Effect> {
        self.edit_selected(true, move |i| i.opacity = value)
    }

    fn set_width(&mut self, sw: f64) -> Vec<Effect> {
        self.edit_selected(true, move |i| {
            if let ItemData::Shape { sw: w, .. } | ItemData::Stroke { sw: w, .. } = &mut i.data {
                *w = sw;
            }
        })
    }

    /// Sorts the selection by source url, else by file name (Arrange By path).
    fn arrange_path(&mut self) -> Vec<Effect> {
        let entries: Vec<Entry> = self
            .selected_entries()
            .into_iter()
            .map(|mut e| {
                let asset = self
                    .selected_items()
                    .iter()
                    .find(|i| i.id == e.id)
                    .and_then(|i| match &i.data {
                        ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                            Some(asset.clone())
                        }
                        _ => None,
                    });
                if let Some((url, file)) = asset.and_then(|a| self.asset_sources.get(&a)) {
                    e.name = if url.is_empty() {
                        file.clone()
                    } else {
                        url.clone()
                    };
                }
                e
            })
            .collect();
        if entries.len() < 2 {
            return Vec::new();
        }
        let placed =
            arrange::arrange_sorted(&entries, SortKey::Name, IMAGE_GAP, self.view_aspect());
        self.set_rects(placed)
    }

    /// Edit the caption of the one selected image (Comment).
    fn edit_comment(&mut self) -> Vec<Effect> {
        let Some((id, _)) = self.one_image() else {
            return Vec::new();
        };
        let comment = match self.doc.get(&id).map(|i| &i.data) {
            Some(ItemData::Image { comment, .. }) => comment.clone(),
            _ => String::new(),
        };
        let mut out = Vec::new();
        self.begin_edit_field(id, false, comment, EditField::Comment, &mut out);
        out
    }

    /// Reset cropping: the selected cropped images grow back to the full picture.
    fn reset_crop(&mut self) -> Vec<Effect> {
        self.edit_selected(true, |i| {
            let rect = i.rect;
            if let ItemData::Image { crop, .. } = &mut i.data
                && let Some(window) = crop.take()
            {
                i.rect = crop::full_rect(rect, window);
            }
        })
    }

    /// Enter crop mode on the one selected image.
    fn start_crop(&mut self) -> Vec<Effect> {
        if self.locked() {
            return Vec::new();
        }
        let items = self.selected_items();
        let [item] = items.as_slice() else {
            return Vec::new();
        };
        let ItemData::Image { crop, .. } = &item.data else {
            return Vec::new();
        };
        let window = crop.unwrap_or(crop::WHOLE);
        self.crop_edit = Some(CropEdit {
            id: item.id.clone(),
            full: crop::full_rect(item.rect, window),
            crop: window,
            original: item.clone(),
        });
        self.drag = Drag::None;
        vec![Effect::Redraw]
    }

    /// Ends crop mode keeping the crop (the item keeps its visible part at the same scale).
    fn commit_crop(&mut self) -> Vec<Effect> {
        let Some(edit) = self.crop_edit.take() else {
            return Vec::new();
        };
        let CropEdit {
            id,
            full,
            crop: window,
            original,
        } = edit;
        let before = original;
        let mut after = before.clone();
        after.rect = crop::item_rect(full, window);
        if let ItemData::Image { crop, .. } = &mut after.data {
            *crop = if is_whole(window) { None } else { Some(window) };
        }
        let mut out = Vec::new();
        if after != before {
            self.doc.put(after.clone());
            self.push_changes(
                vec![Change {
                    id,
                    before: Some(before),
                    after: Some(after),
                }],
                &mut out,
            );
        }
        out.push(Effect::Redraw);
        out
    }

    /// Leaves crop mode without changing the item.
    fn cancel_crop(&mut self) -> Vec<Effect> {
        if self.crop_edit.take().is_some() {
            vec![Effect::Redraw]
        } else {
            Vec::new()
        }
    }

    /// The crop mode's keys: Enter commits, Escape cancels. The slideshow takes every key while it is open.
    fn overlay_key(&mut self, name: &str) -> Option<Vec<Effect>> {
        if self.slideshow.is_some() {
            return Some(match name {
                "esc" => self.slideshow_close(),
                "left" => self.slideshow_step(-1),
                "right" => self.slideshow_step(1),
                " " => self.slideshow_toggle(),
                _ => Vec::new(),
            });
        }
        if self.crop_edit.is_some() {
            match name {
                "enter" => return Some(self.commit_crop()),
                "esc" => return Some(self.cancel_crop()),
                _ => {}
            }
        }
        None
    }

    /// A press in crop mode: a handle or the window starts a drag; a press outside the window commits.
    fn crop_press(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let Some(edit) = self.crop_edit.as_ref() else {
            return;
        };
        let (full, start_crop) = (edit.full, edit.crop);
        let window = crop::item_rect(full, start_crop);
        let handle = hit::handle_at(
            RotRect::new(window, 0.0),
            p,
            self.px(HANDLE_RADIUS_PX),
            CROP_FAR,
        )
        .filter(|h| *h != Handle::Rotate);
        if handle.is_some() || window.contains_point(p) {
            self.drag = Drag::Crop {
                handle,
                start_world: p,
                start_crop,
            };
        } else {
            out.extend(self.commit_crop());
        }
    }

    /// The grid lines in screen px (x positions, y positions) for the visible area. Empty when the grid is off.
    pub fn grid_lines(&self) -> (Vec<f64>, Vec<f64>) {
        if self.grid == GridMode::None {
            return (Vec::new(), Vec::new());
        }
        let step = grid::visible_step(self.camera.zoom);
        let visible = self.camera.visible_world(self.viewport);
        let (xs, ys) = grid::lines(visible, step, MAX_GRID_LINES);
        let xs: Vec<f64> = xs
            .into_iter()
            .map(|x| self.camera.world_to_screen(Vec2::new(x, 0.0)).x)
            .collect();
        let ys: Vec<f64> = ys
            .into_iter()
            .map(|y| self.camera.world_to_screen(Vec2::new(0.0, y)).y)
            .collect();
        (xs, ys)
    }

    pub fn grid(&self) -> GridMode {
        self.grid
    }

    pub fn snap_to_grid(&self) -> bool {
        self.snap_grid
    }

    pub fn canvas_gray(&self) -> bool {
        self.canvas_gray
    }

    /// The full image rect and the crop window, both in screen px, while in crop mode.
    pub fn crop_view(&self) -> Option<(RotRect, Rect)> {
        let edit = self.crop_edit.as_ref()?;
        let rotation = self.doc.get(&edit.id).map_or(0.0, |i| i.rot);
        let to_screen = |r: Rect| {
            let tl = self.camera.world_to_screen(r.top_left());
            Rect::new(tl.x, tl.y, r.w * self.camera.zoom, r.h * self.camera.zoom)
        };
        Some((
            RotRect::new(to_screen(edit.full), rotation),
            to_screen(crop::item_rect(edit.full, edit.crop)),
        ))
    }

    pub fn hierarchy_open(&self) -> bool {
        self.hierarchy_open
    }

    /// Closes the hierarchy panel (its close button).
    pub fn close_hierarchy(&mut self) -> Vec<Effect> {
        self.hierarchy_open = false;
        vec![Effect::Redraw]
    }

    /// Frames first (top of the stack first), each followed by its children at depth 1, then every other item.
    pub fn hierarchy_rows(&self) -> Vec<HierarchyRow> {
        let items = self.doc.ordered();
        let mut rows = Vec::new();
        for frame in items.iter().rev().filter(|i| i.kind() == ItemKind::Frame) {
            rows.push(self.hierarchy_row(frame, 0));
            for child in items.iter().rev().filter(|i| {
                i.kind() != ItemKind::Frame && i.frame.as_deref() == Some(frame.id.as_str())
            }) {
                rows.push(self.hierarchy_row(child, 1));
            }
        }
        for item in items
            .iter()
            .rev()
            .filter(|i| i.kind() != ItemKind::Frame && !self.is_framed(i))
        {
            rows.push(self.hierarchy_row(item, 0));
        }
        rows
    }

    /// Whether the item belongs to a frame that exists.
    fn is_framed(&self, item: &Item) -> bool {
        item.frame.as_ref().is_some_and(|f| {
            self.doc
                .get(f)
                .is_some_and(|fr| fr.kind() == ItemKind::Frame)
        })
    }

    fn hierarchy_row(&self, item: &Item, depth: i32) -> HierarchyRow {
        HierarchyRow {
            id: item.id.clone(),
            depth,
            label: label_of(item),
            kind: item.kind().as_str(),
            selected: self.is_selected(&item.id),
            locked: item.locked,
        }
    }

    /// A hierarchy click: selects the item, or toggles it with Shift.
    pub fn hierarchy_pick(&mut self, id: &str, shift: bool) -> Vec<Effect> {
        if self.doc.get(id).is_none() {
            return Vec::new();
        }
        if !shift {
            self.selection = vec![id.to_string()];
        } else if self.is_selected(id) {
            self.selection.retain(|s| s.as_str() != id);
        } else {
            self.selection.push(id.to_string());
        }
        vec![Effect::Redraw]
    }

    /// Selects the item and zooms to it (double-click in the hierarchy).
    pub fn hierarchy_open_item(&mut self, id: &str) -> Vec<Effect> {
        let Some(item) = self.doc.get(id).cloned() else {
            return Vec::new();
        };
        self.selection = vec![item.id.clone()];
        self.fit_to(item.rot_rect().aabb());
        vec![Effect::Redraw, Effect::Camera]
    }

    fn slideshow_start(&mut self) -> Vec<Effect> {
        let slides: Vec<(String, String)> = self
            .doc
            .ordered()
            .into_iter()
            .filter(|i| self.selection.is_empty() || self.is_selected(&i.id))
            .filter_map(|i| match &i.data {
                ItemData::Image { asset, comment, .. } => Some((asset.clone(), comment.clone())),
                _ => None,
            })
            .collect();
        if slides.is_empty() {
            return Vec::new();
        }
        self.slideshow = Some(Slideshow {
            slides,
            index: 0,
            playing: false,
        });
        vec![Effect::Redraw]
    }

    /// The slide on show, while the slideshow is open.
    pub fn slideshow(&self) -> Option<SlideshowView> {
        let show = self.slideshow.as_ref()?;
        let (asset, caption) = show.slides.get(show.index)?.clone();
        Some(SlideshowView {
            asset,
            caption,
            index: show.index,
            count: show.slides.len(),
            playing: show.playing,
        })
    }

    /// Moves to the next (1) or previous (-1) slide, wrapping around.
    pub fn slideshow_step(&mut self, delta: i32) -> Vec<Effect> {
        let Some(show) = self.slideshow.as_mut() else {
            return Vec::new();
        };
        let count = i64::try_from(show.slides.len()).unwrap_or(1).max(1);
        let index = i64::try_from(show.index).unwrap_or(0) + i64::from(delta);
        show.index = usize::try_from(index.rem_euclid(count)).unwrap_or(0);
        vec![Effect::Redraw]
    }

    pub fn slideshow_toggle(&mut self) -> Vec<Effect> {
        if let Some(show) = self.slideshow.as_mut() {
            show.playing = !show.playing;
        }
        vec![Effect::Redraw]
    }

    pub fn slideshow_close(&mut self) -> Vec<Effect> {
        if self.slideshow.take().is_some() {
            vec![Effect::Redraw]
        } else {
            Vec::new()
        }
    }

    /// The page calls this every 3 s while the slideshow plays.
    pub fn slideshow_tick(&mut self) -> Vec<Effect> {
        match self.slideshow.as_ref() {
            Some(show) if show.playing => self.slideshow_step(1),
            _ => Vec::new(),
        }
    }

    /// The presence to send: cursor (world, None off the canvas), selection and viewport.
    pub fn presence(&self) -> Presence {
        let cursor = self.pointer_screen.map(|s| {
            let w = self.world(s);
            Point { x: w.x, y: w.y }
        });
        let visible = self.camera.visible_world(self.viewport);
        Presence {
            cursor,
            selection: self.selection.clone(),
            viewport: Some(Viewport {
                x: visible.x,
                y: visible.y,
                zoom: self.camera.zoom,
                w: self.viewport.x,
                h: self.viewport.y,
            }),
        }
    }

    /// Follows a peer: the camera shows the peer's visible world rect in our viewport.
    pub fn follow_viewport(&mut self, v: &Viewport) -> Vec<Effect> {
        if v.zoom <= 0.0
            || v.w <= 0.0
            || v.h <= 0.0
            || self.viewport.x <= 0.0
            || self.viewport.y <= 0.0
        {
            return Vec::new();
        }
        let rect = Rect::new(v.x, v.y, v.w / v.zoom, v.h / v.zoom);
        self.camera = Camera::fit(rect, self.viewport, 0.0);
        vec![Effect::Camera, Effect::Presence]
    }

    /// True once after a user pan or zoom (the page then stops following a peer).
    pub fn take_follow_cancelled(&mut self) -> bool {
        std::mem::take(&mut self.follow_cancelled)
    }

    /// The asset of each image and video item, in draw order.
    pub fn image_items(&self) -> Vec<(String, String)> {
        self.doc
            .ordered()
            .into_iter()
            .filter_map(|i| match &i.data {
                ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                    Some((i.id.clone(), asset.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The items to export, in draw order (the selection only when asked).
    pub fn items_for_export(&self, selection_only: bool) -> Vec<Item> {
        self.doc
            .ordered()
            .into_iter()
            .filter(|i| !selection_only || self.is_selected(&i.id))
            .cloned()
            .collect()
    }

    pub fn selection_bounds(&self) -> Option<Rect> {
        if self.selection.is_empty() {
            None
        } else {
            self.doc.bounds(&self.selection)
        }
    }

    /// Makes each item an image of `asset`, keeping its centre and its longer side. One history batch.
    pub fn replace_asset(&mut self, item_ids: &[String], asset: &AssetView) -> Vec<Effect> {
        if self.locked() {
            return Vec::new();
        }
        let natural_w = f64::from(asset.width.max(1));
        let natural_h = f64::from(asset.height.max(1));
        self.asset_sizes
            .insert(asset.id.clone(), (natural_w, natural_h));
        let mut changes = Vec::new();
        for id in item_ids {
            let Some(before) = self.doc.get(id).cloned() else {
                continue;
            };
            if !matches!(before.data, ItemData::Image { .. }) {
                continue;
            }
            let longer_old = before.rect.w.max(before.rect.h);
            let longer_new = natural_w.max(natural_h);
            let scale = if longer_new > 0.0 {
                longer_old / longer_new
            } else {
                1.0
            };
            let (w, h) = (natural_w * scale, natural_h * scale);
            let centre = before.rect.center();
            let mut after = before.clone();
            after.rect = Rect::new(centre.x - w / 2.0, centre.y - h / 2.0, w, h);
            if let ItemData::Image {
                asset: a,
                name,
                crop,
                ..
            } = &mut after.data
            {
                *a = asset.id.clone();
                *name = asset.file_name.clone();
                *crop = None;
            }
            self.doc.put(after.clone());
            changes.push(Change {
                id: id.clone(),
                before: Some(before),
                after: Some(after),
            });
        }
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    /// Adds items from a backup: new ids, the same relative layout, centred on the view, on top. One history batch.
    pub fn import_items(&mut self, items: Vec<Item>) -> Vec<Effect> {
        if self.locked() || items.is_empty() {
            return Vec::new();
        }
        let Some(block) = Rect::bounds_of(items.iter().map(|i| i.rect)) else {
            return Vec::new();
        };
        let ids: HashMap<String, String> = items
            .iter()
            .map(|i| (i.id.clone(), (self.new_id)()))
            .collect();
        let mut groups: HashMap<String, String> = HashMap::new();
        let base = self.doc.ordered().last().map(|i| i.z.clone());
        let zs = zorder::ranks_between(base.as_deref(), None, items.len());
        let shift = self.view_center() - block.center();
        let mut changes = Vec::new();
        let mut selection = Vec::new();
        for (item, z) in items.into_iter().zip(zs) {
            let Some(id) = ids.get(&item.id).cloned() else {
                continue;
            };
            let mut copy = item;
            copy.id = id.clone();
            copy.version = 0;
            copy.z = z;
            copy.rect = copy.rect.translate(shift);
            copy.frame = copy.frame.as_ref().and_then(|f| ids.get(f)).cloned();
            if let Some(g) = copy.group.take() {
                let mapped = groups.entry(g).or_insert_with(|| (self.new_id)()).clone();
                copy.group = Some(mapped);
            }
            self.doc.put(copy.clone());
            selection.push(id.clone());
            changes.push(Change {
                id,
                before: None,
                after: Some(copy),
            });
        }
        self.selection = selection;
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    pub fn doc_ref(&self) -> &Doc {
        &self.doc
    }

    /// The label of the open text editor's target field.
    pub fn editing_field(&self) -> EditField {
        self.editing_field
    }

    /// Creates a yellow note (a Text item) at `p` and opens the editor on it.
    fn start_note(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let id = (self.new_id)();
        let item = Item::new(
            id.clone(),
            self.top_z(),
            Rect::new(p.x, p.y, NOTE_WIDTH, NOTE_HEIGHT),
            ItemData::Text {
                text: String::new(),
                color: "ink".to_string(),
                fill: Some("yellow".to_string()),
                size: NOTE_SIZE,
                align: TextAlign::Left,
            },
        );
        self.doc.put(item);
        self.begin_edit(id, true, String::new(), out);
    }

    /// Ends the text edit. Commits the text to the item (new items are created, empty new items are dropped).
    pub fn commit_text(&mut self, raw: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        let Some((id, is_new)) = self.editing.take() else {
            return out;
        };
        let field = std::mem::replace(&mut self.editing_field, EditField::Text);
        self.editing_text.clear();
        let text = raw.trim_end().to_string();
        if let Some(before) = self.doc.get(&id).cloned()
            && !self.read_only
        {
            let mut after = before.clone();
            if field != EditField::Text {
                match (&mut after.data, field) {
                    (ItemData::Image { comment, .. }, EditField::Comment) => {
                        *comment = text.clone();
                    }
                    (ItemData::Frame { title }, EditField::Title) => *title = text.clone(),
                    _ => {}
                }
                if after != before {
                    self.doc.put(after.clone());
                    self.push_changes(
                        vec![Change {
                            id: id.clone(),
                            before: Some(before),
                            after: Some(after),
                        }],
                        &mut out,
                    );
                }
            } else {
                let size = match &after.data {
                    ItemData::Text { size, .. } => *size,
                    _ => TEXT_SIZE,
                };
                if let ItemData::Text { text: body, .. } = &mut after.data {
                    *body = text.clone();
                }
                if is_new {
                    if text.is_empty() {
                        self.doc.remove(&id);
                    } else {
                        let lines = text.lines().count().max(1) as f64;
                        after.rect.h = (lines * size * 1.4 + 12.0).max(TEXT_MIN_HEIGHT);
                        self.doc.put(after.clone());
                        self.push_changes(
                            vec![Change {
                                id: id.clone(),
                                before: None,
                                after: Some(after),
                            }],
                            &mut out,
                        );
                    }
                } else if after != before {
                    self.doc.put(after.clone());
                    self.push_changes(
                        vec![Change {
                            id: id.clone(),
                            before: Some(before),
                            after: Some(after),
                        }],
                        &mut out,
                    );
                }
            }
        }
        self.prune_selection();
        out.push(Effect::EndEdit);
        out.push(Effect::Redraw);
        out
    }

    /// Ends the text edit without keeping the text. A new item is removed; an existing one is left as it was.
    pub fn cancel_text(&mut self) -> Vec<Effect> {
        let Some((id, is_new)) = self.editing.take() else {
            return Vec::new();
        };
        self.editing_field = EditField::Text;
        self.editing_text.clear();
        if is_new {
            self.doc.remove(&id);
        }
        self.prune_selection();
        vec![Effect::EndEdit, Effect::Redraw]
    }

    /// Places an uploaded image centred on `at` (world), scaled so its longer side is at most 600 world units, on top.
    pub fn add_image(&mut self, asset: &AssetView, at: Vec2) -> Vec<Effect> {
        self.add_images(std::slice::from_ref(asset), at)
    }

    /// Places uploaded images as one block centred on `at` (world). Each is sized like [`Controller::add_image`], the block
    /// is packed with a gap so the images are spread out rather than stacked, and they go on top in the order given. The
    /// whole add is one history batch and the new items are selected.
    pub fn add_images(&mut self, assets: &[AssetView], at: Vec2) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.add_placed(assets, at, image_data);
        self.finish(before, out)
    }

    /// Places uploaded videos the same way as [`Controller::add_images`]: sized from the asset, one block, one history
    /// batch, and the new items selected. Each starts unmuted and not looping.
    pub fn add_videos(&mut self, assets: &[AssetView], at: Vec2) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.add_placed(assets, at, video_data);
        self.finish(before, out)
    }

    /// Places assets as one block centred on `at`; `data_of` builds the data of each new item.
    fn add_placed(
        &mut self,
        assets: &[AssetView],
        at: Vec2,
        data_of: fn(&AssetView) -> ItemData,
    ) -> Vec<Effect> {
        if self.locked() || assets.is_empty() {
            return Vec::new();
        }
        let entries: Vec<Entry> = assets
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                let (w, h) = image_size(asset);
                Entry {
                    id: index.to_string(),
                    rect: Rect::new(0.0, 0.0, w, h),
                    name: asset.file_name.clone(),
                    added: 0,
                    hue: None,
                }
            })
            .collect();
        let aspect = if self.viewport.x > 0.0 && self.viewport.y > 0.0 {
            self.viewport.x / self.viewport.y
        } else {
            1.0
        };
        let placed = arrange::pack_optimal(&entries, IMAGE_GAP, aspect);
        let Some(block) = Rect::bounds_of(placed.iter().map(|(_, rect)| *rect)) else {
            return Vec::new();
        };
        let shift = at - block.center();
        let mut rects: Vec<Option<Rect>> = vec![None; assets.len()];
        for (id, rect) in &placed {
            if let Ok(index) = id.parse::<usize>()
                && let Some(slot) = rects.get_mut(index)
            {
                *slot = Some(rect.translate(shift));
            }
        }
        let mut changes = Vec::new();
        let mut ids = Vec::new();
        for (asset, rect) in assets.iter().zip(rects) {
            let Some(rect) = rect else {
                continue;
            };
            let id = (self.new_id)();
            let item = Item::new(id.clone(), self.top_z(), rect, data_of(asset));
            self.doc.put(item.clone());
            ids.push(id.clone());
            changes.push(Change {
                id,
                before: None,
                after: Some(item),
            });
        }
        if changes.is_empty() {
            return Vec::new();
        }
        self.selection = ids;
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    /// Sets the `muted` or `loop` prop (`flag` is "muted" or "loop") of one video. Other flags and other items are left
    /// alone. One history batch.
    pub fn set_video_flag(&mut self, id: &str, flag: &str, on: bool) -> Vec<Effect> {
        let before = self.view_state();
        let out = self.set_video_flag_inner(id, flag, on);
        self.finish(before, out)
    }

    fn set_video_flag_inner(&mut self, id: &str, flag: &str, on: bool) -> Vec<Effect> {
        if self.locked() {
            return Vec::new();
        }
        let Some(original) = self.doc.get(id).cloned() else {
            return Vec::new();
        };
        if original.locked {
            return Vec::new();
        }
        let mut after = original.clone();
        match (&mut after.data, flag) {
            (ItemData::Video { muted, .. }, "muted") => *muted = on,
            (ItemData::Video { looping, .. }, "loop") => *looping = on,
            _ => return Vec::new(),
        }
        if after == original {
            return Vec::new();
        }
        self.doc.put(after.clone());
        let mut out = Vec::new();
        self.push_changes(
            vec![Change {
                id: id.to_string(),
                before: Some(original),
                after: Some(after),
            }],
            &mut out,
        );
        out.push(Effect::Redraw);
        out
    }

    /// The centre of the viewport in world units.
    pub fn view_center(&self) -> Vec2 {
        self.camera.screen_to_world(self.viewport / 2.0)
    }

    /// The world position under the pointer's last event, or `None` before any pointer event.
    pub fn pointer_world(&self) -> Option<Vec2> {
        self.pointer_screen.map(|screen| self.world(screen))
    }

    /// The eraser circle while the Eraser tool is on and the pointer is on the canvas: (screen x, screen y, radius px).
    pub fn eraser_view(&self) -> Option<(f64, f64, f64)> {
        if self.tool != Tool::Eraser {
            return None;
        }
        self.pointer_screen.map(|at| (at.x, at.y, ERASE_PX))
    }

    /// The rows in draw order (bottom first). Items the eraser would delete at the pointer are flagged `erase_hover`.
    pub fn rows(&self) -> Vec<ItemRow> {
        let hover: HashSet<String> = match self.pointer_world() {
            Some(p) if self.tool == Tool::Eraser => self.erase_targets(p).into_iter().collect(),
            _ => HashSet::new(),
        };
        self.draw_order()
            .into_iter()
            .map(|item| {
                let mut row = self.row(item);
                row.erase_hover = hover.contains(&item.id);
                row
            })
            .collect()
    }

    /// The selection box (world) for the handles, and whether any selected item is locked.
    pub fn selection_box(&self) -> Option<(RotRect, bool)> {
        let items = self.selected_items();
        match items.as_slice() {
            [] => None,
            [one] => Some((one.rot_rect(), one.locked)),
            many => {
                let ids: Vec<String> = many.iter().map(|i| i.id.clone()).collect();
                let bounds = self.doc.bounds(&ids)?;
                Some((RotRect::new(bounds, 0.0), many.iter().any(|i| i.locked)))
            }
        }
    }

    /// The marquee rectangle in SCREEN px, while dragging on empty canvas.
    pub fn marquee(&self) -> Option<Rect> {
        self.marquee
    }

    /// Snap guide line ends in SCREEN px.
    pub fn guides(&self) -> Vec<(Vec2, Vec2)> {
        self.guides.clone()
    }

    /// The item being edited and its text.
    pub fn editing(&self) -> Option<(String, String)> {
        self.editing
            .as_ref()
            .map(|(id, _)| (id.clone(), self.editing_text.clone()))
    }

    pub fn selected_ids(&self) -> Vec<String> {
        self.selection.clone()
    }

    // ----- internals -----

    fn next_op_id(&mut self) -> String {
        self.next_op += 1;
        format!("c{}", self.next_op)
    }

    /// A world length for a screen length in pixels.
    fn px(&self, px: f64) -> f64 {
        px / self.camera.zoom
    }

    fn world(&self, screen: Vec2) -> Vec2 {
        self.camera.screen_to_world(screen)
    }

    fn guide_screen(&self, guide: Guide) -> (Vec2, Vec2) {
        match guide {
            Guide::Vertical { x, from, to } => (
                self.camera.world_to_screen(Vec2::new(x, from)),
                self.camera.world_to_screen(Vec2::new(x, to)),
            ),
            Guide::Horizontal { y, from, to } => (
                self.camera.world_to_screen(Vec2::new(from, y)),
                self.camera.world_to_screen(Vec2::new(to, y)),
            ),
        }
    }

    fn set_cursor(&mut self, cursor: &'static str, out: &mut Vec<Effect>) {
        if self.cursor != cursor {
            self.cursor = cursor;
            out.push(Effect::Cursor(cursor));
        }
    }

    fn is_selected(&self, id: &str) -> bool {
        self.selection.iter().any(|s| s == id)
    }

    /// The item and its group members (the item alone when it has no group).
    fn group_ids(&self, id: &str) -> Vec<String> {
        match self.doc.get(id).and_then(|i| i.group.clone()) {
            Some(group) => self
                .doc
                .group_members(&group)
                .into_iter()
                .map(|i| i.id.clone())
                .collect(),
            None => vec![id.to_string()],
        }
    }

    /// The selected items in draw order.
    fn selected_items(&self) -> Vec<Item> {
        self.doc
            .ordered()
            .into_iter()
            .filter(|i| self.selection.contains(&i.id))
            .cloned()
            .collect()
    }

    /// Items in draw and hit order, top-most last: every Frame sits below the other items (so an item inside a frame
    /// can be picked), and each group keeps its draw order.
    fn draw_order(&self) -> Vec<&Item> {
        let (frames, others): (Vec<&Item>, Vec<&Item>) = self
            .doc
            .ordered()
            .into_iter()
            .partition(|i| i.kind() == ItemKind::Frame);
        frames.into_iter().chain(others).collect()
    }

    fn hit_items(&self) -> Vec<HitItem> {
        self.draw_order()
            .into_iter()
            .map(|i| HitItem {
                id: i.id.clone(),
                shape: i.rot_rect(),
                locked: i.locked,
            })
            .collect()
    }

    fn all_ids(&self) -> Vec<String> {
        self.doc
            .ordered()
            .into_iter()
            .map(|i| i.id.clone())
            .collect()
    }

    fn all_bounds(&self) -> Option<Rect> {
        let ids = self.all_ids();
        self.doc.bounds(&ids)
    }

    /// Top z rank of the document, or a rank for an empty document.
    fn top_z(&self) -> String {
        let top = self.doc.ordered().last().map(|i| i.z.clone());
        zorder::rank_between(top.as_deref(), None)
    }

    fn prune_selection(&mut self) {
        let doc = &self.doc;
        self.selection.retain(|id| doc.get(id).is_some());
    }

    fn row(&self, item: &Item) -> ItemRow {
        let mut row = ItemRow {
            id: item.id.clone(),
            kind: item.kind(),
            rect: item.rect,
            rotation: item.rot,
            flip_x: item.flip_x,
            flip_y: item.flip_y,
            opacity: item.opacity,
            asset: String::new(),
            pixelated: false,
            text: String::new(),
            color: String::new(),
            fill: String::new(),
            font_size: 0.0,
            align: "left",
            locked: item.locked,
            selected: self.is_selected(&item.id),
            editing: self.editing.as_ref().is_some_and(|(id, _)| *id == item.id),
            crop: None,
            gray: self.canvas_gray,
            path: String::new(),
            shape: "",
            stroke_width: 0.0,
            stroke_color: String::new(),
            fill_color: String::new(),
            comment: String::new(),
            erase_hover: false,
            muted: false,
            looping: false,
        };
        let (w, h) = (item.rect.w, item.rect.h);
        match &item.data {
            ItemData::Image {
                asset,
                pixelated,
                crop,
                gray,
                comment,
                ..
            } => {
                row.asset = asset.clone();
                row.pixelated = *pixelated;
                row.crop = *crop;
                row.gray = *gray || self.canvas_gray;
                row.comment = comment.clone();
            }
            ItemData::Shape {
                shape,
                stroke,
                fill,
                sw,
                pts,
                head,
                ..
            } => {
                row.shape = shape.as_str();
                row.color = stroke.clone();
                row.stroke_color = stroke.clone();
                row.stroke_width = *sw;
                row.fill_color = fill.clone().unwrap_or_default();
                row.path = draw::shape_path(*shape, w, h, pts, *head);
            }
            ItemData::Stroke { pts, color, sw } => {
                row.color = color.clone();
                row.stroke_color = color.clone();
                row.stroke_width = *sw;
                row.path = draw::stroke_path(pts);
            }
            ItemData::Text {
                text,
                color,
                fill,
                size,
                align,
            } => {
                row.text = text.clone();
                row.color = color.clone();
                row.fill = fill.clone().unwrap_or_default();
                row.font_size = *size;
                row.align = align.as_str();
            }
            ItemData::Frame { title } => row.text = title.clone(),
            ItemData::Video {
                asset,
                muted,
                looping,
                ..
            } => {
                row.asset = asset.clone();
                row.muted = *muted;
                row.looping = *looping;
            }
        }
        if row.editing {
            match self.editing_field {
                EditField::Comment => row.comment = self.editing_text.clone(),
                EditField::Text | EditField::Title => row.text = self.editing_text.clone(),
            }
        }
        row
    }

    /// Pushes a user action's changes: one op per change (sent), then one history batch.
    fn push_changes(&mut self, mut changes: Vec<Change>, out: &mut Vec<Effect>) {
        self.follow_bindings(&mut changes);
        for change in &changes {
            let op = match (&change.before, &change.after) {
                (None, Some(after)) => {
                    let op_id = self.next_op_id();
                    Some(create_op(op_id, after))
                }
                (Some(before), None) => {
                    let op_id = self.next_op_id();
                    Some(delete_op(op_id, before))
                }
                (Some(before), Some(after)) => {
                    let op_id = self.next_op_id();
                    update_op(op_id, before, after)
                }
                (None, None) => None,
            };
            if let Some(op) = op {
                self.track(&op);
                out.push(Effect::Send(op));
            }
        }
        self.history.record(changes);
    }

    /// Keeps the connectors bound to items in step with this batch. A moved, resized or rotated item re-routes the
    /// connectors bound to it; a deleted item unbinds them. A connector the user moved unbinds each end whose item did
    /// not move with it. Connectors are not bindable, so one pass is enough.
    fn follow_bindings(&mut self, changes: &mut Vec<Change>) {
        let batch_ids: Vec<String> = changes.iter().map(|c| c.id.clone()).collect();
        // Unbind: a connector the user moved (not a drag follower) lets go of the ends whose item is not in the batch.
        for change in changes.iter_mut() {
            let Some(before) = change.before.as_ref() else {
                continue;
            };
            let Some(after) = change.after.as_mut() else {
                continue;
            };
            if before.rect == after.rect
                || bind_ids(after).is_none()
                || self.drag_followers.iter().any(|f| f.id == after.id)
            {
                continue;
            }
            if let ItemData::Shape {
                start_bind,
                end_bind,
                ..
            } = &mut after.data
            {
                for end in [start_bind, end_bind] {
                    if end.as_ref().is_some_and(|bound| !batch_ids.contains(bound)) {
                        *end = None;
                    }
                }
            }
            self.doc.put(after.clone());
        }

        // Items this batch moved, resized, rotated or deleted, and those deleted.
        let mut touched: Vec<String> = Vec::new();
        let mut deleted: Vec<String> = Vec::new();
        for change in changes.iter() {
            match (&change.before, &change.after) {
                (Some(b), Some(a)) if a.rect != b.rect || a.rot != b.rot => {
                    touched.push(change.id.clone())
                }
                (Some(_), None) => {
                    touched.push(change.id.clone());
                    deleted.push(change.id.clone());
                }
                _ => {}
            }
        }
        if touched.is_empty() {
            return;
        }

        let connectors: Vec<Item> = self
            .doc
            .ordered()
            .into_iter()
            .filter(|i| {
                bind_ids(i).is_some_and(|(start, end)| {
                    [start, end]
                        .iter()
                        .flatten()
                        .any(|bound| touched.contains(bound))
                })
            })
            .cloned()
            .collect();
        for connector in connectors {
            let mut bound = connector.clone();
            if let ItemData::Shape {
                start_bind,
                end_bind,
                ..
            } = &mut bound.data
            {
                for end in [start_bind, end_bind] {
                    if end.as_ref().is_some_and(|id| deleted.contains(id)) {
                        *end = None;
                    }
                }
            }
            let routed = self.routed(bound);
            if routed == connector {
                continue;
            }
            self.doc.put(routed.clone());
            let id = connector.id.clone();
            match changes.iter().position(|c| c.id == id && c.after.is_some()) {
                Some(index) => changes[index].after = Some(routed),
                None => changes.push(Change {
                    id,
                    before: Some(connector),
                    after: Some(routed),
                }),
            }
        }
    }

    /// `connector` with each end re-routed to the item it is bound to in the document now. Other items are returned as is.
    fn routed(&self, mut connector: Item) -> Item {
        let Some((start_id, end_id)) = bind_ids(&connector) else {
            return connector;
        };
        let start = start_id.as_deref().and_then(|id| self.doc.get(id));
        let end = end_id.as_deref().and_then(|id| self.doc.get(id));
        bind::route(&mut connector, start, end);
        connector
    }

    /// Re-routes the drag followers from the items as they are in the document now (no ops).
    fn route_followers(&mut self) {
        let ids: Vec<String> = self.drag_followers.iter().map(|f| f.id.clone()).collect();
        for id in ids {
            if let Some(item) = self.doc.get(&id).cloned() {
                let routed = self.routed(item);
                self.doc.put(routed);
            }
        }
    }

    /// The connectors bound to an item of `originals` (and not among them): they follow the drag.
    fn followers_of(&self, originals: &[Item]) -> Vec<Item> {
        let ids: Vec<&str> = originals.iter().map(|o| o.id.as_str()).collect();
        self.doc
            .ordered()
            .into_iter()
            .filter(|i| {
                !ids.contains(&i.id.as_str())
                    && bind_ids(i).is_some_and(|(start, end)| {
                        [start, end]
                            .iter()
                            .flatten()
                            .any(|bound| ids.contains(&bound.as_str()))
                    })
            })
            .cloned()
            .collect()
    }

    /// Binds a new line or arrow when its creation ends. The start was bound at the press. The end is the target under
    /// the release point, dropped when it is the start's target. Then the connector is routed.
    fn bind_new_connector(&self, item: &mut Item) {
        let Some((_, end_point)) = bind::endpoints(item) else {
            return;
        };
        let end = bind::target_at(&self.draw_order(), end_point, &[item.id.as_str()]);
        let ItemData::Shape {
            start_bind,
            end_bind,
            ..
        } = &mut item.data
        else {
            return;
        };
        *end_bind = end.filter(|bound| start_bind.as_ref() != Some(bound));
        let (start_id, end_id) = (start_bind.clone(), end_bind.clone());
        let start = start_id.as_deref().and_then(|id| self.doc.get(id));
        let end_item = end_id.as_deref().and_then(|id| self.doc.get(id));
        bind::route(item, start, end_item);
    }

    /// Applies undo or redo edits to the document and sends them.
    fn apply_edits(&mut self, edits: Vec<Edit>, out: &mut Vec<Effect>) {
        for edit in edits {
            match edit {
                Edit::Create(item) => {
                    let op_id = self.next_op_id();
                    let op = create_op(op_id, &item);
                    self.track(&op);
                    out.push(Effect::Send(op));
                    self.doc.put(item);
                }
                Edit::Update { from, to } => {
                    let op_id = self.next_op_id();
                    if let Some(op) = update_op(op_id, &from, &to) {
                        self.track(&op);
                        out.push(Effect::Send(op));
                    }
                    self.doc.put(to);
                }
                Edit::Delete(item) => {
                    let op_id = self.next_op_id();
                    let op = delete_op(op_id, &item);
                    self.track(&op);
                    out.push(Effect::Send(op));
                    self.doc.remove(&item.id);
                }
            }
        }
    }

    /// The items a drag moves: the unlocked selection, plus the children of any selected Frame.
    fn drag_originals(&self) -> Vec<Item> {
        let selected = self.selected_items();
        let mut out: Vec<Item> = Vec::new();
        for item in &selected {
            if !item.locked {
                out.push(item.clone());
            }
        }
        for item in &selected {
            if item.kind() == ItemKind::Frame {
                for child in self.doc.children_of_frame(&item.id) {
                    if !child.locked && !out.iter().any(|o| o.id == child.id) {
                        out.push(child.clone());
                    }
                }
            }
        }
        out
    }

    /// The changes between the drag's starting items and the document now.
    fn changed_since(&self, originals: Vec<Item>) -> Vec<Change> {
        let mut changes = Vec::new();
        for before in originals {
            let Some(after) = self.doc.get(&before.id).cloned() else {
                continue;
            };
            if after != before {
                let id = before.id.clone();
                changes.push(Change {
                    id,
                    before: Some(before),
                    after: Some(after),
                });
            }
        }
        changes
    }

    fn fit_to(&mut self, bounds: Rect) {
        let mut camera = Camera::fit(bounds, self.viewport, FIT_MARGIN_PX);
        if camera.zoom > 1.0 {
            camera = Camera {
                pan: self.viewport / 2.0 - bounds.center(),
                zoom: 1.0,
            };
        }
        self.camera = camera;
    }

    fn zoom_about_centre(&mut self, factor: f64) -> Vec<Effect> {
        let centre = self.viewport / 2.0;
        self.camera = self.camera.zoom_at(centre, factor);
        vec![Effect::Camera]
    }

    fn zoom_selection(&mut self) -> Vec<Effect> {
        match self.doc.bounds(&self.selection) {
            Some(bounds) => {
                self.fit_to(bounds);
                vec![Effect::Camera]
            }
            None => Vec::new(),
        }
    }

    fn begin_edit(&mut self, id: String, is_new: bool, text: String, out: &mut Vec<Effect>) {
        self.begin_edit_field(id, is_new, text, EditField::Text, out);
    }

    fn begin_edit_field(
        &mut self,
        id: String,
        is_new: bool,
        text: String,
        field: EditField,
        out: &mut Vec<Effect>,
    ) {
        self.editing = Some((id.clone(), is_new));
        self.editing_field = field;
        self.editing_text = text.clone();
        out.push(Effect::EditText { id, text, field });
        out.push(Effect::Redraw);
    }

    /// The frame whose title band (the strip above its top edge) contains `p`: its id and title.
    fn frame_title_at(&self, p: Vec2) -> Option<(String, String)> {
        let band = self.px(FRAME_TITLE_PX);
        let frame = self.doc.ordered().into_iter().rev().find(|i| {
            let r = i.rect;
            i.kind() == ItemKind::Frame
                && p.x >= r.left()
                && p.x <= r.right()
                && p.y >= r.top() - band
                && p.y < r.top()
        })?;
        match &frame.data {
            ItemData::Frame { title } => Some((frame.id.clone(), title.clone())),
            _ => None,
        }
    }

    /// Creates an empty text item at `p` on top and opens the editor on it.
    fn start_new_text(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let id = (self.new_id)();
        let item = Item::new(
            id.clone(),
            self.top_z(),
            Rect::new(p.x, p.y, TEXT_WIDTH, TEXT_HEIGHT),
            ItemData::Text {
                text: String::new(),
                color: "ink".to_string(),
                fill: None,
                size: TEXT_SIZE,
                align: TextAlign::Left,
            },
        );
        self.doc.put(item);
        self.begin_edit(id, true, String::new(), out);
    }

    fn edit_selected_text(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.read_only {
            return out;
        }
        let selected = self.selected_items();
        let target = match selected.as_slice() {
            [item] if !item.locked => match &item.data {
                ItemData::Text { text, .. } => Some((item.id.clone(), text.clone())),
                _ => None,
            },
            _ => None,
        };
        if let Some((id, text)) = target {
            self.begin_edit(id, false, text, &mut out);
        }
        out
    }

    /// Applies `change` to every selected item (skipping locked ones when `skip_locked`) and records one batch.
    fn edit_selected(&mut self, skip_locked: bool, change: impl Fn(&mut Item)) -> Vec<Effect> {
        if self.read_only {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for before in self.selected_items() {
            if skip_locked && before.locked {
                continue;
            }
            let mut after = before.clone();
            change(&mut after);
            if after == before {
                continue;
            }
            let id = before.id.clone();
            self.doc.put(after.clone());
            changes.push(Change {
                id,
                before: Some(before),
                after: Some(after),
            });
        }
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    fn nudge(&mut self, dx: f64, dy: f64) -> Vec<Effect> {
        self.edit_selected(true, |i| i.rect = i.rect.translate(Vec2::new(dx, dy)))
    }

    fn toggle_lock(&mut self) -> Vec<Effect> {
        let any_unlocked = self.selected_items().iter().any(|i| !i.locked);
        if any_unlocked {
            self.edit_selected(false, |i| i.locked = true)
        } else {
            self.edit_selected(false, |i| i.locked = false)
        }
    }

    /// Moves the unlocked selection in z order. `order` gets every item and the selected ids.
    fn reorder(&mut self, order: Reorder) -> Vec<Effect> {
        let selected: Vec<String> = self
            .selected_items()
            .into_iter()
            .filter(|i| !i.locked)
            .map(|i| i.id)
            .collect();
        if self.read_only || selected.is_empty() {
            return Vec::new();
        }
        let all: Vec<(String, String)> = self
            .doc
            .ordered()
            .into_iter()
            .map(|i| (i.id.clone(), i.z.clone()))
            .collect();
        let moved = order(&all, &selected);
        let mut changes = Vec::new();
        for (id, z) in moved {
            let Some(before) = self.doc.get(&id).cloned() else {
                continue;
            };
            if before.z == z {
                continue;
            }
            let mut after = before.clone();
            after.z = z;
            self.doc.put(after.clone());
            changes.push(Change {
                id,
                before: Some(before),
                after: Some(after),
            });
        }
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    fn duplicate_selected(&mut self) -> Vec<Effect> {
        let sources: Vec<Item> = self
            .selected_items()
            .into_iter()
            .filter(|i| !i.locked)
            .collect();
        if self.read_only || sources.is_empty() {
            return Vec::new();
        }
        let base = self.doc.ordered().last().map(|i| i.z.clone());
        let zs = zorder::ranks_between(base.as_deref(), None, sources.len());
        let mut changes = Vec::new();
        let mut copies = Vec::new();
        for (source, z) in sources.iter().zip(zs) {
            let id = (self.new_id)();
            let mut item = source.clone();
            item.id = id.clone();
            item.version = 0;
            item.z = z;
            item.rect = item.rect.translate(Vec2::new(16.0, 16.0));
            self.doc.put(item.clone());
            copies.push(id.clone());
            changes.push(Change {
                id,
                before: None,
                after: Some(item),
            });
        }
        self.selection = copies;
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    fn delete_selected(&mut self) -> Vec<Effect> {
        let items: Vec<Item> = self
            .selected_items()
            .into_iter()
            .filter(|i| !i.locked)
            .collect();
        if self.read_only || items.is_empty() {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for item in items {
            self.doc.remove(&item.id);
            let id = item.id.clone();
            changes.push(Change {
                id,
                before: Some(item),
                after: None,
            });
        }
        self.selection.clear();
        let mut out = Vec::new();
        self.push_changes(changes, &mut out);
        out.push(Effect::Redraw);
        out
    }

    fn select_all(&mut self) -> Vec<Effect> {
        self.selection = self.all_ids();
        vec![Effect::Redraw]
    }

    fn escape(&mut self) -> Vec<Effect> {
        self.selection.clear();
        self.tool = Tool::Select;
        vec![Effect::Redraw]
    }

    /// A right-click: selects the item under the pointer (or clears the selection on empty canvas, unless the canvas is
    /// locked), remembers the world point and opens the menu at the pointer.
    fn open_context_menu(&mut self, at: Vec2, p: Vec2, out: &mut Vec<Effect>) {
        let items = self.hit_items();
        let hit = hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), true);
        if !self.canvas_locked {
            match hit {
                Some(id) if !self.is_selected(&id) => self.selection = self.group_ids(&id),
                Some(_) => {}
                None => self.selection.clear(),
            }
        }
        self.menu_world = p;
        self.menu = Some(MenuState {
            anchor: at,
            open: Vec::new(),
            highlight: None,
        });
        out.push(Effect::Redraw);
    }

    // ----- menu -----

    /// Whether the menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The open menu's panels and rows, in screen px within the canvas area. Empty when closed.
    pub fn menu_view(&self) -> (Vec<MenuPanelView>, Vec<MenuRowView>) {
        let (panels, rows, _) = self.menu_geometry();
        (panels, rows)
    }

    /// The pointer is over row `row` (-1 when it leaves every row): highlight it, and open or close submenus.
    pub fn menu_hover(&mut self, row: i32) -> Vec<Effect> {
        if self.menu.is_none() {
            return Vec::new();
        }
        let (_, _, addrs) = self.menu_geometry();
        let target = usize::try_from(row)
            .ok()
            .and_then(|i| addrs.get(i).copied());
        match target {
            Some((level, ri)) => match self.row_at(level, ri) {
                Some(row) if row != Row::Sep => self.focus(level, ri, row),
                _ => Vec::new(),
            },
            None => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.highlight = None;
                }
                vec![Effect::Redraw]
            }
        }
    }

    /// A click on row `row` at `x_fraction` (0..1 across the row, for the toggle halves).
    pub fn menu_click(&mut self, row: i32, x_fraction: f64) -> Vec<Effect> {
        let before = self.view_state();
        let (_, _, addrs) = self.menu_geometry();
        let out = match usize::try_from(row)
            .ok()
            .and_then(|i| addrs.get(i).copied())
        {
            Some((level, ri)) => self.activate(level, ri, x_fraction),
            None => Vec::new(),
        };
        self.finish(before, out)
    }

    /// Closes the menu (a press outside it, or the menu must go).
    pub fn menu_dismiss(&mut self) -> Vec<Effect> {
        if self.menu.take().is_some() {
            vec![Effect::Redraw]
        } else {
            Vec::new()
        }
    }

    /// The menu's keys. None when the key is not a menu key.
    fn menu_key(&mut self, name: &str) -> Option<Vec<Effect>> {
        let menu = self.menu.as_ref()?;
        let level = menu.open.len();
        let highlight = match menu.highlight {
            Some((l, r)) if l == level => Some(r),
            _ => None,
        };
        let rows = menu_level(&menu.open)?;
        match name {
            "down" | "up" => {
                let selectable: Vec<bool> = rows.iter().map(|r| self.selectable(r)).collect();
                let next = step_row(&selectable, highlight, name == "down").or(highlight);
                if let (Some(menu), Some(next)) = (self.menu.as_mut(), next) {
                    menu.highlight = Some((level, next));
                }
                Some(vec![Effect::Redraw])
            }
            "right" => {
                let Some(r) = highlight else {
                    return Some(Vec::new());
                };
                let Some(Row::Sub(_, children)) = rows.get(r).copied() else {
                    return Some(Vec::new());
                };
                let selectable: Vec<bool> = children.iter().map(|c| self.selectable(c)).collect();
                let first = step_row(&selectable, None, true);
                if let Some(menu) = self.menu.as_mut() {
                    menu.open.truncate(level);
                    menu.open.push(r);
                    menu.highlight = first.map(|i| (level + 1, i));
                }
                Some(vec![Effect::Redraw])
            }
            "left" | "esc" => {
                if level == 0 {
                    // Escape with no submenu open closes the menu; Left does nothing.
                    if name == "esc" {
                        self.menu = None;
                        return Some(vec![Effect::Redraw]);
                    }
                    return Some(Vec::new());
                }
                self.close_deepest();
                Some(vec![Effect::Redraw])
            }
            "enter" => {
                let Some(r) = highlight else {
                    return Some(Vec::new());
                };
                Some(self.activate(level, r, 0.75))
            }
            _ => None,
        }
    }

    /// Pops the deepest open submenu and highlights the row that opened it.
    fn close_deepest(&mut self) {
        if let Some(menu) = self.menu.as_mut()
            && let Some(removed) = menu.open.pop()
        {
            menu.highlight = Some((menu.open.len(), removed));
        }
    }

    /// Highlights a row; a submenu row opens its submenu and deeper levels are closed.
    fn focus(&mut self, level: usize, ri: usize, row: Row) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.highlight = Some((level, ri));
            menu.open.truncate(level);
            if matches!(row, Row::Sub(..)) {
                menu.open.push(ri);
            }
        }
        vec![Effect::Redraw]
    }

    /// Activates a row: a submenu opens, a toggle sets its half (x at or past 0.5 is Average), an enabled command
    /// closes the menu and runs. A disabled or separator row does nothing.
    fn activate(&mut self, level: usize, ri: usize, x: f64) -> Vec<Effect> {
        let Some(row) = self.row_at(level, ri) else {
            return Vec::new();
        };
        match row {
            Row::Sep => Vec::new(),
            Row::Sub(..) => self.focus(level, ri, row),
            Row::Toggle => {
                self.normalize_average = x >= 0.5;
                let mut out = self.focus(level, ri, row);
                out.push(Effect::Redraw);
                out
            }
            Row::Cmd(id) => {
                let Some(cmd) = command(id) else {
                    return Vec::new();
                };
                if !self.enabled(cmd) {
                    return Vec::new();
                }
                self.menu = None;
                let at = self.menu_world;
                let mut out = vec![Effect::Redraw];
                out.extend(self.run_command_at(id, Some(at)));
                out
            }
        }
    }

    /// The row at (level, index) of the open menu.
    fn row_at(&self, level: usize, ri: usize) -> Option<Row> {
        let menu = self.menu.as_ref()?;
        menu_level(menu.open.get(..level)?)?.get(ri).copied()
    }

    /// Whether the keyboard can land on this row.
    fn selectable(&self, row: &Row) -> bool {
        match row {
            Row::Sep => false,
            Row::Sub(..) | Row::Toggle => true,
            Row::Cmd(id) => command(id).is_some_and(|c| self.enabled(c)),
        }
    }

    /// Panels and rows with their geometry, and for each row its (level, index) address.
    fn menu_geometry(&self) -> (Vec<MenuPanelView>, Vec<MenuRowView>, Vec<(usize, usize)>) {
        let mut panels = Vec::new();
        let mut rows_out = Vec::new();
        let mut addrs = Vec::new();
        let Some(menu) = self.menu.as_ref() else {
            return (panels, rows_out, addrs);
        };
        let vp = self.viewport;
        // The parent's row y and the parent panel, for the next level.
        let mut parent: Option<(f64, MenuPanelView)> = None;
        for level in 0..=menu.open.len() {
            let Some(rows) = menu_level(&menu.open[..level]) else {
                break;
            };
            let w = self.panel_width(rows);
            let h = PAD * 2.0 + rows.iter().map(row_height).sum::<f64>();
            let (mut x, mut y) = match &parent {
                None => (menu.anchor.x, menu.anchor.y),
                Some((row_y, p)) => (p.x + p.w, row_y - PAD),
            };
            if x + w > vp.x {
                x = match &parent {
                    None => vp.x - w,
                    Some((_, p)) => p.x - w,
                };
            }
            x = x.max(0.0);
            if y + h > vp.y {
                y = vp.y - h;
            }
            y = y.max(0.0);
            let panel = MenuPanelView { x, y, w, h };
            let mut row_y = y + PAD;
            let mut open_row_y = None;
            for (ri, row) in rows.iter().enumerate() {
                let rh = row_height(row);
                if level < menu.open.len() && menu.open[level] == ri {
                    open_row_y = Some(row_y);
                }
                let highlighted = menu.highlight == Some((level, ri));
                rows_out.push(self.row_view(level, row, x, row_y, w, rh, highlighted));
                addrs.push((level, ri));
                row_y += rh;
            }
            parent = Some((open_row_y.unwrap_or(y + PAD), panel.clone()));
            panels.push(panel);
        }
        (panels, rows_out, addrs)
    }

    /// The width of a panel: its widest row, at least MIN_W.
    fn panel_width(&self, rows: &[Row]) -> f64 {
        rows.iter().map(|r| self.row_width(r)).fold(MIN_W, f64::max)
    }

    fn row_width(&self, row: &Row) -> f64 {
        match row {
            Row::Sep => 0.0,
            Row::Toggle => (chars(TOGGLE_LEFT) + chars(TOGGLE_RIGHT)) * LABEL_CHAR_W + ARROW_ROOM,
            Row::Sub(label, _) => chars(label) * LABEL_CHAR_W + ARROW_ROOM,
            Row::Cmd(id) => match command(id) {
                Some(cmd) => {
                    chars(cmd.label) * LABEL_CHAR_W
                        + chars(&key_text(cmd.keys)) * SHORTCUT_CHAR_W
                        + ARROW_ROOM
                }
                None => ARROW_ROOM,
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row_view(
        &self,
        panel: usize,
        row: &Row,
        panel_x: f64,
        y: f64,
        panel_w: f64,
        h: f64,
        highlighted: bool,
    ) -> MenuRowView {
        let mut view = MenuRowView {
            panel,
            x: panel_x + PAD / 2.0,
            y,
            w: panel_w - PAD,
            h,
            kind: "separator",
            label: "",
            shortcut: String::new(),
            enabled: false,
            checked: -1,
            highlighted,
            toggle_left: "",
            toggle_right: "",
            toggle_right_on: false,
        };
        match row {
            Row::Sep => {}
            Row::Sub(label, _) => {
                view.kind = "submenu";
                view.label = *label;
                view.enabled = true;
            }
            Row::Toggle => {
                view.kind = "toggle";
                view.enabled = true;
                view.toggle_left = TOGGLE_LEFT;
                view.toggle_right = TOGGLE_RIGHT;
                view.toggle_right_on = self.normalize_average;
            }
            Row::Cmd(id) => {
                view.kind = "item";
                if let Some(cmd) = command(id) {
                    view.label = cmd.label;
                    view.shortcut = key_text(cmd.keys);
                    view.enabled = self.enabled(cmd);
                    view.checked = self.checked_of(cmd.id);
                }
            }
        }
        view
    }

    /// The check state of a command: 1 on, 0 off, -1 no check slot.
    fn checked_of(&self, id: &str) -> i32 {
        let selected = self.selected_items();
        let images: Vec<&Item> = selected
            .iter()
            .filter(|i| i.kind() == ItemKind::Image)
            .collect();
        let videos: Vec<&Item> = selected
            .iter()
            .filter(|i| i.kind() == ItemKind::Video)
            .collect();
        match id {
            "video_mute" if !videos.is_empty() => i32::from(
                videos
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Video { muted: true, .. })),
            ),
            "video_loop" if !videos.is_empty() => i32::from(
                videos
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Video { looping: true, .. })),
            ),
            "toggle_bilinear" if !images.is_empty() => i32::from(images.iter().all(|i| {
                matches!(
                    i.data,
                    ItemData::Image {
                        pixelated: false,
                        ..
                    }
                )
            })),
            "toggle_grayscale" if !images.is_empty() => i32::from(
                images
                    .iter()
                    .all(|i| matches!(i.data, ItemData::Image { gray: true, .. })),
            ),
            "toggle_locked" if !selected.is_empty() => i32::from(selected.iter().all(|i| i.locked)),
            "lock_canvas" => i32::from(self.canvas_locked),
            "canvas_grayscale" => i32::from(self.canvas_gray),
            "hierarchy" => i32::from(self.hierarchy_open),
            "grid_none" => i32::from(self.grid == GridMode::None),
            "grid_lines" => i32::from(self.grid == GridMode::Lines),
            "grid_dots" => i32::from(self.grid == GridMode::Dots),
            "grid_snap" => i32::from(self.snap_grid),
            "draw" => i32::from(self.tool == Tool::Pen),
            "tool_select" => i32::from(self.tool == Tool::Select),
            "tool_text" => i32::from(self.tool == Tool::Text),
            "tool_hand" => i32::from(self.tool == Tool::Hand),
            "tool_rect" => i32::from(self.tool == Tool::Rect),
            "tool_ellipse" => i32::from(self.tool == Tool::Ellipse),
            "tool_diamond" => i32::from(self.tool == Tool::Diamond),
            "tool_line" => i32::from(self.tool == Tool::Line),
            "tool_arrow" => i32::from(self.tool == Tool::Arrow),
            "tool_pen" => i32::from(self.tool == Tool::Pen),
            "tool_eraser" => i32::from(self.tool == Tool::Eraser),
            "tool_frame" => i32::from(self.tool == Tool::Frame),
            "eyedropper" => i32::from(self.tool == Tool::Eyedropper),
            _ => -1,
        }
    }

    /// What a press with the Text tool does: edit a text item under the pointer, or create one here.
    fn text_press(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        if self.read_only {
            return;
        }
        let items = self.hit_items();
        let mut existing: Option<(String, String)> = None;
        if let Some(id) = hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), false)
            && let Some(Item {
                data: ItemData::Text { text, .. },
                ..
            }) = self.doc.get(&id)
        {
            existing = Some((id.clone(), text.clone()));
        }
        match existing {
            Some((id, text)) => self.begin_edit(id, false, text, out),
            None => {
                self.start_new_text(p, out);
                self.tool = Tool::Select;
            }
        }
    }

    /// A left press with the Select tool: handles, then items, then the empty canvas (marquee).
    fn select_press(&mut self, at: Vec2, p: Vec2, mods: Mods, out: &mut Vec<Effect>) {
        if let Some((shape, locked)) = self.selection_box()
            && !self.read_only
            && !locked
            && let Some(handle) = hit::handle_at(
                shape,
                p,
                self.px(HANDLE_RADIUS_PX),
                self.px(ROTATE_OFFSET_PX),
            )
        {
            let originals = self.selected_items();
            self.drag_followers = self.followers_of(&originals);
            self.drag = if handle == Handle::Rotate {
                Drag::Rotate {
                    start_world: p,
                    center: shape.rect.center(),
                    originals,
                    start_rot: shape.rotation,
                }
            } else {
                Drag::Resize {
                    handle,
                    start_world: p,
                    originals,
                    start_box: shape,
                }
            };
            out.push(Effect::Redraw);
            return;
        }

        let items = self.hit_items();
        // A frame's title band is outside its rectangle, so it is checked before the items.
        let hit_id = self
            .frame_title_at(p)
            .map(|(id, _)| id)
            .or_else(|| hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), true));
        if let Some(id) = hit_id {
            if mods.shift {
                let group = self.group_ids(&id);
                if self.is_selected(&id) {
                    self.selection.retain(|s| !group.contains(s));
                } else {
                    for g in group {
                        if !self.selection.contains(&g) {
                            self.selection.push(g);
                        }
                    }
                }
            } else {
                let was_selected = self.is_selected(&id);
                if !was_selected {
                    self.selection = self.group_ids(&id);
                }
                self.drag = Drag::Pending {
                    start_screen: at,
                    start_world: p,
                    on_item: Some(id),
                    was_selected,
                };
            }
            out.push(Effect::Redraw);
            return;
        }

        if !mods.shift {
            self.selection.clear();
        }
        let base = if mods.shift {
            self.selection.clone()
        } else {
            Vec::new()
        };
        self.drag = Drag::Marquee {
            start_screen: at,
            base,
        };
        out.push(Effect::Redraw);
    }

    fn pointer_down(&mut self, at: Vec2, button: Button, mods: Mods) -> Vec<Effect> {
        let mut out = Vec::new();
        let p = self.world(at);
        if self.slideshow.is_some() {
            return out;
        }
        if button == Button::Left && self.crop_edit.is_some() {
            self.crop_press(p, &mut out);
            return out;
        }
        // While the menu is open a left press is for the page (it dismisses the menu). A locked canvas takes no
        // left press except the Hand tool's pan.
        if button == Button::Left && self.menu.is_some() {
            return out;
        }
        if button == Button::Left && self.canvas_locked && self.tool != Tool::Hand {
            return out;
        }
        if button == Button::Middle || (button == Button::Left && self.tool == Tool::Hand) {
            self.drag = Drag::Pan { last_screen: at };
            self.set_cursor("grabbing", &mut out);
            return out;
        }
        match button {
            Button::Right => self.open_context_menu(at, p, &mut out),
            Button::Left => match self.tool {
                Tool::Text => self.text_press(p, &mut out),
                Tool::Select => self.select_press(at, p, mods, &mut out),
                Tool::Hand => {}
                Tool::Eyedropper => self.eyedrop_press(p, &mut out),
                Tool::Rect
                | Tool::Ellipse
                | Tool::Diamond
                | Tool::Line
                | Tool::Arrow
                | Tool::Frame
                    if !self.locked() =>
                {
                    self.create_press(self.tool, p, &mut out)
                }
                Tool::Pen if !self.locked() => self.pen_press(p, &mut out),
                Tool::Eraser if !self.locked() => self.erase_press(p, &mut out),
                _ => {}
            },
            Button::Middle | Button::None => {}
        }
        out
    }

    /// Starts a shape, line, arrow or frame: a provisional item at the press, sized by the drag.
    fn create_press(&mut self, tool: Tool, p: Vec2, out: &mut Vec<Effect>) {
        let id = (self.new_id)();
        let mut item = Item::new(
            id.clone(),
            self.top_z(),
            Rect::new(p.x, p.y, 0.0, 0.0),
            blank_data(tool),
        );
        // A line or arrow starts bound to the target under the press.
        if matches!(tool, Tool::Line | Tool::Arrow) {
            let start = bind::target_at(&self.draw_order(), p, &[]);
            if let ItemData::Shape { start_bind, .. } = &mut item.data {
                *start_bind = start;
            }
        }
        self.doc.put(item);
        self.drag = Drag::Create { id, tool, start: p };
        out.push(Effect::Redraw);
    }

    fn pen_press(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let id = (self.new_id)();
        let item = Item::new(
            id.clone(),
            self.top_z(),
            Rect::new(p.x, p.y, 1.0, 1.0),
            ItemData::Stroke {
                pts: Vec::new(),
                color: "ink".to_string(),
                sw: PEN_WIDTH,
            },
        );
        self.doc.put(item);
        self.drag = Drag::Pen {
            id,
            points: vec![p],
        };
        out.push(Effect::Redraw);
    }

    fn erase_press(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let mut changes = Vec::new();
        self.erase_at(p, &mut changes);
        self.drag = Drag::Erase { changes };
        out.push(Effect::Redraw);
    }

    /// The ids of the strokes, lines and arrows the eraser at `p` (world) would remove, in draw order.
    fn erase_targets(&self, p: Vec2) -> Vec<String> {
        let tol = self.px(ERASE_PX);
        self.doc
            .ordered()
            .into_iter()
            .filter(|i| erase_hit(i, p, tol))
            .map(|i| i.id.clone())
            .collect()
    }

    /// Removes every stroke, line or arrow the point touches, recording each removal.
    fn erase_at(&mut self, p: Vec2, changes: &mut Vec<Change>) {
        for id in self.erase_targets(p) {
            if let Some(item) = self.doc.remove(&id) {
                changes.push(Change {
                    id,
                    before: Some(item),
                    after: None,
                });
            }
        }
    }

    /// The eyedropper: a press on an image samples its original picture at that point.
    fn eyedrop_press(&mut self, p: Vec2, out: &mut Vec<Effect>) {
        let items = self.hit_items();
        let Some(id) = hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), true) else {
            return;
        };
        let sample = self.doc.get(&id).and_then(|item| {
            let ItemData::Image { asset, crop, .. } = &item.data else {
                return None;
            };
            let (w, h) = (item.rect.w, item.rect.h);
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            let local = item.rot_rect().to_local(p) - item.rect.top_left();
            let mut x = local.x / w;
            let mut y = local.y / h;
            if item.flip_x {
                x = 1.0 - x;
            }
            if item.flip_y {
                y = 1.0 - y;
            }
            if let Some(c) = crop {
                x = c.x + x * c.w;
                y = c.y + y * c.h;
            }
            Some((asset.clone(), x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)))
        });
        if let Some((asset, x_frac, y_frac)) = sample {
            out.push(Effect::Eyedropper {
                asset,
                x_frac,
                y_frac,
            });
        }
    }

    fn pointer_move(&mut self, at: Vec2, mods: Mods) -> Vec<Effect> {
        let mut out = Vec::new();
        let p = self.world(at);
        match self.drag.clone() {
            Drag::None => {
                let cursor = self.hover_cursor(p);
                self.set_cursor(cursor, &mut out);
            }
            Drag::Pending {
                start_screen,
                start_world,
                ..
            } => {
                if (at - start_screen).length() >= DRAG_START_PX {
                    if self.read_only {
                        self.drag = Drag::None;
                    } else {
                        let originals = self.drag_originals();
                        self.drag_followers = self.followers_of(&originals);
                        self.drag = Drag::Move {
                            start_world,
                            originals,
                        };
                        return self.pointer_move(at, mods);
                    }
                }
            }
            Drag::Move {
                start_world,
                originals,
            } => {
                let mut delta = p - start_world;
                self.guides.clear();
                if !mods.ctrl
                    && self.snap_grid
                    && let Some(moving) =
                        Rect::bounds_of(originals.iter().map(|i| i.rot_rect().aabb()))
                {
                    let moved = moving.translate(delta);
                    let snapped = grid::snap_rect_position(moved, grid::CELL);
                    delta = delta + (snapped.top_left() - moved.top_left());
                } else if !mods.ctrl
                    && let Some(moving) =
                        Rect::bounds_of(originals.iter().map(|i| i.rot_rect().aabb()))
                {
                    let moving = moving.translate(delta);
                    let others: Vec<Rect> = self
                        .doc
                        .ordered()
                        .into_iter()
                        .filter(|i| !originals.iter().any(|o| o.id == i.id))
                        .take(SNAP_OTHERS)
                        .map(|i| i.rot_rect().aabb())
                        .collect();
                    let threshold = snap::world_threshold(SNAP_PX, self.camera.zoom);
                    let snapped = snap::snap_move(moving, &others, threshold);
                    delta = delta + snapped.offset;
                    self.guides = snapped
                        .guides
                        .into_iter()
                        .map(|g| self.guide_screen(g))
                        .collect();
                }
                for original in &originals {
                    let mut item = original.clone();
                    item.rect = item.rect.translate(delta);
                    self.doc.put(item);
                }
                self.route_followers();
                out.push(Effect::Redraw);
            }
            Drag::Resize {
                handle,
                start_world,
                originals,
                start_box,
            } => {
                let delta = p - start_world;
                if let [original] = originals.as_slice() {
                    let keep_aspect = (original.kind() == ItemKind::Image) ^ mods.shift;
                    let shape = hit::resize(
                        original.rot_rect(),
                        handle,
                        delta,
                        keep_aspect,
                        mods.alt,
                        MIN_SIZE,
                    );
                    let mut item = original.clone();
                    item.rect = if self.snap_grid && !mods.ctrl && shape.rotation.abs() < 1e-12 {
                        grid::snap_rect_edges(shape.rect, grid::CELL)
                    } else {
                        shape.rect
                    };
                    item.rot = shape.rotation;
                    self.doc.put(item);
                } else {
                    let mut new_box =
                        hit::resize(start_box, handle, delta, mods.shift, mods.alt, MIN_SIZE);
                    if self.snap_grid && !mods.ctrl {
                        new_box.rect = grid::snap_rect_edges(new_box.rect, grid::CELL);
                    }
                    let from: Vec<Rect> = originals.iter().map(|i| i.rect).collect();
                    let rects = hit::scale_rects(&from, start_box.rect, new_box.rect);
                    for (original, rect) in originals.iter().zip(rects) {
                        let mut item = original.clone();
                        item.rect = rect;
                        self.doc.put(item);
                    }
                }
                self.route_followers();
                out.push(Effect::Redraw);
            }
            Drag::Rotate {
                start_world,
                center,
                originals,
                start_rot,
            } => {
                let angle = hit::rotate_to(center, start_world, p, start_rot, mods.shift);
                if let [original] = originals.as_slice() {
                    let mut item = original.clone();
                    item.rot = angle;
                    self.doc.put(item);
                } else {
                    let delta = angle - start_rot;
                    for original in &originals {
                        let r = original.rect;
                        let moved_centre = center + (r.center() - center).rotate(delta);
                        let mut item = original.clone();
                        item.rect = Rect::new(
                            moved_centre.x - r.w / 2.0,
                            moved_centre.y - r.h / 2.0,
                            r.w,
                            r.h,
                        );
                        item.rot = original.rot + delta;
                        self.doc.put(item);
                    }
                }
                self.route_followers();
                out.push(Effect::Redraw);
            }
            Drag::Marquee { start_screen, base } => {
                let items = self.hit_items();
                let area = Rect::from_points(self.world(start_screen), p);
                let mut selection = base;
                for id in hit::marquee(&items, area, Marquee::Intersect) {
                    if !selection.contains(&id) {
                        selection.push(id);
                    }
                }
                self.selection = selection;
                self.marquee = Some(Rect::from_points(start_screen, at));
                out.push(Effect::Redraw);
            }
            Drag::Pan { last_screen } => {
                self.camera = self.camera.pan_by(at - last_screen);
                self.drag = Drag::Pan { last_screen: at };
                out.push(Effect::Camera);
            }
            Drag::Create { id, tool, start } => {
                self.create_update(&id, tool, start, p, mods.shift);
                out.push(Effect::Redraw);
            }
            Drag::Pen { id, mut points } => {
                if points.last() != Some(&p) {
                    points.push(p);
                }
                self.set_stroke_points(&id, &points);
                self.drag = Drag::Pen { id, points };
                out.push(Effect::Redraw);
            }
            Drag::Erase { mut changes } => {
                self.erase_at(p, &mut changes);
                self.drag = Drag::Erase { changes };
                out.push(Effect::Redraw);
            }
            Drag::Crop {
                handle,
                start_world,
                start_crop,
            } => {
                if let Some(edit) = self.crop_edit.as_mut() {
                    edit.crop = crop::drag_crop(edit.full, start_crop, handle, p - start_world);
                }
                out.push(Effect::Redraw);
            }
        }
        out
    }

    /// Sizes the item being drawn: a box from the start to the pointer (square with Shift), or a line from the start to
    /// the pointer (45 degrees with Shift).
    fn create_update(&mut self, id: &str, tool: Tool, start: Vec2, p: Vec2, shift: bool) {
        let Some(mut item) = self.doc.get(id).cloned() else {
            return;
        };
        match tool {
            Tool::Line | Tool::Arrow => {
                let end = if shift { draw::snap_45(start, p) } else { p };
                let bounds = Rect::from_points(start, end);
                let origin = bounds.top_left();
                item.rect = Rect::new(origin.x, origin.y, bounds.w.max(1.0), bounds.h.max(1.0));
                if let ItemData::Shape { pts, .. } = &mut item.data {
                    *pts = vec![start - origin, end - origin];
                }
            }
            _ => {
                item.rect = if shift {
                    draw::square_drag(start, p)
                } else {
                    Rect::from_points(start, p)
                };
            }
        }
        self.doc.put(item);
    }

    /// Rewrites the provisional pen stroke from its world points.
    fn set_stroke_points(&mut self, id: &str, points: &[Vec2]) {
        let Some(mut item) = self.doc.get(id).cloned() else {
            return;
        };
        let (rect, local) = draw::normalize_points(points, PEN_WIDTH);
        item.rect = rect;
        item.data = ItemData::Stroke {
            pts: local,
            color: "ink".to_string(),
            sw: PEN_WIDTH,
        };
        self.doc.put(item);
    }

    /// The items a new frame takes as children: those whose centre is inside it (not frames, not already its own).
    fn adopt_into_frame(&mut self, frame_id: &str, frame_rect: Rect, changes: &mut Vec<Change>) {
        let children: Vec<Item> = self
            .doc
            .ordered()
            .into_iter()
            .filter(|i| {
                i.id != frame_id
                    && i.kind() != ItemKind::Frame
                    && i.frame.as_deref() != Some(frame_id)
                    && draw::in_frame(frame_rect, i.rect)
            })
            .cloned()
            .collect();
        for before in children {
            let mut after = before.clone();
            after.frame = Some(frame_id.to_string());
            self.doc.put(after.clone());
            changes.push(Change {
                id: before.id.clone(),
                before: Some(before),
                after: Some(after),
            });
        }
    }

    /// After a move: items that left their frame lose it, items inside a (not moving) frame get it. Children of a
    /// frame that moves with it keep it.
    fn reframe(&mut self, dragged: &[String]) {
        let moved: Vec<Item> = self
            .doc
            .ordered()
            .into_iter()
            .filter(|i| dragged.contains(&i.id) && i.kind() != ItemKind::Frame)
            .cloned()
            .collect();
        for item in moved {
            if item.frame.as_ref().is_some_and(|f| dragged.contains(f)) {
                continue;
            }
            let target = self
                .doc
                .ordered()
                .into_iter()
                .rev()
                .find(|f| {
                    f.kind() == ItemKind::Frame
                        && !dragged.contains(&f.id)
                        && draw::in_frame(f.rect, item.rect)
                })
                .map(|f| f.id.clone());
            if target != item.frame {
                let mut after = item.clone();
                after.frame = target;
                self.doc.put(after);
            }
        }
    }

    fn pointer_up(&mut self, at: Vec2, mods: Mods) -> Vec<Effect> {
        let mut out = Vec::new();
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        match drag {
            Drag::None | Drag::Pan { .. } | Drag::Crop { .. } => {}
            Drag::Pending {
                on_item,
                was_selected,
                ..
            } => {
                if let Some(id) = on_item
                    && was_selected
                    && !mods.shift
                {
                    self.selection = self.group_ids(&id);
                }
                out.push(Effect::Redraw);
            }
            Drag::Move { originals, .. } => {
                self.guides.clear();
                let dragged: Vec<String> = originals.iter().map(|i| i.id.clone()).collect();
                self.reframe(&dragged);
                // The followers' ops go out with the dragged items' (their before is the state at drag start).
                let mut originals = originals;
                originals.extend(self.drag_followers.iter().cloned());
                let changes = self.changed_since(originals);
                self.push_changes(changes, &mut out);
                out.push(Effect::Redraw);
            }
            Drag::Resize { originals, .. } | Drag::Rotate { originals, .. } => {
                self.guides.clear();
                let mut originals = originals;
                originals.extend(self.drag_followers.iter().cloned());
                let changes = self.changed_since(originals);
                self.push_changes(changes, &mut out);
                out.push(Effect::Redraw);
            }
            Drag::Marquee { .. } => {
                self.marquee = None;
                out.push(Effect::Redraw);
            }
            Drag::Create { id, tool, .. } => {
                match self.doc.get(&id).cloned() {
                    Some(mut item) if item.rect.w >= 1.0 || item.rect.h >= 1.0 => {
                        if matches!(tool, Tool::Line | Tool::Arrow) {
                            self.bind_new_connector(&mut item);
                            self.doc.put(item.clone());
                        }
                        let frame_rect = item.rect;
                        let mut changes = vec![Change {
                            id: id.clone(),
                            before: None,
                            after: Some(item),
                        }];
                        if tool == Tool::Frame {
                            self.adopt_into_frame(&id, frame_rect, &mut changes);
                        }
                        self.selection = vec![id];
                        self.push_changes(changes, &mut out);
                    }
                    _ => {
                        self.doc.remove(&id);
                    }
                }
                out.push(Effect::Redraw);
            }
            Drag::Pen { id, points } => {
                let simplified = draw::simplify(&points, self.px(1.0));
                if simplified.len() < 2 {
                    self.doc.remove(&id);
                } else {
                    self.set_stroke_points(&id, &simplified);
                    if let Some(item) = self.doc.get(&id).cloned() {
                        self.push_changes(
                            vec![Change {
                                id,
                                before: None,
                                after: Some(item),
                            }],
                            &mut out,
                        );
                    }
                }
                out.push(Effect::Redraw);
            }
            Drag::Erase { changes } => {
                if !changes.is_empty() {
                    self.push_changes(changes, &mut out);
                }
                out.push(Effect::Redraw);
            }
        }
        self.drag_followers.clear();
        let cursor = self.hover_cursor(self.world(at));
        self.set_cursor(cursor, &mut out);
        out
    }

    /// The cursor for a pointer over `p` with no button held.
    fn hover_cursor(&self, p: Vec2) -> &'static str {
        match self.tool {
            Tool::Text => "text",
            Tool::Hand => "grab",
            // The page draws the eraser circle instead of a cursor.
            Tool::Eraser => "none",
            Tool::Select => {
                if let Some((shape, locked)) = self.selection_box()
                    && !self.read_only
                    && !locked
                    && let Some(handle) = hit::handle_at(
                        shape,
                        p,
                        self.px(HANDLE_RADIUS_PX),
                        self.px(ROTATE_OFFSET_PX),
                    )
                {
                    return handle_cursor(handle);
                }
                if self.frame_title_at(p).is_some() {
                    return "move";
                }
                let items = self.hit_items();
                if hit::hit_test(&items, p, self.px(HIT_TOLERANCE_PX), false).is_some() {
                    "move"
                } else {
                    "default"
                }
            }
            _ => "crosshair",
        }
    }
}

/// The cursor name for a selection handle.
/// The size an image is placed at: its pixel size, scaled down so its longer side is at most [`MAX_IMAGE_SIDE`].
fn image_size(asset: &AssetView) -> (f64, f64) {
    let width = f64::from(asset.width.max(1));
    let height = f64::from(asset.height.max(1));
    let scale = (MAX_IMAGE_SIDE / width.max(height)).min(1.0);
    (width * scale, height * scale)
}

/// The data a new image item starts with. `added` stays 0 until the page sets it (as for every image).
fn image_data(asset: &AssetView) -> ItemData {
    ItemData::Image {
        asset: asset.id.clone(),
        crop: None,
        gray: false,
        pixelated: false,
        name: asset.file_name.clone(),
        // `added` is set by CV.7.
        added: 0,
        avg: None,
        comment: String::new(),
    }
}

/// The data a new video item starts with: full volume, unmuted, not looping. `added` stays 0, as for images.
fn video_data(asset: &AssetView) -> ItemData {
    ItemData::Video {
        asset: asset.id.clone(),
        name: asset.file_name.clone(),
        added: 0,
        volume: 1.0,
        muted: false,
        looping: false,
    }
}

/// The data a tool's new item starts with.
fn blank_data(tool: Tool) -> ItemData {
    let shape = |shape: ShapeKind, head: bool| ItemData::Shape {
        shape,
        stroke: "ink".to_string(),
        fill: None,
        sw: if matches!(shape, ShapeKind::Line | ShapeKind::Arrow) {
            CONNECTOR_WIDTH
        } else {
            SHAPE_WIDTH
        },
        pts: Vec::new(),
        start_bind: None,
        end_bind: None,
        head,
    };
    match tool {
        Tool::Ellipse => shape(ShapeKind::Ellipse, false),
        Tool::Diamond => shape(ShapeKind::Diamond, false),
        Tool::Line => shape(ShapeKind::Line, false),
        Tool::Arrow => shape(ShapeKind::Arrow, true),
        Tool::Frame => ItemData::Frame {
            title: "Frame".to_string(),
        },
        _ => shape(ShapeKind::Rect, false),
    }
}

/// The start and end bindings of a line or arrow. None for any other item.
fn bind_ids(item: &Item) -> Option<(Option<String>, Option<String>)> {
    match &item.data {
        ItemData::Shape {
            shape: ShapeKind::Line | ShapeKind::Arrow,
            start_bind,
            end_bind,
            ..
        } => Some((start_bind.clone(), end_bind.clone())),
        _ => None,
    }
}

/// Whether the eraser at `p` (world) touches a stroke, line or arrow.
fn erase_hit(item: &Item, p: Vec2, tolerance: f64) -> bool {
    match &item.data {
        ItemData::Stroke { pts, .. } => {
            draw::hits_polyline(pts, p - item.rect.top_left(), tolerance)
        }
        ItemData::Shape {
            shape: ShapeKind::Line | ShapeKind::Arrow,
            pts,
            ..
        } => {
            // Points of a line are local; the pointer is turned into the item's unrotated frame.
            let origin = item.rect.top_left();
            let world: Vec<Vec2> = if pts.len() >= 2 {
                pts.iter().map(|q| origin + *q).collect()
            } else {
                vec![origin, origin + Vec2::new(item.rect.w, item.rect.h)]
            };
            draw::hits_polyline(&world, item.rot_rect().to_local(p), tolerance)
        }
        _ => false,
    }
}

/// Sets the stroke colour of a shape, stroke or text item.
fn set_stroke(item: &mut Item, name: &str) {
    match &mut item.data {
        ItemData::Shape { stroke, .. } => *stroke = name.to_string(),
        ItemData::Stroke { color, .. } | ItemData::Text { color, .. } => {
            *color = name.to_string();
        }
        _ => {}
    }
}

/// Sets the fill of a shape or text item (None clears it).
fn set_fill(item: &mut Item, fill: Option<String>) {
    if let ItemData::Shape { fill: f, .. } | ItemData::Text { fill: f, .. } = &mut item.data {
        *f = fill;
    }
}

fn is_whole(r: Rect) -> bool {
    r.x.abs() < 1e-9 && r.y.abs() < 1e-9 && (r.w - 1.0).abs() < 1e-9 && (r.h - 1.0).abs() < 1e-9
}

/// The hierarchy label of an item.
fn label_of(item: &Item) -> String {
    match &item.data {
        ItemData::Image { name, comment, .. } => {
            if comment.trim().is_empty() {
                name.clone()
            } else {
                comment.clone()
            }
        }
        ItemData::Text { text, .. } => {
            if text.trim().is_empty() {
                "Text".to_string()
            } else {
                text.chars().take(40).collect()
            }
        }
        ItemData::Shape { shape, .. } => match shape {
            ShapeKind::Rect => "Rectangle".to_string(),
            other => capitalise(other.as_str()),
        },
        ItemData::Video { name, .. } => name.clone(),
        ItemData::Stroke { .. } => "Pen stroke".to_string(),
        ItemData::Frame { title } => {
            if title.is_empty() {
                "Frame".to_string()
            } else {
                title.clone()
            }
        }
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn handle_cursor(handle: Handle) -> &'static str {
    match handle {
        Handle::Rotate => "grab",
        Handle::NW | Handle::SE => "nwse",
        Handle::NE | Handle::SW => "nesw",
        Handle::N | Handle::S => "ns",
        Handle::E | Handle::W => "ew",
    }
}

/// One row of the menu tree: a command, a submenu, a separator, or the Normalize From first / Average toggle.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Row {
    Cmd(&'static str),
    Sub(&'static str, &'static [Row]),
    Sep,
    Toggle,
}

const TOGGLE_LEFT: &str = "From first";
const TOGGLE_RIGHT: &str = "Average";
const ROW_H: f64 = 28.0;
const SEP_H: f64 = 9.0;
const PAD: f64 = 6.0;
const MIN_W: f64 = 220.0;
/// Text width estimates for panel sizing (label, shortcut) and room for the submenu arrow and padding.
const LABEL_CHAR_W: f64 = 7.5;
const SHORTCUT_CHAR_W: f64 = 7.0;
const ARROW_ROOM: f64 = 72.0;
const NOTE_WIDTH: f64 = 240.0;
const NOTE_HEIGHT: f64 = 120.0;
const NOTE_SIZE: f64 = 20.0;
const PEN_WIDTH: f64 = 3.0;
const SHAPE_WIDTH: f64 = 2.0;
/// Lines and arrows start thicker than shapes, so they read at a glance (owner review, 2026-11-19).
const CONNECTOR_WIDTH: f64 = 4.0;
/// How close, in screen px, the eraser must be to a stroke or line.
const ERASE_PX: f64 = 6.0;
/// The height of a frame's title band above its top edge, in screen px.
const FRAME_TITLE_PX: f64 = 24.0;
const MAX_GRID_LINES: usize = 400;
/// A rotate offset so far away that the rotate handle is never hit (used in crop mode).
const CROP_FAR: f64 = 1.0e12;

const fn kb(key: &'static str, ctrl: bool, shift: bool, alt: bool) -> Option<Keys> {
    Some(Keys {
        key,
        ctrl,
        shift,
        alt,
    })
}

const fn cmd(
    id: &'static str,
    label: &'static str,
    keys: Option<Keys>,
    needs_selection: bool,
    popout_only: bool,
    built: bool,
) -> Command {
    Command {
        id,
        label,
        keys,
        needs_selection,
        popout_only,
        built,
    }
}

/// Every command, with our keys (PureRef's where PureRef has one; see PROJECT.md CV.6b).
const COMMANDS: &[Command] = &[
    cmd(
        "hierarchy",
        "Hierarchy",
        kb("j", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "undo",
        "Undo",
        kb("z", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "redo",
        "Redo",
        kb("z", true, true, false),
        false,
        false,
        true,
    ),
    cmd(
        "copy",
        "Copy image",
        kb("c", true, false, false),
        true,
        false,
        true,
    ),
    cmd("copy_as_file", "Copy as file", None, true, false, true),
    cmd(
        "paste",
        "Paste",
        kb("v", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "note",
        "Note",
        kb("n", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "draw",
        "Draw",
        kb("d", true, true, false),
        false,
        false,
        true,
    ),
    cmd(
        "mode_transparent",
        "Transparent to mouse",
        kb("t", true, false, false),
        false,
        true,
        true,
    ),
    cmd(
        "mode_overlay",
        "Overlay selection",
        kb("y", true, false, false),
        false,
        true,
        true,
    ),
    cmd(
        "mode_on_top",
        "Always on top",
        kb("a", true, true, false),
        false,
        true,
        true,
    ),
    cmd(
        "mode_on_top_of",
        "Always on top of...",
        kb("a", true, true, true),
        false,
        true,
        true,
    ),
    cmd(
        "mode_on_bottom",
        "Always on bottom",
        kb("b", true, true, false),
        false,
        true,
        true,
    ),
    cmd(
        "win_maximize",
        "Maximize",
        kb("f", true, false, false),
        false,
        true,
        true,
    ),
    cmd(
        "win_minimize",
        "Minimize",
        kb("m", true, false, false),
        false,
        true,
        true,
    ),
    cmd(
        "win_resize_to_selection",
        "Resize to selection",
        kb("r", true, true, false),
        false,
        true,
        true,
    ),
    cmd(
        "win_lock",
        "Lock window",
        kb("w", true, false, false),
        false,
        true,
        true,
    ),
    cmd(
        "new_window",
        "Pop out",
        kb("p", true, true, false),
        false,
        false,
        true,
    ),
    cmd(
        "opacity_up",
        "Increase opacity",
        kb("up", true, false, true),
        false,
        true,
        true,
    ),
    cmd(
        "opacity_down",
        "Decrease opacity",
        kb("down", true, false, true),
        false,
        true,
        true,
    ),
    cmd(
        "new_canvas",
        "New canvas",
        kb("k", true, true, false),
        false,
        false,
        true,
    ),
    cmd(
        "close",
        "Close",
        kb("q", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "select_all",
        "Select all",
        kb("a", true, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "reset_crop",
        "Reset cropping",
        kb("c", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "reset_transform",
        "Reset transform",
        kb("t", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "open_source",
        "Open source",
        kb("o", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "toggle_bilinear",
        "Bilinear sampling",
        kb("t", false, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "toggle_grayscale",
        "Grayscale",
        kb("g", false, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "toggle_locked",
        "Locked",
        kb("l", false, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "group",
        "Group",
        kb("g", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "ungroup",
        "Ungroup",
        kb("g", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "crop",
        "Crop image",
        kb("c", true, true, true),
        true,
        false,
        true,
    ),
    cmd(
        "comment",
        "Comment",
        kb("c", false, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "flip_h",
        "Flip horizontally",
        kb("h", false, true, true),
        true,
        false,
        true,
    ),
    cmd(
        "flip_v",
        "Flip vertically",
        kb("v", false, true, true),
        true,
        false,
        true,
    ),
    cmd(
        "send_back",
        "Send to back",
        kb("down", false, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "send_front",
        "Send to front",
        kb("up", false, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "slideshow",
        "Slideshow",
        kb("s", false, false, true),
        false,
        false,
        true,
    ),
    cmd(
        "delete",
        "Delete",
        kb("del", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_optimal",
        "Optimal",
        kb("p", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_name",
        "By name",
        kb("l", true, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_path",
        "By path",
        kb("d", true, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_order",
        "By order",
        kb("o", true, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_addition",
        "By addition",
        kb("a", true, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "arrange_random",
        "Randomly",
        kb("r", true, false, true),
        true,
        false,
        true,
    ),
    cmd(
        "normalize_height",
        "Height",
        kb("left", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "normalize_width",
        "Width",
        kb("right", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "normalize_size",
        "Size",
        kb("up", true, true, false),
        true,
        false,
        true,
    ),
    cmd("normalize_area", "Area", None, true, false, true),
    cmd(
        "normalize_scale",
        "Scale",
        kb("down", true, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "manage_images",
        "Manage images",
        kb("j", true, true, false),
        false,
        false,
        true,
    ),
    cmd("replace_image", "Replace image", None, true, false, true),
    cmd(
        "relink",
        "Relink missing images",
        kb("r", true, true, true),
        false,
        false,
        true,
    ),
    cmd("add_images", "Add images...", None, false, false, true),
    cmd("export_canvas", "Canvas as image", None, false, false, true),
    cmd(
        "export_selection",
        "Selection as image",
        None,
        true,
        false,
        true,
    ),
    cmd(
        "export_all",
        "All images to folder",
        None,
        false,
        false,
        true,
    ),
    cmd(
        "export_selected",
        "Selected images to folder",
        None,
        true,
        false,
        true,
    ),
    cmd("export_backup", "Backup file...", None, false, false, true),
    cmd(
        "export_open_backup",
        "Open backup...",
        None,
        false,
        false,
        true,
    ),
    cmd("grid_none", "Grid none", None, false, false, true),
    cmd("grid_lines", "Grid lines", None, false, false, true),
    cmd("grid_dots", "Grid dots", None, false, false, true),
    cmd("grid_snap", "Snap to grid", None, false, false, true),
    cmd("reset_camera", "Reset camera", None, false, false, true),
    cmd(
        "reset_camera_zoom",
        "Reset camera zoom",
        None,
        false,
        false,
        true,
    ),
    cmd("lock_canvas", "Lock canvas", None, false, false, true),
    cmd("canvas_grayscale", "Grayscale", None, false, false, true),
    cmd(
        "zoom_fit",
        "Zoom to fit",
        kb("f", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "zoom_selection",
        "Zoom to selection",
        kb("f", false, true, false),
        true,
        false,
        true,
    ),
    cmd(
        "tool_select",
        "Select tool",
        kb("v", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_text",
        "Text tool",
        kb("t", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_hand",
        "Hand tool",
        kb("h", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_rect",
        "Rectangle",
        kb("r", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_ellipse",
        "Ellipse",
        kb("o", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_diamond",
        "Diamond",
        kb("d", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_line",
        "Line",
        kb("l", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_arrow",
        "Arrow",
        kb("a", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_pen",
        "Pen",
        kb("p", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_eraser",
        "Eraser",
        kb("e", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "tool_frame",
        "Frame",
        kb("m", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "eyedropper",
        "Eyedropper",
        kb("i", false, false, false),
        false,
        false,
        true,
    ),
    cmd("align_left", "Left", None, true, false, true),
    cmd("align_right", "Right", None, true, false, true),
    cmd("align_top", "Top", None, true, false, true),
    cmd("align_bottom", "Bottom", None, true, false, true),
    cmd("align_stack", "Stack", None, true, false, true),
    cmd(
        "distribute_x",
        "Distribute horizontal",
        None,
        true,
        false,
        true,
    ),
    cmd(
        "distribute_y",
        "Distribute vertical",
        None,
        true,
        false,
        true,
    ),
    cmd(
        "align_center_x",
        "Center horizontally",
        None,
        true,
        false,
        true,
    ),
    cmd(
        "align_center_y",
        "Center vertically",
        None,
        true,
        false,
        true,
    ),
    cmd(
        "duplicate",
        "Duplicate",
        kb("d", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "escape",
        "Escape",
        kb("esc", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "edit_text",
        "Edit text",
        kb("enter", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "zoom_in",
        "Zoom in",
        kb("plus", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "zoom_out",
        "Zoom out",
        kb("minus", false, false, false),
        false,
        false,
        true,
    ),
    cmd(
        "nudge_up",
        "Nudge up",
        kb("up", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_down",
        "Nudge down",
        kb("down", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_left",
        "Nudge left",
        kb("left", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_right",
        "Nudge right",
        kb("right", false, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_big_up",
        "Nudge up 10",
        kb("up", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_big_down",
        "Nudge down 10",
        kb("down", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_big_left",
        "Nudge left 10",
        kb("left", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "nudge_big_right",
        "Nudge right 10",
        kb("right", true, false, false),
        true,
        false,
        true,
    ),
    cmd(
        "forward",
        "Forward",
        kb("up", false, true, true),
        true,
        false,
        true,
    ),
    cmd(
        "backward",
        "Backward",
        kb("down", false, true, true),
        true,
        false,
        true,
    ),
    cmd("opacity_100", "100 %", None, true, false, true),
    cmd("opacity_75", "75 %", None, true, false, true),
    cmd("opacity_50", "50 %", None, true, false, true),
    cmd("opacity_25", "25 %", None, true, false, true),
    cmd("stroke_ink", "Ink", None, true, false, true),
    cmd("stroke_gray", "Gray", None, true, false, true),
    cmd("stroke_red", "Red", None, true, false, true),
    cmd("stroke_orange", "Orange", None, true, false, true),
    cmd("stroke_yellow", "Yellow", None, true, false, true),
    cmd("stroke_green", "Green", None, true, false, true),
    cmd("stroke_teal", "Teal", None, true, false, true),
    cmd("stroke_blue", "Blue", None, true, false, true),
    cmd("stroke_purple", "Purple", None, true, false, true),
    cmd("stroke_pink", "Pink", None, true, false, true),
    cmd("stroke_white", "White", None, true, false, true),
    cmd("fill_none", "None", None, true, false, true),
    cmd("fill_ink", "Ink", None, true, false, true),
    cmd("fill_gray", "Gray", None, true, false, true),
    cmd("fill_red", "Red", None, true, false, true),
    cmd("fill_orange", "Orange", None, true, false, true),
    cmd("fill_yellow", "Yellow", None, true, false, true),
    cmd("fill_green", "Green", None, true, false, true),
    cmd("fill_teal", "Teal", None, true, false, true),
    cmd("fill_blue", "Blue", None, true, false, true),
    cmd("fill_purple", "Purple", None, true, false, true),
    cmd("fill_pink", "Pink", None, true, false, true),
    cmd("fill_white", "White", None, true, false, true),
    cmd("width_2", "Thin (2)", None, true, false, true),
    cmd("width_4", "Medium (4)", None, true, false, true),
    cmd("width_8", "Bold (8)", None, true, false, true),
    cmd(
        "video_play",
        "Play or pause",
        kb(" ", false, false, false),
        true,
        false,
        true,
    ),
    cmd("video_mute", "Mute", None, true, false, true),
    cmd("video_loop", "Loop", None, true, false, true),
];

/// The menu tree, in PureRef's order.
const MENU_ROOT: &[Row] = &[
    Row::Cmd("hierarchy"),
    Row::Sep,
    Row::Cmd("undo"),
    Row::Cmd("redo"),
    Row::Sep,
    Row::Sub("Copy", &[Row::Cmd("copy"), Row::Cmd("copy_as_file")]),
    Row::Cmd("paste"),
    Row::Cmd("note"),
    Row::Cmd("draw"),
    Row::Sep,
    Row::Sub(
        "Mode",
        &[
            Row::Cmd("mode_transparent"),
            Row::Cmd("mode_overlay"),
            Row::Cmd("mode_on_top"),
            Row::Cmd("mode_on_top_of"),
            Row::Cmd("mode_on_bottom"),
        ],
    ),
    Row::Sub(
        "Window",
        &[
            Row::Cmd("win_maximize"),
            Row::Cmd("win_minimize"),
            Row::Cmd("win_resize_to_selection"),
            Row::Cmd("win_lock"),
            Row::Cmd("new_window"),
            Row::Cmd("opacity_up"),
            Row::Cmd("opacity_down"),
        ],
    ),
    Row::Sub(
        "Canvas",
        &[
            Row::Sub(
                "Grid",
                &[
                    Row::Cmd("grid_none"),
                    Row::Cmd("grid_lines"),
                    Row::Cmd("grid_dots"),
                    Row::Sep,
                    Row::Cmd("grid_snap"),
                ],
            ),
            Row::Cmd("reset_camera"),
            Row::Cmd("reset_camera_zoom"),
            Row::Cmd("lock_canvas"),
            Row::Cmd("canvas_grayscale"),
            Row::Sep,
            Row::Cmd("zoom_fit"),
            Row::Cmd("zoom_selection"),
            Row::Sep,
            Row::Cmd("eyedropper"),
            Row::Sub(
                "Tools",
                &[
                    Row::Cmd("tool_select"),
                    Row::Cmd("tool_text"),
                    Row::Cmd("tool_hand"),
                    Row::Sep,
                    Row::Cmd("tool_rect"),
                    Row::Cmd("tool_ellipse"),
                    Row::Cmd("tool_diamond"),
                    Row::Cmd("tool_line"),
                    Row::Cmd("tool_arrow"),
                    Row::Cmd("tool_pen"),
                    Row::Cmd("tool_eraser"),
                    Row::Cmd("tool_frame"),
                ],
            ),
        ],
    ),
    Row::Sub(
        "Images",
        &[
            Row::Cmd("select_all"),
            Row::Sep,
            Row::Sub(
                "Selection",
                &[
                    Row::Cmd("reset_crop"),
                    Row::Cmd("reset_transform"),
                    Row::Cmd("open_source"),
                    Row::Cmd("toggle_bilinear"),
                    Row::Cmd("toggle_grayscale"),
                    Row::Cmd("toggle_locked"),
                    Row::Sub(
                        "Opacity",
                        &[
                            Row::Cmd("opacity_100"),
                            Row::Cmd("opacity_75"),
                            Row::Cmd("opacity_50"),
                            Row::Cmd("opacity_25"),
                        ],
                    ),
                    Row::Sep,
                    Row::Cmd("group"),
                    Row::Cmd("ungroup"),
                    Row::Cmd("crop"),
                    Row::Cmd("comment"),
                    Row::Sep,
                    Row::Cmd("flip_h"),
                    Row::Cmd("flip_v"),
                    Row::Cmd("send_back"),
                    Row::Cmd("send_front"),
                    Row::Cmd("slideshow"),
                    Row::Sep,
                    Row::Cmd("duplicate"),
                    Row::Cmd("delete"),
                ],
            ),
            Row::Sub(
                "Video",
                &[
                    Row::Cmd("video_play"),
                    Row::Cmd("video_mute"),
                    Row::Cmd("video_loop"),
                ],
            ),
            Row::Sub(
                "Arrange",
                &[
                    Row::Cmd("arrange_optimal"),
                    Row::Cmd("arrange_name"),
                    Row::Cmd("arrange_path"),
                    Row::Cmd("arrange_order"),
                    Row::Cmd("arrange_addition"),
                    Row::Cmd("arrange_random"),
                ],
            ),
            Row::Sub(
                "Align",
                &[
                    Row::Cmd("align_left"),
                    Row::Cmd("align_right"),
                    Row::Cmd("align_top"),
                    Row::Cmd("align_bottom"),
                    Row::Cmd("align_stack"),
                    Row::Cmd("distribute_x"),
                    Row::Cmd("distribute_y"),
                    Row::Sep,
                    Row::Cmd("align_center_x"),
                    Row::Cmd("align_center_y"),
                ],
            ),
            Row::Sub(
                "Normalize",
                &[
                    Row::Toggle,
                    Row::Sep,
                    Row::Cmd("normalize_height"),
                    Row::Cmd("normalize_width"),
                    Row::Cmd("normalize_size"),
                    Row::Cmd("normalize_area"),
                    Row::Cmd("normalize_scale"),
                ],
            ),
            Row::Sep,
            Row::Cmd("manage_images"),
            Row::Cmd("replace_image"),
            Row::Cmd("relink"),
        ],
    ),
    Row::Sub(
        "Style",
        &[
            Row::Sub(
                "Stroke colour",
                &[
                    Row::Cmd("stroke_ink"),
                    Row::Cmd("stroke_gray"),
                    Row::Cmd("stroke_red"),
                    Row::Cmd("stroke_orange"),
                    Row::Cmd("stroke_yellow"),
                    Row::Cmd("stroke_green"),
                    Row::Cmd("stroke_teal"),
                    Row::Cmd("stroke_blue"),
                    Row::Cmd("stroke_purple"),
                    Row::Cmd("stroke_pink"),
                    Row::Cmd("stroke_white"),
                ],
            ),
            Row::Sub(
                "Fill",
                &[
                    Row::Cmd("fill_none"),
                    Row::Sep,
                    Row::Cmd("fill_ink"),
                    Row::Cmd("fill_gray"),
                    Row::Cmd("fill_red"),
                    Row::Cmd("fill_orange"),
                    Row::Cmd("fill_yellow"),
                    Row::Cmd("fill_green"),
                    Row::Cmd("fill_teal"),
                    Row::Cmd("fill_blue"),
                    Row::Cmd("fill_purple"),
                    Row::Cmd("fill_pink"),
                    Row::Cmd("fill_white"),
                ],
            ),
            Row::Sub(
                "Stroke width",
                &[
                    Row::Cmd("width_2"),
                    Row::Cmd("width_4"),
                    Row::Cmd("width_8"),
                ],
            ),
        ],
    ),
    Row::Sep,
    Row::Cmd("add_images"),
    Row::Sub(
        "Export",
        &[
            Row::Cmd("export_canvas"),
            Row::Cmd("export_selection"),
            Row::Cmd("export_all"),
            Row::Cmd("export_selected"),
            Row::Sep,
            Row::Cmd("export_backup"),
            Row::Cmd("export_open_backup"),
        ],
    ),
];

fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Commands that change the canvas (refused while read-only or locked). Navigation, copy and the lock itself stay.
fn edits(id: &str) -> bool {
    // Tools, grid, window, export and pop-out commands never change the canvas's items.
    let view = id.starts_with("tool_")
        || id.starts_with("grid_")
        || id.starts_with("mode_")
        || id.starts_with("win_")
        || id.starts_with("export_");
    !(view
        || matches!(
            id,
            "hierarchy"
                | "copy"
                | "copy_as_file"
                | "select_all"
                | "reset_camera"
                | "reset_camera_zoom"
                | "zoom_fit"
                | "zoom_selection"
                | "zoom_in"
                | "zoom_out"
                | "eyedropper"
                | "draw"
                | "new_canvas"
                | "close"
                | "lock_canvas"
                | "canvas_grayscale"
                | "undo"
                | "redo"
                | "escape"
                | "slideshow"
                | "video_play"
                | "manage_images"
                | "open_source"
                | "new_window"
                | "opacity_up"
                | "opacity_down"
        ))
}

/// The rows of the menu level reached by following `open` from the root. None when the path is not valid.
fn menu_level(open: &[usize]) -> Option<&'static [Row]> {
    let mut rows: &'static [Row] = MENU_ROOT;
    for &i in open {
        match rows.get(i)? {
            Row::Sub(_, children) => rows = children,
            _ => return None,
        }
    }
    Some(rows)
}

fn row_height(row: &Row) -> f64 {
    match row {
        Row::Sep => SEP_H,
        _ => ROW_H,
    }
}

/// A character count as a width factor.
fn chars(s: &str) -> f64 {
    s.chars().count() as f64
}

/// The next selectable row from `current` (None: the first going forward, the last going back), if any.
fn step_row(selectable: &[bool], current: Option<usize>, forward: bool) -> Option<usize> {
    if forward {
        let start = current.map_or(0, |c| c + 1);
        (start..selectable.len()).find(|&i| selectable[i])
    } else {
        let end = current.unwrap_or(selectable.len()).min(selectable.len());
        (0..end).rev().find(|&i| selectable[i])
    }
}

/// The value a Normalize operation aims for: the first known value, or the mean of the known values when `average`.
fn pick_target(values: &[Option<f64>], average: bool) -> Option<f64> {
    let known: Vec<f64> = values.iter().flatten().copied().collect();
    if known.is_empty() {
        return None;
    }
    if !average && let Some(first) = values.first().copied().flatten() {
        return Some(first);
    }
    Some(known.iter().sum::<f64>() / known.len() as f64)
}

/// The key name of a key event: a lower-case letter or one of the names in [`Keys::key`].
fn key_name(text: &str) -> String {
    let mapped = match text {
        "\u{F700}" => "up",
        "\u{F701}" => "down",
        "\u{F702}" => "left",
        "\u{F703}" => "right",
        "\u{7f}" | "\u{8}" => "del",
        "\u{1b}" => "esc",
        "\n" | "\r" => "enter",
        "=" | "+" => "plus",
        "-" => "minus",
        _ => return text.to_lowercase(),
    };
    mapped.to_string()
}

/// The shortcut as the menu shows it: "Ctrl+Alt+Shift+K", arrows as "Up" and so on. Empty for none.
fn key_text(keys: Option<Keys>) -> String {
    let Some(k) = keys else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if k.ctrl {
        parts.push("Ctrl".to_string());
    }
    if k.alt {
        parts.push("Alt".to_string());
    }
    if k.shift {
        parts.push("Shift".to_string());
    }
    let name = match k.key {
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "del" => "Del".to_string(),
        "plus" => "+".to_string(),
        "minus" => "-".to_string(),
        "enter" => "Enter".to_string(),
        "esc" => "Esc".to_string(),
        " " => "Space".to_string(),
        other => other.to_uppercase(),
    };
    parts.push(name);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_proto::canvases::{AssetView, Hello, ItemState, OpAction, RoomCanvas, You};

    const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
    };
    const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
    };
    const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
        alt: false,
    };

    fn controller() -> Controller {
        let mut n = 0;
        let mut c = Controller::new(
            "u1".to_string(),
            Box::new(move || {
                n += 1;
                format!("n{n}")
            }),
        );
        c.set_viewport(800.0, 600.0);
        c
    }

    fn state(item: &Item) -> ItemState {
        ItemState {
            id: item.id.clone(),
            kind: item.kind().as_str().to_string(),
            z: item.z.clone(),
            version: item.version,
            props: item.to_props(),
            deleted: false,
            created_by: "u1".to_string(),
            updated_by: "u1".to_string(),
            updated_at: 0,
        }
    }

    fn hello(items: &[Item]) -> Hello {
        Hello {
            you: You {
                conn_id: "c".to_string(),
                user_id: "u1".to_string(),
                name: "Me".to_string(),
                color: "#ffffff".to_string(),
                access: "manage".to_string(),
            },
            seq: 0,
            canvas: RoomCanvas {
                title: "Board".to_string(),
                archived: false,
            },
            items: items.iter().map(state).collect(),
            peers: Vec::new(),
            media: Vec::new(),
        }
    }

    fn text_item(id: &str, z: &str, x: f64, y: f64) -> Item {
        Item::new(
            id.to_string(),
            z.to_string(),
            Rect::new(x, y, 100.0, 40.0),
            ItemData::Text {
                text: "t".to_string(),
                color: "ink".to_string(),
                fill: None,
                size: 24.0,
                align: TextAlign::Left,
            },
        )
    }

    /// A controller loaded with `items`, at zoom 1 (screen = world + pan).
    fn loaded(items: &[Item]) -> Controller {
        let mut c = controller();
        c.load(&hello(items));
        c.camera = Camera::default();
        c
    }

    fn at(c: &Controller, x: f64, y: f64) -> Vec2 {
        c.camera().world_to_screen(Vec2::new(x, y))
    }

    fn sends(fx: &[Effect]) -> Vec<&ClientOp> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::Send(op) => Some(op),
                _ => None,
            })
            .collect()
    }

    fn prop_f64(op: &ClientOp, key: &str) -> Option<f64> {
        op.props.as_ref()?.get(key)?.as_f64()
    }

    fn prop_bool(op: &ClientOp, key: &str) -> Option<bool> {
        op.props.as_ref()?.get(key)?.as_bool()
    }

    fn click(c: &mut Controller, p: Vec2, mods: Mods) {
        c.pointer(PointerKind::Down, p, Button::Left, mods);
        c.pointer(PointerKind::Up, p, Button::Left, mods);
    }

    fn drag(c: &mut Controller, from: Vec2, to: Vec2, mods: Mods) -> Vec<Effect> {
        c.pointer(PointerKind::Down, from, Button::Left, mods);
        c.pointer(PointerKind::Move, to, Button::None, mods);
        c.pointer(PointerKind::Up, to, Button::Left, mods)
    }

    #[test]
    fn text_tool_click_and_commit_sends_one_create_with_text() {
        let mut c = loaded(&[]);
        c.set_tool(Tool::Text);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let fx = c.commit_text("hi");
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].action, OpAction::Create);
        assert_eq!(
            ops[0]
                .props
                .as_ref()
                .and_then(|p| p.get("text"))
                .and_then(|v| v.as_str()),
            Some("hi")
        );
    }

    #[test]
    fn empty_text_commit_sends_nothing_and_removes_the_item() {
        let mut c = loaded(&[]);
        c.set_tool(Tool::Text);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let fx = c.commit_text("");
        assert!(sends(&fx).is_empty());
        assert!(c.rows().is_empty());
    }

    #[test]
    fn click_selects_an_item() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        assert_eq!(c.selected_ids(), vec!["t1".to_string()]);
    }

    #[test]
    fn drag_inside_item_sends_one_update_with_new_x() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let a = at(&c, 20.0, 20.0);
        let b = at(&c, 70.0, 20.0);
        let fx = drag(&mut c, a, b, CTRL);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].action, OpAction::Update);
        assert_eq!(prop_f64(ops[0], "x"), Some(50.0));
    }

    #[test]
    fn locked_item_is_selected_but_does_not_move() {
        let mut item = text_item("t1", "a0", 0.0, 0.0);
        item.locked = true;
        let mut c = loaded(&[item]);
        let a = at(&c, 20.0, 20.0);
        let b = at(&c, 70.0, 20.0);
        let fx = drag(&mut c, a, b, CTRL);
        assert!(sends(&fx).is_empty());
        assert_eq!(c.selected_ids(), vec!["t1".to_string()]);
    }

    #[test]
    fn read_only_drag_and_delete_send_nothing() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        c.set_read_only(true);
        let a = at(&c, 20.0, 20.0);
        let b = at(&c, 70.0, 20.0);
        let fx = drag(&mut c, a, b, NONE);
        assert!(sends(&fx).is_empty());
        let (used, fx) = c.key("\u{7f}", NONE);
        assert!(!used);
        assert!(sends(&fx).is_empty());
    }

    #[test]
    fn shift_click_toggles_selection() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 200.0, 0.0),
        ]);
        let p1 = at(&c, 10.0, 10.0);
        let p2 = at(&c, 210.0, 10.0);
        click(&mut c, p1, NONE);
        click(&mut c, p2, SHIFT);
        assert_eq!(c.selected_ids(), vec!["t1".to_string(), "t2".to_string()]);
        click(&mut c, p1, SHIFT);
        assert_eq!(c.selected_ids(), vec!["t2".to_string()]);
    }

    #[test]
    fn marquee_over_two_items_selects_both() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 300.0, 300.0),
        ]);
        let a = at(&c, -20.0, -20.0);
        let b = at(&c, 500.0, 500.0);
        drag(&mut c, a, b, NONE);
        assert_eq!(c.selected_ids().len(), 2);
    }

    #[test]
    fn delete_sends_delete_op() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let (used, fx) = c.key("\u{7f}", NONE);
        assert!(used);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].action, OpAction::Delete);
    }

    #[test]
    fn undo_after_move_sends_update_back_to_old_x() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let a = at(&c, 20.0, 20.0);
        let b = at(&c, 70.0, 20.0);
        drag(&mut c, a, b, CTRL);
        let (used, fx) = c.key("z", CTRL);
        assert!(used);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].action, OpAction::Update);
        assert_eq!(prop_f64(ops[0], "x"), Some(0.0));
    }

    #[test]
    fn undo_with_nothing_to_undo_sends_nothing() {
        let mut c = loaded(&[]);
        let (used, fx) = c.key("z", CTRL);
        assert!(used);
        assert!(sends(&fx).is_empty());
    }

    #[test]
    fn wheel_up_zooms_in_and_keeps_the_point_under_the_cursor() {
        let mut c = loaded(&[]);
        let p = Vec2::new(400.0, 300.0);
        let before = c.camera().screen_to_world(p);
        c.scroll(p, Vec2::new(0.0, 1.0), NONE);
        assert!(c.camera().zoom > 1.0);
        let after = c.camera().screen_to_world(p);
        assert!((after.x - before.x).abs() < 1e-9);
        assert!((after.y - before.y).abs() < 1e-9);
    }

    #[test]
    fn menu_front_gives_the_item_the_highest_z() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 200.0, 0.0),
        ]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        c.run_command("send_front");
        let rows = c.rows();
        assert_eq!(rows.last().map(|r| r.id.as_str()), Some("t1"));
    }

    #[test]
    fn flip_horizontal_sends_flip_x() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let fx = c.run_command("flip_h");
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(prop_bool(ops[0], "flip_x"), Some(true));
        assert!(c.rows()[0].flip_x);
    }

    #[test]
    fn add_image_scales_to_600_and_centres_on_the_point() {
        let mut c = loaded(&[]);
        let asset = AssetView {
            id: "a1".to_string(),
            sha256: "00".to_string(),
            mime: "image/png".to_string(),
            bytes: 10,
            width: 1200,
            height: 600,
            frame_count: 1,
            file_name: "x.png".to_string(),
            source_url: String::new(),
            thumbs: Vec::new(),
            url: "/v1/assets/a1".to_string(),
            created_at: String::new(),
            media: "Image".to_string(),
            duration_ms: None,
            has_audio: false,
        };
        let fx = c.add_image(&asset, Vec2::new(0.0, 0.0));
        assert_eq!(sends(&fx).len(), 1);
        let row = c
            .rows()
            .into_iter()
            .find(|r| r.asset == "a1")
            .expect("image row");
        assert_eq!(row.rect, Rect::new(-300.0, -150.0, 600.0, 300.0));
    }

    fn square_asset(id: &str) -> AssetView {
        AssetView {
            id: id.to_string(),
            sha256: "00".to_string(),
            mime: "image/png".to_string(),
            bytes: 10,
            width: 100,
            height: 100,
            frame_count: 1,
            file_name: format!("{id}.png"),
            source_url: String::new(),
            thumbs: Vec::new(),
            url: format!("/v1/assets/{id}"),
            created_at: String::new(),
            media: "Image".to_string(),
            duration_ms: None,
            has_audio: false,
        }
    }

    #[test]
    fn several_images_are_spread_not_stacked() {
        let mut c = loaded(&[]);
        let assets: Vec<AssetView> = (0..3).map(|n| square_asset(&format!("a{n}"))).collect();
        let fx = c.add_images(&assets, Vec2::new(0.0, 0.0));
        assert_eq!(sends(&fx).len(), 3);
        let rects: Vec<Rect> = c.rows().into_iter().map(|r| r.rect).collect();
        assert_eq!(rects.len(), 3);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
            }
        }
        let centre = Rect::bounds_of(rects.iter().copied())
            .expect("the block has bounds")
            .center();
        assert!(
            centre.x.abs() < 1.0 && centre.y.abs() < 1.0,
            "block centre ({}, {}) is near the origin",
            centre.x,
            centre.y
        );
    }

    #[test]
    fn load_fits_the_camera_without_zooming_past_one() {
        let mut big = text_item("t1", "a0", 0.0, 0.0);
        big.rect = Rect::new(0.0, 0.0, 4000.0, 2000.0);
        let mut c = controller();
        c.load(&hello(&[big]));
        assert!(c.camera().zoom <= 1.0);
        assert!(c.camera().zoom > 0.0);
    }

    const ALT: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: true,
    };
    const CTRL_SHIFT: Mods = Mods {
        shift: true,
        ctrl: true,
        alt: false,
    };
    const CTRL_ALT: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: true,
    };

    fn image_item(id: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item::new(
            id.to_string(),
            format!("a{id}"),
            Rect::new(x, y, w, h),
            ItemData::Image {
                asset: format!("asset-{id}"),
                crop: None,
                gray: false,
                pixelated: false,
                name: id.to_string(),
                added: 0,
                avg: None,
                comment: String::new(),
            },
        )
    }

    fn right_click(c: &mut Controller, p: Vec2) {
        c.pointer(PointerKind::Down, p, Button::Right, NONE);
    }

    /// The index of the first menu row with this label (across every open panel).
    fn row_of(c: &Controller, label: &str) -> i32 {
        let (_, rows) = c.menu_view();
        let i = rows
            .iter()
            .position(|r| r.label == label)
            .expect("row is in the menu");
        i32::try_from(i).expect("small index")
    }

    fn rect_of(c: &Controller, id: &str) -> Rect {
        c.rows()
            .into_iter()
            .find(|r| r.id == id)
            .expect("item row")
            .rect
    }

    #[test]
    fn shift_up_sends_the_selected_item_to_the_front() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 200.0, 0.0),
        ]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let (used, _) = c.key("\u{F700}", SHIFT);
        assert!(used);
        assert_eq!(c.rows().last().map(|r| r.id.as_str()), Some("t1"));
    }

    #[test]
    fn up_nudges_the_selection_by_one() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let (used, fx) = c.key("\u{F700}", NONE);
        assert!(used);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(prop_f64(ops[0], "y"), Some(-1.0));
    }

    #[test]
    fn ctrl_right_nudges_by_ten() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let (_, fx) = c.key("\u{F703}", CTRL);
        assert_eq!(prop_f64(sends(&fx)[0], "x"), Some(10.0));
    }

    #[test]
    fn alt_l_locks_then_unlocks() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let (_, fx) = c.key("l", ALT);
        assert_eq!(prop_bool(sends(&fx)[0], "locked"), Some(true));
        let (_, fx) = c.key("l", ALT);
        assert_eq!(prop_bool(sends(&fx)[0], "locked"), Some(false));
    }

    #[test]
    fn ctrl_alt_l_arranges_by_name_left_to_right() {
        let mut c = loaded(&[
            image_item("img10", 0.0, 0.0, 100.0, 100.0),
            image_item("img2", 300.0, 0.0, 100.0, 100.0),
            image_item("img1", 600.0, 0.0, 100.0, 100.0),
        ]);
        // One row: a very wide viewport.
        c.set_viewport(8000.0, 100.0);
        c.key("a", CTRL);
        c.key("l", CTRL_ALT);
        let x1 = rect_of(&c, "img1").x;
        let x2 = rect_of(&c, "img2").x;
        let x10 = rect_of(&c, "img10").x;
        assert!(x1 < x2 && x2 < x10, "{x1} {x2} {x10}");
    }

    #[test]
    fn ctrl_shift_left_normalizes_height_to_the_mean() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 200.0, 0.0),
            text_item("t3", "a2", 400.0, 0.0),
        ]);
        let heights = [50.0, 100.0, 200.0];
        for (id, h) in ["t1", "t2", "t3"].iter().zip(heights) {
            let mut item = c.doc.get(id).cloned().expect("item");
            item.rect.h = h;
            c.doc.put(item);
        }
        c.key("a", CTRL);
        c.key("\u{F702}", CTRL_SHIFT);
        let mean = 350.0 / 3.0;
        assert!((rect_of(&c, "t2").h - mean).abs() < 1e-6);
        assert!((rect_of(&c, "t3").h - mean).abs() < 1e-6);
    }

    #[test]
    fn normalize_from_first_uses_the_first_selected() {
        let mut c = loaded(&[
            text_item("t1", "a0", 0.0, 0.0),
            text_item("t2", "a1", 200.0, 0.0),
        ]);
        for (id, h) in [("t1", 50.0), ("t2", 100.0)] {
            let mut item = c.doc.get(id).cloned().expect("item");
            item.rect.h = h;
            c.doc.put(item);
        }
        c.key("a", CTRL);
        let p = at(&c, 10.0, 10.0);
        right_click(&mut c, p);
        let images = row_of(&c, "Images");
        c.menu_hover(images);
        let normalize = row_of(&c, "Normalize");
        c.menu_hover(normalize);
        let toggle = c
            .menu_view()
            .1
            .iter()
            .position(|r| r.kind == "toggle")
            .expect("toggle row");
        c.menu_click(i32::try_from(toggle).expect("index"), 0.2);
        c.menu_dismiss();
        c.key("\u{F702}", CTRL_SHIFT);
        assert!((rect_of(&c, "t2").h - 50.0).abs() < 1e-6);
    }

    #[test]
    fn right_click_opens_a_level_zero_panel_inside_the_viewport() {
        let mut c = loaded(&[]);
        right_click(&mut c, Vec2::new(100.0, 100.0));
        assert!(c.menu_open());
        let (panels, _) = c.menu_view();
        let first = &panels[0];
        assert!(first.x >= 0.0 && first.x + first.w <= 800.0);
        assert!(first.y >= 0.0 && first.y + first.h <= 600.0);
    }

    #[test]
    fn hovering_images_opens_a_second_panel_to_its_right() {
        let mut c = loaded(&[]);
        right_click(&mut c, Vec2::new(100.0, 100.0));
        c.menu_hover(row_of(&c, "Images"));
        let (panels, _) = c.menu_view();
        assert_eq!(panels.len(), 2);
        assert!(panels[1].x >= panels[0].x + panels[0].w - 1e-9);
    }

    #[test]
    fn a_panel_that_would_leave_the_viewport_moves_left() {
        let mut c = loaded(&[]);
        right_click(&mut c, Vec2::new(700.0, 100.0));
        c.menu_hover(row_of(&c, "Images"));
        let (panels, _) = c.menu_view();
        assert_eq!(panels.len(), 2);
        assert!(panels[0].x + panels[0].w <= 800.0 + 1e-9);
        assert!(panels[1].x + panels[1].w <= panels[0].x + 1e-9);
        assert!(panels[1].x >= 0.0);
    }

    #[test]
    fn escape_closes_the_menu() {
        let mut c = loaded(&[]);
        right_click(&mut c, Vec2::new(100.0, 100.0));
        let (used, _) = c.key("\u{1b}", NONE);
        assert!(used);
        assert!(!c.menu_open());
    }

    #[test]
    fn a_disabled_row_click_does_nothing() {
        let mut c = loaded(&[]);
        right_click(&mut c, Vec2::new(100.0, 100.0));
        c.menu_hover(row_of(&c, "Copy"));
        let fx = c.menu_click(row_of(&c, "Copy image"), 0.5);
        assert!(fx.is_empty());
        assert!(c.menu_open());
    }

    #[test]
    fn shortcuts_for_new_canvas_and_close_remain_and_help_is_gone() {
        let mut c = loaded(&[]);
        assert_eq!(c.key("k", CTRL_SHIFT), (true, vec![Effect::NewCanvas]));
        assert_eq!(c.key("q", CTRL), (true, vec![Effect::CloseCanvas]));
        assert_eq!(c.key("h", CTRL), (false, Vec::new()));
        assert_eq!(c.key("[", CTRL), (false, Vec::new()));
    }

    #[test]
    fn acks_of_quick_nudges_never_snap_the_item_back() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        let mut op_ids = Vec::new();
        for _ in 0..3 {
            let (_, fx) = c.key("\u{F703}", NONE);
            op_ids.extend(sends(&fx).iter().map(|op| op.op_id.clone()));
        }
        assert_eq!(op_ids.len(), 3);
        for (n, op_id) in op_ids.iter().enumerate() {
            let mut item = text_item(
                "t1",
                "a0",
                f64::from(u32::try_from(n + 1).expect("small")),
                0.0,
            );
            item.version = u64::try_from(n).expect("small") + 2;
            c.ack(op_id, &state(&item));
            assert_eq!(rect_of(&c, "t1").x, 3.0);
        }
    }

    #[test]
    fn remote_changes_are_ignored_while_a_local_op_is_pending() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        let p = at(&c, 10.0, 10.0);
        click(&mut c, p, NONE);
        c.key("\u{F703}", NONE);
        let mut remote = text_item("t1", "a0", 50.0, 0.0);
        remote.version = 9;
        assert!(c.apply(&state(&remote)).is_empty());
        assert_eq!(rect_of(&c, "t1").x, 1.0);
    }

    #[test]
    fn a_rectangle_drag_creates_one_shape_and_the_tool_stays_until_escape() {
        let mut c = loaded(&[]);
        c.set_tool(Tool::Rect);
        let from = at(&c, 10.0, 10.0);
        let to = at(&c, 60.0, 40.0);
        let fx = drag(&mut c, from, to, NONE);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].action, OpAction::Create);
        assert_eq!(c.tool(), Tool::Rect);
        c.key("\u{1b}", NONE);
        assert_eq!(c.tool(), Tool::Select);
    }

    #[test]
    fn a_new_frame_takes_the_items_whose_centre_is_inside() {
        let mut c = loaded(&[text_item("t1", "a0", 0.0, 0.0)]);
        c.set_tool(Tool::Frame);
        let from = at(&c, -50.0, -50.0);
        let to = at(&c, 200.0, 200.0);
        let fx = drag(&mut c, from, to, NONE);
        assert_eq!(sends(&fx).len(), 2);
        let child = c.doc_ref().get("t1").expect("item");
        assert!(child.frame.is_some());
    }

    #[test]
    fn eyedropper_on_an_image_reports_the_point_in_the_picture() {
        let mut c = loaded(&[image_item("img1", 0.0, 0.0, 100.0, 50.0)]);
        c.set_tool(Tool::Eyedropper);
        let p = at(&c, 25.0, 25.0);
        let fx = c.pointer(PointerKind::Down, p, Button::Left, NONE);
        assert_eq!(
            fx,
            vec![Effect::Eyedropper {
                asset: "asset-img1".to_string(),
                x_frac: 0.25,
                y_frac: 0.5,
            }]
        );
    }

    #[test]
    fn crop_mode_commits_the_visible_part_at_the_same_scale() {
        let mut c = loaded(&[image_item("img1", 0.0, 0.0, 100.0, 100.0)]);
        let p = at(&c, 50.0, 50.0);
        click(&mut c, p, NONE);
        c.run_command("crop");
        assert!(c.crop_view().is_some());
        let from = at(&c, 100.0, 50.0);
        let to = at(&c, 50.0, 50.0);
        drag(&mut c, from, to, NONE);
        c.key("\n", NONE);
        let row = c
            .rows()
            .into_iter()
            .find(|r| r.id == "img1")
            .expect("image row");
        assert!((row.rect.w - 50.0).abs() < 1e-6);
        assert!(row.crop.is_some());
        assert!(c.crop_view().is_none());
    }

    #[test]
    fn hierarchy_lists_frame_children_under_their_frame() {
        let mut framed = text_item("t1", "a0", 0.0, 0.0);
        framed.frame = Some("f1".to_string());
        let frame = Item::new(
            "f1".to_string(),
            "a1".to_string(),
            Rect::new(-10.0, -10.0, 300.0, 300.0),
            ItemData::Frame {
                title: "Board".to_string(),
            },
        );
        let loose = text_item("t2", "a2", 500.0, 500.0);
        let c = loaded(&[frame, framed, loose]);
        let rows = c.hierarchy_rows();
        let order: Vec<(&str, i32)> = rows.iter().map(|r| (r.id.as_str(), r.depth)).collect();
        assert_eq!(order, vec![("f1", 0), ("t1", 1), ("t2", 0)]);
        assert_eq!(rows[0].label, "Board");
    }

    #[test]
    fn slideshow_steps_over_the_images_and_escape_closes_it() {
        let mut c = loaded(&[
            image_item("img1", 0.0, 0.0, 10.0, 10.0),
            image_item("img2", 20.0, 0.0, 10.0, 10.0),
        ]);
        c.run_command("slideshow");
        assert_eq!(c.slideshow().map(|s| s.count), Some(2));
        c.slideshow_step(1);
        assert_eq!(c.slideshow().map(|s| s.index), Some(1));
        let (used, _) = c.key("\u{1b}", NONE);
        assert!(used);
        assert!(c.slideshow().is_none());
    }

    #[test]
    fn presence_has_no_cursor_after_the_pointer_leaves() {
        let mut c = loaded(&[]);
        let p = at(&c, 10.0, 10.0);
        c.pointer(PointerKind::Move, p, Button::None, NONE);
        assert!(c.presence().cursor.is_some());
        c.pointer_left();
        assert!(c.presence().cursor.is_none());
    }

    #[test]
    fn grid_lines_are_empty_when_off_and_drawn_when_on() {
        let mut c = loaded(&[]);
        assert!(c.grid_lines().0.is_empty());
        c.run_command("grid_lines");
        let (xs, ys) = c.grid_lines();
        assert!(!xs.is_empty() && !ys.is_empty());
        assert_eq!(c.grid(), GridMode::Lines);
    }

    /// A plain rectangle, 100 by 100, for the arrow binding tests.
    fn rect_item(id: &str, z: &str, x: f64, y: f64) -> Item {
        Item::new(
            id.to_string(),
            z.to_string(),
            Rect::new(x, y, 100.0, 100.0),
            ItemData::Shape {
                shape: ShapeKind::Rect,
                stroke: "ink".to_string(),
                fill: None,
                sw: 2.0,
                pts: Vec::new(),
                start_bind: None,
                end_bind: None,
                head: false,
            },
        )
    }

    /// The first line or arrow in the document.
    fn connector(c: &Controller) -> Item {
        c.doc
            .ordered()
            .into_iter()
            .find(|i| bind::is_connector(i))
            .cloned()
            .expect("a connector")
    }

    #[test]
    fn arrow_binds_to_the_targets_at_its_press_and_release() {
        let mut c = loaded(&[
            rect_item("r1", "a0", 0.0, 0.0),
            rect_item("r2", "a1", 300.0, 0.0),
        ]);
        c.set_tool(Tool::Arrow);
        let (a, b) = (at(&c, 50.0, 50.0), at(&c, 350.0, 50.0));
        drag(&mut c, a, b, NONE);
        assert_eq!(
            bind_ids(&connector(&c)),
            Some((Some("r1".to_string()), Some("r2".to_string())))
        );
    }

    #[test]
    fn arrow_ending_on_its_start_target_leaves_the_end_free() {
        let mut c = loaded(&[rect_item("r1", "a0", 0.0, 0.0)]);
        c.set_tool(Tool::Arrow);
        let (a, b) = (at(&c, 20.0, 20.0), at(&c, 60.0, 60.0));
        drag(&mut c, a, b, NONE);
        assert_eq!(
            bind_ids(&connector(&c)),
            Some((Some("r1".to_string()), None))
        );
    }

    #[test]
    fn moving_a_bound_item_sends_the_arrow_with_it() {
        let mut c = loaded(&[
            rect_item("r1", "a0", 0.0, 0.0),
            rect_item("r2", "a1", 300.0, 0.0),
        ]);
        c.set_tool(Tool::Arrow);
        let (a, b) = (at(&c, 50.0, 50.0), at(&c, 350.0, 50.0));
        drag(&mut c, a, b, NONE);
        let arrow_id = connector(&c).id;
        c.set_tool(Tool::Select);
        let (from, to) = (at(&c, 350.0, 20.0), at(&c, 350.0, 120.0));
        let fx = drag(&mut c, from, to, NONE);
        let ops = sends(&fx);
        assert!(ops.iter().any(|op| op.item_id == "r2"));
        assert!(ops.iter().any(|op| op.item_id == arrow_id));
        assert_eq!(
            bind_ids(&connector(&c)),
            Some((Some("r1".to_string()), Some("r2".to_string())))
        );
    }

    #[test]
    fn moving_an_arrow_away_unbinds_both_ends() {
        let mut c = loaded(&[
            rect_item("r1", "a0", 0.0, 0.0),
            rect_item("r2", "a1", 300.0, 0.0),
        ]);
        c.set_tool(Tool::Arrow);
        let (a, b) = (at(&c, 50.0, 50.0), at(&c, 350.0, 50.0));
        drag(&mut c, a, b, NONE);
        c.set_tool(Tool::Select);
        // Clear the selection so the arrow's own handles do not take the press.
        let empty = at(&c, 600.0, 500.0);
        click(&mut c, empty, NONE);
        let (from, to) = (at(&c, 200.0, 50.0), at(&c, 200.0, 100.0));
        drag(&mut c, from, to, NONE);
        assert_eq!(bind_ids(&connector(&c)), Some((None, None)));
    }

    #[test]
    fn an_item_inside_a_frame_is_picked_before_the_frame() {
        let frame = Item::new(
            "f1".to_string(),
            "c0".to_string(),
            Rect::new(0.0, 0.0, 200.0, 200.0),
            ItemData::Frame {
                title: "F".to_string(),
            },
        );
        let mut c = loaded(&[text_item("t1", "a0", 50.0, 50.0), frame]);
        let ids: Vec<String> = c.rows().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["f1".to_string(), "t1".to_string()]);
        let p = at(&c, 60.0, 60.0);
        click(&mut c, p, NONE);
        assert_eq!(c.selected_ids(), vec!["t1".to_string()]);
    }

    #[test]
    fn a_frame_title_band_selects_and_drags_the_frame() {
        let frame = Item::new(
            "f1".to_string(),
            "a0".to_string(),
            Rect::new(0.0, 0.0, 200.0, 200.0),
            ItemData::Frame {
                title: "F".to_string(),
            },
        );
        let mut c = loaded(&[frame]);
        let p = at(&c, 50.0, -10.0);
        click(&mut c, p, NONE);
        assert_eq!(c.selected_ids(), vec!["f1".to_string()]);
        let (from, to) = (at(&c, 50.0, -10.0), at(&c, 50.0, 40.0));
        let fx = drag(&mut c, from, to, NONE);
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(prop_f64(ops[0], "y"), Some(50.0));
    }

    #[test]
    fn a_new_frame_is_titled_frame() {
        match blank_data(Tool::Frame) {
            ItemData::Frame { title } => assert_eq!(title, "Frame"),
            _ => panic!("the Frame tool makes a frame"),
        }
    }

    #[test]
    fn the_eraser_flags_what_it_would_delete_and_shows_its_circle() {
        let stroke = Item::new(
            "s1".to_string(),
            "a0".to_string(),
            Rect::new(0.0, 0.0, 100.0, 1.0),
            ItemData::Stroke {
                pts: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)],
                color: "ink".to_string(),
                sw: 2.0,
            },
        );
        let mut c = loaded(&[stroke, text_item("t1", "a1", 300.0, 300.0)]);
        let fx = c.set_tool(Tool::Eraser);
        assert!(fx.iter().any(|e| matches!(e, Effect::Cursor("none"))));
        let p = at(&c, 50.0, 0.0);
        c.pointer(PointerKind::Move, p, Button::None, NONE);
        let rows = c.rows();
        assert!(rows.iter().any(|r| r.id == "s1" && r.erase_hover));
        assert!(rows.iter().any(|r| r.id == "t1" && !r.erase_hover));
        assert_eq!(c.eraser_view(), Some((p.x, p.y, ERASE_PX)));
    }

    #[test]
    fn pop_out_is_ctrl_shift_p() {
        let cmd = COMMANDS
            .iter()
            .find(|c| c.id == "new_window")
            .expect("the command exists");
        assert_eq!(cmd.label, "Pop out");
        let keys = cmd.keys.as_ref().expect("it has keys");
        assert_eq!(
            (keys.key, keys.ctrl, keys.shift, keys.alt),
            ("p", true, true, false)
        );
    }

    fn video_item(id: &str, x: f64, y: f64) -> Item {
        Item::new(
            id.to_string(),
            format!("a{id}"),
            Rect::new(x, y, 160.0, 90.0),
            ItemData::Video {
                asset: format!("asset-{id}"),
                name: format!("{id}.mp4"),
                added: 0,
                volume: 1.0,
                muted: false,
                looping: false,
            },
        )
    }

    #[test]
    fn add_videos_creates_selected_video_items() {
        let mut c = loaded(&[]);
        let mut clip = square_asset("v1");
        clip.media = "Video".to_string();
        clip.file_name = "clip.mp4".to_string();
        let fx = c.add_videos(&[clip], Vec2::new(0.0, 0.0));
        assert_eq!(sends(&fx).len(), 1);
        let row = c
            .rows()
            .into_iter()
            .find(|r| r.asset == "v1")
            .expect("video row");
        assert_eq!(row.kind, ItemKind::Video);
        assert!(!row.muted && !row.looping);
        assert_eq!(c.selected_ids(), vec![row.id.clone()]);
    }

    #[test]
    fn double_click_on_a_video_asks_for_a_toggle() {
        let mut c = loaded(&[video_item("v1", 0.0, 0.0)]);
        let p = at(&c, 50.0, 50.0);
        let fx = c.double_click(p);
        assert!(fx.contains(&Effect::VideoToggle("v1".to_string())));
        assert!(sends(&fx).is_empty());
    }

    #[test]
    fn space_plays_one_selected_video_only() {
        let mut c = loaded(&[video_item("v1", 0.0, 0.0), video_item("v2", 300.0, 0.0)]);
        let (used, _) = c.key(" ", NONE);
        assert!(!used, "nothing selected: Space is not used");
        let p = at(&c, 50.0, 50.0);
        click(&mut c, p, NONE);
        let (used, fx) = c.key(" ", NONE);
        assert!(used);
        assert!(fx.contains(&Effect::VideoToggle("v1".to_string())));
        c.run_command("select_all");
        let (used, _) = c.key(" ", NONE);
        assert!(!used, "two selected: Space is not used");
    }

    #[test]
    fn video_mute_toggles_the_prop_and_sends_one_op() {
        let mut c = loaded(&[video_item("v1", 0.0, 0.0)]);
        let p = at(&c, 50.0, 50.0);
        click(&mut c, p, NONE);
        let fx = c.run_command("video_mute");
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(prop_bool(ops[0], "muted"), Some(true));
        assert!(c.rows().iter().any(|r| r.id == "v1" && r.muted));
        let fx = c.run_command("video_loop");
        let ops = sends(&fx);
        assert_eq!(ops.len(), 1);
        assert_eq!(prop_bool(ops[0], "loop"), Some(true));
    }

    #[test]
    fn set_video_flag_changes_one_video() {
        let mut c = loaded(&[video_item("v1", 0.0, 0.0), video_item("v2", 300.0, 0.0)]);
        let fx = c.set_video_flag("v1", "loop", true);
        assert_eq!(sends(&fx).len(), 1);
        let rows = c.rows();
        assert!(rows.iter().any(|r| r.id == "v1" && r.looping));
        assert!(rows.iter().any(|r| r.id == "v2" && !r.looping));
        assert!(sends(&c.set_video_flag("v1", "loop", true)).is_empty());
    }

    #[test]
    fn video_commands_need_a_video_and_image_only_ones_skip_it() {
        let image = image_item("i1", 400.0, 0.0, 100.0, 100.0);
        let mut c = loaded(&[video_item("v1", 0.0, 0.0), image]);
        let on_image = at(&c, 450.0, 50.0);
        click(&mut c, on_image, NONE);
        assert!(!c.enabled(command("video_mute").expect("command")));
        assert!(!c.enabled(command("video_play").expect("command")));
        let on_video = at(&c, 50.0, 50.0);
        click(&mut c, on_video, NONE);
        assert!(c.enabled(command("video_mute").expect("command")));
        assert!(c.enabled(command("video_play").expect("command")));
        assert!(!c.enabled(command("crop").expect("command")));
        let fx = c.run_command("comment");
        assert!(sends(&fx).is_empty());
        assert!(c.crop_view().is_none());
    }

    #[test]
    fn hierarchy_lists_a_video_by_its_file_name() {
        let c = loaded(&[video_item("v1", 0.0, 0.0)]);
        let row = c
            .hierarchy_rows()
            .into_iter()
            .find(|r| r.id == "v1")
            .expect("hierarchy row");
        assert_eq!(row.label, "v1.mp4");
        assert_eq!(row.kind, "Video");
    }

    #[test]
    fn image_items_include_videos() {
        let c = loaded(&[video_item("v1", 0.0, 0.0)]);
        assert_eq!(
            c.image_items(),
            vec![("v1".to_string(), "asset-v1".to_string())]
        );
    }
}
