//! File operations for the Files tab: listing a folder, copy, move, rename, new folder, Recycle Bin delete, and the
//! batch rename (a plan with a preview, then the renames, with an undo list).
//!
//! Nothing here touches the UI. Errors are plain strings, because the Files tab shows them in a toast.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One row of a folder listing.
#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// The entries of a folder: folders first, then files, each group by name (case-insensitive).
pub fn list_dir(dir: &Path) -> io::Result<Vec<Item>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        out.push(Item {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: meta.modified().ok(),
            path,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Undo records. Each one reverses a change; the list is undone from the end.
#[derive(Debug, Clone)]
pub enum Undo {
    /// The item was renamed or moved from `from` to `to`; undo moves it back.
    Moved { from: PathBuf, to: PathBuf },
    /// A folder was created at `path`; undo removes it (only while it is still empty).
    Created { path: PathBuf },
}

/// Applies an undo record. Returns the error text when it could not be undone.
pub fn undo(record: &Undo) -> Result<(), String> {
    match record {
        Undo::Moved { from, to } => {
            if from.exists() {
                return Err(format!("{} already exists", from.display()));
            }
            fs::rename(to, from).map_err(|e| e.to_string())
        }
        Undo::Created { path } => fs::remove_dir(path).map_err(|e| e.to_string()),
    }
}

/// `dir/name` if free, else `dir/name (2)`, `dir/name (3)`, ... for files the stem goes before the extension.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 2;
    loop {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Checks a name typed by the user (a file or folder name, not a path).
pub fn check_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("The name is empty.".into());
    }
    if t.chars().any(|c| "<>:\"/\\|?*".contains(c) || c.is_control()) {
        return Err("A name can't contain < > : \" / \\ | ? *".into());
    }
    if t == "." || t == ".." {
        return Err("That name is reserved.".into());
    }
    Ok(())
}

/// Renames an item in place. Refuses to overwrite. Returns the undo record.
pub fn rename(from: &Path, new_name: &str) -> Result<Undo, String> {
    check_name(new_name)?;
    let parent = from.parent().ok_or("The item has no folder.")?;
    let to = parent.join(new_name.trim());
    if to == from {
        return Err("The name is the same.".into());
    }
    if to.exists() {
        return Err(format!("{} already exists.", new_name.trim()));
    }
    fs::rename(from, &to).map_err(|e| e.to_string())?;
    Ok(Undo::Moved {
        from: from.to_path_buf(),
        to,
    })
}

/// Creates a folder called `name` in `parent` (numbered if taken). Returns its path and the undo record.
pub fn new_folder(parent: &Path, name: &str) -> Result<(PathBuf, Undo), String> {
    let name = if name.trim().is_empty() {
        "New folder"
    } else {
        name.trim()
    };
    check_name(name)?;
    let path = unique_path(parent, name);
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok((
        path.clone(),
        Undo::Created { path },
    ))
}

/// Copies `src` (a file or a whole folder) into `dest_dir` under its own name, numbering it if taken.
pub fn copy_into(src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let name = src
        .file_name()
        .ok_or("Nothing to copy.")?
        .to_string_lossy()
        .into_owned();
    let target = unique_path(dest_dir, &name);
    if src.is_dir() {
        copy_dir(src, &target).map_err(|e| e.to_string())?;
    } else {
        fs::copy(src, &target).map_err(|e| e.to_string())?;
    }
    Ok(target)
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// Moves `src` into `dest_dir` under its own name (numbered if taken). Returns the new path and the undo record.
/// A move across drives copies and then removes the original.
pub fn move_into(src: &Path, dest_dir: &Path) -> Result<(PathBuf, Undo), String> {
    let name = src
        .file_name()
        .ok_or("Nothing to move.")?
        .to_string_lossy()
        .into_owned();
    let to = unique_path(dest_dir, &name);
    if fs::rename(src, &to).is_err() {
        if src.is_dir() {
            copy_dir(src, &to).map_err(|e| e.to_string())?;
            fs::remove_dir_all(src).map_err(|e| e.to_string())?;
        } else {
            fs::copy(src, &to).map_err(|e| e.to_string())?;
            fs::remove_file(src).map_err(|e| e.to_string())?;
        }
    }
    Ok((
        to.clone(),
        Undo::Moved {
            from: src.to_path_buf(),
            to,
        },
    ))
}

/// Sends a file or folder to the Recycle Bin. False when Windows refused.
pub fn recycle(path: &Path) -> bool {
    rimelight_platform::recycle(path)
}

/// Settings of a batch rename.
#[derive(Debug, Clone)]
pub struct BatchRule {
    /// The new name. `{name}` is the old stem (after find and replace), `{ext}` the extension, `{n}` the counter.
    pub pattern: String,
    /// Text to find in the old stem, and what to put in its place. An empty find does nothing.
    pub find: String,
    pub replace: String,
    pub start: u32,
    /// Digits for `{n}`, zero-padded (0 means no padding).
    pub pad: usize,
}

/// One line of the batch rename preview.
#[derive(Debug, Clone)]
pub struct PlannedRename {
    pub from: PathBuf,
    pub new_name: String,
    /// Why this line can't be done, or None when it can.
    pub error: Option<String>,
}

/// Builds the new names for `paths` (in order) from the rule, and checks each one. Duplicates among the new names,
/// names that already exist and are not being renamed away, and names with forbidden characters all become errors.
pub fn plan_batch(paths: &[PathBuf], rule: &BatchRule) -> Vec<PlannedRename> {
    let moving: HashSet<PathBuf> = paths.iter().cloned().collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_dir = path.is_dir();
        let (stem, ext) = split_name(&file_name, is_dir);
        let stem = if rule.find.is_empty() {
            stem
        } else {
            stem.replace(&rule.find, &rule.replace)
        };
        let n = rule.start as u64 + i as u64;
        let counter = if rule.pad > 0 {
            format!("{n:0pad$}", pad = rule.pad)
        } else {
            n.to_string()
        };
        let mut new_name = rule
            .pattern
            .replace("{name}", &stem)
            .replace("{ext}", &ext)
            .replace("{n}", &counter);
        // A pattern without {ext} keeps the old extension.
        if !rule.pattern.contains("{ext}") && !ext.is_empty() {
            new_name.push('.');
            new_name.push_str(&ext);
        }
        let new_name = new_name.trim().to_string();
        let error = if let Err(e) = check_name(&new_name) {
            Some(e)
        } else if !seen.insert(new_name.to_lowercase()) {
            Some("Two items would get the same name.".to_string())
        } else {
            let target = path.parent().unwrap_or(Path::new("")).join(&new_name);
            if new_name != file_name && target.exists() && !moving.contains(&target) {
                Some(format!("{new_name} already exists."))
            } else {
                None
            }
        };
        out.push(PlannedRename {
            from: path.clone(),
            new_name,
            error,
        });
    }
    out
}

/// Splits a file name into (stem, extension). Folders have no extension.
fn split_name(name: &str, is_dir: bool) -> (String, String) {
    if is_dir {
        return (name.to_string(), String::new());
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// Applies the plan's renames, in two steps so that a name swap (a to b and b to a) works. Lines with an error are
/// skipped. Returns the undo records of the renames that happened, and the first error text, if any.
pub fn apply_batch(plan: &[PlannedRename]) -> (Vec<Undo>, Option<String>) {
    let mut done = Vec::new();
    let mut first_error = None;
    // Step 1: everything to a temporary name, so no target is still taken by a source.
    let mut staged: Vec<(PathBuf, PathBuf, String)> = Vec::new();
    for (i, line) in plan.iter().enumerate() {
        let old_name = line
            .from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if line.error.is_some() || line.new_name == old_name {
            continue;
        }
        let parent = line.from.parent().unwrap_or(Path::new(""));
        let tmp = parent.join(format!(".rm-batch-{i}-{}", std::process::id()));
        match fs::rename(&line.from, &tmp) {
            Ok(()) => staged.push((line.from.clone(), tmp, line.new_name.clone())),
            Err(e) => {
                first_error.get_or_insert(format!("{}: {e}", line.from.display()));
            }
        }
    }
    // Step 2: from the temporary name to the final one.
    for (from, tmp, new_name) in staged {
        let parent = from.parent().unwrap_or(Path::new(""));
        let to = parent.join(&new_name);
        match fs::rename(&tmp, &to) {
            Ok(()) => done.push(Undo::Moved { from, to }),
            Err(e) => {
                // Put it back where it was.
                let _ = fs::rename(&tmp, &from);
                first_error.get_or_insert(format!("{new_name}: {e}"));
            }
        }
    }
    (done, first_error)
}
