//! The Text tab (Notepad's job): plain text files with their encoding (UTF-8, with or without BOM, UTF-16) and line endings
//! kept on save; Markdown as flat blocks for the native preview; and the JSON tools (format, minify, validate, sort keys,
//! the collapsible tree). The editor (ui/text_page.slint) holds the text; Rust reads it when it needs it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::Serialize as _;
use serde_json::Value;
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::app::{self, App, toast, with_app};
use crate::files;
use crate::highlight;
use crate::ui;

/// The largest file the tab opens (a bigger one is a log for another tool).
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Above this size the editor is not highlighted (the layers would be too big to rebuild as you type).
const HIGHLIGHT_MAX_BYTES: usize = 200 * 1024;

/// How long the editor waits after the last edit before it highlights again.
const HIGHLIGHT_DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Enc {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Enc {
    fn label(self) -> &'static str {
        match self {
            Enc::Utf8 => "UTF-8",
            Enc::Utf8Bom => "UTF-8 BOM",
            Enc::Utf16Le => "UTF-16 LE",
            Enc::Utf16Be => "UTF-16 BE",
        }
    }
}

/// The open text file and the tab's state. The editor holds the text itself (TextDoc.text), always with LF line ends.
#[derive(Default)]
pub struct TextState {
    pub path: Option<PathBuf>,
    enc: Enc,
    /// The file used CRLF line ends; saving writes them back.
    crlf: bool,
    /// The caret as a byte offset into the text (the editor reports it), and the last selection the find tools made.
    caret: usize,
    selection: Option<(usize, usize)>,
    /// JSON tree: the paths of the containers folded shut.
    collapsed: HashSet<String>,
    /// The action waiting for the answer to "Save changes to ...?".
    pending: Option<Pending>,
    /// Bumped on each edit: a highlight scheduled earlier does nothing when a later edit has come.
    hl_gen: u32,
    /// Undo and redo: `last` is the text as of the last edit (or load); `undo` holds the text before each step.
    last: String,
    undo: Vec<String>,
    redo: Vec<String>,
    /// When the last edit came: edits within UNDO_BURST of it are one undo step.
    last_edit: Option<Instant>,
}

/// Undo steps kept (each one a copy of the text).
const UNDO_MAX: usize = 200;
/// Edits closer together than this are one undo step (typing a word is one Undo, not one per letter).
const UNDO_BURST: Duration = Duration::from_millis(800);

/// What to do once the unsaved-changes question is answered (Save, or Don't save; Cancel does nothing).
pub enum Pending {
    Open(PathBuf),
    New,
    Close,
    /// The window is closing: finish the close (app::finish_close).
    Quit,
}

fn doc(app: &Rc<App>) -> ui::TextDoc<'_> {
    app.ui.global::<ui::TextDoc>()
}

fn text_of(app: &Rc<App>) -> String {
    doc(app).get_text().to_string()
}

/// Connects the editor's callbacks (called once at start-up).
pub fn wire(app: &Rc<App>) {
    let d = doc(app);
    d.on_caret_moved(|i| {
        with_app(|a| {
            a.text.borrow_mut().caret = i.max(0) as usize;
            update_status(a);
        })
    });
    d.on_edited(|| with_app(edited));
    d.on_insert_tab(|at, anchor| with_app(move |a| insert_tab(a, at, anchor)));
    d.on_save(|| {
        with_app(|a| {
            save(a, false);
        })
    });
    d.on_save_as(|| {
        with_app(|a| {
            save(a, true);
        })
    });
    d.on_new_doc(|| with_app(new_doc));
    d.on_set_kind(|k| with_app(|a| apply_kind(a, k)));
    d.on_format_json(|n| with_app(|a| format_json(a, n)));
    d.on_minify_json(|| with_app(minify_json));
    d.on_validate_json(|| with_app(validate_json));
    d.on_sort_json(|| with_app(sort_json));
    d.on_toggle_node(|p: SharedString| with_app(|a| toggle_node(a, &p)));
    d.on_find_next(|needle: SharedString| with_app(|a| find_next(a, &needle)));
    d.on_find_prev(|needle: SharedString| with_app(|a| find_prev(a, &needle)));
    d.on_open_link(|url: SharedString| {
        rimelight_platform::open_uri(&url);
    });
    d.on_replace_one(|needle: SharedString, repl: SharedString| {
        with_app(|a| replace_one(a, &needle, &repl))
    });
    d.on_replace_all(|needle: SharedString, repl: SharedString| {
        with_app(|a| replace_all(a, &needle, &repl))
    });
    d.on_revert(|| with_app(revert));
    d.on_undo(|| with_app(undo));
    d.on_redo(|| with_app(redo));
    d.on_share(|| with_app(share));
    d.on_open_folder(|| {
        with_app(|a| {
            let path = a.text.borrow().path.clone();
            if let Some(path) = path {
                files::reveal(&path);
            }
        })
    });
    d.set_text_font(app.prefs.borrow().text_font);
    d.on_toggle_info(|| with_app(toggle_info));
    d.on_set_text_font(|i| with_app(|a| set_font(a, i)));
    app.ui
        .global::<ui::Shell>()
        .on_confirm_answer(|choice| with_app(|a| answer(a, choice)));
}

/// The window's title part for this tab: the file's name (the editor shows the unsaved marker).
pub fn title(app: &Rc<App>) -> String {
    app.text
        .borrow()
        .path
        .as_deref()
        .map(files::file_name)
        .unwrap_or_else(|| "Untitled".into())
}

/// Opens a text file in the tab (asks about unsaved changes first; the file is read once the answer is in).
pub fn open(app: &Rc<App>, path: &Path) {
    if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        toast(app, "That file is too big for the Text tab.");
        return;
    }
    if !settle(app, Pending::Open(path.to_path_buf())) {
        return;
    }
    load(app, path);
}

/// Reads a file into the tab (no question asked: the caller has dealt with unsaved changes).
fn load(app: &Rc<App>, path: &Path) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            toast(
                app,
                format!("Could not read {}: {e}", files::file_name(path)),
            );
            return;
        }
    };
    let (enc, text) = decode(&bytes);
    let crlf = text.contains("\r\n");
    {
        let mut st = app.text.borrow_mut();
        // The highlight generation carries over, so a result still on its way from the last file is discarded.
        let hl_ticket = st.hl_gen;
        *st = TextState {
            path: Some(path.to_path_buf()),
            enc,
            crlf,
            ..TextState::default()
        };
        st.hl_gen = hl_ticket;
    }
    set_meta(app, path);
    let d = doc(app);
    let text = text.replace("\r\n", "\n");
    app.text.borrow_mut().last = text.clone();
    d.set_text(text.into());
    history_flags(app);
    d.set_dirty(false);
    d.set_view(if d.get_markdown() { 1 } else { 0 });
    app::go(app, "text");
    refresh(app);
    update_status(app);
    refresh_info(app);
    app::update_title(app);
    highlight_now(app);
}

/// An empty, untitled document (asks about unsaved changes first).
pub fn new_doc(app: &Rc<App>) {
    if settle(app, Pending::New) {
        fresh(app);
    }
}

/// Makes the tab an empty, untitled document (no question asked). The editor takes the focus (focus-serial).
fn fresh(app: &Rc<App>) {
    clear(app);
    let d = doc(app);
    d.set_has_doc(true);
    d.set_focus_serial(d.get_focus_serial() + 1);
    app::go(app, "text");
    refresh(app);
    update_status(app);
    app::update_title(app);
}

/// Empties the tab (no question asked: the caller has already dealt with unsaved changes).
pub fn clear(app: &Rc<App>) {
    {
        let mut st = app.text.borrow_mut();
        let hl_ticket = st.hl_gen;
        *st = TextState {
            hl_gen: hl_ticket,
            ..TextState::default()
        };
    }
    let d = doc(app);
    d.set_text("".into());
    d.set_has_doc(false);
    d.set_dirty(false);
    d.set_file_name("".into());
    d.set_show_info(false);
    d.set_info_name("".into());
    d.set_info_detail("".into());
    d.set_markdown(false);
    d.set_json(false);
    d.set_kind(KIND_PLAIN);
    d.set_view(0);
    d.set_find_status("".into());
    d.set_message("".into());
    d.set_status("".into());
    d.set_hl_on(false);
    d.set_code_file(false);
    d.set_caret_prefix("".into());
    // The preview, the tree and the colour layers go too: each can hold copies of a large text.
    d.set_md_blocks(ModelRc::default());
    d.set_json_rows(ModelRc::default());
    drop_layers(app);
    history_flags(app);
}

/// Frees the colour layers (one copy of the text per colour) once they no longer show.
fn drop_layers(app: &Rc<App>) {
    use slint::Model as _;
    let d = doc(app);
    if d.get_hl_texts().row_count() > 0 {
        d.set_hl_colours(ModelRc::default());
        d.set_hl_texts(ModelRc::default());
    }
}

/// The X in the top bar: back to the empty tab (asking first about unsaved changes).
pub fn close(app: &Rc<App>) {
    if settle(app, Pending::Close) {
        clear(app);
    }
}

/// True when the text tab has unsaved changes.
pub fn is_dirty(app: &Rc<App>) -> bool {
    doc(app).get_dirty()
}

/// Returns true when the action may run now (no unsaved changes). Otherwise the in-app question is shown, the action is
/// kept, and `answer` runs it (or not) later; this returns false.
pub fn settle(app: &Rc<App>, then: Pending) -> bool {
    if !is_dirty(app) {
        return true;
    }
    app.text.borrow_mut().pending = Some(then);
    let shell = app.ui.global::<ui::Shell>();
    shell.set_confirm_name(title(app).into());
    shell.set_confirm_open(true);
    false
}

/// The question's answer: 0 Save (then the action, if the save worked), 1 Don't save (the action, dropping the changes),
/// 2 Cancel (nothing).
fn answer(app: &Rc<App>, choice: i32) {
    if crate::app::take_ask_answer(app, choice) {
        return;
    }
    // The same dialog asks about an unsaved canvas (the picture page) when one is waiting for the answer.
    if crate::viewer::has_pending(app) {
        crate::viewer::answer(app, choice);
        return;
    }
    app.ui.global::<ui::Shell>().set_confirm_open(false);
    let pending = app.text.borrow_mut().pending.take();
    let Some(then) = pending else { return };
    let go_on = match choice {
        0 => save(app, false),
        1 => true,
        _ => false,
    };
    if go_on {
        run(app, then);
    }
}

fn run(app: &Rc<App>, then: Pending) {
    match then {
        Pending::Open(path) => load(app, &path),
        Pending::New => fresh(app),
        Pending::Close => {
            clear(app);
            app::update_title(app);
        }
        Pending::Quit => app::finish_close(app),
    }
}

/// Discard changes: the file is read again from disk and the modified flag cleared (an untitled text is emptied).
pub fn revert(app: &Rc<App>) {
    let path = app.text.borrow().path.clone();
    match path {
        Some(p) => {
            let view = doc(app).get_view();
            load(app, &p);
            doc(app).set_view(view);
            toast(app, "Changes discarded");
        }
        None => {
            doc(app).set_text("".into());
            doc(app).set_dirty(false);
            {
                let mut st = app.text.borrow_mut();
                st.last.clear();
                st.undo.clear();
                st.redo.clear();
            }
            history_flags(app);
            refresh(app);
            update_status(app);
            refresh_info(app);
        }
    }
}

/// Writes the text (Save, or Save as when there is no file yet or `as_new`). Returns false when nothing was saved.
pub fn save(app: &Rc<App>, as_new: bool) -> bool {
    let current = app.text.borrow().path.clone();
    let path = match current {
        Some(p) if !as_new => p,
        current => {
            // The name keeps the file's stem (or "Untitled") with the extension of the file type chosen in the toolbar.
            let stem = current
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            let name = format!(
                "{stem}.{}",
                KINDS[doc(app).get_kind().clamp(0, KINDS.len() as i32 - 1) as usize].1
            );
            let picked = rfd::FileDialog::new()
                .set_title("Save as")
                .set_file_name(name)
                .add_filter("Text", files::TEXT_EXTS)
                .add_filter("Everything", &["*"])
                .save_file();
            match picked {
                Some(p) => p,
                None => return false,
            }
        }
    };
    let (enc, crlf) = {
        let st = app.text.borrow();
        (st.enc, st.crlf)
    };
    let mut text = text_of(app);
    if crlf {
        text = text.replace('\n', "\r\n");
    }
    if let Err(e) = std::fs::write(&path, encode(enc, &text)) {
        toast(
            app,
            format!("Could not save {}: {e}", files::file_name(&path)),
        );
        return false;
    }
    app.text.borrow_mut().path = Some(path.clone());
    set_meta(app, &path);
    doc(app).set_dirty(false);
    app::remember(app, &path);
    app::update_title(app);
    update_status(app);
    refresh_info(app);
    toast(app, format!("Saved {}", files::file_name(&path)));
    true
}

fn set_meta(app: &Rc<App>, path: &Path) {
    let ext = files::ext_of(path);
    let d = doc(app);
    d.set_has_doc(true);
    d.set_file_name(files::file_name(path).into());
    d.set_kind(kind_of_ext(&ext));
    d.set_markdown(d.get_kind() == KIND_MD);
    d.set_json(d.get_kind() == KIND_JSON);
}

// The file types of the toolbar's combo, in its order: the label and the extension Save as uses.
const KIND_PLAIN: i32 = 0;
const KIND_MD: i32 = 1;
const KIND_JSON: i32 = 2;
const KINDS: [(&str, &str); 9] = [
    ("Plain text", "txt"),
    ("Markdown", "md"),
    ("JSON", "json"),
    ("TOML", "toml"),
    ("YAML", "yaml"),
    ("XML", "xml"),
    ("CSV", "csv"),
    ("INI", "ini"),
    ("Log", "log"),
];

/// The file type of an extension (plain text for the ones the list does not know).
fn kind_of_ext(ext: &str) -> i32 {
    match ext {
        "md" | "markdown" => KIND_MD,
        "json" => KIND_JSON,
        "toml" => 3,
        "yaml" | "yml" => 4,
        "xml" => 5,
        "csv" => 6,
        "ini" | "cfg" | "conf" => 7,
        "log" => 8,
        _ => KIND_PLAIN,
    }
}

/// The file type the toolbar's combo picked: sets the mode (the preview for Markdown, the tree tools for JSON) and keeps
/// the file's name. Runs again when the combo is set from Rust too, so it changes only what differs.
fn apply_kind(app: &Rc<App>, k: i32) {
    let d = doc(app);
    let k = k.clamp(0, KINDS.len() as i32 - 1);
    let (was_md, was_json) = (d.get_markdown(), d.get_json());
    let md = k == KIND_MD;
    let json = k == KIND_JSON;
    d.set_kind(k);
    d.set_markdown(md);
    d.set_json(json);
    if md && !was_md {
        d.set_view(1);
    } else if json && !was_json {
        d.set_view(0);
    } else if !md && was_md {
        d.set_view(0);
    }
    refresh(app);
    update_status(app);
    refresh_info(app);
    highlight_now(app);
}

fn edited(app: &Rc<App>) {
    record(app);
    app.text.borrow_mut().selection = None;
    doc(app).set_dirty(true);
    update_status(app);
    refresh(app);
    app::update_title(app);
    schedule_highlight(app);
}

/// Tab in the editor: a tab character replaces the selection (from `anchor` to the caret `at`, byte offsets), and the
/// caret goes after it. The editor's own text input never sees the Tab key, so the insert is done here.
fn insert_tab(app: &Rc<App>, at: i32, anchor: i32) {
    let text = text_of(app);
    let from = (at.min(anchor).max(0) as usize).min(text.len());
    let to = (at.max(anchor).max(0) as usize).min(text.len());
    if !text.is_char_boundary(from) || !text.is_char_boundary(to) {
        return;
    }
    // Spaces up to the next 4-column tab stop: the editor font has no glyph for a tab character (it shows as a box).
    let line_start = text[..from].rfind('\n').map_or(0, |i| i + 1);
    let column = text[line_start..from].chars().count();
    let spaces = 4 - column % 4;
    let pad = " ".repeat(spaces);
    let new = format!("{}{}{}", &text[..from], pad, &text[to..]);
    let d = doc(app);
    d.set_text(new.into());
    app.text.borrow_mut().selection = None;
    d.set_sel_start((from + spaces) as i32);
    d.set_sel_end((from + spaces) as i32);
    d.set_sel_serial(d.get_sel_serial() + 1);
    edited(app);
}

/// Keeps the text before this edit for Undo. Edits in a burst (UNDO_BURST apart) share one step.
fn record(app: &Rc<App>) {
    let now = text_of(app);
    {
        let mut st = app.text.borrow_mut();
        if now == st.last {
            return;
        }
        let before = std::mem::replace(&mut st.last, now);
        let burst = st.last_edit.is_some_and(|t| t.elapsed() < UNDO_BURST);
        st.last_edit = Some(Instant::now());
        st.redo.clear();
        if !burst {
            st.undo.push(before);
            if st.undo.len() > UNDO_MAX {
                st.undo.remove(0);
            }
        }
    }
    history_flags(app);
}

/// Undo (back) or redo: swaps the editor's text for the step's text.
fn step(app: &Rc<App>, back: bool) {
    let text = {
        let mut guard = app.text.borrow_mut();
        let st = &mut *guard;
        let (from, to) = if back {
            (&mut st.undo, &mut st.redo)
        } else {
            (&mut st.redo, &mut st.undo)
        };
        let Some(prev) = from.pop() else {
            return;
        };
        to.push(std::mem::replace(&mut st.last, prev.clone()));
        st.last_edit = None;
        prev
    };
    let d = doc(app);
    d.set_text(text.into());
    d.set_dirty(true);
    refresh(app);
    update_status(app);
    app::update_title(app);
    schedule_highlight(app);
    history_flags(app);
}

fn undo(app: &Rc<App>) {
    step(app, true);
}

fn redo(app: &Rc<App>) {
    step(app, false);
}

/// Sets the Undo and Redo buttons' enabled state from the history.
fn history_flags(app: &Rc<App>) {
    let (has_undo, has_redo) = {
        let st = app.text.borrow();
        (!st.undo.is_empty(), !st.redo.is_empty())
    };
    let d = doc(app);
    d.set_can_undo(has_undo);
    d.set_can_redo(has_redo);
}

/// Share (as the picture and video tabs): the saved file. Changes not saved yet are not in it.
fn share(app: &Rc<App>) {
    let Some(path) = app.text.borrow().path.clone().filter(|p| p.exists()) else {
        toast(app, "Save the text first, then share it.");
        return;
    };
    if is_dirty(app) {
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

/// Highlights the editor's text for its file type now (the overlay layers under the editor; see highlight.rs). Turns the
/// overlay off for a plain file or a text over the size limit.
fn highlight_now(app: &Rc<App>) {
    let d = doc(app);
    let ext = app
        .text
        .borrow()
        .path
        .as_deref()
        .map(files::ext_of)
        .unwrap_or_default();
    let syntax = highlight::syntax_for(&ext, d.get_kind());
    let text = text_of(app);
    // Markdown keeps word wrap (prose), which the overlay can't follow, so it is not coloured.
    let capable = syntax.is_some() && text.len() <= HIGHLIGHT_MAX_BYTES;
    d.set_code_file(capable);
    let Some(syntax) = syntax.filter(|_| capable) else {
        d.set_hl_on(false);
        drop_layers(app);
        return;
    };
    // The text is highlighted on a background thread (syntect is slow on big files); the result comes back on the
    // event loop and is dropped if an edit or another file has come since (the generation no longer matches).
    let ticket = {
        let mut st = app.text.borrow_mut();
        st.hl_gen = st.hl_gen.wrapping_add(1);
        st.hl_gen
    };
    let syntax = syntax.to_string();
    // The app theme is not yet passed to the Text tab, so the highlighting uses the dark colours.
    const DARK: bool = true;
    std::thread::spawn(move || {
        let layers = highlight::layers(&text, &syntax, DARK).unwrap_or_default();
        let _ =
            slint::invoke_from_event_loop(move || with_app(|a| apply_layers(a, ticket, layers)));
    });
}

/// Shows the colour layers a background highlight produced, if no later edit or file has replaced them.
fn apply_layers(app: &Rc<App>, ticket: u32, layers: Vec<(i32, String)>) {
    if app.text.borrow().hl_gen != ticket {
        return;
    }
    let d = doc(app);
    if layers.is_empty() {
        d.set_hl_on(false);
        drop_layers(app);
        return;
    }
    let kinds: Vec<slint::Color> = layers
        .iter()
        .map(|(k, _)| slint::Color::from_rgb_u8((*k >> 16) as u8, (*k >> 8) as u8, *k as u8))
        .collect();
    let texts: Vec<SharedString> = layers.into_iter().map(|(_, s)| s.into()).collect();
    d.set_hl_colours(ModelRc::new(VecModel::from(kinds)));
    d.set_hl_texts(ModelRc::new(VecModel::from(texts)));
    d.set_hl_on(true);
}

/// The font of plain text (0 Mono, 1 Sans, 2 Serif): kept in the settings, so it is the same next time.
fn set_font(app: &Rc<App>, font: i32) {
    let font = font.clamp(0, 2);
    doc(app).set_text_font(font);
    app.prefs.borrow_mut().text_font = font;
    app.prefs.borrow().save();
}

/// Highlights again after a short pause in typing. Until then the editor shows its own text colour: the overlay would
/// not match the text that was just typed.
fn schedule_highlight(app: &Rc<App>) {
    let hl_ticket = {
        let mut st = app.text.borrow_mut();
        st.hl_gen = st.hl_gen.wrapping_add(1);
        st.hl_gen
    };
    doc(app).set_hl_on(false);
    slint::Timer::single_shot(HIGHLIGHT_DELAY, move || {
        with_app(|a| {
            if a.text.borrow().hl_gen == hl_ticket {
                highlight_now(a);
                // The Details facts follow the text after the pause too (only while the panel is open).
                refresh_info(a);
            }
        })
    });
}

/// Ln, Col, characters, line ends and encoding, separated by "  |  ".
fn update_status(app: &Rc<App>) {
    let text = text_of(app);
    let status = {
        let st = app.text.borrow();
        let mut c = st.caret.min(text.len());
        while !text.is_char_boundary(c) {
            c -= 1;
        }
        let before = &text[..c];
        // The text up to the caret (with a mark after it): the editor lays it out to find the caret's height, so it can
        // scroll the caret into view.
        doc(app).set_caret_prefix(format!("{before}|").into());
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        let eol = if st.crlf { "CRLF" } else { "LF" };
        format!(
            "Ln {line}, Col {col}  |  {} characters  |  {eol}  |  {}",
            text.chars().count(),
            st.enc.label()
        )
    };
    doc(app).set_status(status.into());
}

/// The Details info button: opens or closes the panel, and fills it when it opens.
fn toggle_info(app: &Rc<App>) {
    let show = !doc(app).get_show_info();
    doc(app).set_show_info(show);
    if show {
        update_info(app);
    }
}

/// Fills the Details panel again, but only while it is open: the facts cost a copy of the text, so they are not kept
/// up to date while the panel is closed (toggle_info fills it when it opens). Called after a load, a save, a change of
/// file type, and each pause in typing (schedule_highlight), never on each key.
fn refresh_info(app: &Rc<App>) {
    if doc(app).get_show_info() {
        update_info(app);
    }
}

/// The Details panel's name and lines: the file type, size on disk (the text as save would write it), encoding, line
/// ends, lines, words and characters, then for a saved file its folder and when it changed (or "Not saved yet").
fn update_info(app: &Rc<App>) {
    let text = text_of(app);
    let (path, enc, crlf) = {
        let st = app.text.borrow();
        (st.path.clone(), st.enc, st.crlf)
    };
    let d = doc(app);
    let kind = KINDS[d.get_kind().clamp(0, KINDS.len() as i32 - 1) as usize].0;
    let body = if crlf {
        text.replace('\n', "\r\n")
    } else {
        text.clone()
    };
    let size = encode(enc, &body).len() as u64;
    let lines = text.matches('\n').count() + 1;
    let words = text.split_whitespace().count();
    let mut detail = vec![
        kind.to_string(),
        files::human_size(size),
        enc.label().to_string(),
        if crlf { "CRLF" } else { "LF" }.to_string(),
        format!(
            "{lines} lines, {words} words, {} characters",
            text.chars().count()
        ),
    ];
    let name = match path.as_deref() {
        Some(p) => {
            if let Ok(modified) = std::fs::metadata(p).and_then(|m| m.modified())
                && let Ok(age) = modified.elapsed()
            {
                let days = age.as_secs() / 86_400;
                detail.push(match days {
                    0 => "Changed today".to_string(),
                    1 => "Changed yesterday".to_string(),
                    n => format!("Changed {n} days ago"),
                });
            }
            detail.push(String::new());
            detail.push(files::folder_name(p));
            files::file_name(p)
        }
        None => {
            detail.push("Not saved yet".to_string());
            "Untitled".to_string()
        }
    };
    d.set_info_name(name.into());
    d.set_info_detail(detail.join("\n").into());
}

/// Rebuilds the preview (Markdown) or the tree (JSON) after a change.
fn refresh(app: &Rc<App>) {
    use slint::Model as _;
    let d = doc(app);
    // A preview or tree left from an earlier kind of file is freed (it can be as big as that file).
    if !d.get_markdown() && d.get_md_blocks().row_count() > 0 {
        d.set_md_blocks(ModelRc::default());
    }
    if !d.get_json() && d.get_json_rows().row_count() > 0 {
        d.set_json_rows(ModelRc::default());
    }
    let text = text_of(app);
    if d.get_markdown() {
        d.set_md_blocks(ModelRc::new(VecModel::from(markdown_blocks(&text))));
    }
    if d.get_json() {
        let collapsed = app.text.borrow().collapsed.clone();
        let mut rows = Vec::new();
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            walk(&v, "", "root".into(), 0, &collapsed, &mut rows);
        }
        d.set_json_rows(ModelRc::new(VecModel::from(rows)));
    }
}

fn set_text(app: &Rc<App>, text: &str) {
    let d = doc(app);
    d.set_text(text.into());
    d.set_dirty(true);
    app.text.borrow_mut().selection = None;
    update_status(app);
    refresh(app);
    app::update_title(app);
    schedule_highlight(app);
}

/// Moves the caret and selects `start..end` in the editor (the editor applies it).
fn select(app: &Rc<App>, start: usize, end: usize) {
    app.text.borrow_mut().selection = Some((start, end));
    let d = doc(app);
    d.set_sel_start(start as i32);
    d.set_sel_end(end as i32);
    d.set_sel_serial(d.get_sel_serial() + 1);
}

// --- Encoding -------------------------------------------------------------------------------------------------------

/// The text and its encoding, from the bytes (BOM first; without one, UTF-8, invalid bytes shown as �).
fn decode(bytes: &[u8]) -> (Enc, String) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (Enc::Utf8Bom, String::from_utf8_lossy(rest).into_owned());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (Enc::Utf16Le, utf16(rest, u16::from_le_bytes));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (Enc::Utf16Be, utf16(rest, u16::from_be_bytes));
    }
    (Enc::Utf8, String::from_utf8_lossy(bytes).into_owned())
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

fn encode(enc: Enc, text: &str) -> Vec<u8> {
    match enc {
        Enc::Utf8 => text.as_bytes().to_vec(),
        Enc::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        Enc::Utf16Le => {
            let mut out = vec![0xFF, 0xFE];
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            out
        }
        Enc::Utf16Be => {
            let mut out = vec![0xFE, 0xFF];
            out.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            out
        }
    }
}

// --- Find and replace ------------------------------------------------------------------------------------------------

/// The first match at or after `from` (ASCII letters match in either case; the offsets are byte offsets).
fn find_bytes(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || from > h.len() || n.len() > h.len() - from {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Every match of `needle`, as byte offsets in order.
fn matches_of(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = find_bytes(hay, needle, pos) {
        out.push(i);
        pos = i + needle.len();
    }
    out
}

/// The find bar's count: "3 of 12" when the selection is one of the matches, else "12 matches".
fn show_matches(app: &Rc<App>, text: &str, needle: &str) {
    let hits = matches_of(text, needle);
    let sel = app.text.borrow().selection;
    let status = if hits.is_empty() {
        "No matches".to_string()
    } else {
        match sel.and_then(|(s, _)| hits.iter().position(|&h| h == s)) {
            Some(i) => format!("{} of {}", i + 1, hits.len()),
            None => format!("{} matches", hits.len()),
        }
    };
    doc(app).set_find_status(status.into());
}

/// Selects the next match after the selected one (or after the caret), wrapping to the first.
fn find_next(app: &Rc<App>, needle: &str) {
    if needle.is_empty() {
        doc(app).set_find_status("".into());
        return;
    }
    let text = text_of(app);
    let (sel, caret) = {
        let st = app.text.borrow();
        (st.selection, st.caret)
    };
    let from = match sel {
        Some((s, e))
            if s < e
                && text
                    .get(s..e)
                    .is_some_and(|t| t.eq_ignore_ascii_case(needle)) =>
        {
            e
        }
        _ => caret.min(text.len()),
    };
    if let Some(i) = find_bytes(&text, needle, from).or_else(|| find_bytes(&text, needle, 0)) {
        select(app, i, i + needle.len());
    }
    show_matches(app, &text, needle);
}

/// Selects the match before the selected one (or before the caret), wrapping to the last.
fn find_prev(app: &Rc<App>, needle: &str) {
    if needle.is_empty() {
        doc(app).set_find_status("".into());
        return;
    }
    let text = text_of(app);
    let (sel, caret) = {
        let st = app.text.borrow();
        (st.selection, st.caret)
    };
    let before = sel.map(|(s, _)| s).unwrap_or(caret.min(text.len()));
    let hits = matches_of(&text, needle);
    if let Some(i) = hits
        .iter()
        .rev()
        .find(|&&h| h < before)
        .or(hits.last())
        .copied()
    {
        select(app, i, i + needle.len());
    }
    show_matches(app, &text, needle);
}

fn replace_one(app: &Rc<App>, needle: &str, repl: &str) {
    if needle.is_empty() {
        return;
    }
    let text = text_of(app);
    // The match the find tools selected comes first; otherwise the next one after the caret.
    let selected = app.text.borrow().selection;
    let hit = match selected {
        Some((s, e))
            if s < e
                && text
                    .get(s..e)
                    .is_some_and(|t| t.eq_ignore_ascii_case(needle)) =>
        {
            Some(s)
        }
        _ => {
            let from = app.text.borrow().caret.min(text.len());
            find_bytes(&text, needle, from).or_else(|| find_bytes(&text, needle, 0))
        }
    };
    let Some(i) = hit else {
        doc(app).set_find_status("No matches".into());
        return;
    };
    let out = format!("{}{repl}{}", &text[..i], &text[i + needle.len()..]);
    set_text(app, &out);
    select(app, i, i + repl.len());
}

fn replace_all(app: &Rc<App>, needle: &str, repl: &str) {
    if needle.is_empty() {
        return;
    }
    let text = text_of(app);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut n = 0;
    while let Some(i) = find_bytes(&text, needle, pos) {
        out.push_str(&text[pos..i]);
        out.push_str(repl);
        pos = i + needle.len();
        n += 1;
    }
    if n == 0 {
        doc(app).set_find_status("No matches".into());
        return;
    }
    out.push_str(&text[pos..]);
    set_text(app, &out);
    doc(app).set_find_status(format!("Replaced {n}").into());
}

// --- Markdown ---------------------------------------------------------------------------------------------------------

/// The StyledText of a Markdown string (plain text when it does not parse).
fn styled(md: &str) -> slint::StyledText {
    slint::StyledText::from_markdown(md).unwrap_or_else(|_| slint::StyledText::from_plain_text(md))
}

/// Markdown for plain text: the characters that would start a style are escaped.
fn escape_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Markdown for a code span.
fn code_md(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

/// The bullet of a list item at a nesting depth: a dot, a hollow dot, then a small square.
fn bullet(depth: i32) -> &'static str {
    match depth {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

/// A block of the given kind with its inline Markdown (`md`) and plain text (`text`, for code).
fn block(
    kind: i32,
    level: i32,
    indent: i32,
    check: i32,
    marker: &str,
    text: &str,
    md: &str,
) -> ui::MdBlock {
    ui::MdBlock {
        kind,
        level,
        indent,
        check,
        text: text.into(),
        marker: marker.into(),
        styled: styled(md),
        cells: ModelRc::new(VecModel::default()),
    }
}

/// A numbered or bulleted list being read: whether it is numbered, and the number of its next item.
struct ListLevel {
    ordered: bool,
    next: u64,
}

/// An open list item: its nesting depth, its task state (-1 none, 0 open, 1 done) and its marker.
struct ItemInfo {
    indent: i32,
    check: i32,
    marker: String,
}

/// Collects the Markdown into flat blocks (see ui::MdBlock). The inline text is kept as Markdown, which StyledText draws
/// with its bold, italic, code, strikethrough and links.
#[derive(Default)]
struct Blocks {
    out: Vec<ui::MdBlock>,
    /// The inline Markdown of the block being read.
    md: String,
    /// The text of the code block being read.
    code: String,
    in_code: bool,
    /// The destinations of the links being read (written when the link ends).
    links: Vec<String>,
    lists: Vec<ListLevel>,
    items: Vec<ItemInfo>,
    quote: i32,
    /// The table being read: its cells, the current row and column, the columns, and whether the header is read.
    cells: Vec<ui::MdCell>,
    row: i32,
    col: i32,
    cols: i32,
    head: bool,
}

impl Blocks {
    /// The inline Markdown read so far, trimmed (and empties the buffer).
    fn take_md(&mut self) -> String {
        std::mem::take(&mut self.md).trim().to_string()
    }

    /// Ends the text collected so far as a paragraph, list item or quote. Nothing is written for empty text.
    fn flush(&mut self) {
        let md = self.take_md();
        if md.is_empty() {
            return;
        }
        if let Some(item) = self.items.last() {
            let b = block(2, 0, item.indent, item.check, &item.marker, "", &md);
            self.out.push(b);
        } else if self.quote > 0 {
            self.out.push(block(4, 0, 0, -1, "", "", &md));
        } else {
            self.out.push(block(1, 0, 0, -1, "", "", &md));
        }
    }
}

pub fn markdown_blocks(src: &str) -> Vec<ui::MdBlock> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut b = Blocks::default();
    let mut heading = 0;
    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                b.flush();
                heading = level as i32;
            }
            Event::End(TagEnd::Heading(_)) => {
                let md = b.take_md();
                if !md.is_empty() {
                    b.out.push(block(0, heading, 0, -1, "", "", &md));
                }
            }
            Event::Start(Tag::Paragraph) => {
                if b.quote == 0 {
                    b.flush();
                }
            }
            // The paragraphs of a quote stay one block: they are separated by a blank line inside it.
            Event::End(TagEnd::Paragraph) => {
                if b.quote > 0 && b.items.is_empty() {
                    b.md.push_str("\n\n");
                } else {
                    b.flush();
                }
            }
            Event::Start(Tag::List(start)) => {
                b.flush();
                b.lists.push(ListLevel {
                    ordered: start.is_some(),
                    next: start.unwrap_or(1),
                });
            }
            Event::End(TagEnd::List(_)) => {
                b.flush();
                b.lists.pop();
            }
            Event::Start(Tag::Item) => {
                b.flush();
                let depth = (b.lists.len() as i32 - 1).max(0);
                let marker = match b.lists.last_mut() {
                    Some(l) if l.ordered => {
                        let m = format!("{}.", l.next);
                        l.next += 1;
                        m
                    }
                    _ => bullet(depth).to_string(),
                };
                b.items.push(ItemInfo {
                    indent: depth,
                    check: -1,
                    marker,
                });
            }
            Event::End(TagEnd::Item) => {
                b.flush();
                b.items.pop();
            }
            Event::TaskListMarker(done) => {
                if let Some(item) = b.items.last_mut() {
                    item.check = i32::from(done);
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                b.flush();
                b.quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                b.flush();
                b.quote -= 1;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                b.flush();
                b.in_code = true;
                b.code.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                b.in_code = false;
                let code = std::mem::take(&mut b.code);
                let code = code.trim_end_matches('\n');
                if !code.is_empty() {
                    b.out.push(block(3, 0, 0, -1, "", code, ""));
                }
            }
            Event::Start(Tag::Table(_)) => {
                b.flush();
                b.cells.clear();
                b.row = 0;
                b.col = 0;
                b.cols = 0;
            }
            Event::Start(Tag::TableHead) => {
                b.head = true;
                b.col = 0;
            }
            Event::End(TagEnd::TableHead) => {
                b.head = false;
                b.row = 1;
                b.col = 0;
            }
            Event::Start(Tag::TableRow) => b.col = 0,
            Event::End(TagEnd::TableRow) => b.row += 1,
            Event::Start(Tag::TableCell) => b.md.clear(),
            Event::End(TagEnd::TableCell) => {
                let md = b.take_md();
                // The header cells are bold.
                let md = if b.head && !md.is_empty() {
                    format!("**{md}**")
                } else {
                    md
                };
                let cell = ui::MdCell {
                    row: b.row,
                    col: b.col,
                    header: b.head,
                    styled: styled(&md),
                };
                b.cells.push(cell);
                b.col += 1;
                b.cols = b.cols.max(b.col);
            }
            Event::End(TagEnd::Table) => {
                let mut table = block(7, b.cols, b.row, -1, "", "", "");
                table.cells = ModelRc::new(VecModel::from(std::mem::take(&mut b.cells)));
                b.out.push(table);
            }
            Event::Rule => {
                b.flush();
                b.out.push(block(6, 0, 0, -1, "", "", ""));
            }
            Event::Text(t) if b.in_code => b.code.push_str(&t),
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => b.md.push_str(&escape_md(&t)),
            Event::Code(t) => b.md.push_str(&code_md(&t)),
            Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => b.md.push('*'),
            Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => b.md.push_str("**"),
            Event::Start(Tag::Strikethrough) | Event::End(TagEnd::Strikethrough) => {
                b.md.push_str("~~")
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                b.md.push('[');
                b.links.push(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                let dest = b.links.pop().unwrap_or_default();
                let dest = dest
                    .replace(' ', "%20")
                    .replace('(', "%28")
                    .replace(')', "%29");
                b.md.push_str(&format!("]({dest})"));
            }
            // A soft break keeps its line (as GitHub and Obsidian show it), so a quote's two lines stay two lines.
            Event::SoftBreak => b.md.push('\n'),
            Event::HardBreak => b.md.push('\n'),
            // Images show their alt text (the text events); other tags add nothing.
            _ => {}
        }
    }
    b.flush();
    b.out
}

// --- JSON -------------------------------------------------------------------------------------------------------------

fn parse_json(app: &Rc<App>, text: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Some(v),
        Err(e) => {
            let msg = e.to_string();
            let msg = msg
                .split(" at line ")
                .next()
                .unwrap_or_default()
                .to_string();
            doc(app).set_message(
                format!(
                    "Not valid JSON: line {}, column {}: {msg}",
                    e.line(),
                    e.column()
                )
                .into(),
            );
            let off = offset_of(text, e.line(), e.column());
            select(app, off, off);
            None
        }
    }
}

/// The byte offset of a 1-based line and column.
fn offset_of(text: &str, line: usize, col: usize) -> usize {
    let mut off = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let mut o = (off + col.saturating_sub(1)).min(off + l.len());
            while !text.is_char_boundary(o) {
                o -= 1;
            }
            return o;
        }
        off += l.len();
    }
    text.len()
}

fn pretty(v: &Value, indent: &[u8]) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(indent),
    );
    let _ = v.serialize(&mut ser);
    String::from_utf8(buf).unwrap_or_default()
}

fn format_json(app: &Rc<App>, indent: i32) {
    let text = text_of(app);
    let Some(v) = parse_json(app, &text) else {
        return;
    };
    let spaces = if indent == 4 { "    " } else { "  " };
    set_text(app, &pretty(&v, spaces.as_bytes()));
    doc(app).set_message(format!("Formatted with {} spaces", spaces.len()).into());
}

fn minify_json(app: &Rc<App>) {
    let text = text_of(app);
    let Some(v) = parse_json(app, &text) else {
        return;
    };
    let out = serde_json::to_string(&v).unwrap_or_default();
    let saved = text.chars().count().saturating_sub(out.chars().count());
    set_text(app, &out);
    doc(app).set_message(format!("Minified ({saved} characters less)").into());
}

fn validate_json(app: &Rc<App>) {
    let text = text_of(app);
    let Some(v) = parse_json(app, &text) else {
        return;
    };
    let what = match &v {
        Value::Object(m) => format!("{} keys", m.len()),
        Value::Array(a) => format!("{} items", a.len()),
        _ => "one value".to_string(),
    };
    doc(app).set_message(format!("Valid JSON: {what}").into());
}

fn sort_json(app: &Rc<App>) {
    let text = text_of(app);
    let Some(mut v) = parse_json(app, &text) else {
        return;
    };
    sort_keys(&mut v);
    set_text(app, &pretty(&v, b"  "));
    doc(app).set_message("Keys sorted (A to Z, every level)".into());
}

/// Puts every object's keys in order, at every level.
fn sort_keys(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> = std::mem::take(m).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, child) in entries.iter_mut() {
                sort_keys(child);
            }
            *m = entries.into_iter().collect();
        }
        Value::Array(a) => a.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

fn toggle_node(app: &Rc<App>, path: &str) {
    {
        let mut st = app.text.borrow_mut();
        if !st.collapsed.remove(path) {
            st.collapsed.insert(path.to_string());
        }
    }
    refresh(app);
}

/// One row of the tree: its depth, key, value summary, and whether it folds (containers with items).
fn walk(
    v: &Value,
    path: &str,
    label: String,
    depth: i32,
    collapsed: &HashSet<String>,
    rows: &mut Vec<ui::JsonRow>,
) {
    let (container, value) = match v {
        Value::Object(m) if !m.is_empty() => (true, format!("{} keys", m.len())),
        Value::Array(a) if !a.is_empty() => (true, format!("{} items", a.len())),
        Value::Object(_) => (false, "{}".to_string()),
        Value::Array(_) => (false, "[]".to_string()),
        scalar => {
            let text = serde_json::to_string(scalar).unwrap_or_default();
            let short: String = text.chars().take(160).collect();
            (
                false,
                if short.len() < text.len() {
                    format!("{short}…")
                } else {
                    short
                },
            )
        }
    };
    let open = !collapsed.contains(path);
    rows.push(ui::JsonRow {
        depth,
        label: label.into(),
        value: value.into(),
        container,
        open,
        path: path.into(),
    });
    if container && open {
        match v {
            Value::Object(m) => {
                for (k, child) in m {
                    walk(
                        child,
                        &format!("{path}/{k}"),
                        k.to_string(),
                        depth + 1,
                        collapsed,
                        rows,
                    );
                }
            }
            Value::Array(a) => {
                for (i, child) in a.iter().enumerate() {
                    walk(
                        child,
                        &format!("{path}/{i}"),
                        format!("[{i}]"),
                        depth + 1,
                        collapsed,
                        rows,
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_round_trip() {
        let text = "héllo\nwörld";
        for enc in [Enc::Utf8, Enc::Utf8Bom, Enc::Utf16Le, Enc::Utf16Be] {
            let (back_enc, back) = decode(&encode(enc, text));
            assert_eq!(back_enc, enc);
            assert_eq!(back, text);
        }
    }

    #[test]
    fn finds_ignoring_ascii_case() {
        assert_eq!(find_bytes("Hello hello", "HELLO", 1), Some(6));
        assert_eq!(matches_of("aXa xa", "A").len(), 3);
        assert_eq!(offset_of("ab\ncd", 2, 2), 4);
    }

    #[test]
    fn markdown_blocks_cover_the_common_forms() {
        let md = "# Title\n\nSome *text*.\n\n- [x] done\n- open\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncode\n```\n\n> quote\n\n---\n";
        let blocks = markdown_blocks(md);
        let kinds: Vec<i32> = blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![0, 1, 2, 2, 7, 3, 4, 6]);
        assert_eq!(blocks[2].check, 1);
        assert_eq!(blocks[3].check, -1);
    }

    #[test]
    fn sorts_keys_at_every_level() {
        let mut v: Value =
            serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":[{"z":0,"y":0}]}}"#).unwrap();
        sort_keys(&mut v);
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"a":{"c":[{"y":0,"z":0}],"d":2},"b":1}"#
        );
    }
}
