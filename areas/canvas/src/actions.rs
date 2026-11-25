//! Canvas actions (CV.7, F2, CV.13): copying images and files to the clipboard, opening a source link, exporting a PNG,
//! saving originals to a folder, the backup zip (save, and open, which creates a new canvas in the same home), and the
//! asset usage list with its cleanup, plus replacing or relinking an image with one picked file.
//!
//! Every network call and every file dialog runs through [`DataSource::fetch`]: the API and the stored token are given on a
//! worker, and the answer comes back on the UI thread, where the toasts are shown. Local work (decoding, rendering, zipping)
//! also runs on the worker, and its failure comes back as the `Err(String)` of the worker's result (shown as a toast).
//! The desktop-only parts sit behind the cfg of the native file dialogs; other targets show a toast.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
use std::borrow::Cow;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use std::path::{Path, PathBuf};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use serde_json::{Map, json};
use space_canvas::geom::Rect;
use space_canvas::model::Item;
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_proto::canvases::{AssetUsage, AssetView};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use space_proto::canvases::{CanvasView, CreateCanvas, ItemState};

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::export;
use crate::images;
use crate::window::Ui;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::{list, page};
use rimelight_ui_kit::toast::{self, Tone};
use space_page_kit::page_data::{self, DataSource};

/// Where an action runs: the window to answer on, the data source, and the canvas (and its project, for a project canvas)
/// the action works on.
#[derive(Clone)]
pub struct Ctx {
    pub ui: Ui,
    pub source: Arc<DataSource>,
    pub canvas_id: String,
    pub project_key: Option<String>,
}

/// A step that runs on the worker: `Err` is a message for the reader, shown as a danger toast.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
type Local<T> = Result<T, String>;

/// What a worker step gives back: its own failure (a message) or a network failure (a client error).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
type Outcome<T> = Result<Local<T>, ClientError>;

/// The image file types a picture can be picked as, by extension.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// The file name of a path, or an empty name when it has none.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The picture file type of a file name, from its extension. `None` for any other file.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn picture_mime(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let (_, extension) = lower.rsplit_once('.')?;
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// Settles a worker result on the UI thread: the value of a success (a cancelled dialog is a success with `None`), or a
/// toast for the failure and `None`.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn settle<T>(result: Outcome<T>) -> Option<T> {
    match result {
        Ok(Ok(value)) => Some(value),
        Ok(Err(message)) => {
            toast::announce(Tone::Danger, message);
            None
        }
        Err(error) => {
            toast::announce(Tone::Danger, page_data::error_text(&error));
            None
        }
    }
}

/// A new UUID (version 4) for an item restored from a backup. Unique, not secret (the same scheme as the page's own ids).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn fresh_id() -> String {
    use std::hash::{BuildHasher, Hasher};
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

/// The original bytes of each asset, with its file name. Runs on the worker.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn download_originals(
    api: &ApiClient,
    token: &str,
    canvas: &str,
    assets: &[AssetView],
) -> Result<Vec<(String, Vec<u8>)>, ClientError> {
    let mut files = Vec::with_capacity(assets.len());
    for asset in assets {
        let bytes = api.canvas_asset_bytes(token, canvas, &asset.id).await?;
        files.push((asset.file_name.clone(), bytes));
    }
    Ok(files)
}

/// The first frame of a picture as RGBA pixels: (width, height, bytes). Runs on the worker.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn first_frame_rgba(bytes: &[u8]) -> Local<(usize, usize, Vec<u8>)> {
    let decoded = images::decode(bytes)?;
    let first = decoded.first();
    let pixels = first.as_bytes().to_vec();
    Ok((first.width() as usize, first.height() as usize, pixels))
}

/// Downloads the original of `asset` and puts its first frame on the clipboard as full-size pixels. The thumbnail is not used.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub fn copy_original_image(ctx: &Ctx, asset: AssetView) {
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| async move {
            let bytes = api.canvas_asset_bytes(&token, &canvas, &asset.id).await?;
            Ok::<_, ClientError>(first_frame_rgba(&bytes))
        },
        move |_ui, result| {
            let Some((width, height, pixels)) = settle(result) else {
                return;
            };
            let image = arboard::ImageData {
                width,
                height,
                bytes: Cow::Owned(pixels),
            };
            match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_image(image)) {
                Ok(()) => toast::announce(Tone::Success, "Copied image."),
                Err(error) => toast::announce(
                    Tone::Danger,
                    format!("The image could not be copied: {error}"),
                ),
            }
        },
    );
}

/// Copying images is not built for the web, Android or iOS yet.
#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
pub fn copy_original_image(ctx: &Ctx, asset: AssetView) {
    let _ = (ctx, asset);
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Writes the files into the copy folder in the temp directory, with their names made unique. Runs on the worker.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn write_copy_files(files: Vec<(String, Vec<u8>)>) -> Local<Vec<PathBuf>> {
    let dir = std::env::temp_dir().join("rimelight-space-copy");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("The copy folder could not be made: {error}"))?;
    let names: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
    let unique = export::unique_file_names(&names);
    let mut paths = Vec::with_capacity(unique.len());
    for (name, (_, bytes)) in unique.iter().zip(files.iter()) {
        let path = dir.join(name);
        std::fs::write(&path, bytes)
            .map_err(|error| format!("{name} could not be written: {error}"))?;
        paths.push(path);
    }
    Ok(paths)
}

/// Downloads the original files of `assets`, writes them as files and puts them on the clipboard as a file list.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
pub fn copy_files(ctx: &Ctx, assets: Vec<AssetView>) {
    if assets.is_empty() {
        toast::announce(Tone::Neutral, "No images to copy.");
        return;
    }
    let canvas = ctx.canvas_id.clone();
    let count = assets.len();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| async move {
            let files = download_originals(&api, &token, &canvas, &assets).await?;
            Ok::<_, ClientError>(write_copy_files(files))
        },
        move |_ui, result| {
            let Some(paths) = settle(result) else {
                return;
            };
            match arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.set().file_list(paths.as_slice()))
            {
                Ok(()) => toast::announce(Tone::Success, format!("Copied {count} files.")),
                Err(error) => toast::announce(
                    Tone::Danger,
                    format!("The files could not be copied: {error}"),
                ),
            }
        },
    );
}

/// Copying files is not built for the web, Android or iOS yet.
#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
pub fn copy_files(ctx: &Ctx, assets: Vec<AssetView>) {
    let _ = (ctx, assets);
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Opens a source link in the browser. Only web addresses open; anything else is refused with a toast. Runs on the UI thread.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn open_source(url: &str) {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        toast::announce(Tone::Warning, "Only web links can be opened.");
        return;
    }
    if open::that(url).is_err() {
        toast::announce(Tone::Danger, "The link could not be opened.");
    }
}

/// Opening links is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn open_source(url: &str) {
    let _ = url;
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Shows the save dialog for a PNG, renders the items on the worker (light colours, white background, at most 16384 px on
/// the longer side) and writes the file. `Ok(None)` when the dialog is cancelled; otherwise the file name.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn save_png(
    items: Vec<Item>,
    pictures: HashMap<String, images::Buffer>,
    bounds: Rect,
    file_stem: String,
) -> Local<Option<String>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export as image")
        .set_file_name(format!("{file_stem}.png"))
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let scale = export::fit_scale(bounds, 16384);
    let refs: Vec<&Item> = items.iter().collect();
    let rendered = export::render(&refs, &pictures, bounds, scale);
    let png = export::png_bytes(&rendered)?;
    let path = file.path().to_path_buf();
    std::fs::write(&path, png)
        .map_err(|error| format!("The image could not be written: {error}"))?;
    Ok(Some(name_of(&path)))
}

/// Exports the items as one PNG (the caller gives the bounds of what to draw and the decoded originals by asset id). Renders
/// on the worker, after the save dialog.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn export_image(
    ctx: &Ctx,
    items: Vec<Item>,
    pictures: HashMap<String, images::Buffer>,
    bounds: Rect,
    file_stem: String,
) {
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        toast::announce(Tone::Warning, "There is nothing to export.");
        return;
    }
    ctx.ui.fetch(
        &ctx.source,
        move |_api, _token| async move {
            Ok::<_, ClientError>(save_png(items, pictures, bounds, file_stem).await)
        },
        move |_ui, result| {
            if let Some(Some(name)) = settle(result) {
                toast::announce(Tone::Success, format!("Exported {name}."));
            }
        },
    );
}

/// Exporting images is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn export_image(
    ctx: &Ctx,
    items: Vec<Item>,
    pictures: HashMap<String, images::Buffer>,
    bounds: Rect,
    file_stem: String,
) {
    let _ = (ctx, items, pictures, bounds, file_stem);
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Writes each file into `folder` under a unique name. Runs on the worker. The count of files written.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn write_into_folder(folder: &Path, files: Vec<(String, Vec<u8>)>) -> Local<usize> {
    let names: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
    let unique = export::unique_file_names(&names);
    for (name, (_, bytes)) in unique.iter().zip(files.iter()) {
        std::fs::write(folder.join(name), bytes)
            .map_err(|error| format!("{name} could not be written: {error}"))?;
    }
    Ok(files.len())
}

/// Shows the folder dialog, downloads the originals and writes them. `Ok(None)` when the dialog is cancelled.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn save_originals_to_folder(
    api: Arc<ApiClient>,
    token: String,
    canvas: String,
    files: Vec<(String, AssetView)>,
) -> Outcome<Option<usize>> {
    let Some(folder) = rfd::AsyncFileDialog::new()
        .set_title("Save images to folder")
        .pick_folder()
        .await
    else {
        return Ok(Ok(None));
    };
    // The original is taken from the disk cache when it is there (videos are kept there after an upload or a play).
    let cache = images::DiskCache::open();
    let mut named = Vec::with_capacity(files.len());
    for (name, asset) in &files {
        let bytes = match cache.get(&asset.sha256, "orig") {
            Some(bytes) => bytes,
            None => api.canvas_asset_bytes(&token, &canvas, &asset.id).await?,
        };
        named.push((name.clone(), bytes));
    }
    Ok(write_into_folder(folder.path(), named).map(Some))
}

/// Saves the original file of each image into a folder the user picks, under names made unique. Each entry is the file name
/// to use and the asset.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn save_images_to_folder(ctx: &Ctx, assets: Vec<(String, AssetView)>) {
    if assets.is_empty() {
        toast::announce(Tone::Neutral, "No images to save.");
        return;
    }
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| save_originals_to_folder(api, token, canvas, assets),
        move |_ui, result| {
            if let Some(Some(count)) = settle(result) {
                toast::announce(Tone::Success, format!("Saved {count} images."));
            }
        },
    );
}

/// Saving images to a folder is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn save_images_to_folder(ctx: &Ctx, assets: Vec<(String, AssetView)>) {
    let _ = (ctx, assets);
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Writes the backup zip: `canvas_json` with the asset list added under "assets", and the asset files under assets/.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn write_backup(
    path: &Path,
    mut canvas_json: Value,
    assets_json: Value,
    entries: &[(String, Vec<u8>)],
) -> Local<()> {
    let Some(object) = canvas_json.as_object_mut() else {
        return Err(
            "The backup could not be written: the canvas data is not an object.".to_string(),
        );
    };
    object.insert("assets".to_string(), assets_json);
    let zip = export::backup_zip(&canvas_json, entries)?;
    std::fs::write(path, zip).map_err(|error| format!("The backup could not be written: {error}"))
}

/// Shows the save dialog for the backup, downloads the asset originals and writes the zip. `Ok(None)` when cancelled.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn save_backup_zip(
    api: Arc<ApiClient>,
    token: String,
    canvas: String,
    canvas_json: Value,
    assets: Vec<AssetView>,
    title: String,
) -> Outcome<Option<String>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save backup")
        .set_file_name(format!("{title}.rimelight-canvas.zip"))
        .add_filter("Canvas backup", &["zip"])
        .save_file()
        .await
    else {
        return Ok(Ok(None));
    };
    let names: Vec<String> = assets.iter().map(|asset| asset.file_name.clone()).collect();
    let unique = export::unique_file_names(&names);
    let mut entries = Vec::with_capacity(assets.len());
    let mut assets_json = Vec::with_capacity(assets.len());
    for (asset, file_name) in assets.iter().zip(unique) {
        let bytes = api.canvas_asset_bytes(&token, &canvas, &asset.id).await?;
        assets_json.push(json!({
            "id": asset.id,
            "file": file_name,
            "mime": asset.mime,
            "sha256": asset.sha256,
        }));
        entries.push((file_name, bytes));
    }
    let path = file.path().to_path_buf();
    let written = write_backup(&path, canvas_json, Value::Array(assets_json), &entries);
    Ok(written.map(|()| Some(name_of(&path))))
}

/// Saves a backup zip of the canvas: `canvas_json` (version, title, items) and the originals of `assets`.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn save_backup(ctx: &Ctx, canvas_json: Value, assets: Vec<AssetView>, title: String) {
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| save_backup_zip(api, token, canvas, canvas_json, assets, title),
        move |_ui, result| {
            if let Some(Some(name)) = settle(result) {
                toast::announce(Tone::Success, format!("Saved backup {name}."));
            }
        },
    );
}

/// Saving a backup is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn save_backup(ctx: &Ctx, canvas_json: Value, assets: Vec<AssetView>, title: String) {
    let _ = (ctx, canvas_json, assets, title);
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// A restored canvas and its items, ready for the page to import once the new canvas's room has opened.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
type Restored = (CanvasView, Vec<Item>);

/// Rebuilds the items of a backup on a new canvas: fresh item ids (frame and group links follow them), asset ids remapped to
/// the uploaded copies. Items that cannot be read are left out.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn restore_items(json: &Value, asset_map: &HashMap<String, String>) -> Vec<Item> {
    let raw: Vec<&Value> = json
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default();
    let mut id_map: HashMap<String, String> = HashMap::new();
    for item in &raw {
        if let Some(old) = item.get("id").and_then(Value::as_str) {
            id_map.entry(old.to_string()).or_insert_with(fresh_id);
        }
    }
    let mut items = Vec::with_capacity(raw.len());
    for item in raw {
        let (Some(kind), Some(z)) = (
            item.get("kind").and_then(Value::as_str),
            item.get("z").and_then(Value::as_str),
        ) else {
            continue;
        };
        let mut props: Map<String, Value> = item
            .get("props")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(old) = props
            .get("asset")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            let new = asset_map.get(&old).cloned().unwrap_or_default();
            props.insert("asset".to_string(), Value::String(new));
        }
        for key in ["frame", "group"] {
            if let Some(old) = props.get(key).and_then(Value::as_str).map(str::to_owned) {
                match id_map.get(&old).cloned() {
                    Some(new) => {
                        props.insert(key.to_string(), Value::String(new));
                    }
                    None => {
                        props.remove(key);
                    }
                }
            }
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(|old| id_map.get(old).cloned())
            .unwrap_or_else(fresh_id);
        let state = ItemState {
            id,
            kind: kind.to_string(),
            z: z.to_string(),
            version: 0,
            props,
            deleted: false,
            created_by: String::new(),
            updated_by: String::new(),
            updated_at: 0,
        };
        if let Some(restored) = Item::from_state(&state) {
            items.push(restored);
        }
    }
    items
}

/// Shows the open dialog, reads the backup and creates a new canvas in the same home with its assets and items. `Ok(None)`
/// when cancelled.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn restore_backup(
    api: Arc<ApiClient>,
    token: String,
    project_key: Option<String>,
) -> Outcome<Option<Restored>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Open backup")
        .add_filter("Canvas backup", &["zip"])
        .pick_file()
        .await
    else {
        return Ok(Ok(None));
    };
    let bytes = file.read().await;
    let (json, files) = match export::read_backup(&bytes) {
        Ok(backup) => backup,
        Err(message) => return Ok(Err(message)),
    };
    let title = json
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Canvas");
    let view = api
        .create_canvas(
            &token,
            &CreateCanvas {
                title: Some(format!("{title} (restored)")),
                project_key,
            },
        )
        .await?;
    let mut asset_map: HashMap<String, String> = HashMap::new();
    let listed = json
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for entry in &listed {
        let (Some(old), Some(file_name)) = (
            entry.get("id").and_then(Value::as_str),
            entry.get("file").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Some((_, data)) = files.iter().find(|(name, _)| name == file_name) else {
            continue;
        };
        let mime = entry
            .get("mime")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        let uploaded = api
            .upload_canvas_asset(&token, &view.id, file_name, mime, data.clone())
            .await?;
        asset_map.insert(old.to_string(), uploaded.id);
    }
    let items = restore_items(&json, &asset_map);
    Ok(Ok(Some((view, items))))
}

/// Opens a backup zip as a new canvas in the same home as the open canvas, then opens that canvas and imports its items.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn open_backup(ctx: &Ctx) {
    let project_key = ctx.project_key.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| restore_backup(api, token, project_key),
        move |ui, result| {
            if let Some(Some((view, items))) = settle(result) {
                let route = list::route_of(&view);
                page::queue_import(view.id.clone(), items);
                ui.open_route(&route);
            }
        },
    );
}

/// Opening a backup is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn open_backup(ctx: &Ctx) {
    let _ = ctx;
    toast::announce(Tone::Neutral, "Not available here yet.");
}

/// Loads the canvas's images with how many live items use each. The answer goes to `done` (on the UI thread).
pub fn load_asset_usage(
    ctx: &Ctx,
    done: impl FnOnce(Result<Vec<AssetUsage>, ClientError>) + Send + 'static,
) {
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| async move { api.canvas_assets(&token, &canvas).await },
        move |_ui, result| done(result),
    );
}

/// Deletes each asset in `ids` (the server refuses one that an item still uses). Stops at the first failure. The count of
/// the assets deleted and the failure's message, if any.
async fn delete_assets(
    api: Arc<ApiClient>,
    token: String,
    canvas: String,
    ids: Vec<String>,
) -> Result<(usize, Option<String>), ClientError> {
    let mut deleted = 0;
    for id in &ids {
        match api.delete_canvas_asset(&token, &canvas, id).await {
            Ok(()) => deleted += 1,
            Err(error) => return Ok((deleted, Some(page_data::error_text(&error)))),
        }
    }
    Ok((deleted, None))
}

/// Deletes the listed images of the canvas that no item uses. `done` gets the count deleted; a failure is shown as a toast.
pub fn delete_unused_assets(
    ctx: &Ctx,
    ids: Vec<String>,
    done: impl FnOnce(usize) + Send + 'static,
) {
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| delete_assets(api, token, canvas, ids),
        move |_ui, result| match result {
            Ok((deleted, failure)) => {
                if let Some(message) = failure {
                    toast::announce(Tone::Danger, message);
                }
                done(deleted);
            }
            Err(error) => {
                toast::announce(Tone::Danger, page_data::error_text(&error));
                done(0);
            }
        },
    );
}

/// Shows the open dialog for one picture, decodes it, uploads it to the canvas. `Ok(None)` when cancelled.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
async fn pick_and_upload(
    api: Arc<ApiClient>,
    token: String,
    canvas: String,
) -> Outcome<Option<(AssetView, images::Decoded)>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Choose an image")
        .add_filter("Images", &IMAGE_EXTENSIONS)
        .pick_file()
        .await
    else {
        return Ok(Ok(None));
    };
    let name = file.file_name();
    let bytes = file.read().await;
    let decoded = match images::decode(&bytes) {
        Ok(decoded) => decoded,
        Err(message) => return Ok(Err(message)),
    };
    let mime = picture_mime(&name).unwrap_or("application/octet-stream");
    let asset = api
        .upload_canvas_asset(&token, &canvas, &name, mime, bytes)
        .await?;
    Ok(Ok(Some((asset, decoded))))
}

/// Picks one picture, uploads it to the canvas and decodes it. `done` gets the new asset and its decoded picture, or `None`
/// when the dialog was cancelled or the picture failed (a toast says why).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn pick_and_upload_one(
    ctx: &Ctx,
    done: impl FnOnce(Option<(AssetView, images::Decoded)>) + Send + 'static,
) {
    let canvas = ctx.canvas_id.clone();
    ctx.ui.fetch(
        &ctx.source,
        move |api, token| pick_and_upload(api, token, canvas),
        move |_ui, result| done(settle(result).flatten()),
    );
}

/// Picking pictures is not built for the web or Android.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn pick_and_upload_one(
    ctx: &Ctx,
    done: impl FnOnce(Option<(AssetView, images::Decoded)>) + Send + 'static,
) {
    let _ = ctx;
    toast::announce(Tone::Neutral, "Not available here yet.");
    done(None);
}
