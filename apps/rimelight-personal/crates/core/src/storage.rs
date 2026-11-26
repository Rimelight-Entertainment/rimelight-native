//! Storage (Assets) API client (`/api/storage`). Each asset is an R2 object addressed by a
//! `folder/name` key and has a row with an id, a public CDN URL, a size and a MIME type.
//! Rename and move share one `PATCH`, which rewrites the key. The key helpers below port the
//! Solid client's naming rules exactly, including its extension handling.

use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, percent_encode};

/// One asset as the page shows it. `name` is the filename, `key` the full R2 path.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageFile {
    pub id: String,
    pub name: String,
    pub key: String,
    pub url: String,
    pub size: u64,
    pub mime_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    key: String,
    url: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    mime_type: Option<String>,
}

impl From<RawFile> for StorageFile {
    /// Solid reads `name || filename`, so an empty `name` falls back to `filename`.
    fn from(raw: RawFile) -> Self {
        Self {
            id: raw.id,
            name: raw
                .name
                .filter(|n| !n.is_empty())
                .or(raw.filename)
                .unwrap_or_default(),
            key: raw.key,
            url: raw.url,
            size: raw.size.unwrap_or(0),
            mime_type: raw.mime_type.filter(|m| !m.is_empty()),
        }
    }
}

#[derive(Deserialize)]
struct FileList {
    #[serde(default)]
    files: Vec<RawFile>,
}

/// Body for `PATCH /api/storage/files`, used for both rename and move.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePatch<'a> {
    pub id: &'a str,
    pub new_key: &'a str,
    pub new_name: &'a str,
}

/// One file in an upload. `mime` is sent as the part's `Content-Type`.
#[derive(Debug, Clone, Copy)]
pub struct UploadFile<'a> {
    pub name: &'a str,
    pub mime: &'a str,
    pub bytes: &'a [u8],
}

const BOUNDARY: &str = "rimelight-personal-storage-boundary";

pub fn decode_files(bytes: &[u8]) -> Result<Vec<StorageFile>, ApiError> {
    serde_json::from_slice::<FileList>(bytes)
        .map(|list| list.files.into_iter().map(StorageFile::from).collect())
        .map_err(|e| ApiError::Decode(e.to_string()))
}

/// Builds the `multipart/form-data` body Solid's upload sends: a `folder` field, then one
/// `file` part per file. The folder is sent even when empty, as Solid does.
pub fn multipart_body(folder: &str, files: &[UploadFile<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"folder\"\r\n\r\n{folder}\r\n"
        )
        .as_bytes(),
    );
    for file in files {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                escape_quoted(file.name),
                file.mime,
            )
            .as_bytes(),
        );
        body.extend_from_slice(file.bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Percent-encodes the characters a browser encodes in a multipart `filename` parameter.
fn escape_quoted(name: &str) -> String {
    name.replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

impl ApiClient {
    pub fn storage_files(
        &self,
        on_done: impl FnOnce(Result<Vec<StorageFile>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/storage/files")),
            decode_files,
            on_done,
        );
    }

    /// Uploads `files` into `folder` (empty for root). The response body is ignored; the
    /// caller reloads the list.
    pub fn upload_storage_files(
        &self,
        folder: &str,
        files: &[UploadFile<'_>],
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = multipart_body(folder, files);
        let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
        let mut request = Request::new(
            Method::POST,
            self.url("/storage/upload"),
            Headers::new(&[("Content-Type", content_type.as_str())]),
        );
        request.body = body;
        self.send(request, |_: &[u8]| Ok(()), on_done);
    }

    /// Deletes one asset with `?id=`, as Solid's single-file delete does.
    pub fn delete_storage_file(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = self.url(&format!("/storage/files?id={}", percent_encode(id)));
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Deletes several assets with one `?ids=a,b` request, as Solid's batch delete does.
    pub fn delete_storage_files(
        &self,
        ids: &[String],
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let joined = ids
            .iter()
            .map(|id| percent_encode(id))
            .collect::<Vec<_>>()
            .join(",");
        let url = self.url(&format!("/storage/files?ids={joined}"));
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Renames and/or moves one asset. Solid's client sends this `PATCH`; the response body is
    /// ignored, and only the status is checked.
    pub fn update_storage_file(
        &self,
        patch: &FilePatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(patch)
            .expect("request bodies are plain structs and always serialize");
        let mut request = Request::new(
            Method::PATCH,
            self.url("/storage/files"),
            Headers::new(&[("Content-Type", "application/json")]),
        );
        request.body = body;
        self.send(request, |_: &[u8]| Ok(()), on_done);
    }
}

/// Trims a folder path and drops leading and trailing slashes (Solid `normalizeFolder`).
pub fn normalize_folder(input: &str) -> String {
    input.trim().trim_matches('/').to_owned()
}

/// Solid's filename sanitizer. It trims, then replaces each character outside `[A-Za-z0-9._-]`
/// with one `-` per UTF-16 code unit, as the JavaScript regex does.
pub fn sanitize_basename(input: &str) -> String {
    let mut out = String::new();
    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            for _ in 0..ch.len_utf16() {
                out.push('-');
            }
        }
    }
    out
}

/// The part after the last `.`, or the whole name when it has no dot. An empty name gives "".
/// Solid's `name.split(".").pop()`.
pub fn extension_of(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or("")
}

/// The key for `file_name` inside `folder`. Root keys are the bare file name.
pub fn join_key(folder: &str, file_name: &str) -> String {
    if folder.is_empty() {
        file_name.to_owned()
    } else {
        format!("{folder}/{file_name}")
    }
}

/// Splits a key into its folder and its basename (the name without its last extension).
/// The Solid inspector uses this to fill its fields.
pub fn split_key(key: &str) -> (String, String) {
    let (folder, filename) = match key.rsplit_once('/') {
        Some((folder, filename)) => (folder.to_owned(), filename),
        None => (String::new(), key),
    };
    let basename = match filename.rfind('.') {
        Some(dot) => &filename[..dot],
        None => filename,
    };
    (folder, basename.to_owned())
}

/// The filename after the Rename & Move form is saved: the sanitized basename plus the
/// current name's extension. Solid appends the extension even when the name has no dot, so
/// `README` becomes `base.README`, and it skips the dot only when the extension is empty.
pub fn rename_filename(current_name: &str, basename: &str) -> String {
    let ext = extension_of(current_name);
    let sanitized = sanitize_basename(basename);
    if ext.is_empty() {
        sanitized
    } else {
        format!("{sanitized}.{ext}")
    }
}

/// The filename a drag-to-folder move keeps. Solid splits off the last extension, then
/// re-appends it, so a name with no dot gets its own text as the extension: `README` becomes
/// `README.README`.
pub fn move_filename(name: &str) -> String {
    let ext = extension_of(name);
    let base = match name.rfind('.') {
        Some(dot) => &name[..dot],
        None => name,
    };
    if ext.is_empty() {
        base.to_owned()
    } else {
        format!("{base}.{ext}")
    }
}

/// The key shown in the inspector's preview while the form is being edited. Solid does not
/// sanitize the basename here, so the preview can differ from the saved key.
pub fn preview_key(folder: &str, basename: &str, current_name: &str) -> String {
    let preview = format!("{basename}.{}", extension_of(current_name));
    join_key(folder, &preview)
}

/// Which preview a file gets. The MIME type decides first, then the key's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Document,
}

pub fn media_kind(mime: Option<&str>, key: &str) -> MediaKind {
    let mime = mime.unwrap_or("");
    let ext = key.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let ext = ext.as_str();
    if mime.starts_with("image/")
        || matches!(
            ext,
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "svg" | "avif"
        )
    {
        MediaKind::Image
    } else if mime.starts_with("video/") || matches!(ext, "mp4" | "webm" | "mov" | "mkv") {
        MediaKind::Video
    } else if mime.starts_with("audio/") || matches!(ext, "mp3" | "wav" | "aac" | "ogg" | "flac") {
        MediaKind::Audio
    } else {
        MediaKind::Document
    }
}

/// The MIME type sent with an upload, inferred from the extension. egui exposes no browser
/// `File.type`, so unknown extensions become `application/octet-stream`.
pub fn mime_for(name: &str) -> &'static str {
    match extension_of(name).to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_falls_back_to_filename_and_defaults_missing_fields() {
        let json = br#"{"files":[
            {"id":"a","name":"","filename":"photo.png","key":"pics/photo.png","url":"u1","size":12,"mimeType":"image/png","userId":"x"},
            {"id":"b","key":"notes.txt","url":"u2","size":null,"mimeType":""}
        ]}"#;
        let files = decode_files(json).unwrap();
        assert_eq!(files[0].name, "photo.png");
        assert_eq!(files[0].size, 12);
        assert_eq!(files[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(files[1].name, "");
        assert_eq!(files[1].size, 0);
        assert_eq!(files[1].mime_type, None);
    }

    #[test]
    fn decode_without_files_key_is_empty() {
        assert!(decode_files(b"{}").unwrap().is_empty());
    }

    #[test]
    fn multipart_body_has_folder_and_file_parts() {
        let file = UploadFile {
            name: "a\"b.txt",
            mime: "text/plain",
            bytes: b"hi",
        };
        let body = String::from_utf8(multipart_body("docs", &[file])).unwrap();
        assert!(body.contains("name=\"folder\"\r\n\r\ndocs\r\n"));
        assert!(body.contains(
            "name=\"file\"; filename=\"a%22b.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n"
        ));
        assert!(body.ends_with("--rimelight-personal-storage-boundary--\r\n"));
    }

    #[test]
    fn sanitize_maps_each_utf16_unit_to_a_dash() {
        assert_eq!(sanitize_basename("  my file (1).png "), "my-file--1-.png");
        assert_eq!(sanitize_basename("a\u{1F600}"), "a--");
        assert_eq!(sanitize_basename("   "), "");
    }

    #[test]
    fn move_filename_matches_solid_quirks() {
        assert_eq!(move_filename("photo.jpg"), "photo.jpg");
        assert_eq!(move_filename("archive.tar.gz"), "archive.tar.gz");
        assert_eq!(move_filename("README"), "README.README");
        assert_eq!(move_filename("trailing."), "trailing");
    }

    #[test]
    fn rename_filename_keeps_current_extension() {
        assert_eq!(rename_filename("a.png", "new name"), "new-name.png");
        assert_eq!(rename_filename("README", "x"), "x.README");
        assert_eq!(rename_filename("trailing.", "x"), "x");
    }

    #[test]
    fn split_key_separates_folder_and_basename() {
        assert_eq!(split_key("avatars/me.png"), ("avatars".into(), "me".into()));
        assert_eq!(split_key("me"), (String::new(), "me".into()));
        assert_eq!(split_key("a/b/c.tar.gz"), ("a/b".into(), "c.tar".into()));
    }

    #[test]
    fn join_and_normalize_folders() {
        assert_eq!(join_key("", "x.png"), "x.png");
        assert_eq!(join_key("a/b", "x.png"), "a/b/x.png");
        assert_eq!(normalize_folder(" /a/b/ "), "a/b");
    }

    #[test]
    fn preview_key_uses_raw_basename() {
        assert_eq!(preview_key("", "my file", "a.png"), "my file.png");
        assert_eq!(preview_key("x", "b", "README"), "x/b.README");
    }

    #[test]
    fn media_kind_checks_mime_then_extension() {
        assert_eq!(media_kind(Some("image/png"), "x"), MediaKind::Image);
        assert_eq!(media_kind(None, "clip.MP4"), MediaKind::Video);
        assert_eq!(media_kind(None, "song.flac"), MediaKind::Audio);
        assert_eq!(
            media_kind(Some("application/pdf"), "a.pdf"),
            MediaKind::Document
        );
    }

    #[test]
    fn mime_for_uses_extension_case_insensitively() {
        assert_eq!(mime_for("A.PNG"), "image/png");
        assert_eq!(mime_for("noext"), "application/octet-stream");
    }
}
