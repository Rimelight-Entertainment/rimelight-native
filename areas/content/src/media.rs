//! Media library (CMS.6): the grid of assets (thumbnail, name, size), upload through the native file picker (desktop), alt text,
//! delete, copy URL and folders. The same page is the picker other pages open (`picker` mode: Pick sends the asset up).
//!
//! Folders: the list takes a `folder` filter and an asset's key carries its folder path (`events/2026/poster.png` is in
//! `events/2026`). UNSURE: the server has no folder list, so the folder chips come from the assets of the current folder.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use space_client::content::MediaAsset;
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{DataCell, DataSource, error_text};

/// Splits an asset key into (folder, file name). A key without a slash is in the root folder "".
pub fn split_key(key: &str) -> (String, String) {
    match key.rfind('/') {
        Some(at) => (key[..at].to_string(), key[at + 1..].to_string()),
        None => (String::new(), key.to_string()),
    }
}

/// A size as the list shows it: bytes, KB or MB.
pub fn size_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Whether an asset is an image the grid can show as a thumbnail.
pub fn is_image(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// The mime type for an upload, from the file's extension. Unknown files are `application/octet-stream`.
pub fn mime_for(file_name: &str) -> &'static str {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "txt" | "md" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// One grid cell.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub alt: String,
    pub size: String,
    pub image: bool,
}

/// Maps an asset to its grid cell.
pub fn media_row(asset: &MediaAsset) -> MediaRow {
    MediaRow {
        id: asset.id.clone(),
        name: split_key(&asset.key).1,
        url: asset.url.clone(),
        alt: asset.alt.clone().unwrap_or_default(),
        size: size_label(asset.size),
        image: is_image(&asset.mime),
    }
}

/// The folders of a listing: each folder path once, sorted, "" (root) left out.
pub fn folders(assets: &[MediaAsset]) -> Vec<String> {
    let mut out: Vec<String> = assets
        .iter()
        .map(|a| split_key(&a.key).0)
        .filter(|f| !f.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The window side of the media library.
pub trait MediaWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_folder(&self, folder: &str, folders: Vec<String>);
    fn show_rows(&self, rows: Vec<MediaRow>);
    fn show_notice(&self, text: &str);
    fn copy_text(&self, text: &str);
    fn ask_reload(&self);
    fn on_reload(&self, f: impl Fn() + 'static);
    fn on_folder(&self, f: impl Fn(String) + 'static);
    fn on_upload(&self, f: impl Fn() + 'static);
    fn on_alt(&self, f: impl Fn(String, String) + 'static);
    fn on_delete(&self, f: impl Fn(String) + 'static);
    fn on_copy(&self, f: impl Fn(String) + 'static);
    fn on_pick(&self, f: impl Fn(String) + 'static);
}

#[derive(Default)]
struct Shared {
    generation: u64,
    folder: String,
    assets: Vec<MediaAsset>,
}

/// The media page. Clones share the state. `picked` receives (id, url) when a picker picks an asset (picker mode).
pub struct Media<W: MediaWindow> {
    ui: slint::Weak<W>,
    data: DataCell,
    shared: Arc<Mutex<Shared>>,
    picked: Option<Rc<dyn Fn(String, String)>>,
}

impl<W: MediaWindow> Clone for Media<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            shared: self.shared.clone(),
            picked: self.picked.clone(),
        }
    }
}

/// Wires the page on `ui`. `picked` is set when the page is a picker (the other page's callback).
pub fn wire<W: MediaWindow>(
    ui: &W,
    data: DataCell,
    picked: Option<Rc<dyn Fn(String, String)>>,
) -> Media<W> {
    let page = Media {
        ui: ui.as_weak(),
        data,
        shared: Arc::new(Mutex::new(Shared::default())),
        picked,
    };
    let p = page.clone();
    ui.on_reload(move || p.load());
    let p = page.clone();
    ui.on_folder(move |folder| p.open_folder(&folder));
    let p = page.clone();
    ui.on_upload(move || p.upload());
    let p = page.clone();
    ui.on_alt(move |id, alt| p.set_alt(&id, &alt));
    let p = page.clone();
    ui.on_delete(move |id| p.delete(&id));
    let p = page.clone();
    ui.on_copy(move |id| p.copy(&id));
    let p = page.clone();
    ui.on_pick(move |id| p.pick(&id));
    page
}

impl<W: MediaWindow> Media<W> {
    fn run<T, F, Fut, D>(&self, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(Arc<space_client::api::ApiClient>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, ClientError>>,
        D: FnOnce(&W, Result<T, ClientError>) + Send + 'static,
    {
        let source: Option<Arc<DataSource>> = self.data.borrow().clone();
        if let Some(source) = source {
            source.fetch(&self.ui, work, done);
        }
    }

    /// Loads the assets of the current folder. The shell calls it when the media route opens.
    pub fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (generation, folder) = {
            let mut s = lock(&self.shared);
            s.generation += 1;
            ui.show_state(LoadState::Loading);
            (s.generation, s.folder.clone())
        };
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move {
                let filter = (!folder.is_empty()).then_some(folder.as_str());
                api.content_media(&token, filter).await
            },
            move |ui, result| {
                let mut s = lock(&shared);
                if s.generation != generation {
                    return;
                }
                match result {
                    Ok(assets) => {
                        s.assets = assets;
                        let rows: Vec<MediaRow> = s.assets.iter().map(media_row).collect();
                        ui.show_folder(&s.folder, folders(&s.assets));
                        ui.show_state(if rows.is_empty() {
                            LoadState::Empty
                        } else {
                            LoadState::Loaded
                        });
                        ui.show_rows(rows);
                    }
                    Err(e) => {
                        ui.show_state(LoadState::Error);
                        ui.show_error(&error_text(&e));
                    }
                }
            },
        );
    }

    fn open_folder(&self, folder: &str) {
        lock(&self.shared).folder = folder.to_string();
        self.load();
    }

    /// Upload: the desktop file picker, then the bytes to the library in the current folder. Web and Android have no
    /// native picker here (UNSURE: the canvas area's picker is the model when this is wired).
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn upload(&self) {
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else { return };
        let api = source.api();
        let folder = lock(&self.shared).folder.clone();
        let ui_weak = self.ui.clone();
        space_page_kit::task::spawn_task(move || async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Upload to the media library")
                .pick_file()
                .await
            else {
                return;
            };
            let name = file.file_name();
            let bytes = file.read().await;
            let folder = (!folder.is_empty()).then_some(folder.as_str());
            let result = api
                .upload_content_media(&token, &name, mime_for(&name), bytes, None, folder)
                .await;
            space_page_kit::page_data::ui_thread(&ui_weak, move |ui| match result {
                Ok(_) => ui.ask_reload(),
                Err(e) => ui.show_notice(&error_text(&e)),
            });
        });
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn upload(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.show_notice("Upload needs the desktop app.");
        }
    }

    fn set_alt(&self, id: &str, alt: &str) {
        let (id, alt) = (id.to_string(), alt.trim().to_string());
        self.run(
            move |api, token| async move { api.update_content_media_alt(&token, &id, &alt).await },
            move |ui, result| match result {
                Ok(_) => ui.ask_reload(),
                Err(e) => ui.show_notice(&error_text(&e)),
            },
        );
    }

    /// Deletes an asset. The window asks for confirmation first; the server refuses an asset that pages still use.
    fn delete(&self, id: &str) {
        let id = id.to_string();
        self.run(
            move |api, token| async move { api.delete_content_media(&token, &id).await },
            move |ui, result| match result {
                Ok(()) => ui.ask_reload(),
                Err(e) => ui.show_notice(&error_text(&e)),
            },
        );
    }

    fn copy(&self, id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let url = lock(&self.shared)
            .assets
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.url.clone());
        if let Some(url) = url {
            ui.copy_text(&url);
            ui.show_notice("Link copied.");
        }
    }

    /// Picker mode: the asset goes to the page that opened the picker.
    fn pick(&self, id: &str) {
        let Some(picked) = self.picked.clone() else {
            return;
        };
        let url = lock(&self.shared)
            .assets
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.url.clone());
        if let Some(url) = url {
            picked(id.to_string(), url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_split_into_folder_and_name() {
        assert_eq!(
            split_key("events/2026/poster.png"),
            ("events/2026".into(), "poster.png".into())
        );
        assert_eq!(split_key("logo.svg"), ("".into(), "logo.svg".into()));
    }

    #[test]
    fn sizes_read_in_bytes_kb_and_mb() {
        assert_eq!(size_label(12), "12 B");
        assert_eq!(size_label(2048), "2 KB");
        assert_eq!(size_label(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
    }

    #[test]
    fn mime_follows_the_extension() {
        assert_eq!(mime_for("Photo.JPG"), "image/jpeg");
        assert_eq!(mime_for("notes"), "application/octet-stream");
        assert!(is_image("image/webp"));
        assert!(!is_image("application/pdf"));
    }
}
