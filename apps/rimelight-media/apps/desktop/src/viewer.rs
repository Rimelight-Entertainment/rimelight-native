//! The picture page: open (in the background), browse the folder, zoom, crop, rotate, flip, resize, markup and save.
//! The canvas (Ctrl+N) is a new blank picture with no file: it uses the same markup engine.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use slint::{ComponentHandle as _, Model as _, ModelRc, SharedString, VecModel};

use crate::app::{App, go, post, remember, toast, update_title, with_app};
use crate::files;
use crate::picture;
use crate::ui;
use rimelight_picture::{Mark, Picture};

/// A mark being moved, or one of its box handles dragged to resize it.
#[derive(Clone)]
struct Grab {
    index: usize,
    /// The handle held: -1 left or top, 0 middle, 1 right or bottom, for x and y (None moves the mark).
    handle: Option<(i8, i8)>,
    start: (f32, f32),
    origin: Mark,
    /// Shift was held when the handle was pressed: a corner drag keeps the box's proportions.
    keep_ratio: bool,
}

/// The handle of a mark's box under (x, y), if any: the 4 corners and the 4 edge middles. An axis the box has no
/// extent along (a straight horizontal or vertical line) has no handles across it.
fn handle_at(m: &Mark, x: f32, y: f32, reach: f32) -> Option<(i8, i8)> {
    if m.points.is_empty() {
        return None;
    }
    let (bx, by, bw, bh) = m.bounds();
    let mut best = None;
    let mut best_d = reach;
    for hx in -1i8..=1 {
        for hy in -1i8..=1 {
            if (hx == 0 && hy == 0) || (hx != 0 && bw < 1.0) || (hy != 0 && bh < 1.0) {
                continue;
            }
            let px = bx + bw * (hx + 1) as f32 / 2.0;
            let py = by + bh * (hy + 1) as f32 / 2.0;
            let d = (px - x).hypot(py - y);
            if d <= best_d {
                best_d = d;
                best = Some((hx, hy));
            }
        }
    }
    best
}

/// The mark resized by dragging its handle (hx, hy) by (dx, dy) picture pixels: the opposite side stays put and every
/// point scales within the box. Strokes and shapes keep their stroke width; text scales its font size instead.
/// With `keep` (Shift held) a corner drag keeps the box's own proportions: the height follows the width.
fn resized(origin: &Mark, hx: i8, hy: i8, dx: f32, dy: f32, keep: bool) -> Mark {
    let (x0, y0, w, h) = origin.bounds();
    let (mut nl, mut nr, mut nt, mut nb) = (x0, x0 + w, y0, y0 + h);
    match hx {
        -1 => nl = (nl + dx).min(nr - 1.0),
        1 => nr = (nr + dx).max(nl + 1.0),
        _ => {}
    }
    match hy {
        -1 => nt = (nt + dy).min(nb - 1.0),
        1 => nb = (nb + dy).max(nt + 1.0),
        _ => {}
    }
    if keep && hx != 0 && hy != 0 && w >= 1.0 && h >= 1.0 {
        // The opposite corner stays put; the dragged side moves to the height the width gives at the original ratio.
        let nh = (nr - nl) * h / w;
        if hy == 1 {
            nb = nt + nh;
        } else {
            nt = nb - nh;
        }
    }
    let (nw, nh) = (nr - nl, nb - nt);
    let mut m = origin.clone();
    if origin.kind == 4 {
        // Text: the font scales with the box (height for an edge, the mean of both for a corner).
        let (rx, ry) = (
            if w >= 1.0 { nw / w } else { 1.0 },
            if h >= 1.0 { nh / h } else { 1.0 },
        );
        let f = match (hx, hy) {
            (0, _) => ry,
            (_, 0) => rx,
            _ => (rx + ry) / 2.0,
        }
        .max(0.05);
        m.width = origin.width * f;
        if let Some(p) = m.points.first_mut() {
            *p = (
                if hx == -1 { nl } else { x0 },
                if hy == -1 { nt } else { y0 },
            );
        }
        return m;
    }
    let map_x = |px: f32| {
        if w >= 1.0 {
            nl + (px - x0) / w * nw
        } else {
            px + (nl - x0)
        }
    };
    let map_y = |py: f32| {
        if h >= 1.0 {
            nt + (py - y0) / h * nh
        } else {
            py + (nt - y0)
        }
    };
    m.points = origin
        .points
        .iter()
        .map(|&(px, py)| (map_x(px), map_y(py)))
        .collect();
    m
}

/// The action waiting for the answer to the in-app question about unsaved canvas changes.
type Then = Box<dyn FnOnce(&Rc<App>)>;

#[derive(Default)]
pub struct ViewState {
    pub path: Option<PathBuf>,
    /// What Revert goes back to, kept only when it isn't already held elsewhere: while it is None, the picture as opened
    /// is the oldest Undo step (history[0]), or `pic` itself when there is no Undo step. It is set when the Undo cap drops
    /// that oldest step (push_history), or for an animation frozen on a frame other than the first (freeze_anim). So an
    /// opened picture is held once, not twice.
    original: Option<Picture>,
    pic: Option<Picture>,
    folder: Vec<PathBuf>,
    index: usize,
    marks: Vec<Mark>,
    /// The marks before each markup change (Undo) and the ones taken back (Redo), newest last.
    marks_undo: Vec<Vec<Mark>>,
    marks_redo: Vec<Vec<Mark>>,
    drawing: Option<Mark>,
    text_at: Option<(f32, f32)>,
    generation: u64,
    file_size: u64,
    /// Frames of an animated GIF or WebP, their pictures for the screen (made on first show) and the one showing.
    anim: Option<Rc<rimelight_picture::Animation>>,
    anim_images: Vec<Option<slint::Image>>,
    anim_frame: usize,
    anim_playing: bool,
    anim_timer: slint::Timer,
    /// Earlier versions of the picture (Undo) and undone ones (Redo), newest last.
    history: Vec<Picture>,
    future: Vec<Picture>,
    /// Filmstrip: its thumbnails (for the folder in thumbs_dir) and the pictures picked to compare.
    thumbs: Option<Rc<VecModel<ui::Thumb>>>,
    thumbs_dir: Option<PathBuf>,
    thumbs_gen: u64,
    picked: Vec<usize>,
    /// Canvas: a new document with no file, and its background colour.
    pub canvas: bool,
    canvas_bg: [u8; 4],
    /// The rectangle selection (picture pixels: x, y, width, height), and a rectangle being dragged out.
    sel: Option<(f32, f32, f32, f32)>,
    selecting: Option<(f32, f32)>,
    /// The mark picked for moving or resizing, and the drag in progress.
    selected: Option<usize>,
    grab: Option<Grab>,
    /// The hand tool's drag on empty space: the press (picture space) and the pan it started from.
    pan_grab: Option<(f32, f32, f32, f32)>,
    /// Up to six recent colours, newest first.
    recent: Vec<[u8; 4]>,
    /// Slint images of the picture marks (made once per picture, kept while the mark exists).
    mark_images: Vec<(Arc<Picture>, slint::Image)>,
    /// What to do after the in-app question about unsaved canvas changes.
    pending: Option<Then>,
    /// The slideshow: running (on) and the timer that moves on to the next picture every SLIDESHOW_SECS.
    slideshow: bool,
    slideshow_timer: slint::Timer,
}

/// Seconds each picture stays on screen in the slideshow.
const SLIDESHOW_SECS: u64 = 4;

/// Picture edits kept for Undo and Redo, capped by their total size (a large photo or canvas is tens of MB per step).
const HISTORY_BYTES: usize = 256 * 1024 * 1024;
/// Marks undo steps are small (vectors), so they are capped by count.
const MAX_MARKS_HISTORY: usize = 100;

/// Adds a picture to an undo or redo stack, dropping the oldest steps while the stack is over HISTORY_BYTES.
fn keep_picture(stack: &mut Vec<Picture>, pic: Picture) {
    stack.push(pic);
    let mut total: usize = stack.iter().map(|p| p.rgba.len()).sum();
    while total > HISTORY_BYTES && !stack.is_empty() {
        total -= stack.remove(0).rgba.len();
    }
}

/// Adds a picture to the Undo stack, like keep_picture. The first step the cap drops while `original` is empty is the
/// picture as opened: it is moved into `original` for Revert (no copy).
fn push_history(s: &mut ViewState, pic: Picture) {
    s.history.push(pic);
    let mut total: usize = s.history.iter().map(|p| p.rgba.len()).sum();
    while total > HISTORY_BYTES && !s.history.is_empty() {
        let old = s.history.remove(0);
        total -= old.rgba.len();
        if s.original.is_none() {
            s.original = Some(old);
        }
    }
}
/// Paint bucket: how far a pixel's colour may differ from the seed's (per channel).
const FILL_TOLERANCE: u8 = 16;

fn viewer(app: &Rc<App>) -> ui::Viewer<'_> {
    app.ui.global::<ui::Viewer>()
}

pub fn wire(app: &Rc<App>) {
    let v = viewer(app);
    v.on_prev(|| with_app(|a| step(a, -1)));
    v.on_next(|| with_app(|a| step(a, 1)));
    v.on_rotate(|| with_app(|a| edit(a, rimelight_picture::rotate_cw)));
    v.on_flip(|lr| with_app(move |a| edit(a, move |p| rimelight_picture::flip(p, lr))));
    v.on_set_mode(|m| with_app(|a| set_mode(a, &m)));
    v.on_apply_crop(|| {
        with_app(|a| {
            let v = viewer(a);
            let (x, y, w, h) = (
                v.get_crop_x(),
                v.get_crop_y(),
                v.get_crop_w(),
                v.get_crop_h(),
            );
            edit(a, move |p| rimelight_picture::crop(p, x, y, w, h));
            set_mode(a, "");
        })
    });
    v.on_crop_preset(|aspect| with_app(move |a| crop_preset(a, aspect)));
    v.on_resize_edited(|width_changed| with_app(move |a| resize_edited(a, width_changed)));
    v.on_apply_resize(|| {
        with_app(|a| {
            let v = viewer(a);
            let (Ok(w), Ok(h)) = (
                v.get_resize_w().trim().parse::<u32>(),
                v.get_resize_h().trim().parse::<u32>(),
            ) else {
                toast(a, "Type a width and a height in pixels.");
                return;
            };
            if w == 0 || h == 0 || w > 30_000 || h > 30_000 {
                toast(a, "Width and height must be between 1 and 30000 pixels.");
                return;
            }
            edit(a, move |p| rimelight_picture::resize(p, w, h));
            set_mode(a, "");
        })
    });
    v.on_mark_begin(|x, y| with_app(move |a| mark_begin(a, x, y)));
    v.on_mark_move(|x, y| with_app(move |a| mark_move(a, x, y)));
    v.on_mark_end(|| with_app(mark_end));
    v.on_commit_text(|| with_app(commit_text));
    v.on_undo(|| with_app(undo));
    v.on_redo(|| with_app(redo));
    v.on_print(|| with_app(print));
    v.on_share(|| with_app(share));
    v.on_toggle_filmstrip(|| with_app(toggle_filmstrip));
    v.on_toggle_slideshow(|| with_app(slideshow_toggle));
    v.on_thumb_clicked(|i, add| with_app(move |a| thumb_clicked(a, i as usize, add)));
    v.on_close_compare(|| with_app(close_compare));
    v.on_clear_marks(|| {
        with_app(|a| {
            let mut s = a.view.borrow_mut();
            if !s.marks.is_empty() {
                snap_marks(&mut s);
                s.marks.clear();
            }
            s.selected = None;
            s.grab = None;
            drop(s);
            refresh_marks(a);
        })
    });
    v.on_apply_marks(|| {
        with_app(|a| {
            let marks = std::mem::take(&mut a.view.borrow_mut().marks);
            if !marks.is_empty() {
                let on_canvas = a.view.borrow().canvas;
                edit(a, move |p| {
                    rimelight_picture::burn_marks(p, &marks, on_canvas)
                        .unwrap_or_else(|_| p.clone())
                });
            }
            set_mode(a, "");
        })
    });
    v.on_set_tool(|t| with_app(move |a| set_tool(a, t)));
    v.on_pick_colour(|c| with_app(move |a| pick_colour(a, [c.red(), c.green(), c.blue(), 255])));
    v.on_pick_hex(|t: SharedString| with_app(move |a| pick_hex(a, &t)));
    v.on_picker_sync(|| with_app(|a| picker_sync(a)));
    v.on_picker_changed(|| with_app(|a| picker_show(a)));
    v.on_picker_use(|| with_app(|a| picker_use(a)));
    v.on_picker_field(|i, t: SharedString| {
        with_app(move |a| picker_field(a, i.max(0) as usize, &t))
    });
    v.on_canvas_resize(|w: SharedString, h: SharedString, transparent: bool| {
        with_app(move |a| canvas_resize_typed(a, &w, &h, transparent));
    });
    v.on_canvas_preset(|p| with_app(move |a| canvas_preset(a, p)));
    v.on_crop_selection(|| with_app(crop_selection));
    v.on_cut_selection(|| with_app(|a| remove_selection(a, true)));
    v.on_delete_selection(|| with_app(|a| remove_selection(a, false)));
    v.on_save(|| with_app(save));
    v.on_save_as(|| with_app(save_as));
    v.on_copy(|| with_app(copy));
    v.on_revert(|| {
        with_app(|a| {
            // The X on a canvas closes it (after the in-app question when it has changes); a picture reverts its edits.
            if a.view.borrow().canvas {
                settle_canvas(a, close);
                return;
            }
            // The picture as opened: kept aside, else the oldest Undo step, else the picture showing (see `original`).
            let restored = {
                let mut s = a.view.borrow_mut();
                let orig = s
                    .original
                    .take()
                    .or_else(|| (!s.history.is_empty()).then(|| s.history.remove(0)));
                if let Some(p) = orig {
                    s.pic = Some(p);
                }
                if s.pic.is_some() {
                    clear_markup(&mut s);
                    s.history.clear();
                    s.future.clear();
                }
                s.pic.is_some()
            };
            if restored {
                show_picture(a, false);
                viewer(a).set_dirty(false);
                refresh_undo(a);
                refresh_marks(a);
            }
        })
    });
    v.on_toggle_info(|| with_app(|a| viewer(a).set_show_info(!viewer(a).get_show_info())));
    v.on_anim_toggle(|| with_app(anim_toggle));
    v.on_anim_step(|d| with_app(move |a| anim_step(a, d)));
    v.on_save_frame(|| with_app(save_frame));
    v.on_delete_current(|| with_app(delete_current));
    v.on_open_folder(|| {
        with_app(|a| {
            if let Some(p) = a.view.borrow().path.clone() {
                files::reveal(&p);
            }
        })
    });
    app.ui
        .global::<ui::Shell>()
        .on_new_canvas(|preset, w, h, transparent| {
            with_app(move |a| new_canvas(a, preset, &w, &h, transparent));
        });
}

/// Opens a picture file: the folder list is built at once, the pixels arrive from a worker thread.
pub fn open(app: &Rc<App>, path: &Path) {
    open_with(app, path, None);
}

/// Opens several pictures at once: the filmstrip shows only them, in the order given.
pub fn open_set(app: &Rc<App>, paths: &[PathBuf]) {
    if let Some(first) = paths.first() {
        open_with(app, first, Some(paths.to_vec()));
    }
}

/// Forgets the markup in progress and the marks (the picture itself stays).
/// Keeps the marks as they are now, as one Undo step (a new change clears Redo).
fn snap_marks(s: &mut ViewState) {
    let before = s.marks.clone();
    push_marks_undo(s, before);
}

/// Adds an Undo step holding the marks as they were before a change.
fn push_marks_undo(s: &mut ViewState, before: Vec<Mark>) {
    s.marks_undo.push(before);
    if s.marks_undo.len() > MAX_MARKS_HISTORY {
        s.marks_undo.remove(0);
    }
    s.marks_redo.clear();
}

fn clear_markup(s: &mut ViewState) {
    s.marks.clear();
    s.marks_undo.clear();
    s.marks_redo.clear();
    s.drawing = None;
    s.text_at = None;
    s.sel = None;
    s.selecting = None;
    s.selected = None;
    s.grab = None;
    s.mark_images.clear();
}

/// A canvas with changes not saved yet.
pub fn canvas_unsaved(app: &Rc<App>) -> bool {
    let s = app.view.borrow();
    s.canvas && (viewer(app).get_dirty() || !s.marks.is_empty() || s.drawing.is_some())
}

fn open_with(app: &Rc<App>, path: &Path, set: Option<Vec<PathBuf>>) {
    if canvas_unsaved(app) {
        let (p, set) = (path.to_path_buf(), set);
        settle_canvas(app, move |a| open_with(a, &p, set));
        return;
    }
    let folder = set.unwrap_or_else(|| files::sibling_pictures(path));
    let index = folder.iter().position(|p| p == path).unwrap_or(0);
    let generation = {
        let mut s = app.view.borrow_mut();
        if s.folder != folder {
            // A new list of pictures: the filmstrip makes its thumbnails again.
            s.thumbs = None;
            s.thumbs_dir = None;
            s.thumbs_gen += 1;
            s.picked.clear();
        }
        s.generation += 1;
        s.canvas = false;
        s.path = Some(path.to_path_buf());
        s.folder = folder;
        s.index = index;
        s.pic = None;
        s.original = None;
        clear_markup(&mut s);
        s.file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        s.history.clear();
        s.future.clear();
        stop_anim(&mut s);
        s.generation
    };
    viewer(app).set_animated(false);
    viewer(app).set_canvas(false);
    let v = viewer(app);
    v.set_loading(true);
    v.set_error(SharedString::new());
    v.set_mode(SharedString::new());
    v.set_dirty(false);
    v.set_fit(true);
    v.set_pan_x(0.0);
    v.set_pan_y(0.0);
    v.set_info_name(files::file_name(path).into());
    update_position(app);
    refresh_marks(app);
    go(app, "image");
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        let animation = rimelight_picture::load_animation(&p);
        let result = match &animation {
            Some(anim) => Ok(anim.frames[0].0.clone()),
            None => rimelight_picture::load(&p),
        };
        post(move |a| {
            if a.view.borrow().generation != generation {
                return;
            }
            viewer(a).set_loading(false);
            match result {
                Ok(pic) => {
                    {
                        let mut s = a.view.borrow_mut();
                        s.original = None;
                        s.pic = Some(pic);
                    }
                    show_picture(a, true);
                    if let Some(anim) = animation {
                        start_anim(a, anim);
                    }
                }
                Err(e) => {
                    viewer(a).set_picture(slint::Image::default());
                    viewer(a).set_error(format!("This picture can't be shown.\n{e}").into());
                }
            }
        });
    });
}

/// Shows a picture that isn't a file yet (a fresh snip). `path` is where it was saved, if it was.
pub fn open_picture(app: &Rc<App>, pic: Picture, path: Option<PathBuf>) {
    if canvas_unsaved(app) {
        settle_canvas(app, move |a| open_picture(a, pic, path));
        return;
    }
    {
        let mut s = app.view.borrow_mut();
        s.generation += 1;
        s.canvas = false;
        s.folder = path
            .as_ref()
            .map(|p| files::sibling_pictures(p))
            .unwrap_or_default();
        s.index = path
            .as_ref()
            .and_then(|p| s.folder.iter().position(|f| f == p))
            .unwrap_or(0);
        s.file_size = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        s.path = path.clone();
        s.original = None;
        s.pic = Some(pic);
        clear_markup(&mut s);
        s.history.clear();
        s.future.clear();
    }
    let v = viewer(app);
    v.set_canvas(false);
    v.set_loading(false);
    v.set_error(SharedString::new());
    v.set_mode(SharedString::new());
    v.set_dirty(path.is_none());
    v.set_fit(true);
    v.set_info_name(
        path.as_deref()
            .map(files::file_name)
            .unwrap_or_else(|| "Snip".into())
            .into(),
    );
    update_position(app);
    refresh_marks(app);
    show_picture(app, true);
    go(app, "image");
    if let Some(p) = &path {
        remember(app, p);
    }
}

fn show_picture(app: &Rc<App>, fresh: bool) {
    let s = app.view.borrow();
    let Some(pic) = s.pic.as_ref() else { return };
    let v = viewer(app);
    v.set_picture(picture::to_slint(pic));
    v.set_pic_w(pic.w as i32);
    v.set_pic_h(pic.h as i32);
    if fresh {
        v.set_fit(true);
    }
    let file_type = s
        .path
        .as_deref()
        .map(|p| files::ext_of(p).to_uppercase())
        .unwrap_or_else(|| "Not saved".into());
    let dims = format!("{} x {}", pic.w, pic.h);
    let size = if s.path.is_some() && s.file_size > 0 {
        files::human_size(s.file_size)
    } else {
        String::new()
    };
    let facts: Vec<String> = [file_type.clone(), dims.clone(), size.clone()]
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect();
    v.set_facts_text(facts.join("  |  ").into());
    v.set_dims_text(dims.into());
    v.set_file_type(file_type.into());
    v.set_size_text(size.into());
    v.set_can_share(s.path.as_ref().is_some_and(|p| p.exists()));
    let mut detail = format!("{} × {} pixels", pic.w, pic.h);
    if let Some(p) = &s.path {
        if s.file_size > 0 {
            detail.push_str(&format!("\n{}", files::human_size(s.file_size)));
        }
        detail.push_str(&format!("\n{}", files::ext_of(p).to_uppercase()));
        if let Ok(modified) = std::fs::metadata(p).and_then(|m| m.modified())
            && let Ok(age) = modified.elapsed()
        {
            let days = age.as_secs() / 86_400;
            detail.push_str(&match days {
                0 => "\nChanged today".to_string(),
                1 => "\nChanged yesterday".to_string(),
                d => format!("\nChanged {d} days ago"),
            });
        }
        detail.push_str(&format!("\n\n{}", files::folder_name(p)));
    } else {
        detail.push_str("\nNot saved yet");
    }
    v.set_info_detail(detail.into());
    drop(s);
    update_title(app);
}

fn update_position(app: &Rc<App>) {
    sync_thumbs(app);
    let s = app.view.borrow();
    let v = viewer(app);
    let n = s.folder.len();
    v.set_position(if n > 1 {
        format!("{} / {n}", s.index + 1).into()
    } else {
        SharedString::new()
    });
    v.set_has_prev(n > 1 && s.index > 0);
    v.set_has_next(n > 1 && s.index + 1 < n);
}

pub fn step(app: &Rc<App>, delta: i32) {
    let next = {
        let s = app.view.borrow();
        let i = s.index as i64 + i64::from(delta);
        (i >= 0 && (i as usize) < s.folder.len()).then(|| s.folder[i as usize].clone())
    };
    if viewer(app).get_dirty() {
        toast(app, "Changes not saved: save them, or undo them, first.");
        return;
    }
    if let Some(p) = next {
        open(app, &p);
    }
}

fn edit(app: &Rc<App>, f: impl FnOnce(&Picture) -> Picture) {
    if app.view.borrow().anim.is_some() {
        freeze_anim(app);
        toast(app, "Editing keeps only this frame, as a still picture.");
    }
    let changed = {
        let mut s = app.view.borrow_mut();
        match s.pic.take() {
            Some(p) => {
                s.pic = Some(f(&p));
                push_history(&mut s, p);
                s.future.clear();
                true
            }
            None => false,
        }
    };
    if changed {
        // A changed picture ends the slideshow (the next picture would drop the edit).
        slideshow_stop(app);
        viewer(app).set_dirty(true);
        show_picture(app, false);
        refresh_undo(app);
    }
}

/// Starts the slideshow, or stops it when it runs (the Slideshow button and the S key). It needs a folder of at least
/// two pictures and no unsaved changes.
pub fn slideshow_toggle(app: &Rc<App>) {
    if app.view.borrow().slideshow {
        slideshow_stop(app);
        return;
    }
    let (canvas, count) = {
        let s = app.view.borrow();
        (s.canvas, s.folder.len())
    };
    if canvas || count < 2 {
        toast(
            app,
            "The slideshow needs a folder with two or more pictures.",
        );
        return;
    }
    if viewer(app).get_dirty() || !viewer(app).get_mode().is_empty() {
        toast(
            app,
            "Finish or undo the change first, then start the slideshow.",
        );
        return;
    }
    {
        let mut s = app.view.borrow_mut();
        s.slideshow = true;
        s.slideshow_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(SLIDESHOW_SECS),
            || with_app(slideshow_tick),
        );
    }
    viewer(app).set_slideshow_on(true);
}

/// Ends the slideshow. Safe to call when it is not running.
pub fn slideshow_stop(app: &Rc<App>) {
    {
        let mut s = app.view.borrow_mut();
        s.slideshow = false;
        s.slideshow_timer.stop();
    }
    viewer(app).set_slideshow_on(false);
}

/// One slideshow step: the next picture in the folder, wrapping from the last back to the first. It stops instead when
/// the Image tab is no longer shown, a canvas or markup/crop/resize is open, or the picture has unsaved changes.
fn slideshow_tick(app: &Rc<App>) {
    if !app.view.borrow().slideshow {
        return;
    }
    // The tick also runs on other tabs: it must not open a picture there (open() switches to the Image tab).
    if app.ui.global::<ui::Shell>().get_page() != "image" {
        slideshow_stop(app);
        return;
    }
    let next = {
        let s = app.view.borrow();
        let n = s.folder.len();
        (!s.canvas && n >= 2).then(|| s.folder[(s.index + 1) % n].clone())
    };
    let v = viewer(app);
    match next {
        Some(p) if !v.get_dirty() && v.get_mode().is_empty() => open(app, &p),
        _ => slideshow_stop(app),
    }
}

pub fn set_mode(app: &Rc<App>, mode: &str) {
    // An open text box is committed on a mode switch, so it is a normal mark from then on.
    commit_text(app);
    if !mode.is_empty() {
        // Crop, resize and markup stop the slideshow: its next picture would drop the edit in progress.
        slideshow_stop(app);
    }
    let v = viewer(app);
    clear_markup(&mut app.view.borrow_mut());
    v.set_text_x(-1.0);
    v.set_text_y(-1.0);
    match mode {
        "crop" => {
            v.set_crop_aspect(0.0);
            v.set_crop_x(0.0);
            v.set_crop_y(0.0);
            v.set_crop_w(1.0);
            v.set_crop_h(1.0);
        }
        "resize" => {
            v.set_resize_w(v.get_pic_w().to_string().into());
            v.set_resize_h(v.get_pic_h().to_string().into());
        }
        _ => {}
    }
    v.set_mode(mode.into());
    refresh_marks(app);
}

fn crop_preset(app: &Rc<App>, aspect: f32) {
    let v = viewer(app);
    let (w, h) = (v.get_pic_w() as f32, v.get_pic_h() as f32);
    let aspect = if aspect < 0.0 { w / h } else { aspect };
    v.set_crop_aspect(aspect);
    if aspect <= 0.0 {
        return;
    }
    let pic_aspect = w / h;
    let (fw, fh) = if aspect > pic_aspect {
        (1.0, pic_aspect / aspect)
    } else {
        (aspect / pic_aspect, 1.0)
    };
    v.set_crop_w(fw);
    v.set_crop_h(fh);
    v.set_crop_x((1.0 - fw) / 2.0);
    v.set_crop_y((1.0 - fh) / 2.0);
}

fn resize_edited(app: &Rc<App>, width_changed: bool) {
    let v = viewer(app);
    if !v.get_resize_keep() {
        return;
    }
    let (pw, ph) = (v.get_pic_w() as f64, v.get_pic_h() as f64);
    if width_changed {
        if let Ok(w) = v.get_resize_w().trim().parse::<f64>() {
            v.set_resize_h(((w * ph / pw).round().max(1.0) as u32).to_string().into());
        }
    } else if let Ok(h) = v.get_resize_h().trim().parse::<f64>() {
        v.set_resize_w(((h * pw / ph).round().max(1.0) as u32).to_string().into());
    }
}

fn ink(app: &Rc<App>) -> [u8; 4] {
    let c = viewer(app).get_ink();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

/// Sets the ink and keeps the colour in the recent list.
fn pick_colour(app: &Rc<App>, rgba: [u8; 4]) {
    viewer(app).set_ink(slint::Color::from_argb_u8(
        rgba[3], rgba[0], rgba[1], rgba[2],
    ));
    remember_colour(app, rgba);
}

/// The custom colour box: "#RRGGBB" (the # is optional).
fn pick_hex(app: &Rc<App>, text: &str) {
    let t = text.trim().trim_start_matches('#');
    let parsed = if t.len() == 6 {
        u32::from_str_radix(t, 16).ok()
    } else {
        None
    };
    match parsed {
        Some(v) => {
            pick_colour(app, [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
            picker_sync(app);
        }
        None => toast(app, "Type a colour as six hex digits, like #3FD6B8."),
    }
}

/// HSV to RGB: hue in degrees, saturation and value from 0 to 1.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let hp = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to(r), to(g), to(b)]
}

/// RGB to HSV: hue in degrees (0 to 360), saturation and value from 0 to 1.
fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// Opens the picker from the ink: its hue, saturation and value follow the current colour.
fn picker_sync(app: &Rc<App>) {
    let c = viewer(app).get_ink();
    let (h, s, v) = rgb_to_hsv([c.red(), c.green(), c.blue()]);
    let pv = viewer(app);
    pv.set_picker_h(h);
    pv.set_picker_s(s);
    pv.set_picker_v(v);
    picker_show(app);
}

/// Shows the picker's preview, hue bar, hex box and the fields of its colour model.
fn picker_show(app: &Rc<App>) {
    picker_show_skip(app, None);
}

/// As picker_show, but the field `skip` (the one being typed in) keeps its text.
fn picker_show_skip(app: &Rc<App>, skip: Option<usize>) {
    let v = viewer(app);
    let rgb = hsv_to_rgb(v.get_picker_h(), v.get_picker_s(), v.get_picker_v());
    let [r, g, b] = rgb;
    let [hr, hg, hb] = hsv_to_rgb(v.get_picker_h(), 1.0, 1.0);
    v.set_picker_rgb(slint::Color::from_argb_u8(255, r, g, b));
    v.set_picker_hue(slint::Color::from_argb_u8(255, hr, hg, hb));
    v.set_picker_hex(format!("#{r:02X}{g:02X}{b:02X}").into());
    let model = v.get_picker_model();
    let fields = model_fields(model, rgb);
    for (i, f) in fields.into_iter().enumerate() {
        if skip == Some(i) {
            continue;
        }
        let text: SharedString = format_field(model, i, f).into();
        match i {
            0 => v.set_picker_a(text),
            1 => v.set_picker_b(text),
            _ => v.set_picker_c(text),
        }
    }
}

/// A typed value in one of the picker's fields (0, 1 or 2 of the colour model): the colour and the square follow.
fn picker_field(app: &Rc<App>, idx: usize, text: &str) {
    if idx > 2 {
        return;
    }
    let Ok(x) = text.trim().replace(',', ".").parse::<f32>() else {
        return;
    };
    let v = viewer(app);
    let model = v.get_picker_model();
    let rgb = hsv_to_rgb(v.get_picker_h(), v.get_picker_s(), v.get_picker_v());
    let mut fields = model_fields(model, rgb);
    fields[idx] = x;
    let [r, g, b] = fields_rgb(model, fields);
    let (h, s, val) = rgb_to_hsv([r, g, b]);
    v.set_picker_h(h);
    v.set_picker_s(s);
    v.set_picker_v(val);
    picker_show_skip(app, Some(idx));
}

/// The three fields of a colour model: RGB (0 to 255), HSL (H degrees, S and L 0 to 100), OKLCH (L 0 to 1, C 0 to
/// 0.4, H degrees).
fn model_fields(model: i32, rgb: [u8; 3]) -> [f32; 3] {
    match model {
        1 => {
            let (h, s, l) = rgb_to_hsl(rgb);
            [h, s * 100.0, l * 100.0]
        }
        2 => rgb_to_oklch(rgb),
        _ => rgb.map(f32::from),
    }
}

fn fields_rgb(model: i32, f: [f32; 3]) -> [u8; 3] {
    match model {
        1 => hsl_to_rgb(
            f[0],
            (f[1] / 100.0).clamp(0.0, 1.0),
            (f[2] / 100.0).clamp(0.0, 1.0),
        ),
        2 => oklch_to_rgb(f[0].clamp(0.0, 1.0), f[1].clamp(0.0, 0.4), f[2]),
        _ => f.map(|c| c.round().clamp(0.0, 255.0) as u8),
    }
}

fn format_field(model: i32, i: usize, v: f32) -> String {
    match (model, i) {
        (2, 0) | (2, 1) => format!("{v:.3}"),
        _ => format!("{v:.0}"),
    }
}

fn to_u8(f: f32) -> u8 {
    (f * 255.0).round().clamp(0.0, 255.0) as u8
}

/// RGB to HSL: hue in degrees, saturation and lightness from 0 to 1.
fn rgb_to_hsl(rgb: [u8; 3]) -> (f32, f32, f32) {
    let (h, _, _) = rgb_to_hsv(rgb);
    let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    (h, s, l)
}

/// HSL to RGB: hue in degrees, saturation and lightness from 0 to 1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [to_u8(r + m), to_u8(g + m), to_u8(b + m)]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// RGB to OKLCH (Ottosson's OKLab): L from 0 to 1, chroma about 0 to 0.4, hue in degrees.
fn rgb_to_oklch(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| srgb_to_linear(f32::from(c) / 255.0));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    let chroma = (a * a + bb * bb).sqrt();
    let hue = if chroma < 1e-4 {
        0.0
    } else {
        bb.atan2(a).to_degrees().rem_euclid(360.0)
    };
    [lightness, chroma, hue]
}

/// OKLCH to RGB (out-of-gamut colours are clipped).
fn oklch_to_rgb(lightness: f32, chroma: f32, hue: f32) -> [u8; 3] {
    let (sin, cos) = hue.to_radians().sin_cos();
    let (a, b) = (chroma * cos, chroma * sin);
    let l = lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m = lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s = lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let bl = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
    [r, g, bl].map(|v| to_u8(linear_to_srgb(v.clamp(0.0, 1.0))))
}

/// The canvas resized to w x h, with the background for the new area (the top bar's size controls).
fn canvas_resize(app: &Rc<App>, w: u32, h: u32, bg: [u8; 4]) {
    if !app.view.borrow().canvas {
        return;
    }
    app.view.borrow_mut().canvas_bg = bg;
    viewer(app).set_canvas_transparent(bg[3] == 0);
    edit(app, move |p| rimelight_picture::canvas_fit(p, w, h, bg));
}

/// The size fields (typed) and the background of the canvas size flyout.
fn canvas_resize_typed(app: &Rc<App>, w: &str, h: &str, transparent: bool) {
    let parsed = w
        .trim()
        .parse::<u32>()
        .ok()
        .zip(h.trim().parse::<u32>().ok());
    let Some((w, h)) = parsed.filter(|(w, h)| (1..=10_000).contains(w) && (1..=10_000).contains(h))
    else {
        toast(app, "Type a width and a height from 1 to 10000 pixels.");
        return;
    };
    canvas_resize(
        app,
        w,
        h,
        if transparent {
            [0, 0, 0, 0]
        } else {
            [255, 255, 255, 255]
        },
    );
}

/// A size preset of the canvas flyout (0 window, 1 1920 x 1080, 2 A4), keeping the background.
fn canvas_preset(app: &Rc<App>, preset: i32) {
    let Some((w, h)) = canvas_size(app, preset, "", "") else {
        return;
    };
    let bg = app.view.borrow().canvas_bg;
    canvas_resize(app, w, h, bg);
}

/// The picker's Use button: its colour becomes the ink and joins the recent colours.
fn picker_use(app: &Rc<App>) {
    let v = viewer(app);
    let [r, g, b] = hsv_to_rgb(v.get_picker_h(), v.get_picker_s(), v.get_picker_v());
    pick_colour(app, [r, g, b, 255]);
}

fn remember_colour(app: &Rc<App>, rgba: [u8; 4]) {
    {
        let mut s = app.view.borrow_mut();
        s.recent.retain(|c| *c != rgba);
        s.recent.insert(0, rgba);
        s.recent.truncate(6);
    }
    let colours: Vec<slint::Color> = app
        .view
        .borrow()
        .recent
        .iter()
        .map(|c| slint::Color::from_argb_u8(c[3], c[0], c[1], c[2]))
        .collect();
    viewer(app).set_recent_colours(ModelRc::new(VecModel::from(colours)));
}

/// Picks a markup tool. Picking another tool lets go of the picked mark (and of the selection, except for the
/// rectangle select tool itself).
fn set_tool(app: &Rc<App>, tool: i32) {
    viewer(app).set_tool(tool);
    {
        let mut s = app.view.borrow_mut();
        s.selected = None;
        s.grab = None;
        s.selecting = None;
        if tool != 10 {
            s.sel = None;
        }
    }
    refresh_marks(app);
}

fn rect_of(a: (f32, f32), b: (f32, f32)) -> (f32, f32, f32, f32) {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

fn mark_begin(app: &Rc<App>, x: f32, y: f32) {
    let v = viewer(app);
    let tool = v.get_tool();
    let width = v.get_ink_width();
    // Marks scale with the picture so they read the same on a small and a large picture.
    let scale = (v.get_pic_w().max(v.get_pic_h()) as f32 / 1600.0).max(1.0);
    let reach = 16.0 * scale;

    // A picked mark (a picture, or one the move tool picked) is dragged from anywhere on it; its box handles resize it.
    // The select tool only drags out a rectangle, so it never picks a mark.
    let picked = if tool == 10 {
        None
    } else {
        let s = app.view.borrow();
        s.selected
            .and_then(|i| s.marks.get(i).map(|m| (i, m.clone())))
            .and_then(|(i, m)| {
                let handle = handle_at(&m, x, y, reach);
                (handle.is_some() || m.distance(x, y) <= reach).then_some((i, handle, m))
            })
    };
    if let Some((index, handle, origin)) = picked {
        app.view.borrow_mut().grab = Some(Grab {
            index,
            handle,
            start: (x, y),
            origin,
            keep_ratio: v.get_mark_shift(),
        });
        return;
    }

    match tool {
        11 => {
            // The move tool picks the nearest mark and drags it. On empty space the drag pans the view instead; the
            // view leaves Fit at the same scale first (as the middle button does), so the pan starts where it is shown.
            let hit = {
                let s = app.view.borrow();
                s.marks
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (i, m.distance(x, y)))
                    .filter(|(_, d)| *d <= reach)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            };
            if hit.is_some() {
                {
                    let mut s = app.view.borrow_mut();
                    s.grab = hit.map(|i| Grab {
                        index: i,
                        handle: None,
                        start: (x, y),
                        origin: s.marks[i].clone(),
                        keep_ratio: false,
                    });
                    s.selected = hit;
                }
                refresh_marks(app);
            } else {
                if v.get_fit() {
                    v.set_scale(v.get_fit_scale());
                    v.set_pan_x(0.0);
                    v.set_pan_y(0.0);
                    v.set_fit(false);
                }
                let mut s = app.view.borrow_mut();
                s.grab = None;
                s.selected = None;
                s.pan_grab = Some((x, y, v.get_pan_x(), v.get_pan_y()));
            }
        }
        10 => {
            // The rectangle select drags out a selection.
            {
                let mut s = app.view.borrow_mut();
                s.selecting = Some((x, y));
                s.sel = None;
            }
            refresh_marks(app);
        }
        8 => eyedrop(app, x, y),
        9 => bucket(app, x, y),
        5 if !v.get_pixel_eraser() => {
            let mut s = app.view.borrow_mut();
            if let Some((i, _)) = s
                .marks
                .iter()
                .enumerate()
                .map(|(i, m)| (i, m.distance(x, y)))
                .filter(|(_, d)| *d <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                snap_marks(&mut s);
                s.marks.remove(i);
            }
            s.selected = None;
            drop(s);
            refresh_marks(app);
        }
        4 => {
            // A text box still open is committed first; a new one starts here.
            commit_text(app);
            app.view.borrow_mut().text_at = Some((x, y));
            v.set_text_x(x);
            v.set_text_y(y);
        }
        _ => {
            // The pixel eraser (kind 8) takes the canvas background (transparent on a photo: it is unused there).
            let mut rgba = if tool == 5 {
                app.view.borrow().canvas_bg
            } else {
                ink(app)
            };
            if tool == 1 {
                rgba[3] = 110;
            }
            let filled = (tool == 3 || tool == 6) && v.get_fill_shape();
            app.view.borrow_mut().drawing = Some(Mark {
                kind: if tool == 5 { 8 } else { tool },
                rgba,
                width: width * scale,
                points: vec![(x, y), (x, y)],
                text: String::new(),
                filled,
                image: None,
            });
            refresh_marks(app);
        }
    }
}

fn mark_move(app: &Rc<App>, x: f32, y: f32) {
    let mut s = app.view.borrow_mut();
    let pan = s.pan_grab;
    if let Some((sx, sy, px, py)) = pan {
        // (x, y) is the pointer's picture-space offset from the press, so the pan moves by that offset at the scale shown.
        drop(s);
        let v = viewer(app);
        let k = v.get_scale();
        v.set_pan_x(px + (x - sx) * k);
        v.set_pan_y(py + (y - sy) * k);
        return;
    }
    if let Some(g) = s.grab.clone() {
        let (dx, dy) = (x - g.start.0, y - g.start.1);
        if let Some(m) = s.marks.get_mut(g.index) {
            match g.handle {
                Some((hx, hy)) => *m = resized(&g.origin, hx, hy, dx, dy, g.keep_ratio),
                None => {
                    m.points = g
                        .origin
                        .points
                        .iter()
                        .map(|(px, py)| (px + dx, py + dy))
                        .collect()
                }
            }
        }
        drop(s);
        refresh_marks(app);
        return;
    }
    if let Some(start) = s.selecting {
        s.sel = Some(rect_of(start, (x, y)));
        drop(s);
        refresh_marks(app);
        return;
    }
    let Some(m) = s.drawing.as_mut() else { return };
    if matches!(m.kind, 0 | 1 | 8) {
        // A pixel eraser stroke is shown as it is drawn (refresh_marks renders it, throttled).
        let last = *m.points.last().unwrap_or(&(x, y));
        if (last.0 - x).hypot(last.1 - y) < 1.5 {
            return;
        }
        m.points.push((x, y));
    } else if let Some(last) = m.points.last_mut() {
        *last = (x, y);
    }
    drop(s);
    refresh_marks(app);
}

fn mark_end(app: &Rc<App>) {
    // A committed eraser stroke changes the composite, which refresh_marks burns once here (at pointer-up).
    let mut s = app.view.borrow_mut();
    if s.pan_grab.take().is_some() {
        return;
    }
    if let Some(g) = s.grab.take() {
        // A move or resize that changed the mark is one Undo step: the marks as they were before the drag.
        if s.marks
            .get(g.index)
            .is_some_and(|m| m.points != g.origin.points || m.width != g.origin.width)
        {
            let mut before = s.marks.clone();
            before[g.index] = g.origin;
            push_marks_undo(&mut s, before);
        }
        drop(s);
        refresh_marks(app);
        return;
    }
    if s.selecting.take().is_some() {
        if s.sel.is_some_and(|(_, _, w, h)| w < 2.0 && h < 2.0) {
            s.sel = None;
        }
        drop(s);
        refresh_marks(app);
        return;
    }
    if let Some(m) = s.drawing.take() {
        let (a, b) = (m.points[0], *m.points.last().unwrap_or(&m.points[0]));
        let tiny = (a.0 - b.0).hypot(a.1 - b.1) < 3.0;
        if !(tiny && matches!(m.kind, 2 | 3 | 6 | 7)) {
            snap_marks(&mut s);
            s.marks.push(m);
        }
    }
    drop(s);
    refresh_marks(app);
}

/// Ends the text box: what was typed becomes a text mark (when there is text). Safe to call when no box is open.
pub fn commit_text(app: &Rc<App>) {
    let v = viewer(app);
    let text = v.get_pending_text().to_string();
    v.set_pending_text(SharedString::new());
    v.set_text_x(-1.0);
    v.set_text_y(-1.0);
    let pos = app.view.borrow_mut().text_at.take();
    let Some(pos) = pos else { return };
    if !text.trim().is_empty() {
        let scale = (v.get_pic_w().max(v.get_pic_h()) as f32 / 1600.0).max(1.0);
        let width = v.get_ink_width() * scale;
        let rgba = ink(app);
        let mut s = app.view.borrow_mut();
        snap_marks(&mut s);
        s.marks.push(Mark {
            kind: 4,
            rgba,
            width,
            points: vec![pos],
            text,
            filled: false,
            image: None,
        });
    }
    refresh_marks(app);
}

/// Undo: the last mark while marking up, otherwise the last edit of the picture (a paint bucket fill is one too).
fn undo(app: &Rc<App>) {
    let markup = viewer(app).get_mode() == "markup";
    if markup && !app.view.borrow().marks_undo.is_empty() {
        let mut s = app.view.borrow_mut();
        if let Some(prev) = s.marks_undo.pop() {
            let now = std::mem::replace(&mut s.marks, prev);
            s.marks_redo.push(now);
        }
        s.selected = None;
        s.grab = None;
        drop(s);
        refresh_marks(app);
        return;
    }
    let changed = {
        let mut s = app.view.borrow_mut();
        match (s.history.pop(), s.pic.take()) {
            (Some(prev), Some(cur)) => {
                keep_picture(&mut s.future, cur);
                s.pic = Some(prev);
                true
            }
            (prev, cur) => {
                s.pic = cur;
                if let Some(p) = prev {
                    keep_picture(&mut s.history, p);
                }
                false
            }
        }
    };
    if changed {
        let dirty = !app.view.borrow().history.is_empty();
        viewer(app).set_dirty(dirty);
        show_picture(app, false);
    }
    refresh_undo(app);
}

fn redo(app: &Rc<App>) {
    let markup = viewer(app).get_mode() == "markup";
    if markup && !app.view.borrow().marks_redo.is_empty() {
        let mut s = app.view.borrow_mut();
        if let Some(next) = s.marks_redo.pop() {
            let now = std::mem::replace(&mut s.marks, next);
            s.marks_undo.push(now);
        }
        s.selected = None;
        s.grab = None;
        drop(s);
        refresh_marks(app);
        return;
    }
    let changed = {
        let mut s = app.view.borrow_mut();
        match (s.future.pop(), s.pic.take()) {
            (Some(next), Some(cur)) => {
                push_history(&mut s, cur);
                s.pic = Some(next);
                true
            }
            (next, cur) => {
                s.pic = cur;
                if let Some(n) = next {
                    keep_picture(&mut s.future, n);
                }
                false
            }
        }
    };
    if changed {
        viewer(app).set_dirty(true);
        show_picture(app, false);
    }
    refresh_undo(app);
}

/// Undo and Redo follow the marks while marking up, and the picture's edits too.
fn refresh_undo(app: &Rc<App>) {
    let s = app.view.borrow();
    let v = viewer(app);
    if v.get_mode() == "markup" {
        v.set_can_undo(!s.marks_undo.is_empty() || !s.history.is_empty());
        v.set_can_redo(!s.marks_redo.is_empty() || !s.future.is_empty());
    } else {
        v.set_can_undo(!s.history.is_empty());
        v.set_can_redo(!s.future.is_empty());
    }
}

fn to_ui(m: &Mark, images: &[(Arc<Picture>, slint::Image)]) -> ui::Mark {
    let [r, g, b, a] = m.rgba;
    let (x, y, w, h) = m.bounds();
    let picture = m
        .image
        .as_ref()
        .and_then(|p| {
            images
                .iter()
                .find(|(q, _)| Arc::ptr_eq(q, p))
                .map(|(_, img)| img.clone())
        })
        .unwrap_or_default();
    ui::Mark {
        kind: m.kind,
        colour: slint::Color::from_argb_u8(a, r, g, b),
        width: if m.kind == 4 {
            m.font_size()
        } else {
            m.stroke_width()
        },
        commands: m.commands().into(),
        x,
        y,
        w,
        h,
        text: m.text.clone().into(),
        picture,
        filled: m.filled,
    }
}

thread_local! {
    /// The committed pixel eraser strokes last burned into the picture (their count and point total), and whether the
    /// picture currently shows that composite.
    static ERASE_PREVIEW: std::cell::Cell<((usize, usize), bool)> = const { std::cell::Cell::new(((0, 0), false)) };
}

/// How a pixel eraser stroke in progress is drawn (a grey outline: the eraser's colour may be transparent on a photo).
const ERASER_OUTLINE: [u8; 4] = [0x80, 0x80, 0x80, 0xC0];

/// Pixel eraser strokes are not ink: the committed ones are burned into the picture (the composite). The burn is a full
/// render of the picture, so it runs only when the committed strokes change (at the stroke's pointer-up, or on undo),
/// never on each pointer move. A stroke in progress is drawn as a vector overlay instead. Returns true when the picture
/// holds the composite, so the overlay leaves the committed marks out.
fn erase_preview(app: &Rc<App>, s: &ViewState) -> bool {
    let committed = s.marks.iter().filter(|m| m.kind == 8);
    let print = committed
        .clone()
        .fold((0, 0), |(n, p), m| (n + 1, p + m.points.len()));
    let dragging = s.drawing.as_ref().is_some_and(|m| m.kind == 8);
    let (shown, showing) = ERASE_PREVIEW.with(|c| c.get());
    if print.0 > 0 {
        if !showing || (shown != print && !dragging) {
            if let Some(pic) = s.pic.as_ref() {
                if let Ok(out) = rimelight_picture::burn_marks(pic, &s.marks, s.canvas) {
                    viewer(app).set_picture(picture::to_slint(&out));
                }
            }
            ERASE_PREVIEW.with(|c| c.set((print, true)));
        }
    } else if showing {
        if let Some(pic) = s.pic.as_ref() {
            viewer(app).set_picture(picture::to_slint(pic));
        }
        ERASE_PREVIEW.with(|c| c.set(((0, 0), false)));
    }
    ERASE_PREVIEW.with(|c| c.get().1)
}

fn refresh_marks(app: &Rc<App>) {
    {
        let mut s = app.view.borrow_mut();
        // The Slint image of each picture mark is made once.
        let live: Vec<Arc<Picture>> = s.marks.iter().filter_map(|m| m.image.clone()).collect();
        for pic in &live {
            if !s.mark_images.iter().any(|(p, _)| Arc::ptr_eq(p, pic)) {
                let img = picture::to_slint(pic);
                s.mark_images.push((Arc::clone(pic), img));
            }
        }
        s.mark_images
            .retain(|(p, _)| live.iter().any(|q| Arc::ptr_eq(p, q)));
    }
    let s = app.view.borrow();
    // When the picture holds the eraser composite, the committed marks are already in it: only the stroke in progress is
    // drawn over it. Otherwise every mark is an overlay (an eraser stroke as a grey outline).
    let composite = erase_preview(app, &s);
    let shown = |m: &Mark| {
        if m.kind == 8 {
            let mut outline = m.clone();
            outline.rgba = ERASER_OUTLINE;
            to_ui(&outline, &s.mark_images)
        } else {
            to_ui(m, &s.mark_images)
        }
    };
    let items: Vec<ui::Mark> = if composite {
        s.drawing.iter().map(shown).collect()
    } else {
        s.marks.iter().chain(s.drawing.iter()).map(shown).collect()
    };
    viewer(app).set_marks(ModelRc::new(VecModel::from(items)));
    drop(s);
    refresh_selection(app);
    refresh_undo(app);
}

/// The rectangle selection and the picked mark's box, for the outlines on the picture.
fn refresh_selection(app: &Rc<App>) {
    let s = app.view.borrow();
    let v = viewer(app);
    match s.sel {
        Some((x, y, w, h)) => {
            v.set_has_sel(true);
            v.set_sel_x(x);
            v.set_sel_y(y);
            v.set_sel_w(w);
            v.set_sel_h(h);
        }
        None => v.set_has_sel(false),
    }
    match s.selected.and_then(|i| s.marks.get(i)) {
        Some(m) => {
            let (x, y, w, h) = m.bounds();
            v.set_has_box(true);
            v.set_box_x(x);
            v.set_box_y(y);
            v.set_box_w(w);
            v.set_box_h(h);
        }
        None => v.set_has_box(false),
    }
}

/// The picture as the person sees it: the picture with the marks burned in.
fn current(app: &Rc<App>) -> Option<Picture> {
    let s = app.view.borrow();
    let pic = s.pic.clone()?;
    // Unapplied marks are part of what the person sees: include them.
    if !s.marks.is_empty() {
        return rimelight_picture::burn_marks(&pic, &s.marks, s.canvas).ok();
    }
    Some(pic)
}

/// The marks go into the picture: returns the render with them burned in and clears the markup. The caller makes the
/// edit (it is kept in Undo as one step).
fn flatten(app: &Rc<App>) -> Option<Picture> {
    let flat = current(app)?;
    clear_markup(&mut app.view.borrow_mut());
    Some(flat)
}

/// The selection as whole pixels of `pic`, clamped to it; None when it is outside or empty.
fn sel_px(pic: &Picture, sel: (f32, f32, f32, f32)) -> Option<(u32, u32, u32, u32)> {
    let x0 = sel.0.floor().max(0.0) as u32;
    let y0 = sel.1.floor().max(0.0) as u32;
    if x0 >= pic.w || y0 >= pic.h {
        return None;
    }
    let x1 = ((sel.0 + sel.2).ceil().max(0.0) as u32).min(pic.w);
    let y1 = ((sel.1 + sel.3).ceil().max(0.0) as u32).min(pic.h);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// The paint bucket: a flood fill of the render, kept as one edit in Undo.
fn bucket(app: &Rc<App>, x: f32, y: f32) {
    if x < 0.0 || y < 0.0 {
        return;
    }
    let Some(flat) = flatten(app) else { return };
    let filled = rimelight_picture::flood_fill(&flat, x as u32, y as u32, ink(app), FILL_TOLERANCE);
    edit(app, move |_| filled);
    refresh_marks(app);
}

/// The eyedropper: the rendered pixel under the pointer becomes the ink.
fn eyedrop(app: &Rc<App>, x: f32, y: f32) {
    if x < 0.0 || y < 0.0 {
        return;
    }
    let Some(flat) = current(app) else { return };
    let (px, py) = (x as u32, y as u32);
    if px >= flat.w || py >= flat.h {
        return;
    }
    let i = ((py * flat.w + px) * 4) as usize;
    pick_colour(app, [flat.rgba[i], flat.rgba[i + 1], flat.rgba[i + 2], 255]);
}

/// Crop to the rectangle selection.
fn crop_selection(app: &Rc<App>) {
    let sel = app.view.borrow().sel;
    let Some(sel) = sel else { return };
    let Some(flat) = flatten(app) else { return };
    let Some((x, y, w, h)) = sel_px(&flat, sel) else {
        return;
    };
    let cropped = rimelight_picture::crop_px(&flat, x, y, w, h);
    edit(app, move |_| cropped);
    refresh_marks(app);
}

/// Cut (copies the selection, then fills it with the background) or Delete (fills it). Without a selection it does nothing.
fn remove_selection(app: &Rc<App>, cut: bool) {
    let sel = app.view.borrow().sel;
    let Some(sel) = sel else { return };
    let Some(flat) = current(app) else { return };
    let Some((x, y, w, h)) = sel_px(&flat, sel) else {
        return;
    };
    if cut {
        let part = rimelight_picture::crop_px(&flat, x, y, w, h);
        if let Err(e) = picture::copy_to_clipboard(&part) {
            toast(app, format!("Could not copy: {e}"));
            return;
        }
    }
    let bg = if app.view.borrow().canvas {
        app.view.borrow().canvas_bg
    } else {
        [255, 255, 255, 255]
    };
    let filled = rimelight_picture::fill_rect(&flat, x, y, w, h, bg);
    clear_markup(&mut app.view.borrow_mut());
    edit(app, move |_| filled);
    refresh_marks(app);
    if cut {
        toast(app, "Cut to the clipboard");
    }
}

/// Delete with a mark picked: the mark goes (it can be redone, like an eraser stroke).
fn delete_selected(app: &Rc<App>) {
    let mut s = app.view.borrow_mut();
    let Some(i) = s.selected.take() else { return };
    if i < s.marks.len() {
        snap_marks(&mut s);
        s.marks.remove(i);
    }
    s.grab = None;
    drop(s);
    refresh_marks(app);
}

/// Puts an image on the clipboard (None when there is no picture there).
pub fn clipboard_image() -> Option<Picture> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get_image().ok()?;
    let (w, h) = (img.width as u32, img.height as u32);
    if w == 0 || h == 0 {
        return None;
    }
    Some(Picture {
        w,
        h,
        rgba: img.bytes.into_owned(),
    })
}

/// Ctrl+V on a page that is not the picture page: the picture on the clipboard opens as a new canvas.
pub fn paste_new(app: &Rc<App>) {
    match clipboard_image() {
        Some(pic) => settle_canvas(app, move |a| install_canvas(a, pic, [255, 255, 255, 255])),
        None => toast(app, "There's no picture on the clipboard."),
    }
}

/// Ctrl+V on the picture page: the clipboard picture comes in.
fn paste_clipboard(app: &Rc<App>) {
    match clipboard_image() {
        Some(pic) => place_picture(app, pic),
        None => toast(app, "There's no picture on the clipboard."),
    }
}

/// A picture pasted or dropped: it becomes the canvas when the canvas is still empty, otherwise a picture mark that
/// can be moved and resized until another tool is picked.
pub fn place_picture(app: &Rc<App>, pic: Picture) {
    let empty_canvas = {
        let s = app.view.borrow();
        s.canvas && s.marks.is_empty() && s.history.is_empty()
    };
    if empty_canvas {
        {
            let mut s = app.view.borrow_mut();
            s.pic = Some(pic);
            s.original = None;
            s.future.clear();
        }
        viewer(app).set_dirty(true);
        show_picture(app, true);
        refresh_undo(app);
        return;
    }
    if viewer(app).get_mode() != "markup" {
        set_mode(app, "markup");
    }
    let v = viewer(app);
    let (pw, ph) = (v.get_pic_w() as f32, v.get_pic_h() as f32);
    let (iw, ih) = (pic.w as f32, pic.h as f32);
    // Half the picture at most, never bigger than the picture it came from.
    let f = (pw * 0.5 / iw).min(ph * 0.5 / ih).min(1.0);
    let (w, h) = (iw * f, ih * f);
    let (x, y) = ((pw - w) / 2.0, (ph - h) / 2.0);
    {
        let mut s = app.view.borrow_mut();
        snap_marks(&mut s);
        s.marks.push(Mark {
            kind: 5,
            rgba: [0, 0, 0, 0],
            width: 1.0,
            points: vec![(x, y), (x + w, y + h)],
            text: String::new(),
            filled: false,
            image: Some(Arc::new(pic)),
        });
        s.selected = Some(s.marks.len() - 1);
        s.grab = None;
    }
    refresh_marks(app);
}

/// A picture dropped on the window while marking up (or on the canvas).
pub fn drop_picture(app: &Rc<App>, path: &Path) {
    match rimelight_picture::load(path) {
        Ok(pic) => place_picture(app, pic),
        Err(e) => toast(app, format!("That picture can't be used: {e}")),
    }
}

/// True while the picture page is marking up (or drawing on the canvas).
pub fn in_markup(app: &Rc<App>) -> bool {
    viewer(app).get_mode() == "markup"
}

fn save(app: &Rc<App>) {
    let path = app.view.borrow().path.clone();
    let canvas = app.view.borrow().canvas;
    match path {
        Some(p) if rimelight_picture::can_save(&p) => write_to(app, &p),
        Some(_) => save_as(app),
        None if canvas => {
            // An unsaved canvas goes to the canvas folder as a PNG, named by the time.
            let dir = app.prefs.borrow().canvas_dir.clone();
            let out = files::free_path(&dir, &files::stamp("Canvas"), "png");
            write_to(app, &out);
        }
        None => save_as(app),
    }
}

fn save_as(app: &Rc<App>) {
    let canvas = app.view.borrow().canvas;
    let (dir, stem) = {
        let s = app.view.borrow();
        match &s.path {
            Some(p) => (
                p.parent().map(Path::to_path_buf),
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Picture")
                    .to_string(),
            ),
            None if s.canvas => (
                Some(app.prefs.borrow().canvas_dir.clone()),
                files::stamp("Canvas"),
            ),
            None => (
                Some(app.prefs.borrow().screenshot_dir.clone()),
                files::stamp("Screenshot"),
            ),
        }
    };
    let ext = app
        .view
        .borrow()
        .path
        .as_deref()
        .map(files::ext_of)
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp"))
        .unwrap_or_else(|| "png".into());
    let mut dialog = rfd::FileDialog::new()
        .set_title("Save as")
        .set_file_name(format!("{stem}.{ext}"))
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"]);
    if !canvas {
        dialog = dialog.add_filter("WebP (lossless)", &["webp"]);
    }
    dialog = dialog.add_filter("BMP", &["bmp"]);
    if let Some(d) = dir {
        dialog = dialog.set_directory(d);
    }
    if let Some(mut path) = dialog.save_file() {
        if !rimelight_picture::can_save(&path) {
            path.set_extension("png");
        }
        write_to(app, &path);
    }
}

fn write_to(app: &Rc<App>, path: &Path) {
    let Some(pic) = current(app) else { return };
    match rimelight_picture::save(&pic, path) {
        Ok(()) => {
            {
                let mut s = app.view.borrow_mut();
                clear_markup(&mut s);
                s.canvas = false;
                // The saved picture is what Revert goes back to now. With no Undo steps that is `pic` itself, so no copy;
                // otherwise (Undo still reaches the steps before the save) a copy is kept aside.
                s.original = (!s.history.is_empty()).then(|| pic.clone());
                s.pic = Some(pic);
                s.path = Some(path.to_path_buf());
                s.file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                s.folder = files::sibling_pictures(path);
                s.index = s.folder.iter().position(|p| p == path).unwrap_or(0);
            }
            let v = viewer(app);
            v.set_canvas(false);
            v.set_dirty(false);
            v.set_info_name(files::file_name(path).into());
            set_mode(app, "");
            show_picture(app, false);
            update_position(app);
            remember(app, path);
            toast(app, format!("Saved {}", files::file_name(path)));
        }
        Err(e) => toast(app, format!("Could not save: {e}")),
    }
}

/// Copies the rectangle selection, or the whole picture (marks included) when nothing is selected.
fn copy(app: &Rc<App>) {
    let Some(pic) = current(app) else { return };
    let sel = app.view.borrow().sel;
    let pic = match sel.and_then(|s| sel_px(&pic, s)) {
        Some((x, y, w, h)) => rimelight_picture::crop_px(&pic, x, y, w, h),
        None => pic,
    };
    match picture::copy_to_clipboard(&pic) {
        Ok(()) => toast(app, "Copied to the clipboard"),
        Err(e) => toast(app, format!("Could not copy: {e}")),
    }
}

/// Zooms by a factor around the middle of the stage.
fn zoom(app: &Rc<App>, factor: f32) {
    let v = viewer(app);
    let current = if v.get_fit() {
        let size = app
            .ui
            .window()
            .size()
            .to_logical(app.ui.window().scale_factor());
        let info = if v.get_show_info() { 280.0 } else { 0.0 };
        let sw = (size.width - info - 48.0).max(1.0);
        let sh = (size.height - 88.0 - 48.0).max(1.0);
        (sw / v.get_pic_w() as f32)
            .min(sh / v.get_pic_h() as f32)
            .min(1.0)
    } else {
        v.get_scale()
    };
    let new = (current * factor).clamp(0.02, 40.0);
    let ratio = new / current;
    v.set_pan_x(if v.get_fit() {
        0.0
    } else {
        v.get_pan_x() * ratio
    });
    v.set_pan_y(if v.get_fit() {
        0.0
    } else {
        v.get_pan_y() * ratio
    });
    v.set_scale(new);
    v.set_fit(false);
}

pub fn on_key(app: &Rc<App>, text: &str, ctrl: bool, shift: bool) -> bool {
    use slint::platform::Key;
    let k = |key: Key| text == SharedString::from(key).as_str();
    let v = viewer(app);
    let mode = v.get_mode();
    if v.get_animated() && mode.is_empty() && !ctrl {
        if text == " " {
            anim_toggle(app);
            return true;
        }
        if text == "," {
            anim_step(app, -1);
            return true;
        }
        if text == "." {
            anim_step(app, 1);
            return true;
        }
    }
    if ctrl {
        let typing = app.view.borrow().text_at.is_some();
        match text.to_ascii_lowercase().as_str() {
            "c" => copy(app),
            // While a text box is open, Ctrl+V pastes text into it.
            "v" if !typing => paste_clipboard(app),
            "s" if shift => save_as(app),
            "s" => save(app),
            "z" => undo(app),
            "y" => redo(app),
            "p" => print(app),
            _ => return false,
        }
        return true;
    }
    if !mode.is_empty() {
        if k(Key::Return) {
            match mode.as_str() {
                "crop" => v.invoke_apply_crop(),
                "markup" => v.invoke_apply_marks(),
                "resize" => v.invoke_apply_resize(),
                _ => {}
            }
            return true;
        }
        if k(Key::Delete) && mode == "markup" && app.view.borrow().text_at.is_none() {
            if app.view.borrow().sel.is_some() {
                remove_selection(app, false);
            } else {
                delete_selected(app);
            }
            return true;
        }
        return false;
    }
    if k(Key::Delete) {
        delete_current(app);
    } else if k(Key::LeftArrow) || k(Key::PageUp) {
        step(app, -1);
    } else if k(Key::RightArrow) || k(Key::PageDown) || text == " " {
        step(app, 1);
    } else if k(Key::Home) {
        let first = app.view.borrow().folder.first().cloned();
        if let Some(p) = first {
            open(app, &p);
        }
    } else if k(Key::End) {
        let last = app.view.borrow().folder.last().cloned();
        if let Some(p) = last {
            open(app, &p);
        }
    } else if text == "+" || text == "=" {
        zoom(app, 1.25);
    } else if text == "-" {
        zoom(app, 1.0 / 1.25);
    } else if text == "0" {
        v.set_fit(true);
    } else if text == "1" {
        v.set_scale(1.0);
        v.set_pan_x(0.0);
        v.set_pan_y(0.0);
        v.set_fit(false);
    } else if text == "r" || text == "R" {
        edit(app, rimelight_picture::rotate_cw);
    } else if text == "c" {
        set_mode(app, "crop");
    } else if text == "m" {
        set_mode(app, "markup");
    } else if text == "i" {
        v.set_show_info(!v.get_show_info());
    } else if text == "s" || text == "S" {
        slideshow_toggle(app);
    } else {
        return false;
    }
    true
}

fn stop_anim(s: &mut ViewState) {
    s.anim_timer.stop();
    s.anim = None;
    s.anim_images.clear();
    s.anim_frame = 0;
    s.anim_playing = false;
}

fn start_anim(app: &Rc<App>, anim: rimelight_picture::Animation) {
    let n = anim.frames.len();
    let cut = anim.cut;
    {
        let mut s = app.view.borrow_mut();
        s.anim_images = vec![None; n];
        s.anim = Some(Rc::new(anim));
        s.anim_frame = 0;
        s.anim_playing = true;
    }
    let v = viewer(app);
    v.set_animated(true);
    if cut {
        toast(
            app,
            "This animation is very large: only its first part is shown.",
        );
    }
    show_frame(app, 0);
    schedule_next(app);
}

/// Shows frame `i` (its screen picture is made the first time).
fn show_frame(app: &Rc<App>, i: usize) {
    let (image, text, playing) = {
        let mut s = app.view.borrow_mut();
        let Some(anim) = s.anim.clone() else { return };
        let i = i % anim.frames.len();
        s.anim_frame = i;
        let image = match s.anim_images[i].clone() {
            Some(img) => img,
            None => {
                let img = picture::to_slint(&anim.frames[i].0);
                s.anim_images[i] = Some(img.clone());
                img
            }
        };
        (
            image,
            format!(
                "{} / {}  ·  {} ms",
                i + 1,
                anim.frames.len(),
                anim.frames[i].1
            ),
            s.anim_playing,
        )
    };
    let v = viewer(app);
    v.set_picture(image);
    v.set_frame_text(text.into());
    v.set_anim_playing(playing);
}

fn schedule_next(app: &Rc<App>) {
    let s = app.view.borrow();
    let Some(anim) = s.anim.as_ref() else { return };
    if !s.anim_playing {
        return;
    }
    let delay = anim.frames[s.anim_frame].1;
    let generation = s.generation;
    s.anim_timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(u64::from(delay)),
        move || {
            with_app(move |a| {
                if a.view.borrow().generation != generation || !a.view.borrow().anim_playing {
                    return;
                }
                let next = a.view.borrow().anim_frame + 1;
                show_frame(a, next);
                schedule_next(a);
            })
        },
    );
}

fn anim_toggle(app: &Rc<App>) {
    {
        let mut s = app.view.borrow_mut();
        if s.anim.is_none() {
            return;
        }
        s.anim_playing = !s.anim_playing;
        if !s.anim_playing {
            s.anim_timer.stop();
        }
    }
    let frame = app.view.borrow().anim_frame;
    show_frame(app, frame);
    schedule_next(app);
}

/// One frame back or forward (pauses the animation).
fn anim_step(app: &Rc<App>, delta: i32) {
    let next = {
        let mut s = app.view.borrow_mut();
        let Some(anim) = s.anim.as_ref() else { return };
        let n = anim.frames.len() as i64;
        s.anim_playing = false;
        s.anim_timer.stop();
        ((s.anim_frame as i64 + i64::from(delta)).rem_euclid(n)) as usize
    };
    show_frame(app, next);
}

/// Keeps the frame showing as a still picture (before an edit).
fn freeze_anim(app: &Rc<App>) {
    let mut s = app.view.borrow_mut();
    if let Some(anim) = s.anim.clone() {
        // Revert goes back to the first frame (the picture as opened), so it is kept aside when another frame is kept.
        if s.anim_frame != 0 && s.original.is_none() && s.history.is_empty() {
            s.original = Some(anim.frames[0].0.clone());
        }
        s.pic = Some(anim.frames[s.anim_frame].0.clone());
    }
    stop_anim(&mut s);
    drop(s);
    viewer(app).set_animated(false);
}

fn save_frame(app: &Rc<App>) {
    let (pic, path, frame) = {
        let s = app.view.borrow();
        let Some(anim) = s.anim.as_ref() else { return };
        (
            anim.frames[s.anim_frame].0.clone(),
            s.path.clone(),
            s.anim_frame,
        )
    };
    let dir = path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| app.prefs.borrow().screenshot_dir.clone());
    let stem = path
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("Animation")
        .to_string();
    let out = files::free_path(&dir, &format!("{stem} frame {}", frame + 1), "png");
    match rimelight_picture::save(&pic, &out) {
        Ok(()) => toast(app, format!("Frame saved: {}", files::file_name(&out))),
        Err(e) => toast(app, format!("Could not save the frame: {e}")),
    }
}

/// Delete: a file goes to the Recycle Bin and the next picture in the folder opens; a snip (or canvas) that was never
/// saved is discarded.
fn delete_current(app: &Rc<App>) {
    let (path, next) = {
        let s = app.view.borrow();
        let next = s
            .folder
            .get(s.index + 1)
            .or_else(|| s.index.checked_sub(1).and_then(|i| s.folder.get(i)))
            .cloned();
        (s.path.clone(), next)
    };
    match path {
        Some(p) => {
            if !rimelight_platform::recycle(&p) {
                toast(app, "Could not delete it (is it open in another app?)");
                return;
            }
            toast(
                app,
                format!("{} moved to the Recycle Bin", files::file_name(&p)),
            );
            app.recents.borrow_mut().retain(|r| r != &p);
            crate::prefs::save_recents(&app.recents.borrow());
            crate::app::refresh_recents(app);
            viewer(app).set_dirty(false);
            match next.filter(|n| n.exists()) {
                Some(n) => open(app, &n),
                None => close(app),
            }
        }
        None => settle_canvas(app, |a| {
            toast(a, "Snip discarded");
            close(a);
        }),
    }
}

/// Empties the picture tab and frees the picture, its edits and the filmstrip.
pub fn close(app: &Rc<App>) {
    slideshow_stop(app);
    {
        let mut s = app.view.borrow_mut();
        stop_anim(&mut s);
        s.generation += 1;
        s.path = None;
        s.pic = None;
        s.original = None;
        s.folder.clear();
        clear_markup(&mut s);
        s.history.clear();
        s.future.clear();
        s.picked.clear();
        s.canvas = false;
        s.pending = None;
    }
    viewer(app).set_comparing(false);
    viewer(app).set_canvas(false);
    viewer(app).set_file_type(SharedString::new());
    viewer(app).set_facts_text(SharedString::new());
    viewer(app).set_dims_text(SharedString::new());
    viewer(app).set_size_text(SharedString::new());
    viewer(app).set_can_share(false);
    refresh_undo(app);
    let v = viewer(app);
    v.set_animated(false);
    v.set_dirty(false);
    v.set_mode(SharedString::new());
    v.set_picture(slint::Image::default());
    v.set_info_name(SharedString::new());
    v.set_position(SharedString::new());
    v.set_has_prev(false);
    v.set_has_next(false);
    v.set_show_info(false);
    v.set_compare_images(ModelRc::default());
    v.set_thumbs(ModelRc::default());
    {
        let mut s = app.view.borrow_mut();
        s.thumbs = None;
        s.thumbs_dir = None;
        s.thumbs_gen += 1;
    }
    refresh_marks(app);
    crate::app::update_title(app);
}

/// The X in the top bar on the picture page: an unsaved canvas asks first (in the app), a picture asks as before.
pub fn close_tab(app: &Rc<App>) {
    if canvas_unsaved(app) {
        settle_canvas(app, close);
        return;
    }
    settle_picture(app, close);
}

/// Before the picture goes away: true when there's nothing unsaved, or the person saved it or chose to discard it.
/// (A canvas is asked about in the app instead: see settle_canvas.)
pub fn settle_picture(app: &Rc<App>, then: impl FnOnce(&Rc<App>) + 'static) {
    if !viewer(app).get_dirty() || app.view.borrow().canvas {
        then(app);
        return;
    }
    let name = app
        .view
        .borrow()
        .path
        .as_deref()
        .map(files::file_name)
        .unwrap_or_else(|| "this snip".into());
    let detail = "Your changes will be lost if you don't save them.";
    crate::app::ask(
        app,
        "Save changes?",
        &name,
        detail,
        "Save",
        "Don't save",
        move |a, choice| match choice {
            0 => {
                save(a);
                if !viewer(a).get_dirty() {
                    then(a);
                }
            }
            1 => then(a),
            _ => {}
        },
    );
}

/// Runs `then` now when the canvas has nothing unsaved; otherwise shows the in-app "Save changes?" question and runs it
/// once the answer is in (see answer).
pub fn settle_canvas(app: &Rc<App>, then: impl FnOnce(&Rc<App>) + 'static) {
    if !canvas_unsaved(app) {
        then(app);
        return;
    }
    app.view.borrow_mut().pending = Some(Box::new(then) as Then);
    let shell = app.ui.global::<ui::Shell>();
    shell.set_confirm_name("Untitled canvas".into());
    shell.set_confirm_open(true);
}

/// True while the in-app question is about the picture page (the text tab's question is the other one).
pub fn has_pending(app: &Rc<App>) -> bool {
    app.view.borrow().pending.is_some()
}

/// The answer to the question: 0 Save (then the action, if the save worked), 1 Don't save (the action, dropping the
/// canvas), 2 Cancel (nothing).
pub fn answer(app: &Rc<App>, choice: i32) {
    app.ui.global::<ui::Shell>().set_confirm_open(false);
    let then = app.view.borrow_mut().pending.take();
    let Some(then) = then else { return };
    match choice {
        0 => {
            save(app);
            if !viewer(app).get_dirty() {
                then(app);
            }
        }
        1 => {
            viewer(app).set_dirty(false);
            app.view.borrow_mut().canvas = false;
            then(app);
        }
        _ => {}
    }
}

/// Ctrl+N and the top bar's Canvas button ask for the size with the "New canvas" popup; this makes the canvas.
fn new_canvas(app: &Rc<App>, preset: i32, w: &str, h: &str, transparent: bool) {
    slideshow_stop(app);
    let Some((w, h)) = canvas_size(app, preset, w, h) else {
        toast(app, "Type a width and a height from 1 to 10000 pixels.");
        return;
    };
    let bg = if transparent {
        [0, 0, 0, 0]
    } else {
        [255, 255, 255, 255]
    };
    let pic = Picture {
        w,
        h,
        rgba: bg.repeat(w as usize * h as usize),
    };
    settle_canvas(app, move |a| install_canvas(a, pic, bg));
}

fn canvas_size(app: &Rc<App>, preset: i32, w: &str, h: &str) -> Option<(u32, u32)> {
    let (w, h) = match preset {
        0 => {
            let size = app.ui.window().size();
            (size.width, size.height)
        }
        1 => (1920, 1080),
        2 => (1240, 1754),
        _ => (w.trim().parse().ok()?, h.trim().parse().ok()?),
    };
    (w > 0 && h > 0 && w <= 10_000 && h <= 10_000).then_some((w, h))
}

/// Makes `pic` the canvas: no file, markup mode on, no undo history.
fn install_canvas(app: &Rc<App>, pic: Picture, bg: [u8; 4]) {
    {
        let mut s = app.view.borrow_mut();
        stop_anim(&mut s);
        s.generation += 1;
        s.canvas = true;
        s.canvas_bg = bg;
        s.path = None;
        s.folder.clear();
        s.index = 0;
        s.file_size = 0;
        // A canvas has no file to revert to, so it keeps one copy of its pixels (in `pic`), not two.
        s.original = None;
        s.pic = Some(pic);
        s.history.clear();
        s.future.clear();
        clear_markup(&mut s);
    }
    let v = viewer(app);
    v.set_animated(false);
    v.set_loading(false);
    v.set_error(SharedString::new());
    v.set_comparing(false);
    v.set_dirty(false);
    v.set_fit(true);
    v.set_pan_x(0.0);
    v.set_pan_y(0.0);
    v.set_canvas(true);
    v.set_canvas_transparent(bg[3] == 0);
    v.set_info_name("Untitled canvas".into());
    v.set_mode(SharedString::new());
    update_position(app);
    show_picture(app, true);
    go(app, "image");
    set_mode(app, "markup");
}

fn print(app: &Rc<App>) {
    let Some(pic) = current(app) else { return };
    let title = app
        .view
        .borrow()
        .path
        .as_deref()
        .map(files::file_name)
        .unwrap_or_else(|| "Picture".into());
    match rimelight_platform::print_image(
        crate::app::main_hwnd(app),
        &pic.rgba,
        pic.w,
        pic.h,
        &title,
    ) {
        Ok(true) => toast(app, "Sent to the printer"),
        Ok(false) => {}
        Err(e) => toast(app, format!("Could not print: {e}")),
    }
}

fn share(app: &Rc<App>) {
    let Some(path) = app.view.borrow().path.clone().filter(|p| p.exists()) else {
        toast(app, "Save the picture first, then share it.");
        return;
    };
    if viewer(app).get_dirty() {
        toast(
            app,
            "Sharing the saved file: changes not saved yet aren't included.",
        );
    }
    if let Err(e) =
        rimelight_platform::share_file(crate::app::main_hwnd(app), &path, &files::file_name(&path))
    {
        toast(app, format!("Could not share: {e}"));
    }
}

fn toggle_filmstrip(app: &Rc<App>) {
    let v = viewer(app);
    v.set_filmstrip(!v.get_filmstrip());
    sync_thumbs(app);
}

/// Keeps the filmstrip in step with the folder: new thumbnails (made in the background) for a new folder, otherwise
/// only the current and picked marks.
fn sync_thumbs(app: &Rc<App>) {
    if !viewer(app).get_filmstrip() {
        return;
    }
    let (folder, dir, same) = {
        let s = app.view.borrow();
        let dir = s
            .folder
            .first()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        let same = s
            .thumbs
            .as_ref()
            .is_some_and(|m| m.row_count() == s.folder.len())
            && s.thumbs_dir == dir;
        (s.folder.clone(), dir, same)
    };
    if !same {
        let model = Rc::new(VecModel::from(
            folder
                .iter()
                .map(|p| ui::Thumb {
                    image: slint::Image::default(),
                    name: files::file_name(p).into(),
                    current: false,
                    selected: false,
                })
                .collect::<Vec<_>>(),
        ));
        let generation = {
            let mut s = app.view.borrow_mut();
            s.thumbs_gen += 1;
            s.thumbs = Some(Rc::clone(&model));
            s.thumbs_dir = dir;
            s.picked.clear();
            s.thumbs_gen
        };
        viewer(app).set_thumbs(ModelRc::from(model));
        std::thread::spawn(move || {
            for (i, p) in folder.into_iter().enumerate() {
                let Some(pic) = rimelight_picture::thumbnail_within(&p, 240, 176) else {
                    continue;
                };
                let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &pic.rgba, pic.w, pic.h,
                );
                let keep_going = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                let flag = std::sync::Arc::clone(&keep_going);
                post(move |a| {
                    let s = a.view.borrow();
                    if s.thumbs_gen != generation {
                        flag.store(false, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                    if let Some(m) = s.thumbs.as_ref()
                        && let Some(mut row) = m.row_data(i)
                    {
                        row.image = slint::Image::from_rgba8(buf);
                        m.set_row_data(i, row);
                    }
                });
                if !keep_going.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }
        });
    }
    let s = app.view.borrow();
    if let Some(m) = s.thumbs.as_ref() {
        for i in 0..m.row_count() {
            if let Some(mut row) = m.row_data(i) {
                let (current, selected) = (i == s.index, s.picked.contains(&i));
                if row.current != current || row.selected != selected {
                    row.current = current;
                    row.selected = selected;
                    m.set_row_data(i, row);
                }
            }
        }
    }
}

/// A click in the filmstrip opens that picture; Ctrl+click (or Shift+click) picks pictures to compare side by side.
fn thumb_clicked(app: &Rc<App>, i: usize, add: bool) {
    let Some(path) = app.view.borrow().folder.get(i).cloned() else {
        return;
    };
    if !add {
        if viewer(app).get_dirty() {
            toast(app, "Changes not saved: save them, or undo them, first.");
            return;
        }
        app.view.borrow_mut().picked.clear();
        viewer(app).set_comparing(false);
        open(app, &path);
        return;
    }
    let picked = {
        let mut s = app.view.borrow_mut();
        if s.picked.is_empty() {
            let current = s.index;
            s.picked.push(current);
        }
        if let Some(at) = s.picked.iter().position(|p| *p == i) {
            s.picked.remove(at);
        } else {
            s.picked.push(i);
        }
        s.picked
            .iter()
            .filter_map(|p| s.folder.get(*p).cloned())
            .collect::<Vec<_>>()
    };
    sync_thumbs(app);
    let v = viewer(app);
    if picked.len() < 2 {
        v.set_comparing(false);
        return;
    }
    // The grid: as square as it can be (2 pictures side by side, 3 as 2 + 1, 4 as 2 by 2, and so on).
    let cols = (picked.len() as f64).sqrt().ceil() as i32;
    let rows = (picked.len() as i32 + cols - 1) / cols;
    v.set_compare_cols(cols);
    v.set_compare_rows(rows);
    v.set_comparing(true);
    v.set_compare_images(ModelRc::new(VecModel::from(vec![
        slint::Image::default();
        picked.len()
    ])));
    let generation = app.view.borrow().generation;
    for (slot, p) in picked.into_iter().enumerate() {
        std::thread::spawn(move || {
            let Ok(pic) = rimelight_picture::load(&p) else {
                return;
            };
            // Side by side, each needs only a screen's worth of pixels.
            let pic = if pic.w.max(pic.h) > 2560 {
                let f = 2560.0 / pic.w.max(pic.h) as f32;
                rimelight_picture::resize(
                    &pic,
                    (pic.w as f32 * f) as u32,
                    (pic.h as f32 * f) as u32,
                )
            } else {
                pic
            };
            let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &pic.rgba, pic.w, pic.h,
            );
            post(move |a| {
                if a.view.borrow().generation != generation {
                    return;
                }
                let model = viewer(a).get_compare_images();
                if slot < model.row_count() {
                    model.set_row_data(slot, slint::Image::from_rgba8(buf));
                }
            });
        });
    }
}

pub fn close_compare(app: &Rc<App>) {
    app.view.borrow_mut().picked.clear();
    viewer(app).set_comparing(false);
    sync_thumbs(app);
}

/// The open picture was renamed on disk: the folder list and the title follow the new name.
pub fn renamed(app: &Rc<App>, new: &Path) {
    {
        let mut s = app.view.borrow_mut();
        s.path = Some(new.to_path_buf());
        s.folder = files::sibling_pictures(new);
        s.index = s.folder.iter().position(|p| p == new).unwrap_or(0);
        s.file_size = std::fs::metadata(new).map(|m| m.len()).unwrap_or(0);
    }
    viewer(app).set_info_name(files::file_name(new).into());
    update_position(app);
    show_picture(app, false);
}
