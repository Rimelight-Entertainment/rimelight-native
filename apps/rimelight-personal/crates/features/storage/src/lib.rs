//! The Storage page controller (web: `src/components/storage/R2FileManager.tsx`; egui: `crates/app/src/storage.rs`).
//!
//! Plain Rust with no Slint. The controller holds the page state (files, the open folder, search and type filter,
//! selection, the inspector and the pending confirmation), calls `/api/storage` through [`StorageApi`], and builds
//! previews: images through `rimelight_picture`, video posters and audio durations through `rimelight_media`, and
//! audio playback through `rimelight_media::Player`. Each state change is reported as a [`StorageView`]. The glue
//! turns [`Thumb`]s into slint images and asks for previews of the cards it shows (lazy loading).
//!
//! Threading follows the Todos page: calls return at once and answer on a worker thread, the lock is never held
//! across a call, and `notify` is called after the lock is released. Lock order: `state`, then `previews`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rimelight_media::{MediaError, Player, PlayerOptions};
use rimelight_personal_core::storage::{
    FilePatch, MediaKind, StorageFile, UploadFile, extension_of, join_key, media_kind, mime_for,
    move_filename, normalize_folder, preview_key, rename_filename, sanitize_basename, split_key,
};
use rimelight_personal_core::{ApiClient, ApiError};

/// The page's Slint markup, for the shell's build script (added as `@rimelight_personal_feature_storage`).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Largest side of a card preview, in pixels.
pub const THUMB_SIDE: u32 = 256;
/// How many decoded previews are kept at once. Older ones are dropped and loaded again when their card shows.
pub const THUMB_CACHE: usize = 256;

/// One file to upload: the name it gets in storage and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUpload {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A rename and/or move: the new full key and the new filename, sent as one `PATCH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUpdate {
    pub id: String,
    pub new_key: String,
    pub new_name: String,
}

pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the calls the page makes, plus the public file download used for previews and playback.
/// [`ApiClient`] implements it; tests inject a fake.
pub trait StorageApi: Send + Sync {
    fn list(&self, on_done: Done<Vec<StorageFile>>);
    fn upload(&self, folder: &str, files: Vec<NewUpload>, on_done: Done<()>);
    fn delete(&self, id: &str, on_done: Done<()>);
    fn delete_many(&self, ids: &[String], on_done: Done<()>);
    fn update(&self, update: FileUpdate, on_done: Done<()>);
    /// Downloads the public file at `url`.
    fn fetch(&self, url: &str, on_done: Done<Vec<u8>>);
}

impl StorageApi for ApiClient {
    fn list(&self, on_done: Done<Vec<StorageFile>>) {
        self.storage_files(on_done);
    }

    fn upload(&self, folder: &str, files: Vec<NewUpload>, on_done: Done<()>) {
        let mimes: Vec<&'static str> = files.iter().map(|f| mime_for(&f.name)).collect();
        let uploads: Vec<UploadFile<'_>> = files
            .iter()
            .zip(&mimes)
            .map(|(f, mime)| UploadFile {
                name: &f.name,
                mime,
                bytes: &f.bytes,
            })
            .collect();
        self.upload_storage_files(folder, &uploads, on_done);
    }

    fn delete(&self, id: &str, on_done: Done<()>) {
        self.delete_storage_file(id, on_done);
    }

    fn delete_many(&self, ids: &[String], on_done: Done<()>) {
        self.delete_storage_files(ids, on_done);
    }

    fn update(&self, update: FileUpdate, on_done: Done<()>) {
        let patch = FilePatch {
            id: &update.id,
            new_key: &update.new_key,
            new_name: &update.new_name,
        };
        self.update_storage_file(&patch, on_done);
    }

    fn fetch(&self, url: &str, on_done: Done<Vec<u8>>) {
        ehttp::fetch(ehttp::Request::get(url.to_owned()), move |result| {
            let outcome = match result {
                Err(e) => Err(ApiError::Transport(e)),
                Ok(resp) if !resp.ok => Err(ApiError::Status {
                    code: resp.status,
                    body: String::from_utf8_lossy(&resp.bytes).into_owned(),
                }),
                Ok(resp) => Ok(resp.bytes),
            };
            on_done(outcome);
        });
    }
}

/// The live API for `api_base`, for example `https://example.com/api/personal`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn StorageApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The type filter pills, in the order the page shows them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TypeFilter {
    #[default]
    All,
    Images,
    Videos,
    Audio,
    Documents,
}

impl TypeFilter {
    /// The pill index used by the markup (0 = all).
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Images,
            2 => Self::Videos,
            3 => Self::Audio,
            4 => Self::Documents,
            _ => Self::All,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::All => 0,
            Self::Images => 1,
            Self::Videos => 2,
            Self::Audio => 3,
            Self::Documents => 4,
        }
    }

    /// The web's mime check for each pill (`mime.startsWith(...)`; documents match pdf, text or doc).
    fn matches(self, mime: &str) -> bool {
        let mime = mime.to_ascii_lowercase();
        match self {
            Self::All => true,
            Self::Images => mime.starts_with("image/"),
            Self::Videos => mime.starts_with("video/"),
            Self::Audio => mime.starts_with("audio/"),
            Self::Documents => {
                mime.contains("pdf") || mime.contains("text") || mime.contains("doc")
            }
        }
    }
}

/// A decoded preview, RGBA, row by row. The glue turns it into a slint image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRowView {
    pub path: String,
    pub label: String,
    /// Nesting depth; 0 for top-level folders.
    pub depth: usize,
    /// Files in this folder and below.
    pub count: usize,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrumbView {
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderCardView {
    pub path: String,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRowView {
    pub id: String,
    pub name: String,
    pub key: String,
    pub url: String,
    /// `formatBytes` text, for example "1.5 KB".
    pub size: String,
    pub mime: String,
    /// The extension in capitals, or "FILE" when the name has none.
    pub ext: String,
    /// `image`, `video`, `audio` or `document`: which preview the card shows.
    pub kind: String,
    /// True when the file sits under a folder (the card's "nested" label).
    pub nested: bool,
    pub selected: bool,
    /// True for the file whose URL was copied last, until the glue clears it.
    pub copied: bool,
    /// The video or audio length as "m:ss" (or "h:mm:ss"); empty until it is known.
    pub duration: String,
    /// True when a decoded preview is ready; ask [`StorageController::thumbnail`] for it.
    pub has_preview: bool,
    /// True while this audio file is the one playing.
    pub playing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderOption {
    /// The folder path. Empty for the root.
    pub value: String,
    pub label: String,
}

/// The "Asset inspector" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorView {
    pub id: String,
    pub name: String,
    pub key: String,
    pub url: String,
    pub size: String,
    pub mime: String,
    pub ext: String,
    pub kind: String,
    /// The editable filename without its last extension.
    pub basename: String,
    /// The folder the file is saved into. Empty for the root.
    pub folder: String,
    pub folder_options: Vec<FolderOption>,
    /// The key the save would write, shown as "Target key preview".
    pub preview_key: String,
    pub saving: bool,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageView {
    pub folders: Vec<FolderRowView>,
    /// Files at the top level, the "All files" row count.
    pub root_count: usize,
    pub root_selected: bool,
    pub crumbs: Vec<CrumbView>,
    pub subfolders: Vec<FolderCardView>,
    pub files: Vec<FileRowView>,
    pub query: String,
    pub filter: usize,
    /// True for the table view, false for the grid.
    pub table: bool,
    pub selected_count: usize,
    /// True when every file in the current listing is selected (the table's select-all box).
    pub all_visible_selected: bool,
    pub loading: bool,
    pub uploading: bool,
    pub error: Option<String>,
    /// True when the folder has neither files nor subfolders ("Folder is empty").
    pub folder_empty: bool,
    pub new_folder_open: bool,
    pub new_folder_name: String,
    pub inspector: Option<InspectorView>,
    /// The text of the open confirmation ("Are you sure you want to delete ...?"), if one is pending.
    pub confirm: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Detail {
    id: String,
    basename: String,
    folder: String,
    saving: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    One { id: String, name: String },
    Many { keys: Vec<String> },
}

#[derive(Default)]
struct State {
    files: Vec<StorageFile>,
    loading: bool,
    error: Option<String>,
    path: String,
    local_folders: Vec<String>,
    selected_keys: Vec<String>,
    query: String,
    filter: TypeFilter,
    table: bool,
    uploading: bool,
    copied_key: Option<String>,
    new_folder: Option<String>,
    detail: Option<Detail>,
    pending: Option<Pending>,
    /// The id of the audio file playing, if any.
    playing: Option<String>,
}

/// Previews shared by the cards: decoded thumbnails, durations, and which files are loading or failed.
#[derive(Default)]
struct Previews {
    thumbs: HashMap<String, Arc<Thumb>>,
    durations: HashMap<String, String>,
    pending: HashSet<String>,
    failed: HashSet<String>,
}

impl Previews {
    fn insert_thumb(&mut self, id: String, thumb: Thumb) {
        if self.thumbs.len() >= THUMB_CACHE
            && let Some(old) = self.thumbs.keys().next().cloned()
        {
            self.thumbs.remove(&old);
        }
        self.thumbs.insert(id, Arc::new(thumb));
    }
}

impl State {
    fn file(&self, id: &str) -> Option<&StorageFile> {
        self.files.iter().find(|f| f.id == id)
    }

    /// Every folder path the listing knows: the ones in file keys plus the ones made this session.
    fn all_folders(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for file in &self.files {
            let parts: Vec<&str> = file.key.split('/').collect();
            let mut current = String::new();
            for part in parts.iter().take(parts.len().saturating_sub(1)) {
                if !current.is_empty() {
                    current.push('/');
                }
                current.push_str(part);
                set.insert(current.clone());
            }
        }
        for folder in &self.local_folders {
            if !folder.is_empty() {
                set.insert(folder.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Files whose folder is `path` (the root is the empty path).
    fn direct_files(&self, path: &str) -> Vec<&StorageFile> {
        self.files
            .iter()
            .filter(|f| parent_of(&f.key) == path)
            .collect()
    }

    /// The child folders shown in the current folder, as full paths.
    fn subfolders(&self, all: &[String]) -> Vec<String> {
        let path = self.path.as_str();
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut set = std::collections::BTreeSet::new();
        for folder in all {
            if path.is_empty() {
                let top = folder.split('/').next().unwrap_or("");
                if !top.is_empty() {
                    set.insert(top.to_owned());
                }
            } else if folder.starts_with(&prefix) && folder != path {
                let remainder = &folder[prefix.len()..];
                let direct = remainder.split('/').next().unwrap_or("");
                if !direct.is_empty() {
                    set.insert(format!("{prefix}{direct}"));
                }
            }
        }
        set.into_iter().collect()
    }

    /// Files in `folder` and every folder below it.
    fn count_under(&self, folder: &str) -> usize {
        let prefix = format!("{folder}/");
        self.files
            .iter()
            .filter(|f| f.key.starts_with(&prefix))
            .count()
    }

    /// The current folder's files after the search and the type pill.
    fn visible(&self) -> Vec<&StorageFile> {
        let query = self.query.trim().to_lowercase();
        self.direct_files(&self.path)
            .into_iter()
            .filter(|f| {
                query.is_empty()
                    || f.name.to_lowercase().contains(&query)
                    || f.key.to_lowercase().contains(&query)
            })
            .filter(|f| self.filter.matches(f.mime_type.as_deref().unwrap_or("")))
            .collect()
    }

    fn view(&self, previews: &Previews) -> StorageView {
        let all = self.all_folders();
        let subs = self.subfolders(&all);
        let visible = self.visible();

        let folders = all
            .iter()
            .map(|folder| FolderRowView {
                path: folder.clone(),
                label: folder.rsplit('/').next().unwrap_or("").to_owned(),
                depth: folder.matches('/').count(),
                count: self.count_under(folder),
                selected: *folder == self.path,
            })
            .collect();

        let mut crumbs = vec![CrumbView {
            label: "Root".to_owned(),
            path: String::new(),
        }];
        let mut acc = String::new();
        for part in self.path.split('/').filter(|p| !p.is_empty()) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(part);
            crumbs.push(CrumbView {
                label: part.to_owned(),
                path: acc.clone(),
            });
        }

        let files: Vec<FileRowView> = visible.iter().map(|f| self.file_row(f, previews)).collect();
        let all_visible_selected =
            !visible.is_empty() && visible.iter().all(|f| self.selected_keys.contains(&f.key));

        StorageView {
            folders,
            root_count: self.direct_files("").len(),
            root_selected: self.path.is_empty(),
            crumbs,
            subfolders: subs
                .iter()
                .map(|path| FolderCardView {
                    path: path.clone(),
                    name: path.rsplit('/').next().unwrap_or("").to_owned(),
                    count: self.count_under(path),
                })
                .collect(),
            folder_empty: files.is_empty() && self.subfolders(&all).is_empty(),
            files,
            query: self.query.clone(),
            filter: self.filter.index(),
            table: self.table,
            selected_count: self.selected_keys.len(),
            all_visible_selected,
            loading: self.loading,
            uploading: self.uploading,
            error: self.error.clone(),
            new_folder_open: self.new_folder.is_some(),
            new_folder_name: self.new_folder.clone().unwrap_or_default(),
            inspector: self.detail.as_ref().and_then(|d| self.inspector(d)),
            confirm: self.pending.as_ref().map(|p| match p {
                Pending::One { name, .. } => {
                    format!("Are you sure you want to delete \"{name}\" from R2 storage?")
                }
                Pending::Many { keys } => {
                    format!("Delete {} selected assets from R2 storage?", keys.len())
                }
            }),
        }
    }

    fn file_row(&self, file: &StorageFile, previews: &Previews) -> FileRowView {
        let ext = extension_of(&file.name);
        FileRowView {
            id: file.id.clone(),
            name: file.name.clone(),
            key: file.key.clone(),
            url: file.url.clone(),
            size: format_bytes(file.size),
            mime: file.mime_type.clone().unwrap_or_default(),
            ext: if ext.is_empty() {
                "FILE".to_owned()
            } else {
                ext.to_uppercase()
            },
            kind: kind_name(media_kind(file.mime_type.as_deref(), &file.key)).to_owned(),
            nested: file.key.contains('/'),
            selected: self.selected_keys.contains(&file.key),
            copied: self.copied_key.as_deref() == Some(file.key.as_str()),
            duration: previews
                .durations
                .get(&file.id)
                .cloned()
                .unwrap_or_default(),
            has_preview: previews.thumbs.contains_key(&file.id),
            playing: self.playing.as_deref() == Some(file.id.as_str()),
        }
    }

    fn inspector(&self, detail: &Detail) -> Option<InspectorView> {
        let file = self.file(&detail.id)?;
        let mut options = vec![FolderOption {
            value: String::new(),
            label: "Root (Top Level)".to_owned(),
        }];
        options.extend(self.all_folders().into_iter().map(|folder| FolderOption {
            label: folder.clone(),
            value: folder,
        }));
        let folder = normalize_folder(&detail.folder);
        Some(InspectorView {
            id: file.id.clone(),
            name: file.name.clone(),
            key: file.key.clone(),
            url: file.url.clone(),
            size: format_bytes(file.size),
            mime: file.mime_type.clone().unwrap_or_default(),
            ext: extension_of(&file.name).to_owned(),
            kind: kind_name(media_kind(file.mime_type.as_deref(), &file.key)).to_owned(),
            basename: detail.basename.clone(),
            folder: detail.folder.clone(),
            folder_options: options,
            preview_key: preview_key(&folder, &detail.basename, &file.name),
            saving: detail.saving,
        })
    }
}

fn kind_name(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
        MediaKind::Audio => "audio",
        MediaKind::Document => "document",
    }
}

/// The folder part of a key: `a/b/c.png` gives `a/b`, a root file gives "".
fn parent_of(key: &str) -> &str {
    match key.rfind('/') {
        Some(index) => &key[..index],
        None => "",
    }
}

/// `formatBytes` from the web page: "0 B" for zero, otherwise one decimal in B, KB, MB or GB.
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_owned();
    }
    const SIZES: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut index = 0;
    let mut value = bytes as f64;
    while value >= 1024.0 && index < SIZES.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    format!("{value:.1} {}", SIZES[index])
}

/// A media length as "m:ss", or "h:mm:ss" from an hour.
pub fn format_duration(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// What a preview download produced.
enum Preview {
    Image(Thumb),
    Video { thumb: Thumb, duration: String },
    Audio { duration: String },
}

/// A file a preview job works on.
#[derive(Clone)]
struct Job {
    id: String,
    url: String,
    name: String,
    kind: MediaKind,
}

/// Decodes a downloaded file for its card. Images decode in memory; video and audio go through a temporary file
/// because `rimelight_media` reads paths. Documents have no preview.
fn build_preview(bytes: &[u8], kind: MediaKind, name: &str, id: &str) -> Result<Preview, String> {
    match kind {
        MediaKind::Image => {
            let picture = rimelight_picture::load_bytes(bytes)?;
            let small = picture.thumbnail(THUMB_SIDE);
            Ok(Preview::Image(Thumb {
                width: small.w,
                height: small.h,
                rgba: small.rgba,
            }))
        }
        MediaKind::Video => {
            let path = write_temp(id, name, bytes)?;
            let poster =
                rimelight_media::poster(&path, 0.1, THUMB_SIDE).map_err(|e| e.to_string())?;
            let duration = duration_of(&path).map(format_duration).unwrap_or_default();
            Ok(Preview::Video {
                thumb: Thumb {
                    width: poster.width,
                    height: poster.height,
                    rgba: poster.bytes().to_vec(),
                },
                duration,
            })
        }
        MediaKind::Audio => {
            let path = write_temp(id, name, bytes)?;
            let duration = duration_of(&path).map(format_duration)?;
            Ok(Preview::Audio { duration })
        }
        MediaKind::Document => Err("documents have no preview".to_owned()),
    }
}

/// The length of a media file in milliseconds. `probe` reads video; audio falls back to opening a player.
fn duration_of(path: &Path) -> Result<u64, String> {
    match rimelight_media::probe(path) {
        Ok(info) => Ok(info.duration_ms),
        Err(_) => Player::open(
            path,
            PlayerOptions {
                max_width: 1,
                max_height: 1,
                audio: false,
            },
        )
        .map(|player| player.duration_ms())
        .map_err(|e| e.to_string()),
    }
}

/// Opens an audio file for playback with sound and starts it.
fn open_audio(path: &Path) -> Result<Player, String> {
    let player = Player::open(
        path,
        PlayerOptions {
            max_width: 1,
            max_height: 1,
            audio: true,
        },
    )
    .map_err(|e: MediaError| e.to_string())?;
    player.play();
    Ok(player)
}

/// Writes `bytes` to a temporary file for `rimelight_media`. The name is built from the id and the extension only.
fn write_temp(id: &str, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("rimelight-personal-storage");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let ext: String = extension_of(name)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let file = if ext.is_empty() {
        dir.join(safe_id)
    } else {
        dir.join(format!("{safe_id}.{ext}"))
    };
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file)
}

type Notify = Arc<dyn Fn(StorageView) + Send + Sync>;

struct Shared {
    api: Arc<dyn StorageApi>,
    state: Mutex<State>,
    previews: Mutex<Previews>,
    audio: Mutex<Option<Player>>,
    notify: Notify,
}

/// The Storage page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct StorageController {
    shared: Arc<Shared>,
}

impl StorageController {
    /// `notify` receives the new view after every state change, possibly on a worker thread.
    pub fn new(
        api: Arc<dyn StorageApi>,
        notify: impl Fn(StorageView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                state: Mutex::default(),
                previews: Mutex::default(),
                audio: Mutex::new(None),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> StorageView {
        let state = self.lock();
        let previews = self.previews();
        state.view(&previews)
    }

    /// The decoded preview of file `id`, if it has one. The glue turns it into a slint image.
    pub fn thumbnail(&self, id: &str) -> Option<Arc<Thumb>> {
        self.previews().thumbs.get(id).cloned()
    }

    /// Starts loading the previews of the files `ids` (the cards the glue is showing). Files that already have a
    /// preview, are loading, or failed are skipped. Results arrive through `notify`.
    pub fn ensure_previews(&self, ids: &[String]) {
        let jobs: Vec<Job> = {
            let state = self.lock();
            let mut previews = self.previews();
            let mut jobs = Vec::new();
            for id in ids {
                let done = previews.thumbs.contains_key(id)
                    || previews.durations.contains_key(id)
                    || previews.failed.contains(id)
                    || previews.pending.contains(id);
                if done {
                    continue;
                }
                let Some(file) = state.file(id) else {
                    continue;
                };
                previews.pending.insert(id.clone());
                jobs.push(Job {
                    id: file.id.clone(),
                    url: file.url.clone(),
                    name: file.name.clone(),
                    kind: media_kind(file.mime_type.as_deref(), &file.key),
                });
            }
            jobs
        };
        for job in jobs {
            let me = self.clone();
            let url = job.url.clone();
            self.shared
                .api
                .fetch(&url, Box::new(move |result| me.finish_preview(job, result)));
        }
    }

    fn finish_preview(&self, job: Job, result: Result<Vec<u8>, ApiError>) {
        let outcome = result
            .map_err(|e| e.to_string())
            .and_then(|bytes| build_preview(&bytes, job.kind, &job.name, &job.id));
        {
            let mut previews = self.previews();
            previews.pending.remove(&job.id);
            match outcome {
                Ok(Preview::Image(thumb)) => previews.insert_thumb(job.id.clone(), thumb),
                Ok(Preview::Video { thumb, duration }) => {
                    previews.insert_thumb(job.id.clone(), thumb);
                    previews.durations.insert(job.id.clone(), duration);
                }
                Ok(Preview::Audio { duration }) => {
                    previews.durations.insert(job.id.clone(), duration);
                }
                Err(_) => {
                    previews.failed.insert(job.id.clone());
                }
            }
        }
        self.change(|_| {});
    }

    /// Starts or stops playback of audio file `id`. Playing a second file stops the first. Returns `false` when the
    /// file is unknown. A file that cannot be played sets the error.
    pub fn toggle_audio(&self, id: &str) -> bool {
        let (url, name) = {
            let state = self.lock();
            if state.playing.as_deref() == Some(id) {
                drop(state);
                self.stop_audio();
                return true;
            }
            match state.file(id) {
                Some(file) => (file.url.clone(), file.name.clone()),
                None => return false,
            }
        };
        let me = self.clone();
        let id = id.to_owned();
        self.shared.api.fetch(
            &url,
            Box::new(move |result| {
                let outcome = result
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| write_temp(&id, &name, &bytes))
                    .and_then(|path| open_audio(&path));
                match outcome {
                    Ok(player) => {
                        *me.audio_slot() = Some(player);
                        me.change(|s| s.playing = Some(id.clone()));
                    }
                    Err(e) => me.fail_message(e),
                }
            }),
        );
        true
    }

    /// Stops the audio that is playing, if any.
    pub fn stop_audio(&self) {
        if let Some(player) = self.audio_slot().take() {
            player.pause();
        }
        self.change(|s| s.playing = None);
    }

    fn audio_slot(&self) -> MutexGuard<'_, Option<Player>> {
        self.shared
            .audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Fetches the file list again.
    pub fn refresh(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared.api.list(Box::new(move |result| {
            me.change(|s| {
                s.loading = false;
                match result {
                    Ok(files) => {
                        let known = &files;
                        s.selected_keys
                            .retain(|key| known.iter().any(|f| &f.key == key));
                        // A good answer keeps an error from the write that triggered it (a file that could not be read).
                        s.files = files;
                    }
                    Err(e) => s.error = Some(e.to_string()),
                }
            });
        }));
    }

    /// Opens a folder (the empty path is the root). Clears the selection, as the page does when it moves.
    pub fn open_folder(&self, path: &str) {
        let path = normalize_folder(path);
        self.change(|s| {
            s.path = path;
            s.selected_keys.clear();
        });
    }

    pub fn set_query(&self, query: &str) {
        self.change(|s| s.query = query.to_owned());
    }

    pub fn set_filter(&self, filter: TypeFilter) {
        self.change(|s| s.filter = filter);
    }

    pub fn set_table(&self, table: bool) {
        self.change(|s| s.table = table);
    }

    pub fn toggle_select(&self, key: &str) {
        self.change(|s| {
            if let Some(index) = s.selected_keys.iter().position(|k| k == key) {
                s.selected_keys.remove(index);
            } else {
                s.selected_keys.push(key.to_owned());
            }
        });
    }

    /// Selects or clears every file in the current listing (the table's select-all box).
    pub fn set_select_visible(&self, on: bool) {
        self.change(|s| {
            let keys: Vec<String> = s.visible().iter().map(|f| f.key.clone()).collect();
            if on {
                for key in keys {
                    if !s.selected_keys.contains(&key) {
                        s.selected_keys.push(key);
                    }
                }
            } else {
                s.selected_keys.retain(|k| !keys.contains(k));
            }
        });
    }

    pub fn open_new_folder(&self) {
        self.change(|s| s.new_folder = Some(String::new()));
    }

    pub fn set_new_folder_name(&self, name: &str) {
        self.change(|s| {
            if let Some(current) = s.new_folder.as_mut() {
                *current = name.to_owned();
            }
        });
    }

    pub fn close_new_folder(&self) {
        self.change(|s| s.new_folder = None);
    }

    /// Makes a folder under the current one. The name is sanitised as the web page does. Returns `false`, and changes
    /// nothing, when the name is blank. The folder exists only in this session (the server has no folder endpoint),
    /// and the page moves into it.
    pub fn create_folder(&self) -> bool {
        let name = {
            let state = self.lock();
            match state.new_folder.as_deref() {
                Some(name) => sanitize_basename(name),
                None => return false,
            }
        };
        if name.is_empty() {
            return false;
        }
        self.change(|s| {
            let new_path = if s.path.is_empty() {
                name.clone()
            } else {
                format!("{}/{name}", s.path)
            };
            s.local_folders.push(new_path.clone());
            s.path = new_path;
            s.new_folder = None;
        });
        true
    }

    /// Uploads `files` into the current folder. Returns `false`, and calls nothing, when there are none.
    pub fn upload(&self, files: Vec<NewUpload>) -> bool {
        if files.is_empty() {
            return false;
        }
        let folder = self.lock().path.clone();
        self.change(|s| {
            s.uploading = true;
            s.error = None;
        });
        let me = self.clone();
        self.shared.api.upload(
            &folder,
            files,
            Box::new(move |result| match result {
                Ok(()) => {
                    me.change(|s| s.uploading = false);
                    me.refresh();
                }
                Err(e) => {
                    me.change(|s| s.uploading = false);
                    me.fail(e);
                }
            }),
        );
        true
    }

    /// Uploads files from disk into the current folder (the file picker's paths, or an OS drop). A file that cannot
    /// be read sets the error and the rest still go. Returns `false` when nothing could be read.
    pub fn upload_paths(&self, paths: &[PathBuf]) -> bool {
        let mut files = Vec::new();
        let mut unreadable = Vec::new();
        for path in paths {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            match std::fs::read(path) {
                Ok(bytes) => files.push(NewUpload {
                    name: name.to_owned(),
                    bytes,
                }),
                Err(e) => unreadable.push(format!("could not read {name}: {e}")),
            }
        }
        // `upload` clears the error when it starts, so the read failures are reported after it.
        let started = self.upload(files);
        if let Some(message) = unreadable.first() {
            self.fail_message(message.clone());
        }
        started
    }

    /// The shell's entry point for files dropped on the window (winit's dropped-file paths, as strings).
    pub fn files_dropped(&self, paths: &[String]) -> bool {
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        self.upload_paths(&paths)
    }

    /// Opens the inspector for file `id`, with its basename and folder filled in.
    pub fn open_detail(&self, id: &str) {
        self.change(|s| {
            let detail = s.file(id).map(|file| {
                let (folder, basename) = split_key(&file.key);
                Detail {
                    id: file.id.clone(),
                    basename,
                    folder,
                    saving: false,
                }
            });
            if detail.is_some() {
                s.detail = detail;
            }
        });
    }

    pub fn close_detail(&self) {
        self.change(|s| s.detail = None);
    }

    pub fn set_detail_basename(&self, basename: &str) {
        self.change(|s| {
            if let Some(detail) = s.detail.as_mut() {
                detail.basename = basename.to_owned();
            }
        });
    }

    pub fn set_detail_folder(&self, folder: &str) {
        self.change(|s| {
            if let Some(detail) = s.detail.as_mut() {
                detail.folder = folder.to_owned();
            }
        });
    }

    /// Saves the inspector's rename and folder change. Returns `false`, and calls nothing, when the basename is blank
    /// or no inspector is open.
    pub fn save_detail(&self) -> bool {
        let update = {
            let state = self.lock();
            let Some(detail) = state.detail.as_ref() else {
                return false;
            };
            if detail.basename.trim().is_empty() {
                return false;
            }
            let Some(file) = state.file(&detail.id) else {
                return false;
            };
            let final_name = rename_filename(&file.name, &detail.basename);
            let folder = normalize_folder(&detail.folder);
            FileUpdate {
                id: file.id.clone(),
                new_key: join_key(&folder, &final_name),
                new_name: final_name,
            }
        };
        self.send_update(update, true);
        true
    }

    /// Moves file `id` into `dest_folder` (the root is empty), keeping its name. This is the drag-to-folder move.
    /// Returns `false` when the file is unknown.
    pub fn move_to_folder(&self, id: &str, dest_folder: &str) -> bool {
        let update = {
            let state = self.lock();
            let Some(file) = state.file(id) else {
                return false;
            };
            let final_name = move_filename(&file.name);
            let folder = normalize_folder(dest_folder);
            FileUpdate {
                id: file.id.clone(),
                new_key: join_key(&folder, &final_name),
                new_name: final_name,
            }
        };
        self.send_update(update, false);
        true
    }

    fn send_update(&self, update: FileUpdate, from_inspector: bool) {
        if from_inspector {
            self.change(|s| {
                if let Some(detail) = s.detail.as_mut() {
                    detail.saving = true;
                }
            });
        }
        let me = self.clone();
        self.shared.api.update(
            update,
            Box::new(move |result| match result {
                Ok(()) => {
                    me.change(|s| {
                        if from_inspector {
                            s.detail = None;
                        }
                    });
                    me.refresh();
                }
                Err(e) => {
                    me.change(|s| {
                        if let Some(detail) = s.detail.as_mut() {
                            detail.saving = false;
                        }
                    });
                    me.fail(e);
                }
            }),
        );
    }

    /// Asks to delete one file. The page then shows the confirmation; [`Self::confirm_delete`] carries it out.
    /// Returns `false` when the file is unknown.
    pub fn ask_delete(&self, id: &str) -> bool {
        self.change(|s| {
            let pending = s.file(id).map(|file| Pending::One {
                id: file.id.clone(),
                name: file.name.clone(),
            });
            if pending.is_some() {
                s.pending = pending;
            }
        });
        self.lock().pending.is_some()
    }

    /// Asks to delete every selected file. Returns `false` when nothing is selected.
    pub fn ask_delete_selected(&self) -> bool {
        let keys = self.lock().selected_keys.clone();
        if keys.is_empty() {
            return false;
        }
        self.change(|s| s.pending = Some(Pending::Many { keys }));
        true
    }

    pub fn cancel_delete(&self) {
        self.change(|s| s.pending = None);
    }

    /// Carries out the pending deletion: `DELETE ?id=` for one file, `DELETE ?ids=` for the selection. Returns
    /// `false` when nothing is pending.
    pub fn confirm_delete(&self) -> bool {
        let pending = {
            let mut state = self.lock();
            match state.pending.take() {
                Some(pending) => pending,
                None => return false,
            }
        };
        match pending {
            Pending::One { id, .. } => {
                self.change(|s| {
                    if s.detail.as_ref().is_some_and(|d| d.id == id) {
                        s.detail = None;
                    }
                });
                let me = self.clone();
                self.shared.api.delete(
                    &id,
                    Box::new(move |result| match result {
                        Ok(()) => me.refresh(),
                        Err(e) => me.fail(e),
                    }),
                );
            }
            Pending::Many { keys } => {
                let ids: Vec<String> = self
                    .lock()
                    .files
                    .iter()
                    .filter(|f| keys.contains(&f.key))
                    .map(|f| f.id.clone())
                    .collect();
                self.change(|s| s.selected_keys.retain(|k| !keys.contains(k)));
                if ids.is_empty() {
                    return true;
                }
                let me = self.clone();
                self.shared.api.delete_many(
                    &ids,
                    Box::new(move |result| match result {
                        Ok(()) => me.refresh(),
                        Err(e) => me.fail(e),
                    }),
                );
            }
        }
        true
    }

    /// Returns the public URL to put on the clipboard and marks `key` as copied. The glue shows the copied state
    /// and calls [`Self::clear_copied`] after two seconds. Returns `None` when the key is unknown.
    pub fn copy_url(&self, key: &str) -> Option<String> {
        let mut url = None;
        self.change(|s| {
            url = s.files.iter().find(|f| f.key == key).map(|f| f.url.clone());
            if url.is_some() {
                s.copied_key = Some(key.to_owned());
            }
        });
        url
    }

    pub fn clear_copied(&self) {
        self.change(|s| s.copied_key = None);
    }

    pub fn dismiss_error(&self) {
        self.change(|s| s.error = None);
    }

    fn fail(&self, error: ApiError) {
        self.fail_message(error.to_string());
    }

    fn fail_message(&self, message: String) {
        self.change(|s| s.error = Some(message));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn previews(&self) -> MutexGuard<'_, Previews> {
        self.shared
            .previews
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, update: impl FnOnce(&mut State)) {
        let view = {
            let mut state = self.lock();
            update(&mut state);
            let previews = self.previews();
            state.view(&previews)
        };
        (self.shared.notify)(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records each call and answers at once from scripted replies.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        list: Mutex<Option<Vec<StorageFile>>>,
        writes: Mutex<VecDeque<Result<(), ApiError>>>,
        downloads: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn next_write(&self) -> Result<(), ApiError> {
            self.writes.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    impl StorageApi for Arc<Fake> {
        fn list(&self, on_done: Done<Vec<StorageFile>>) {
            self.record("list".to_owned());
            let files = self
                .list
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(sample_files);
            on_done(Ok(files));
        }

        fn upload(&self, folder: &str, files: Vec<NewUpload>, on_done: Done<()>) {
            let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
            self.record(format!("upload:{folder}:{}", names.join(",")));
            on_done(self.next_write());
        }

        fn delete(&self, id: &str, on_done: Done<()>) {
            self.record(format!("delete:{id}"));
            on_done(self.next_write());
        }

        fn delete_many(&self, ids: &[String], on_done: Done<()>) {
            self.record(format!("delete_many:{}", ids.join(",")));
            on_done(self.next_write());
        }

        fn update(&self, update: FileUpdate, on_done: Done<()>) {
            self.record(format!(
                "update:{}:{}:{}",
                update.id, update.new_key, update.new_name
            ));
            on_done(self.next_write());
        }

        fn fetch(&self, url: &str, on_done: Done<Vec<u8>>) {
            self.record(format!("fetch:{url}"));
            let reply = match self.downloads.lock().unwrap().get(url) {
                Some(bytes) => Ok(bytes.clone()),
                None => Err(ApiError::Status {
                    code: 404,
                    body: "missing".into(),
                }),
            };
            on_done(reply);
        }
    }

    fn file(id: &str, key: &str, mime: Option<&str>, size: u64) -> StorageFile {
        let name = key.rsplit('/').next().unwrap().to_owned();
        StorageFile {
            id: id.to_owned(),
            name,
            key: key.to_owned(),
            url: format!("https://cdn.example/{key}"),
            size,
            mime_type: mime.map(str::to_owned),
        }
    }

    fn sample_files() -> Vec<StorageFile> {
        vec![
            file("f1", "root.txt", Some("text/plain"), 10),
            file("f2", "pics/cat.png", Some("image/png"), 2048),
            file("f3", "pics/trips/beach.jpg", Some("image/jpeg"), 0),
            file("f4", "clip.mp4", Some("video/mp4"), 3 * 1024 * 1024),
        ]
    }

    fn controller(fake: &Arc<Fake>) -> (StorageController, Arc<Mutex<Vec<StorageView>>>) {
        let seen: Arc<Mutex<Vec<StorageView>>> = Arc::default();
        let sink = seen.clone();
        let api: Arc<dyn StorageApi> = Arc::new(fake.clone());
        let c = StorageController::new(api, move |view| sink.lock().unwrap().push(view));
        (c, seen)
    }

    fn loaded(fake: &Arc<Fake>) -> StorageController {
        let (c, _) = controller(fake);
        c.refresh();
        c
    }

    /// A 2x2 PNG, made by the same crate that decodes it.
    fn png_bytes() -> Vec<u8> {
        let picture = rimelight_picture::Picture {
            w: 2,
            h: 2,
            rgba: vec![255; 16],
        };
        rimelight_picture::encode(&picture, "png").unwrap()
    }

    #[test]
    fn refresh_loads_and_lists_root_files() {
        let fake = Arc::default();
        let c = loaded(&fake);
        let view = c.view();
        assert!(!view.loading);
        assert_eq!(view.root_count, 2, "root.txt and clip.mp4 sit at the top");
        let names: Vec<&str> = view.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["root.txt", "clip.mp4"]);
        assert_eq!(view.folders.len(), 2, "pics and pics/trips");
        assert_eq!(view.folders[0].path, "pics");
        assert_eq!(
            view.folders[0].count, 2,
            "pics holds cat.png and beach.jpg below it"
        );
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn opening_a_folder_shows_its_subfolders_and_breadcrumbs() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_folder("pics");
        let view = c.view();
        assert_eq!(view.crumbs.len(), 2);
        assert_eq!(view.crumbs[1].path, "pics");
        assert_eq!(
            view.subfolders,
            [FolderCardView {
                path: "pics/trips".into(),
                name: "trips".into(),
                count: 1
            }]
        );
        assert_eq!(view.files.len(), 1);
        assert_eq!(view.files[0].name, "cat.png");
        assert!(view.files[0].nested);
        assert_eq!(view.files[0].size, "2.0 KB");
    }

    #[test]
    fn search_and_type_filter_narrow_the_listing() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.set_query("CLIP");
        assert_eq!(c.view().files.len(), 1);
        c.set_query("");
        c.set_filter(TypeFilter::Images);
        assert!(c.view().files.is_empty(), "no images at the root");
        c.set_filter(TypeFilter::Videos);
        assert_eq!(c.view().files[0].name, "clip.mp4");
    }

    #[test]
    fn create_folder_refuses_a_blank_name_and_moves_into_the_new_folder() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_new_folder();
        c.set_new_folder_name("   ");
        assert!(!c.create_folder());
        c.set_new_folder_name("my docs!");
        assert!(c.create_folder());
        let view = c.view();
        assert_eq!(view.crumbs.last().unwrap().path, "my-docs-");
        assert!(!view.new_folder_open);
        assert!(fake.calls().iter().all(|call| !call.starts_with("update")));
    }

    #[test]
    fn upload_posts_to_the_current_folder_then_refreshes() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_folder("pics");
        let files = vec![NewUpload {
            name: "a.png".into(),
            bytes: vec![1],
        }];
        assert!(c.upload(files));
        assert_eq!(fake.calls(), ["list", "upload:pics:a.png", "list"]);
        assert!(!c.view().uploading);
        assert!(!c.upload(Vec::new()));
    }

    #[test]
    fn dropped_paths_are_read_and_uploaded_and_unreadable_ones_are_reported() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_folder("pics");
        let dir = std::env::temp_dir().join("rimelight-personal-storage-test-drop");
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("dropped.txt");
        std::fs::write(&real, b"hello").unwrap();
        let missing = dir.join("missing.txt");
        let paths = vec![real.display().to_string(), missing.display().to_string()];
        assert!(c.files_dropped(&paths));
        assert!(fake.calls().contains(&"upload:pics:dropped.txt".to_owned()));
        let error = c.view().error.unwrap_or_default();
        assert!(
            error.starts_with("could not read missing.txt"),
            "got {error}"
        );
    }

    #[test]
    fn inspector_save_renames_and_moves_then_closes() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_detail("f2");
        let detail = c.view().inspector.unwrap();
        assert_eq!(detail.basename, "cat");
        assert_eq!(detail.folder, "pics");
        c.set_detail_basename("my cat");
        c.set_detail_folder("/archive/");
        assert_eq!(
            c.view().inspector.unwrap().preview_key,
            "archive/my cat.png"
        );
        assert!(c.save_detail());
        assert!(
            fake.calls()
                .contains(&"update:f2:archive/my-cat.png:my-cat.png".to_owned())
        );
        assert!(c.view().inspector.is_none());
    }

    #[test]
    fn inspector_refuses_a_blank_basename() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.open_detail("f1");
        c.set_detail_basename("  ");
        assert!(!c.save_detail());
        assert!(fake.calls().iter().all(|call| !call.starts_with("update")));
    }

    #[test]
    fn a_failed_save_keeps_the_inspector_open_with_the_error() {
        let fake: Arc<Fake> = Arc::default();
        fake.writes.lock().unwrap().push_back(Err(ApiError::Status {
            code: 404,
            body: "no route".into(),
        }));
        let c = loaded(&fake);
        c.open_detail("f1");
        assert!(c.save_detail());
        let view = c.view();
        let inspector = view.inspector.expect("inspector stays open");
        assert!(!inspector.saving);
        assert_eq!(view.error.as_deref(), Some("HTTP 404: no route"));
    }

    #[test]
    fn move_keeps_the_name_under_the_target_folder() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(c.move_to_folder("f1", "pics"));
        assert!(
            fake.calls()
                .contains(&"update:f1:pics/root.txt:root.txt".to_owned())
        );
        assert!(!c.move_to_folder("missing", "pics"));
    }

    #[test]
    fn single_delete_asks_first_then_sends_the_id() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(c.ask_delete("f1"));
        assert_eq!(
            c.view().confirm.as_deref(),
            Some("Are you sure you want to delete \"root.txt\" from R2 storage?")
        );
        assert!(!fake.calls().iter().any(|call| call.starts_with("delete")));
        assert!(c.confirm_delete());
        assert_eq!(fake.calls(), ["list", "delete:f1", "list"]);
        assert!(c.view().confirm.is_none());
    }

    #[test]
    fn cancelled_delete_sends_nothing() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.ask_delete("f1");
        c.cancel_delete();
        assert!(!c.confirm_delete());
        assert_eq!(fake.calls(), ["list"]);
    }

    #[test]
    fn batch_delete_uses_the_selected_keys_and_clears_the_selection() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert!(!c.ask_delete_selected(), "nothing selected yet");
        c.toggle_select("root.txt");
        c.toggle_select("clip.mp4");
        assert_eq!(c.view().selected_count, 2);
        assert!(c.ask_delete_selected());
        assert_eq!(
            c.view().confirm.as_deref(),
            Some("Delete 2 selected assets from R2 storage?")
        );
        assert!(c.confirm_delete());
        assert!(fake.calls().contains(&"delete_many:f1,f4".to_owned()));
        assert_eq!(c.view().selected_count, 0);
    }

    #[test]
    fn select_visible_selects_and_clears_the_listing() {
        let fake = Arc::default();
        let c = loaded(&fake);
        c.set_select_visible(true);
        let view = c.view();
        assert!(view.all_visible_selected);
        assert_eq!(view.selected_count, 2);
        c.set_select_visible(false);
        assert_eq!(c.view().selected_count, 0);
    }

    #[test]
    fn copy_marks_the_key_and_returns_its_url_until_cleared() {
        let fake = Arc::default();
        let c = loaded(&fake);
        assert_eq!(
            c.copy_url("root.txt").as_deref(),
            Some("https://cdn.example/root.txt")
        );
        assert!(c.view().files.iter().any(|f| f.copied));
        c.clear_copied();
        assert!(c.view().files.iter().all(|f| !f.copied));
        assert_eq!(c.copy_url("nope"), None);
    }

    #[test]
    fn failed_delete_sets_the_error() {
        let fake: Arc<Fake> = Arc::default();
        let c = loaded(&fake);
        fake.writes
            .lock()
            .unwrap()
            .push_back(Err(ApiError::Transport("down".into())));
        c.ask_delete("f1");
        c.confirm_delete();
        assert_eq!(c.view().error.as_deref(), Some("network error: down"));
        c.dismiss_error();
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn an_image_preview_loads_once_for_a_visible_card() {
        let fake: Arc<Fake> = Arc::default();
        fake.downloads
            .lock()
            .unwrap()
            .insert("https://cdn.example/pics/cat.png".to_owned(), png_bytes());
        let c = loaded(&fake);
        c.open_folder("pics");
        assert!(c.thumbnail("f2").is_none(), "nothing loads until asked");
        c.ensure_previews(&["f2".to_owned()]);
        c.ensure_previews(&["f2".to_owned()]);
        let thumb = c.thumbnail("f2").expect("preview decoded");
        assert_eq!((thumb.width, thumb.height), (2, 2));
        assert_eq!(thumb.rgba.len(), 16);
        assert!(c.view().files[0].has_preview);
        let fetches = fake
            .calls()
            .iter()
            .filter(|call| call.starts_with("fetch:"))
            .count();
        assert_eq!(fetches, 1, "a second request is skipped");
    }

    #[test]
    fn a_broken_image_is_marked_failed_and_not_fetched_again() {
        let fake: Arc<Fake> = Arc::default();
        fake.downloads.lock().unwrap().insert(
            "https://cdn.example/pics/cat.png".to_owned(),
            b"not a picture".to_vec(),
        );
        let c = loaded(&fake);
        c.open_folder("pics");
        c.ensure_previews(&["f2".to_owned()]);
        c.ensure_previews(&["f2".to_owned()]);
        assert!(c.thumbnail("f2").is_none());
        assert!(!c.view().files[0].has_preview);
        let fetches = fake
            .calls()
            .iter()
            .filter(|call| call.starts_with("fetch:"))
            .count();
        assert_eq!(fetches, 1);
    }

    #[test]
    fn documents_get_no_preview_and_a_missing_download_fails_quietly() {
        let fake: Arc<Fake> = Arc::default();
        let c = loaded(&fake);
        c.ensure_previews(&["f1".to_owned()]);
        assert!(c.thumbnail("f1").is_none());
        assert!(!c.view().files[0].has_preview);
        assert_eq!(
            c.view().error,
            None,
            "a preview failure is not an error banner"
        );
    }

    #[test]
    fn audio_that_cannot_play_sets_the_error_and_stays_stopped() {
        let fake: Arc<Fake> = Arc::default();
        let mut files = sample_files();
        files.push(file("f5", "song.mp3", Some("audio/mpeg"), 100));
        *fake.list.lock().unwrap() = Some(files);
        fake.downloads.lock().unwrap().insert(
            "https://cdn.example/song.mp3".to_owned(),
            b"garbage".to_vec(),
        );
        let c = loaded(&fake);
        assert!(c.toggle_audio("f5"));
        let view = c.view();
        assert!(view.error.is_some(), "playback failure is reported");
        assert!(view.files.iter().all(|f| !f.playing));
        assert!(!c.toggle_audio("missing"));
    }

    #[test]
    fn durations_format_as_minutes_and_hours() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65_000), "1:05");
        assert_eq!(format_duration(3_725_000), "1:02:05");
    }

    #[test]
    fn format_bytes_matches_the_web_helper() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512.0 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MB");
    }
}
