//! Canvas videos (CV-V.3): uploading a video file to a canvas in parts, with its poster thumbnails and its original kept in
//! the disk cache. Desktop only (the FFmpeg backend of `rimelight_media`). Placing a video on a canvas comes in CV-V.4.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use sha2::{Digest, Sha256};
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_proto::canvases::{AssetView, UploadedPart, VideoUpload, VideoUploadComplete};

use super::images::{self, Decoded, DiskCache};

/// File extensions of the videos a canvas takes.
pub const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "webm", "mkv"];

/// The largest video: 500 MB.
pub const VIDEO_MAX_BYTES: u64 = 500 * 1024 * 1024;

/// Bytes read per step while hashing a file.
const HASH_CHUNK_BYTES: usize = 1024 * 1024;

/// The media type of a video file name by its extension (case-insensitive), or None.
pub fn video_mime(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mp4") {
        Some("video/mp4")
    } else if lower.ends_with(".mov") {
        Some("video/quicktime")
    } else if lower.ends_with(".webm") {
        Some("video/webm")
    } else if lower.ends_with(".mkv") {
        Some("video/x-matroska")
    } else {
        None
    }
}

/// Uploads one video file to a canvas and returns its asset (with thumbnails when they could be made).
/// `progress(sent, total)` is called after each part. Runs on the worker.
pub async fn upload_video(
    api: &ApiClient,
    token: &str,
    canvas: &str,
    path: &Path,
    cache: &DiskCache,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<AssetView, ClientError> {
    let info =
        rimelight_media::probe(path).map_err(|error| ClientError::Protocol(error.to_string()))?;
    let total = std::fs::metadata(path).map_err(io_error)?.len();
    if total > VIDEO_MAX_BYTES {
        return Err(ClientError::Protocol("Videos are at most 500 MB.".into()));
    }
    let sha256 = sha256_of(path)?;
    let video = VideoUpload {
        sha256: sha256.clone(),
        mime: info.mime.to_string(),
        bytes: total,
        file_name: file_name_of(path),
        width: info.width,
        height: info.height,
        duration_ms: info.duration_ms,
        has_audio: info.has_audio,
    };
    let start = api.start_video_upload(token, canvas, &video).await?;
    let mut asset = match start.asset {
        Some(asset) => asset,
        None => {
            let upload_id = start
                .upload_id
                .ok_or_else(|| ClientError::Protocol("The server gave no upload id.".into()))?;
            let part_bytes = start
                .part_bytes
                .filter(|size| *size > 0)
                .ok_or_else(|| ClientError::Protocol("The server gave no part size.".into()))?;
            let parts = start
                .parts
                .ok_or_else(|| ClientError::Protocol("The server gave no part count.".into()))?;
            let sent = match send_parts(
                api, token, canvas, &upload_id, &video, path, part_bytes, parts, progress,
            )
            .await
            {
                Ok(sent) => sent,
                Err(error) => {
                    let _ = api
                        .abort_video_upload(token, canvas, &upload_id, &video.sha256, &video.mime)
                        .await;
                    return Err(error);
                }
            };
            let body = VideoUploadComplete { video, parts: sent };
            api.complete_video_upload(token, canvas, &upload_id, &body)
                .await?
        }
    };
    if asset.thumbs.is_empty()
        && let Ok(poster) = rimelight_media::poster(path, 0.1, 1024)
    {
        let decoded = Decoded::from_rgba(poster.width, poster.height, poster.bytes());
        for side in [1024u32, 256] {
            if let Ok(webp) = images::thumbnail_webp(&decoded, side)
                && let Ok(view) = api
                    .upload_canvas_thumb(token, canvas, &asset.id, side, webp)
                    .await
            {
                asset = view;
            }
        }
    }
    cache.put_file(&sha256, "orig", path);
    Ok(asset)
}

/// Sends the parts of an upload one after another. Each part is read from the file at its offset. Returns every part's
/// entity tag. Any failure stops the run and is returned (the caller aborts the upload).
#[allow(clippy::too_many_arguments)]
async fn send_parts(
    api: &ApiClient,
    token: &str,
    canvas: &str,
    upload_id: &str,
    video: &VideoUpload,
    path: &Path,
    part_bytes: u64,
    parts: u32,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<UploadedPart>, ClientError> {
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    let total = video.bytes;
    let mut sent_parts = Vec::with_capacity(parts as usize);
    let mut sent: u64 = 0;
    for n in 1..=parts {
        let offset = u64::from(n - 1) * part_bytes;
        let len = total.saturating_sub(offset).min(part_bytes);
        let mut buf = vec![0u8; len as usize];
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        file.read_exact(&mut buf).map_err(io_error)?;
        let part = api
            .upload_video_part(token, canvas, upload_id, &video.sha256, &video.mime, n, buf)
            .await?;
        sent_parts.push(part);
        sent += len;
        progress(sent, total);
    }
    Ok(sent_parts)
}

/// The SHA-256 of a file as lower-case hex, read in chunks.
fn sha256_of(path: &Path) -> Result<String, ClientError> {
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buf).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// A file system error as a client error.
fn io_error(error: std::io::Error) -> ClientError {
    ClientError::Protocol(error.to_string())
}

/// The file name of a path, or an empty name when it has none.
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(all(test, windows))]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use space_client::api::{ApiClient, BoxFuture, HttpRequest, HttpResponse, Transport};

    use super::*;

    /// Records every request and answers from a queue of replies.
    struct FakeTransport {
        requests: Mutex<Vec<HttpRequest>>,
        replies: Mutex<VecDeque<Result<HttpResponse, ClientError>>>,
    }

    impl Transport for FakeTransport {
        fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, ClientError>> {
            self.requests.lock().expect("requests").push(request);
            let reply = self
                .replies
                .lock()
                .expect("replies")
                .pop_front()
                .unwrap_or_else(|| Err(ClientError::Protocol("no reply queued".into())));
            Box::pin(async move { reply })
        }
    }

    fn reply(status: u16, body: &str) -> Result<HttpResponse, ClientError> {
        Ok(HttpResponse {
            status,
            body: body.to_string(),
            bytes: body.as_bytes().to_vec(),
        })
    }

    fn asset_json(thumbs: &[u32]) -> String {
        serde_json::json!({
            "id": "asset-1",
            "sha256": "a".repeat(64),
            "mime": "video/mp4",
            "bytes": 59529,
            "width": 160,
            "height": 90,
            "frame_count": 1,
            "file_name": "clip.mp4",
            "source_url": "",
            "thumbs": thumbs,
            "url": "/v1/canvases/c1/assets/asset-1",
            "created_at": "2026-11-20T00:00:00Z",
            "media": "Video",
            "duration_ms": 2000,
            "has_audio": true
        })
        .to_string()
    }

    fn fixture() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/clip.mp4"
        ))
    }

    fn cache_in_temp(name: &str) -> DiskCache {
        let dir = std::env::temp_dir().join(format!(
            "rimelight-canvas-video-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        DiskCache::at(dir)
    }

    fn client(replies: Vec<Result<HttpResponse, ClientError>>) -> (ApiClient, Arc<FakeTransport>) {
        let fake = Arc::new(FakeTransport {
            requests: Mutex::new(Vec::new()),
            replies: Mutex::new(replies.into()),
        });
        let api = ApiClient::with_transport("https://api.test/", fake.clone());
        (api, fake)
    }

    #[test]
    fn a_new_video_is_uploaded_in_parts_with_thumbnails() {
        let path = fixture();
        let file = std::fs::read(&path).expect("read fixture");
        let mut hasher = Sha256::new();
        hasher.update(&file);
        let sha: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let (api, fake) = client(vec![
            reply(201, r#"{"upload_id":"u1","part_bytes":67108864,"parts":1}"#),
            reply(200, r#"{"part":1,"etag":"e1"}"#),
            reply(201, &asset_json(&[])),
            reply(200, &asset_json(&[1024])),
            reply(200, &asset_json(&[256, 1024])),
        ]);
        let cache = cache_in_temp("new");
        let progress_calls = Mutex::new(Vec::new());
        let progress = |sent: u64, total: u64| {
            progress_calls.lock().expect("progress").push((sent, total));
        };
        let asset = pollster::block_on(upload_video(&api, "token", "c1", &path, &cache, &progress))
            .expect("upload");
        assert_eq!(asset.thumbs, vec![256, 1024]);

        let requests = fake.requests.lock().expect("requests").clone();
        assert_eq!(requests.len(), 5);
        let start: serde_json::Value =
            serde_json::from_str(requests[0].json_body.as_deref().expect("start body"))
                .expect("start json");
        assert_eq!(start["width"], 160);
        assert_eq!(start["height"], 90);
        assert_eq!(start["has_audio"], true);
        assert_eq!(start["mime"], "video/mp4");
        assert_eq!(start["bytes"], 59529);
        assert_eq!(start["sha256"], sha.as_str());
        let part = requests[1].raw_body.as_ref().expect("part body");
        assert_eq!(part.bytes, file);
        let complete: serde_json::Value =
            serde_json::from_str(requests[2].json_body.as_deref().expect("complete body"))
                .expect("complete json");
        assert_eq!(
            complete["parts"],
            serde_json::json!([{"part": 1, "etag": "e1"}])
        );
        assert!(requests[3].url.ends_with("/thumbs/1024"));
        assert!(requests[4].url.ends_with("/thumbs/256"));
        assert!(
            progress_calls
                .lock()
                .expect("progress")
                .contains(&(59529, 59529))
        );
        assert_eq!(cache.get(&sha, "orig"), Some(file));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!(
            "rimelight-canvas-video-test-{}-new",
            std::process::id()
        )));
    }

    #[test]
    fn a_video_the_canvas_already_has_is_not_sent_again() {
        let path = fixture();
        let (api, fake) = client(vec![reply(
            200,
            &format!(r#"{{"asset": {}}}"#, asset_json(&[256, 1024])),
        )]);
        let cache = cache_in_temp("existing");
        let progress = |_: u64, _: u64| {};
        let asset = pollster::block_on(upload_video(&api, "token", "c1", &path, &cache, &progress))
            .expect("upload");
        assert_eq!(asset.id, "asset-1");
        assert_eq!(fake.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn video_mime_knows_the_four_containers() {
        assert_eq!(video_mime("clip.mp4"), Some("video/mp4"));
        assert_eq!(video_mime("CLIP.MOV"), Some("video/quicktime"));
        assert_eq!(video_mime("clip.WebM"), Some("video/webm"));
        assert_eq!(video_mime("clip.MKV"), Some("video/x-matroska"));
        assert_eq!(video_mime("picture.png"), None);
    }
}
