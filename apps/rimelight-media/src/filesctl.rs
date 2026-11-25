//! Files tab: a simple file explorer. It browses folders and archives (zip, 7z, rar), and does copy, cut, paste, rename,
//! new folder, delete to the Recycle Bin, batch rename (with a preview and undo), archive create and extract.
//!
//! The state lives in `App::explorer`. Archive jobs run on a worker thread and come back through `app::post`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle as _, Model as _, ModelRc, SharedString, VecModel};

use crate::app::{App, open_path, post, toast, with_app};
use crate::archive::{self, ArchiveError};
use crate::fileops::{self, BatchRule, PlannedRename, Undo};
use crate::files::{self, Kind};
use crate::prefs;
use crate::ui;

/// One row of the table: a file or folder on disk, or an entry inside the open archive.
#[derive(Clone)]
struct Row {
    name: String,
    kind: &'static str,
    size: Option<u64>,
    modified: Option<SystemTime>,
    is_dir: bool,
    /// The file on disk (folder view).
    path: Option<PathBuf>,
    /// The entry's path inside the archive (archive view), with `/` between folders.
    entry: Option<String>,
}

/// An archive being browsed: its entries, its password (when it has one), and the folder inside it.
struct ArchiveView {
    path: PathBuf,
    password: Option<String>,
    entries: Vec<archive::Entry>,
    /// The folder inside the archive, with `/` between names. Empty for the archive's root.
    prefix: String,
}

#[derive(Clone)]
enum Target {
    Dir(PathBuf),
    /// A folder inside the open archive (the prefix).
    Inside(String),
    /// The Recycle Bin view (its only crumb).
    Recycle,
}

#[derive(Clone)]
struct Crumb {
    label: String,
    target: Target,
}

/// A job waiting for a password.
enum Pending {
    OpenArchive(PathBuf),
    Extract { archive: PathBuf, dest: PathBuf },
}

#[derive(Default)]
pub struct ExplorerState {
    /// The folder on disk being shown (None until the tab opens).
    dir: Option<PathBuf>,
    archive: Option<ArchiveView>,
    /// The Recycle Bin view (read-only): the records of every drive's $Recycle.Bin.
    recycle: bool,
    rows: Vec<Row>,
    selected: Vec<bool>,
    anchor: Option<usize>,
    /// Table column being sorted (0 name, 1 type, 2 size, 3 modified), and the direction.
    sort_col: i32,
    sort_desc: bool,
    /// Copied or cut paths. True for cut.
    clipboard: Option<(Vec<PathBuf>, bool)>,
    undo: Vec<Undo>,
    pending: Option<Pending>,
    /// Shown in the status bar while an archive job runs.
    busy: String,
    crumbs: Vec<Crumb>,
    /// The batch rename plan from the last preview (what Apply does).
    batch: Vec<PlannedRename>,
    /// The rows the list shows, kept so that a selection change updates them in place.
    rows_model: Option<Rc<VecModel<ui::FileRow>>>,
    /// The folders the tree has open.
    expanded: HashSet<PathBuf>,
    /// The folder each sidebar row stands for (None for headers, hints and the Recycle Bin), in sidebar order.
    side_paths: Vec<Option<PathBuf>>,
    /// The kind of each sidebar row, in the same order (for the page's icon, and to tell the Recycle Bin apart).
    side_kinds: Vec<&'static str>,
    /// Folders pinned to the sidebar, kept in %APPDATA%\Rimelight Media\pinned.txt.
    pinned: Vec<PathBuf>,
    /// Folders picked in the sidebar with Ctrl or Shift. While any are picked, the commands act on them, not on the list.
    tree_selected: HashSet<PathBuf>,
    /// The folder a Shift click extends the selection from.
    tree_anchor: Option<PathBuf>,
    /// Counts the list's scroll requests, so the page scrolls even when the same row is asked for again.
    scroll_serial: i32,
}

fn explorer(app: &Rc<App>) -> std::cell::RefMut<'_, ExplorerState> {
    app.explorer.borrow_mut()
}

fn files_global(app: &Rc<App>) -> ui::Files<'_> {
    app.ui.global::<ui::Files>()
}

pub fn wire(app: &Rc<App>) {
    let f = files_global(app);
    f.on_navigate(|i| with_app(move |a| navigate(a, i as usize)));
    f.on_up(|| with_app(up));
    f.on_refresh(|| with_app(|a| reload(a)));
    f.on_picked(|row, shift, ctrl| with_app(move |a| pick(a, row as usize, shift, ctrl)));
    f.on_activated(|row| with_app(move |a| activate(a, row as usize)));
    f.on_sort_requested(|col| with_app(move |a| sort_by(a, col)));
    f.on_side_clicked(|i, shift, ctrl| with_app(move |a| side_clicked(a, i as usize, shift, ctrl)));
    f.on_side_toggled(|i| with_app(move |a| side_toggled(a, i as usize)));
    f.on_side_unpin(|i| with_app(move |a| side_unpin(a, i as usize)));
    f.on_pin_toggle(|| with_app(pin_toggle));
    f.on_new_folder_start(|| {
        with_app(|a| {
            if blocked(a) {
                refuse_read_only(a);
                return;
            }
            open_dialog(a, 1, "New folder");
        })
    });
    f.on_new_folder_submit(|name| with_app(move |a| new_folder_submit(a, &name)));
    f.on_rename_start(|| with_app(rename_start));
    f.on_rename_submit(|name| with_app(move |a| rename_submit(a, &name)));
    f.on_copy(|| with_app(|a| clipboard_op(a, false)));
    f.on_cut(|| with_app(|a| clipboard_op(a, true)));
    f.on_paste(|| with_app(paste));
    f.on_delete_start(|| with_app(delete_start));
    f.on_delete_submit(|| with_app(delete_submit));
    f.on_undo(|| with_app(undo));
    f.on_batch_start(|| with_app(batch_start));
    f.on_batch_preview(|pattern, find, replace, start, pad| {
        with_app(move |a| batch_preview(a, &pattern, &find, &replace, &start, &pad))
    });
    f.on_batch_apply(|pattern, find, replace, start, pad| {
        with_app(move |a| batch_apply(a, &pattern, &find, &replace, &start, &pad))
    });
    f.on_archive_start(|| with_app(archive_start));
    f.on_archive_submit(|name, password| with_app(move |a| archive_submit(a, &name, &password)));
    f.on_password_submit(|pw| with_app(move |a| password_submit(a, &pw)));
    f.on_dialog_cancel(|| {
        with_app(|a| {
            explorer(a).pending = None;
            close_dialog(a);
        })
    });
    f.on_extract_here(|| with_app(extract_here));
    f.on_extract_to(|| with_app(extract_to));
}

/// Keys on the Files tab: F2 rename, Delete to the Recycle Bin, Ctrl+C copy, Ctrl+X cut, Ctrl+V paste, Ctrl+Z undo.
/// Nothing happens while a dialog is open (its text fields take the keys). Returns true when the key was used.
pub fn on_key(app: &Rc<App>, text: &str, ctrl: bool, shift: bool) -> bool {
    use slint::platform::Key;
    if files_global(app).get_dialog() != 0 {
        return false;
    }
    let k = |key: Key| text == SharedString::from(key).as_str();
    if !ctrl && !shift {
        if k(Key::F2) {
            rename_start(app);
            return true;
        }
        if k(Key::Delete) {
            delete_start(app);
            return true;
        }
        if k(Key::UpArrow) || k(Key::DownArrow) {
            move_selection(app, k(Key::DownArrow));
            return true;
        }
        if k(Key::Return) {
            let first = explorer(app).selected.iter().position(|s| *s);
            if let Some(i) = first {
                activate(app, i);
            }
            return true;
        }
        if k(Key::Backspace) {
            up(app);
            return true;
        }
    }
    if ctrl && !shift {
        match text {
            "c" | "C" => clipboard_op(app, false),
            "x" | "X" => clipboard_op(app, true),
            "v" | "V" => paste(app),
            "z" | "Z" => undo(app),
            _ => return false,
        }
        return true;
    }
    false
}

/// Up and Down move the selection one row (the first Down picks the first row, the first Up the last).
fn move_selection(app: &Rc<App>, down: bool) {
    let target = {
        let st = explorer(app);
        let len = st.rows.len();
        if len == 0 {
            return;
        }
        match (st.selected.iter().position(|s| *s), down) {
            (Some(i), true) => (i + 1).min(len - 1),
            (Some(i), false) => i.saturating_sub(1),
            (None, true) => 0,
            (None, false) => len - 1,
        }
    };
    pick(app, target, false, false);
}

/// Called when the Files tab opens: starts in the home folder the first time.
pub fn enter(app: &Rc<App>) {
    {
        let mut st = explorer(app);
        if st.dir.is_none() && st.archive.is_none() {
            let home = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            st.dir = Some(home);
            st.pinned = prefs::load_pinned();
        }
    }
    reload(app);
}

fn in_archive(app: &Rc<App>) -> bool {
    explorer(app).archive.is_some()
}

/// Whether changes are refused in the current view (an archive, or the Recycle Bin).
fn blocked(app: &Rc<App>) -> bool {
    read_only(&explorer(app))
}

// ---- Loading the view -------------------------------------------------------------------------------------------

/// Reads the current folder or archive folder into the rows, and pushes them to the page.
fn reload(app: &Rc<App>) {
    let result = {
        let mut st = explorer(app);
        load(&mut st)
    };
    if let Err(e) = result {
        toast(app, format!("Could not open the folder: {e}"));
    }
    push(app);
}

fn load(st: &mut ExplorerState) -> Result<(), String> {
    let mut rows = if st.recycle {
        recycle_rows()
    } else if let Some(av) = &st.archive {
        archive_rows(av)
    } else {
        let dir = st.dir.clone().ok_or("No folder.")?;
        let items = fileops::list_dir(&dir).map_err(|e| e.to_string())?;
        items
            .into_iter()
            .map(|item| Row {
                kind: type_label(&item.name, item.is_dir),
                name: item.name,
                size: if item.is_dir { None } else { Some(item.size) },
                modified: item.modified,
                is_dir: item.is_dir,
                path: Some(item.path),
                entry: None,
            })
            .collect()
    };
    sort_rows(&mut rows, st.sort_col, st.sort_desc);
    st.selected = vec![false; rows.len()];
    st.anchor = None;
    st.tree_selected.clear();
    st.rows = rows;
    st.crumbs = crumbs_for(st);
    Ok(())
}

/// The direct children of the archive folder `prefix`: folders (once each) and files.
fn archive_rows(av: &ArchiveView) -> Vec<Row> {
    let base = if av.prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", av.prefix)
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for e in &av.entries {
        let Some(rest) = e.path.strip_prefix(base.as_str()) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            Some((folder, _)) => {
                if seen.insert(folder.to_string()) {
                    out.push(Row {
                        name: folder.to_string(),
                        kind: "Folder",
                        size: None,
                        modified: None,
                        is_dir: true,
                        path: None,
                        entry: Some(format!("{base}{folder}")),
                    });
                }
            }
            None => {
                if seen.insert(rest.to_string()) {
                    out.push(Row {
                        name: rest.to_string(),
                        kind: if e.encrypted && !e.is_dir {
                            "Encrypted"
                        } else {
                            type_label(rest, e.is_dir)
                        },
                        size: if e.is_dir { None } else { Some(e.size) },
                        modified: None,
                        is_dir: e.is_dir,
                        path: None,
                        entry: Some(e.path.clone()),
                    });
                }
            }
        }
    }
    out
}

/// Folders first, then by the chosen column (name, type, size, modified). Names compare without case.
fn sort_rows(rows: &mut [Row], col: i32, desc: bool) {
    rows.sort_by(|a, b| {
        let by_column = match col {
            1 => a.kind.cmp(b.kind),
            2 => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
            3 => a.modified.cmp(&b.modified),
            _ => std::cmp::Ordering::Equal,
        };
        let by_name = a.name.to_lowercase().cmp(&b.name.to_lowercase());
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| if desc { by_column.reverse() } else { by_column })
            .then_with(|| if desc { by_name.reverse() } else { by_name })
    });
}

fn type_label(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "Folder";
    }
    let path = Path::new(name);
    if archive::format_of(path).is_some() {
        return "Archive";
    }
    match files::kind_of(path) {
        Some(Kind::Image) => "Picture",
        Some(Kind::Video) => "Video",
        Some(Kind::Audio) => "Audio",
        Some(Kind::Text) => "Text",
        None => "File",
    }
}

/// The breadcrumb trail: the folders down to the current one, then (inside an archive) the archive and its folders.
fn crumbs_for(st: &ExplorerState) -> Vec<Crumb> {
    if st.recycle {
        return vec![Crumb {
            label: "Recycle Bin".into(),
            target: Target::Recycle,
        }];
    }
    let mut out = Vec::new();
    let folder = match &st.archive {
        Some(av) => av.path.parent().map(Path::to_path_buf),
        None => st.dir.clone(),
    };
    if let Some(folder) = folder {
        let mut ancestors: Vec<PathBuf> = folder.ancestors().map(Path::to_path_buf).collect();
        ancestors.reverse();
        for p in ancestors {
            let label = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            out.push(Crumb {
                label,
                target: Target::Dir(p),
            });
        }
    }
    if let Some(av) = &st.archive {
        let name = av
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(Crumb {
            label: name,
            target: Target::Inside(String::new()),
        });
        let mut acc = String::new();
        for part in av.prefix.split('/').filter(|p| !p.is_empty()) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            out.push(Crumb {
                label: part.to_string(),
                target: Target::Inside(acc.clone()),
            });
        }
    }
    out
}

/// Sends the rows, the trail and the flags to the page.
/// Sends everything the page shows: the rows (a new model), the folder tree, the trail, and the flags and details.
/// Used when the folder changes. A click only changes the selection (see `refresh_selection`).
fn push(app: &Rc<App>) {
    let f = files_global(app);
    {
        let mut st = explorer(app);
        let model = Rc::new(VecModel::from(file_rows(&st)));
        st.rows_model = Some(model.clone());
        f.set_rows(ModelRc::new(model));
        let (items, paths, kinds) = split_entries(sidebar_rows(&st));
        st.side_paths = paths;
        st.side_kinds = kinds;
        f.set_side_rows(ModelRc::new(VecModel::from(items)));
        let crumbs: Vec<SharedString> = st
            .crumbs
            .iter()
            .map(|c| SharedString::from(c.label.as_str()))
            .collect();
        f.set_crumbs(ModelRc::new(VecModel::from(crumbs)));
    }
    common(app);
}

/// Updates the rows that changed selection in place (so a double-click on a row still reaches it), then the flags and
/// the details. Scrolls the list to the row picked, when one is picked.
fn refresh_selection(app: &Rc<App>) {
    {
        let st = explorer(app);
        if let Some(model) = st.rows_model.clone() {
            for (i, row) in file_rows(&st).into_iter().enumerate() {
                if model.row_data(i).map(|r| r.selected) != Some(row.selected) {
                    model.set_row_data(i, row);
                }
            }
        }
    }
    common(app);
}

/// The rows as the page shows them.
fn file_rows(st: &ExplorerState) -> Vec<ui::FileRow> {
    st.rows
        .iter()
        .enumerate()
        .map(|(i, r)| ui::FileRow {
            name: r.name.clone().into(),
            kind: r.kind.into(),
            size: r.size.map(human_size).unwrap_or_default().into(),
            modified: r.modified.map(modified_text).unwrap_or_default().into(),
            selected: st.selected.get(i).copied().unwrap_or(false),
        })
        .collect()
}

/// Windows FILETIME counts 100 ns from 1601; Unix time counts from 1970. This is the difference, in FILETIME ticks.
const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

/// The Recycle Bin view is read-only: it shows what is in the bin, and restoring or emptying it isn't here yet.
fn read_only(st: &ExplorerState) -> bool {
    st.archive.is_some() || st.recycle
}

/// Shows why a change was refused in the current view.
fn refuse_read_only(app: &Rc<App>) {
    let in_archive = explorer(app).archive.is_some();
    toast(
        app,
        if in_archive {
            "Extract the archive first to change its files."
        } else {
            "The Recycle Bin view is read-only for now."
        },
    );
}

/// Opens the Recycle Bin in the list.
fn open_recycle(app: &Rc<App>) {
    {
        let mut st = explorer(app);
        st.recycle = true;
        st.archive = None;
        st.tree_selected.clear();
        st.tree_anchor = None;
    }
    reload(app);
}

/// One Recycle Bin record: its original path, its size, and when it was deleted.
struct RecycleInfo {
    original: String,
    size: u64,
    deleted: SystemTime,
}

/// Reads a `$I` record. Windows 10 (version 2) stores the path's length and the path in UTF-16 after the time; Windows
/// 7 and 8 (version 1) store a fixed 520-byte path field.
fn parse_recycle_info(bytes: &[u8]) -> Option<RecycleInfo> {
    let le64 = |at: usize| -> Option<u64> {
        let raw: [u8; 8] = bytes.get(at..at + 8)?.try_into().ok()?;
        Some(u64::from_le_bytes(raw))
    };
    let version = le64(0)?;
    let size = le64(8)?;
    let filetime = le64(16)?;
    let original = match version {
        1 => utf16_text(bytes.get(24..24 + 520)?),
        2 => {
            let raw: [u8; 4] = bytes.get(24..28)?.try_into().ok()?;
            let chars = u32::from_le_bytes(raw) as usize;
            utf16_text(bytes.get(28..28 + chars * 2)?)
        }
        _ => return None,
    };
    let ticks = filetime.checked_sub(FILETIME_UNIX_DIFF)?.checked_mul(100)?;
    Some(RecycleInfo {
        original,
        size,
        deleted: UNIX_EPOCH + Duration::from_nanos(ticks),
    })
}

/// UTF-16 little-endian text up to the first zero.
fn utf16_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// Every record in the Recycle Bin: one row per `$I` file in each drive's `$Recycle.Bin\<user>` folders. Folders
/// other users own can't be read and are skipped. The row's `entry` holds the original path.
fn recycle_rows() -> Vec<Row> {
    let mut out = Vec::new();
    for letter in 'C'..='Z' {
        let bin = PathBuf::from(format!("{letter}:\\$Recycle.Bin"));
        let Ok(users) = fs::read_dir(&bin) else {
            continue;
        };
        for user in users.flatten() {
            let dir = user.path();
            let Ok(files) = fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if !name.starts_with("$I") {
                    continue;
                }
                let Ok(bytes) = fs::read(file.path()) else {
                    continue;
                };
                let Some(info) = parse_recycle_info(&bytes) else {
                    continue;
                };
                let shown = Path::new(&info.original)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| info.original.clone());
                let is_dir = dir.join(format!("$R{}", &name[2..])).is_dir();
                out.push(Row {
                    kind: if is_dir {
                        "Folder"
                    } else {
                        type_label(&shown, false)
                    },
                    name: shown,
                    size: if is_dir { None } else { Some(info.size) },
                    modified: Some(info.deleted),
                    is_dir,
                    path: None,
                    entry: Some(info.original),
                });
            }
        }
    }
    out
}

/// One sidebar row with its path (None for headers, hints and the Recycle Bin) and its kind for the page's icon.
type SideEntry = (ui::SideItem, Option<PathBuf>, &'static str);

/// The sidebar, top to bottom: Drives (every drive with its folders, cloud folders, the Recycle Bin), Quick access
/// (the user's standard folders), and Pinned (folders the user pinned). Headers and hints have no path.
fn sidebar_rows(st: &ExplorerState) -> Vec<SideEntry> {
    let current = if st.archive.is_none() && !st.recycle {
        st.dir.clone()
    } else {
        None
    };
    let mut out: Vec<SideEntry> = Vec::new();

    push_header(&mut out, "Drives");
    for letter in 'C'..='Z' {
        let root = PathBuf::from(format!("{letter}:\\"));
        if root.exists() {
            add_tree_node(
                &root,
                &format!("{letter}:"),
                0,
                "Drive",
                &st.expanded,
                &st.tree_selected,
                current.as_deref(),
                &mut out,
            );
        }
    }
    for cloud in cloud_folders() {
        let name = file_name_of(&cloud);
        add_tree_node(
            &cloud,
            &name,
            0,
            "Cloud",
            &st.expanded,
            &st.tree_selected,
            current.as_deref(),
            &mut out,
        );
    }
    out.push((
        side_item("Recycle Bin", "Recycle", 0, false, false, false, false, false),
        None,
        "Recycle",
    ));

    push_header(&mut out, "Quick access");
    for (label, kind, path) in quick_access() {
        push_flat(&mut out, &label, kind, path, st, current.as_deref(), false);
    }

    push_header(&mut out, "Pinned");
    if st.pinned.is_empty() {
        out.push((
            side_item("Pin a folder with the pin button", "hint", 0, false, false, false, false, false),
            None,
            "hint",
        ));
    }
    for path in st.pinned.iter().filter(|p| p.is_dir()) {
        push_flat(&mut out, &file_name_of(path), "Folder", path.clone(), st, current.as_deref(), true);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn side_item(
    label: &str,
    kind: &str,
    depth: i32,
    expanded: bool,
    has_children: bool,
    current: bool,
    selected: bool,
    unpinnable: bool,
) -> ui::SideItem {
    ui::SideItem {
        label: label.into(),
        kind: kind.into(),
        depth,
        expanded,
        has_children,
        current,
        selected,
        unpinnable,
    }
}

fn push_header(out: &mut Vec<SideEntry>, title: &str) {
    out.push((side_item(title, "header", 0, false, false, false, false, false), None, "header"));
}

/// A flat sidebar row (no arrow) for a folder with a known kind, such as a quick access folder or a pinned one.
fn push_flat(
    out: &mut Vec<SideEntry>,
    label: &str,
    kind: &'static str,
    path: PathBuf,
    st: &ExplorerState,
    current: Option<&Path>,
    unpinnable: bool,
) {
    let item = side_item(
        label,
        kind,
        0,
        false,
        false,
        current == Some(path.as_path()),
        st.tree_selected.contains(&path),
        unpinnable,
    );
    out.push((item, Some(path), kind));
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The user's standard folders that exist: Desktop, Documents, Downloads (under the profile), and the pictures,
/// music and videos folders as Windows knows them.
fn quick_access() -> Vec<(String, &'static str, PathBuf)> {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default();
    let candidates = vec![
        ("Desktop", "Desktop", home.join("Desktop")),
        ("Documents", "Documents", home.join("Documents")),
        ("Downloads", "Downloads", home.join("Downloads")),
        (
            "Pictures",
            "Picture",
            rimelight_platform::known_folder(rimelight_platform::UserFolder::Pictures)
                .unwrap_or_else(|| home.join("Pictures")),
        ),
        (
            "Music",
            "Audio",
            rimelight_platform::known_folder(rimelight_platform::UserFolder::Music)
                .unwrap_or_else(|| home.join("Music")),
        ),
        (
            "Videos",
            "Video",
            rimelight_platform::known_folder(rimelight_platform::UserFolder::Videos)
                .unwrap_or_else(|| home.join("Videos")),
        ),
    ];
    candidates
        .into_iter()
        .filter(|(_, _, path)| path.is_dir())
        .map(|(label, kind, path)| (label.to_string(), kind, path))
        .collect()
}

/// Cloud sync folders in the profile: Proton Drive, Google Drive and OneDrive, when they exist.
fn cloud_folders() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = fs::read_dir(&home)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .filter(|p| {
                    let name = file_name_of(p);
                    ["Proton Drive", "Google Drive", "OneDrive"]
                        .iter()
                        .any(|cloud| name == *cloud || name.starts_with(&format!("{cloud} ")))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[allow(clippy::too_many_arguments)]
fn add_tree_node(
    path: &Path,
    label: &str,
    depth: i32,
    kind: &'static str,
    expanded: &HashSet<PathBuf>,
    selection: &HashSet<PathBuf>,
    current: Option<&Path>,
    out: &mut Vec<SideEntry>,
) {
    let open = expanded.contains(path);
    let has_children = has_subfolder(path);
    out.push((
        side_item(
            label,
            kind,
            depth,
            open,
            has_children,
            current == Some(path),
            selection.contains(path),
            false,
        ),
        Some(path.to_path_buf()),
        kind,
    ));
    if open {
        for child in subfolders(path) {
            let name = file_name_of(&child);
            add_tree_node(
                &child, &name, depth + 1, "Folder", expanded, selection, current, out,
            );
        }
    }
}

/// Splits the sidebar rows into what the page shows, and the paths and kinds the controller keeps for each row.
fn split_entries(
    entries: Vec<SideEntry>,
) -> (Vec<ui::SideItem>, Vec<Option<PathBuf>>, Vec<&'static str>) {
    let mut rows = Vec::with_capacity(entries.len());
    let mut paths = Vec::with_capacity(entries.len());
    let mut kinds = Vec::with_capacity(entries.len());
    for (item, path, kind) in entries {
        rows.push(item);
        paths.push(path);
        kinds.push(kind);
    }
    (rows, paths, kinds)
}

/// Sends the sidebar again, for a change of its selection, its pins or its open folders.
fn refresh_sidebar(app: &Rc<App>) {
    let entries = {
        let st = explorer(app);
        sidebar_rows(&st)
    };
    let (rows, paths, kinds) = split_entries(entries);
    {
        let mut st = explorer(app);
        st.side_paths = paths;
        st.side_kinds = kinds;
    }
    files_global(app).set_side_rows(ModelRc::new(VecModel::from(rows)));
}

fn has_subfolder(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        })
        .unwrap_or(false)
}

/// The folders directly inside `path`, by name.
fn subfolders(path: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(path)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    out
}

/// A sidebar row was clicked. A plain click opens its folder (the Recycle Bin opens Windows' own view). Ctrl adds or
/// removes it from the folder selection; Shift selects every folder from the anchor to it, in sidebar order.
fn side_clicked(app: &Rc<App>, index: usize, shift: bool, ctrl: bool) {
    let (kind, path) = {
        let st = explorer(app);
        (
            st.side_kinds.get(index).copied().unwrap_or("header"),
            st.side_paths.get(index).cloned().flatten(),
        )
    };
    if kind == "Recycle" {
        open_recycle(app);
        return;
    }
    let Some(path) = path else {
        return;
    };
    if shift || ctrl {
        {
            let mut st = explorer(app);
            let anchor = st.tree_anchor.clone();
            match anchor {
                Some(anchor) if shift => {
                    let from = st.side_paths.iter().position(|p| p.as_ref() == Some(&anchor));
                    if let Some(from) = from {
                        let (lo, hi) = (from.min(index), from.max(index));
                        let range: Vec<PathBuf> = st.side_paths[lo..=hi]
                            .iter()
                            .flatten()
                            .cloned()
                            .collect();
                        st.tree_selected = range.into_iter().collect();
                    }
                }
                _ if ctrl => {
                    if !st.tree_selected.remove(&path) {
                        st.tree_selected.insert(path.clone());
                    }
                    st.tree_anchor = Some(path);
                }
                _ => {
                    st.tree_selected = HashSet::from([path.clone()]);
                    st.tree_anchor = Some(path);
                }
            }
            for s in st.selected.iter_mut() {
                *s = false;
            }
            st.anchor = None;
        }
        refresh_sidebar(app);
        refresh_selection(app);
        return;
    }
    {
        let mut st = explorer(app);
        st.tree_selected.clear();
        st.tree_anchor = Some(path.clone());
        st.archive = None;
        st.recycle = false;
        st.dir = Some(path);
    }
    reload(app);
}

/// A sidebar row's arrow was clicked: open or close that folder.
fn side_toggled(app: &Rc<App>, index: usize) {
    {
        let mut st = explorer(app);
        let path = st.side_paths.get(index).cloned().flatten();
        if let Some(path) = path {
            if !st.expanded.remove(&path) {
                st.expanded.insert(path);
            }
        }
    }
    refresh_sidebar(app);
}

/// The Pin button: pins the one selected folder to the sidebar, or unpins it when it is pinned already.
fn pin_toggle(app: &Rc<App>) {
    let target = {
        let st = explorer(app);
        match selected_paths(&st).as_slice() {
            [one] if st.archive.is_none() && one.is_dir() => Some(one.clone()),
            _ => None,
        }
    };
    let Some(target) = target else {
        toast(app, "Select one folder to pin it.");
        return;
    };
    {
        let mut st = explorer(app);
        if let Some(at) = st.pinned.iter().position(|p| p == &target) {
            st.pinned.remove(at);
        } else {
            st.pinned.push(target);
        }
        prefs::save_pinned(&st.pinned);
    }
    refresh_sidebar(app);
    common(app);
}

/// The x on a pinned folder: removes it from the sidebar.
fn side_unpin(app: &Rc<App>, index: usize) {
    {
        let mut st = explorer(app);
        let path = st.side_paths.get(index).cloned().flatten();
        if let Some(path) = path {
            st.pinned.retain(|p| p != &path);
            prefs::save_pinned(&st.pinned);
        }
    }
    refresh_sidebar(app);
    common(app);
}

/// What the details panel shows: the selection, or the folder when nothing is selected.
struct Details {
    title: String,
    name: String,
    kind: String,
    path: String,
    size: String,
    items: String,
    modified: String,
    created: String,
}

fn details(st: &ExplorerState) -> Details {
    if !st.tree_selected.is_empty() {
        return Details {
            title: format!("{} folders selected", st.tree_selected.len()),
            name: String::new(),
            kind: String::new(),
            path: current_location(st),
            size: String::new(),
            items: String::new(),
            modified: String::new(),
            created: String::new(),
        };
    }
    let picked: Vec<&Row> = st
        .rows
        .iter()
        .zip(st.selected.iter())
        .filter(|(_, s)| **s)
        .map(|(r, _)| r)
        .collect();
    match picked.as_slice() {
        [] => folder_details(st),
        [one] => row_details(st, one),
        many => {
            let total: u64 = many
                .iter()
                .filter(|r| !r.is_dir)
                .filter_map(|r| r.size)
                .sum();
            Details {
                title: format!("{} items selected", many.len()),
                name: String::new(),
                kind: String::new(),
                path: current_location(st),
                size: if total > 0 {
                    human_size(total)
                } else {
                    String::new()
                },
                items: String::new(),
                modified: String::new(),
                created: String::new(),
            }
        }
    }
}

/// Where the folder being shown is: its path, or the archive and its folder inside it.
fn current_location(st: &ExplorerState) -> String {
    if st.recycle {
        return "Recycle Bin".into();
    }
    match &st.archive {
        Some(av) if av.prefix.is_empty() => av.path.display().to_string(),
        Some(av) => format!("{} › {}", av.path.display(), av.prefix.replace('/', " › ")),
        None => st
            .dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_default(),
    }
}

fn folder_details(st: &ExplorerState) -> Details {
    if st.recycle {
        return Details {
            title: "Recycle Bin".into(),
            name: "Recycle Bin".into(),
            kind: "Recycle Bin".into(),
            path: "$Recycle.Bin on each drive".into(),
            size: String::new(),
            items: st.rows.len().to_string(),
            modified: String::new(),
            created: String::new(),
        };
    }
    let (name, path, times) = match &st.archive {
        Some(av) => (
            av.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            av.path.clone(),
            fs::metadata(&av.path).ok(),
        ),
        None => {
            let dir = st.dir.clone().unwrap_or_default();
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string());
            let meta = fs::metadata(&dir).ok();
            (name, dir, meta)
        }
    };
    let archive = st.archive.is_some();
    let label = if archive { "Archive" } else { "Folder" };
    Details {
        title: label.into(),
        name,
        kind: label.into(),
        path: current_location(st),
        size: if archive {
            fs::metadata(&path).map(|m| human_size(m.len())).unwrap_or_default()
        } else {
            String::new()
        },
        items: st.rows.len().to_string(),
        modified: times
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(modified_text)
            .unwrap_or_default(),
        created: times
            .as_ref()
            .and_then(|m| m.created().ok())
            .map(modified_text)
            .unwrap_or_default(),
    }
}

fn row_details(st: &ExplorerState, row: &Row) -> Details {
    let meta = row.path.as_ref().and_then(|p| fs::metadata(p).ok());
    let path = match (&row.path, &st.archive) {
        (Some(p), _) => p.display().to_string(),
        (None, Some(av)) => format!(
            "{} › {}",
            av.path.display(),
            row.entry.as_deref().unwrap_or(row.name.as_str()).replace('/', " › ")
        ),
        // A Recycle Bin record: its original path (kept in `entry`).
        (None, None) => row.entry.clone().unwrap_or_default(),
    };
    let items = match (&row.path, row.is_dir) {
        (Some(p), true) => fs::read_dir(p)
            .map(|rd| rd.count().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    };
    let title = if row.is_dir {
        "Folder"
    } else if archive::format_of(Path::new(&row.name)).is_some() {
        "Archive"
    } else {
        "File"
    };
    Details {
        title: title.into(),
        name: row.name.clone(),
        kind: row.kind.into(),
        path,
        size: if row.is_dir {
            String::new()
        } else {
            row.size.map(human_size).unwrap_or_default()
        },
        items,
        modified: row
            .modified
            .or_else(|| meta.as_ref().and_then(|m| m.modified().ok()))
            .map(modified_text)
            .unwrap_or_default(),
        created: meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .map(modified_text)
            .unwrap_or_default(),
    }
}

/// Sends the flags, the status line, the sort and the details. Shared by a full push and a selection change.
fn common(app: &Rc<App>) {
    let f = files_global(app);
    let st = explorer(app);
    let selected = selection_count(&st);
    let mut status = format!("{} items", st.rows.len());
    if selected > 0 {
        status.push_str(&format!(" · {selected} selected"));
    }
    if !st.busy.is_empty() {
        status = format!("{}   {}", st.busy, status);
    }
    let d = details(&st);
    // The Pin button: one folder selected (not inside an archive). It shows Unpin when that folder is pinned.
    let pin_target = match selected_paths(&st).as_slice() {
        [one] if st.archive.is_none() && one.is_dir() => Some(one.clone()),
        _ => None,
    };
    f.set_can_pin(pin_target.is_some());
    f.set_pin_active(
        pin_target
            .as_ref()
            .map(|p| st.pinned.contains(p))
            .unwrap_or(false),
    );
    f.set_status(status.into());
    f.set_busy(st.busy.clone().into());
    f.set_in_archive(st.archive.is_some());
    f.set_has_selection(selected > 0);
    f.set_single_selected(selected == 1);
    f.set_can_paste(st.clipboard.is_some() && st.archive.is_none() && !st.recycle);
    f.set_read_only(st.archive.is_some() || st.recycle);
    f.set_recycle_view(st.recycle);
    f.set_can_undo(!st.undo.is_empty());
    f.set_sort_column(st.sort_col);
    f.set_sort_descending(st.sort_desc);
    f.set_details_title(d.title.into());
    f.set_details_name(d.name.into());
    f.set_details_kind(d.kind.into());
    f.set_details_path(d.path.into());
    f.set_details_size(d.size.into());
    f.set_details_items(d.items.into());
    f.set_details_modified(d.modified.into());
    f.set_details_created(d.created.into());
}

/// Asks the list to show row `index` (the keyboard and clicks move the selection; the list follows).
fn request_scroll(app: &Rc<App>, index: Option<usize>) {
    let f = files_global(app);
    let serial = {
        let mut st = explorer(app);
        st.scroll_serial = st.scroll_serial.wrapping_add(1);
        st.scroll_serial
    };
    f.set_scroll_row(index.map(|i| i as i32).unwrap_or(-1));
    f.set_scroll_serial(serial);
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn modified_text(t: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = t.into();
    local.format("%Y-%m-%d %H:%M").to_string()
}

// ---- Moving around ----------------------------------------------------------------------------------------------

fn navigate(app: &Rc<App>, index: usize) {
    {
        let mut st = explorer(app);
        let Some(crumb) = st.crumbs.get(index).cloned() else {
            return;
        };
        match crumb.target {
            Target::Dir(p) => {
                st.archive = None;
                st.recycle = false;
                st.dir = Some(p);
            }
            Target::Recycle => {}
            Target::Inside(prefix) => {
                if let Some(av) = st.archive.as_mut() {
                    av.prefix = prefix;
                }
            }
        }
    }
    reload(app);
}

fn up(app: &Rc<App>) {
    {
        let mut st = explorer(app);
        let at_archive_root = st.archive.as_ref().map(|av| av.prefix.is_empty());
        if st.recycle {
            // Up from the Recycle Bin goes to the profile folder.
            st.recycle = false;
            st.dir = std::env::var_os("USERPROFILE").map(PathBuf::from);
        } else if let Some(root) = at_archive_root {
            if root {
                let folder = st
                    .archive
                    .as_ref()
                    .and_then(|av| av.path.parent().map(Path::to_path_buf));
                st.archive = None;
                st.dir = folder;
            } else if let Some(av) = st.archive.as_mut() {
                av.prefix = av
                    .prefix
                    .rsplit_once('/')
                    .map(|(parent, _)| parent.to_string())
                    .unwrap_or_default();
            }
        } else if let Some(parent) = st.dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf) {
            st.dir = Some(parent);
        }
    }
    reload(app);
}

fn sort_by(app: &Rc<App>, col: i32) {
    {
        let mut st = explorer(app);
        if st.sort_col == col {
            st.sort_desc = !st.sort_desc;
        } else {
            st.sort_col = col;
            st.sort_desc = false;
        }
    }
    reload(app);
}

/// Table selection: a plain click selects one row, Ctrl toggles it, Shift selects from the anchor to the row.
fn pick(app: &Rc<App>, row: usize, shift: bool, ctrl: bool) {
    {
        let mut st = explorer(app);
        if row >= st.rows.len() {
            return;
        }
        // Picking a row in the list replaces any folder selection in the tree.
        if !st.tree_selected.is_empty() {
            st.tree_selected.clear();
            st.tree_anchor = None;
            drop(st);
            refresh_sidebar(app);
            st = explorer(app);
        }
        if shift && st.anchor.is_some() {
            let anchor = st.anchor.unwrap_or(row);
            let (lo, hi) = (anchor.min(row), anchor.max(row));
            for (i, s) in st.selected.iter_mut().enumerate() {
                *s = i >= lo && i <= hi;
            }
        } else if ctrl {
            let s = &mut st.selected[row];
            *s = !*s;
            st.anchor = Some(row);
        } else {
            for s in st.selected.iter_mut() {
                *s = false;
            }
            st.selected[row] = true;
            st.anchor = Some(row);
        }
    }
    refresh_selection(app);
    request_scroll(app, Some(row));
}

/// Double-click or Enter: a folder opens, an archive opens as a folder (asking for its password if it has one), and
/// a file opens in its own tab. Inside an archive a file is extracted to a temporary folder and then opened.
fn activate(app: &Rc<App>, row: usize) {
    if explorer(app).recycle {
        toast(app, "Restoring and opening recycled files isn't here yet.");
        return;
    }
    let target = {
        let st = explorer(app);
        match st.rows.get(row).cloned() {
            Some(r) => r,
            None => return,
        }
    };
    if in_archive(app) {
        if target.is_dir {
            let prefix = target.entry.unwrap_or_default();
            if let Some(av) = explorer(app).archive.as_mut() {
                av.prefix = prefix;
            }
            reload(app);
        } else {
            open_from_archive(app, target.entry.unwrap_or_default());
        }
        return;
    }
    let Some(path) = target.path else {
        return;
    };
    if target.is_dir {
        explorer(app).dir = Some(path);
        reload(app);
    } else if archive::format_of(&path).is_some() {
        enter_archive(app, path, None);
    } else {
        open_path(app, &path);
    }
}

fn enter_archive(app: &Rc<App>, path: PathBuf, password: Option<String>) {
    match archive::list(&path, password.as_deref()) {
        Ok(entries) => {
            {
                let mut st = explorer(app);
                st.recycle = false;
                st.archive = Some(ArchiveView {
                    path,
                    password,
                    entries,
                    prefix: String::new(),
                });
            }
            reload(app);
        }
        Err(ArchiveError::PasswordRequired) => {
            explorer(app).pending = Some(Pending::OpenArchive(path.clone()));
            open_password_dialog(app, "This archive is protected. Enter its password.");
        }
        Err(ArchiveError::WrongPassword) => {
            explorer(app).pending = Some(Pending::OpenArchive(path));
            open_password_dialog(app, "The password is wrong.");
        }
        Err(ArchiveError::Failed(text)) => toast(app, format!("Could not open the archive: {text}")),
    }
}

fn open_from_archive(app: &Rc<App>, entry: String) {
    let (archive_path, password, stem) = {
        let st = explorer(app);
        let Some(av) = st.archive.as_ref() else {
            return;
        };
        let stem = av
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
        (av.path.clone(), av.password.clone(), stem)
    };
    let dest = std::env::temp_dir().join("RimelightMedia").join("archive").join(&stem);
    let file = dest.join(entry.replace('/', std::path::MAIN_SEPARATOR_STR));
    let job_dest = dest.clone();
    start_job(app, "Extracting to a temporary folder…", move || {
        archive::extract_all(&archive_path, &job_dest, password.as_deref())
    }, move |a, result| match result {
        Ok(_) => open_path(a, &file),
        Err(e) => toast(a, format!("Could not extract the file: {e}")),
    });
}

// ---- Clipboard, rename, new folder, delete ----------------------------------------------------------------------

/// What the commands act on: the folders picked in the tree while there are any, otherwise the picked rows.
fn selected_paths(st: &ExplorerState) -> Vec<PathBuf> {
    if !st.tree_selected.is_empty() {
        let mut picked: Vec<PathBuf> = st.tree_selected.iter().cloned().collect();
        picked.sort();
        return picked;
    }
    st.rows
        .iter()
        .zip(st.selected.iter())
        .filter(|(_, s)| **s)
        .filter_map(|(r, _)| r.path.clone())
        .collect()
}

/// How many items the selection holds (folders from the tree, or rows from the list).
fn selection_count(st: &ExplorerState) -> usize {
    if !st.tree_selected.is_empty() {
        st.tree_selected.len()
    } else {
        st.selected.iter().filter(|s| **s).count()
    }
}

fn clipboard_op(app: &Rc<App>, cut: bool) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    let paths = selected_paths(&explorer(app));
    if paths.is_empty() {
        toast(app, "Select files or folders first.");
        return;
    }
    let n = paths.len();
    explorer(app).clipboard = Some((paths, cut));
    toast(
        app,
        if cut {
            format!("Cut {n} item(s). Paste them into a folder to move them.")
        } else {
            format!("Copied {n} item(s).")
        },
    );
    push(app);
}

fn paste(app: &Rc<App>) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    let (items, cut, dir) = {
        let st = explorer(app);
        let Some((items, cut)) = st.clipboard.clone() else {
            return;
        };
        (items, cut, st.dir.clone())
    };
    let Some(dir) = dir else {
        return;
    };
    let mut first_error: Option<String> = None;
    let mut moved_undo = Vec::new();
    let mut done = 0;
    for src in &items {
        let result: Result<Option<Undo>, String> = if cut {
            if src.parent() == Some(dir.as_path()) {
                Err("Already in this folder.".to_string())
            } else if src.is_dir() && dir.starts_with(src) {
                Err("A folder can't be moved into itself.".to_string())
            } else {
                fileops::move_into(src, &dir).map(|(_, undo)| Some(undo))
            }
        } else if src.is_dir() && dir.starts_with(src) {
            Err("A folder can't be copied into itself.".to_string())
        } else {
            fileops::copy_into(src, &dir).map(|_| None)
        };
        match result {
            Ok(undo) => {
                done += 1;
                moved_undo.extend(undo);
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    {
        let mut st = explorer(app);
        st.undo.extend(moved_undo);
        if cut && first_error.is_none() {
            st.clipboard = None;
        }
    }
    if let Some(e) = first_error {
        toast(app, format!("Some items were not pasted: {e}"));
    } else if done > 0 {
        toast(app, format!("Pasted {done} item(s)."));
    }
    reload(app);
}

fn new_folder_submit(app: &Rc<App>, name: &str) {
    let dir = explorer(app).dir.clone();
    let Some(dir) = dir else {
        return;
    };
    match fileops::new_folder(&dir, name) {
        Ok((_, undo)) => {
            explorer(app).undo.push(undo);
            close_dialog(app);
            reload(app);
        }
        Err(e) => set_dialog_error(app, &e),
    }
}

fn rename_start(app: &Rc<App>) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    let name = {
        let st = explorer(app);
        match selected_paths(&st).as_slice() {
            [one] => one
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            _ => {
                toast(app, "Select one file or folder to rename.");
                return;
            }
        }
    };
    open_dialog(app, 2, &name);
}

fn rename_submit(app: &Rc<App>, name: &str) {
    let path = selected_paths(&explorer(app)).into_iter().next();
    let Some(path) = path else {
        return;
    };
    match fileops::rename(&path, name) {
        Ok(undo) => {
            explorer(app).undo.push(undo);
            close_dialog(app);
            reload(app);
        }
        Err(e) => set_dialog_error(app, &e),
    }
}

fn delete_start(app: &Rc<App>) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    let names: Vec<String> = selected_paths(&explorer(app))
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    match names.len() {
        0 => toast(app, "Select files or folders first."),
        1 => open_dialog(app, 6, &names[0]),
        n => open_dialog(app, 6, &format!("{n} items")),
    }
}

fn delete_submit(app: &Rc<App>) {
    let paths = selected_paths(&explorer(app));
    let mut failed = 0;
    for p in &paths {
        if !fileops::recycle(p) {
            failed += 1;
        }
    }
    close_dialog(app);
    if failed > 0 {
        toast(app, format!("{failed} item(s) could not go to the Recycle Bin."));
    }
    reload(app);
}

fn undo(app: &Rc<App>) {
    let record = explorer(app).undo.pop();
    let Some(record) = record else {
        return;
    };
    if let Err(e) = fileops::undo(&record) {
        toast(app, format!("Could not undo: {e}"));
    }
    reload(app);
}

// ---- Batch rename -----------------------------------------------------------------------------------------------

fn batch_start(app: &Rc<App>) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    if selected_paths(&explorer(app)).is_empty() {
        toast(app, "Select files or folders first.");
        return;
    }
    explorer(app).batch.clear();
    open_dialog(app, 3, "{name}-{n}");
    let f = files_global(app);
    f.set_batch_rows(ModelRc::new(VecModel::from(Vec::<ui::TableRow>::new())));
    f.set_batch_summary("Type a pattern to see the new names.".into());
    f.set_batch_ok(false);
}

fn rule_from(pattern: &str, find: &str, replace: &str, start: &str, pad: &str) -> Result<BatchRule, String> {
    let start = if start.trim().is_empty() {
        1
    } else {
        start
            .trim()
            .parse::<u32>()
            .map_err(|_| "Start must be a whole number.".to_string())?
    };
    let pad = if pad.trim().is_empty() {
        0
    } else {
        pad.trim()
            .parse::<usize>()
            .map_err(|_| "Digits must be a whole number.".to_string())?
    };
    Ok(BatchRule {
        pattern: pattern.to_string(),
        find: find.to_string(),
        replace: replace.to_string(),
        start,
        pad,
    })
}

/// Plans the rename from the current text and shows the before and after names. Apply does nothing while a line has
/// a problem.
fn batch_preview(app: &Rc<App>, pattern: &str, find: &str, replace: &str, start: &str, pad: &str) {
    let f = files_global(app);
    let rule = match rule_from(pattern, find, replace, start, pad) {
        Ok(r) => r,
        Err(e) => {
            f.set_batch_summary(e.into());
            f.set_batch_ok(false);
            return;
        }
    };
    let paths = selected_paths(&explorer(app));
    let plan = fileops::plan_batch(&paths, &rule);
    let problems = plan.iter().filter(|p| p.error.is_some()).count();
    let rows: Vec<ui::TableRow> = plan
        .iter()
        .map(|p| {
            let old = p
                .from
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ui::TableRow {
                cells: ModelRc::new(VecModel::from(vec![
                    SharedString::from(old.as_str()),
                    SharedString::from(p.new_name.as_str()),
                    SharedString::from(p.error.clone().unwrap_or_else(|| "OK".into()).as_str()),
                ])),
                selected: false,
            }
        })
        .collect();
    let summary = if problems == 0 {
        format!("{} renames ready.", plan.len())
    } else {
        format!("{problems} of {} can't be renamed. Fix them to apply.", plan.len())
    };
    f.set_batch_rows(ModelRc::new(VecModel::from(rows)));
    f.set_batch_summary(summary.into());
    f.set_batch_ok(problems == 0 && !plan.is_empty());
    explorer(app).batch = plan;
}

fn batch_apply(app: &Rc<App>, pattern: &str, find: &str, replace: &str, start: &str, pad: &str) {
    batch_preview(app, pattern, find, replace, start, pad);
    let plan = std::mem::take(&mut explorer(app).batch);
    if plan.is_empty() || plan.iter().any(|p| p.error.is_some()) {
        return;
    }
    let (done, error) = fileops::apply_batch(&plan);
    let count = done.len();
    {
        let mut st = explorer(app);
        st.undo.extend(done);
    }
    close_dialog(app);
    match error {
        Some(e) => toast(app, format!("Renamed {count}, but some failed: {e}")),
        None => toast(app, format!("Renamed {count} item(s). Undo can bring the names back.")),
    }
    reload(app);
}

// ---- Archives ---------------------------------------------------------------------------------------------------

fn archive_start(app: &Rc<App>) {
    if blocked(app) {
        refuse_read_only(app);
        return;
    }
    let suggested = {
        let st = explorer(app);
        let paths = selected_paths(&st);
        match paths.as_slice() {
            [] => None,
            [one] => Some(match one.file_stem() {
                Some(stem) if !one.is_dir() => format!("{}.zip", stem.to_string_lossy()),
                _ => format!("{}.zip", one.file_name().unwrap_or_default().to_string_lossy()),
            }),
            _ => Some("Archive.zip".to_string()),
        }
    };
    match suggested {
        Some(name) => open_dialog(app, 4, &name),
        None => toast(app, "Select files or folders first."),
    }
}

fn archive_submit(app: &Rc<App>, name: &str, password: &str) {
    if let Err(e) = fileops::check_name(name) {
        set_dialog_error(app, &e);
        return;
    }
    let (sources, dir) = {
        let st = explorer(app);
        (selected_paths(&st), st.dir.clone())
    };
    let Some(dir) = dir else {
        return;
    };
    if sources.is_empty() {
        set_dialog_error(app, "Select files or folders first.");
        return;
    }
    let mut file_name = name.trim().to_string();
    if !file_name.to_lowercase().ends_with(".zip") {
        file_name.push_str(".zip");
    }
    let dest = fileops::unique_path(&dir, &file_name);
    let pw = if password.is_empty() {
        None
    } else {
        Some(password.to_string())
    };
    close_dialog(app);
    let shown = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    start_job(
        app,
        "Compressing…",
        move || archive::create_zip(&sources, &dest, pw.as_deref()),
        move |a, result: Result<usize, ArchiveError>| match result {
            Ok(n) => {
                toast(a, format!("Created {shown} with {n} file(s)."));
                reload(a);
            }
        Err(e) => {
            toast(a, format!("Could not create the archive: {e}"));
            reload(a);
        }
    });
}

fn extract_here(app: &Rc<App>) {
    let (archive_path, password, dest) = {
        let st = explorer(app);
        if let Some(av) = st.archive.as_ref() {
            let parent = av.path.parent().map(Path::to_path_buf).unwrap_or_default();
            let stem = av.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            (av.path.clone(), av.password.clone(), fileops::unique_path(&parent, &stem))
        } else {
            let picked: Vec<PathBuf> = selected_paths(&st)
                .into_iter()
                .filter(|p| archive::format_of(p).is_some())
                .collect();
            let Some(path) = picked.first().cloned() else {
                toast(app, "Select an archive first.");
                return;
            };
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            (path, None, fileops::unique_path(&parent, &stem))
        }
    };
    start_extract(app, archive_path, dest, password);
}

fn extract_to(app: &Rc<App>) {
    let (archive_path, password, start_dir) = {
        let st = explorer(app);
        if let Some(av) = st.archive.as_ref() {
            (av.path.clone(), av.password.clone(), st.dir.clone())
        } else {
            let picked = selected_paths(&st)
                .into_iter()
                .find(|p| archive::format_of(p).is_some());
            let Some(path) = picked else {
                toast(app, "Select an archive first.");
                return;
            };
            (path, None, st.dir.clone())
        }
    };
    let Some(chosen) = rfd::FileDialog::new()
        .set_directory(start_dir.unwrap_or_default())
        .pick_folder()
    else {
        return;
    };
    let stem = archive_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest = fileops::unique_path(&chosen, &stem);
    start_extract(app, archive_path, dest, password);
}

fn start_extract(app: &Rc<App>, archive_path: PathBuf, dest: PathBuf, password: Option<String>) {
    let archive_for_job = archive_path.clone();
    let dest_for_job = dest.clone();
    let pw_for_job = password.clone();
    start_job(app, "Extracting…", move || {
        archive::extract_all(&archive_for_job, &dest_for_job, pw_for_job.as_deref())
    }, move |a, result: Result<usize, ArchiveError>| match result {
        Ok(n) => {
            toast(a, format!("Extracted {n} file(s)."));
            reload(a);
        }
        Err(ArchiveError::PasswordRequired) => {
            explorer(a).pending = Some(Pending::Extract {
                archive: archive_path.clone(),
                dest: dest.clone(),
            });
            open_password_dialog(a, "This archive is protected. Enter its password.");
        }
        Err(ArchiveError::WrongPassword) => {
            explorer(a).pending = Some(Pending::Extract {
                archive: archive_path.clone(),
                dest: dest.clone(),
            });
            open_password_dialog(a, "The password is wrong.");
        }
        Err(ArchiveError::Failed(text)) => {
            toast(a, format!("Could not extract: {text}"));
            reload(a);
        }
    });
}

fn password_submit(app: &Rc<App>, password: &str) {
    let pending = explorer(app).pending.take();
    match pending {
        Some(Pending::OpenArchive(path)) => {
            close_dialog(app);
            enter_archive(app, path, Some(password.to_string()));
        }
        Some(Pending::Extract { archive, dest }) => {
            close_dialog(app);
            start_extract(app, archive, dest, Some(password.to_string()));
        }
        None => close_dialog(app),
    }
}

// ---- Jobs, dialogs and the status line --------------------------------------------------------------------------

/// Runs `work` on a worker thread, shows `label` in the status bar until it ends, then calls `done` on the UI thread.
fn start_job<T, W, D>(app: &Rc<App>, label: &str, work: W, done: D)
where
    T: Send + 'static,
    W: FnOnce() -> Result<T, ArchiveError> + Send + 'static,
    D: FnOnce(&Rc<App>, Result<T, ArchiveError>) + Send + 'static,
{
    explorer(app).busy = label.to_string();
    push(app);
    std::thread::spawn(move || {
        let result = work();
        post(move |a| {
            explorer(a).busy.clear();
            done(a, result);
        });
    });
}

/// Opens a dialog of the Files page: 1 new folder, 2 rename, 3 batch rename, 4 new archive. `text` is the field's
/// starting value (the name, or the pattern).
fn open_dialog(app: &Rc<App>, kind: i32, text: &str) {
    let f = files_global(app);
    f.set_dialog_text(text.into());
    f.set_dialog_error("".into());
    f.set_dialog(kind);
}

fn open_password_dialog(app: &Rc<App>, message: &str) {
    let f = files_global(app);
    f.set_dialog_text("".into());
    f.set_dialog_error(message.into());
    f.set_dialog(5);
}

fn set_dialog_error(app: &Rc<App>, message: &str) {
    files_global(app).set_dialog_error(message.into());
}

fn close_dialog(app: &Rc<App>) {
    let f = files_global(app);
    f.set_dialog(0);
    f.set_dialog_error("".into());
    f.set_dialog_text("".into());
}
