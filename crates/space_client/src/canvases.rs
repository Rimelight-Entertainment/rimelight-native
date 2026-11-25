//! Typed client for the canvases endpoints (CV.4, decision 0045): lists, create, rename, archive and unarchive, permanent
//! delete, move to a project, sharing, the snapshot, image assets and thumbnails, quotas and the live-room ticket. Every
//! call takes the session token; ids are checked to be plain path segments before a request is made.

use space_proto::canvases::{
    AssetUsage, AssetUsageList, AssetView, CanvasList, CanvasView, CreateCanvas, ImportUrl,
    LiveTicket, MoveCanvas, Quota, RenameCanvas, SetShare, Share, ShareList, Snapshot,
    UploadedPart, VideoUpload, VideoUploadComplete, VideoUploadStart,
};

use crate::api::{ApiClient, Method, RawBody, encode_query_value};
use crate::error::ClientError;

/// Which canvases `GET /v1/canvases` lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasScope {
    Personal,
    Shared,
    Project(String),
}

/// Joins `key=value` pairs into a query string with a leading `?`, or an empty string when there are none.
fn join_query(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in pairs {
        out.push(if out.is_empty() { '?' } else { '&' });
        out.push_str(key);
        out.push('=');
        out.push_str(&encode_query_value(value));
    }
    out
}

/// A project key as a path segment: 2 to 10 ASCII letters or digits.
fn project_segment(key: &str) -> Result<&str, ClientError> {
    if (2..=10).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(key)
    } else {
        Err(ClientError::Protocol(format!(
            "not a valid project key: {key}"
        )))
    }
}

/// A canvas, asset, user or quota-subject id as a path segment: non-empty ASCII letters, digits, dashes and underscores,
/// so `..`, slashes, `?` and spaces are refused before a request is made.
fn plain_segment<'a>(value: &'a str, what: &str) -> Result<&'a str, ClientError> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(value)
    } else {
        Err(ClientError::Protocol(format!(
            "not a valid {what}: {value}"
        )))
    }
}

/// The thumbnail sizes the server stores (CV.4).
fn thumb_size(px: u32) -> Result<u32, ClientError> {
    match px {
        256 | 1024 => Ok(px),
        _ => Err(ClientError::Protocol(format!("not a thumbnail size: {px}"))),
    }
}

fn to_json<T: serde::Serialize>(body: &T) -> Result<String, ClientError> {
    serde_json::to_string(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

fn parse_body<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, ClientError> {
    serde_json::from_str(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

impl ApiClient {
    /// `GET /v1/canvases?scope=personal|shared|project[&project=KEY][&archived=true]`: the canvases in one scope. `archived`
    /// lists the archived ones instead of the live ones.
    pub async fn canvases(
        &self,
        token: &str,
        scope: &CanvasScope,
        archived: bool,
    ) -> Result<Vec<CanvasView>, ClientError> {
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        match scope {
            CanvasScope::Personal => pairs.push(("scope", "personal")),
            CanvasScope::Shared => pairs.push(("scope", "shared")),
            CanvasScope::Project(key) => {
                pairs.push(("scope", "project"));
                pairs.push(("project", project_segment(key)?));
            }
        }
        if archived {
            pairs.push(("archived", "true"));
        }
        let path = format!("/v1/canvases{}", join_query(&pairs));
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: CanvasList = parse_body(&reply.body)?;
        Ok(list.canvases)
    }

    /// `POST /v1/canvases`: creates a canvas. Returns it as stored.
    pub async fn create_canvas(
        &self,
        token: &str,
        body: &CreateCanvas,
    ) -> Result<CanvasView, ClientError> {
        let reply = self
            .send(
                Method::Post,
                "/v1/canvases",
                Some(token),
                Some(to_json(body)?),
            )
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/canvases/{id}`: one canvas.
    pub async fn canvas(&self, token: &str, id: &str) -> Result<CanvasView, ClientError> {
        let path = format!("/v1/canvases/{}", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `PATCH /v1/canvases/{id}`: renames a canvas. Returns it as stored.
    pub async fn rename_canvas(
        &self,
        token: &str,
        id: &str,
        title: &str,
    ) -> Result<CanvasView, ClientError> {
        let path = format!("/v1/canvases/{}", plain_segment(id, "canvas id")?);
        let body = RenameCanvas {
            title: title.to_owned(),
        };
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/archive`: archives a canvas (no body). Returns it as stored.
    pub async fn archive_canvas(&self, token: &str, id: &str) -> Result<CanvasView, ClientError> {
        let path = format!("/v1/canvases/{}/archive", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/unarchive`: brings an archived canvas back (no body). Returns it as stored.
    pub async fn unarchive_canvas(&self, token: &str, id: &str) -> Result<CanvasView, ClientError> {
        let path = format!("/v1/canvases/{}/unarchive", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/canvases/{id}`: permanent. Removes the canvas and its images.
    pub async fn delete_canvas(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/canvases/{}", plain_segment(id, "canvas id")?);
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `POST /v1/canvases/{id}/move`: moves a canvas to a project. Returns it as stored.
    pub async fn move_canvas(
        &self,
        token: &str,
        id: &str,
        project_key: &str,
    ) -> Result<CanvasView, ClientError> {
        let path = format!("/v1/canvases/{}/move", plain_segment(id, "canvas id")?);
        let body = MoveCanvas {
            project_key: project_segment(project_key)?.to_owned(),
        };
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/canvases/{id}/shares`: who the canvas is shared with.
    pub async fn canvas_shares(&self, token: &str, id: &str) -> Result<Vec<Share>, ClientError> {
        let path = format!("/v1/canvases/{}/shares", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: ShareList = parse_body(&reply.body)?;
        Ok(list.shares)
    }

    /// `PUT /v1/canvases/{id}/shares/{user_id}`: shares the canvas with a person, or changes their role. Returns the share.
    pub async fn set_canvas_share(
        &self,
        token: &str,
        id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<Share, ClientError> {
        let path = format!(
            "/v1/canvases/{}/shares/{}",
            plain_segment(id, "canvas id")?,
            plain_segment(user_id, "user id")?
        );
        let body = SetShare {
            role: role.to_owned(),
        };
        let reply = self
            .send(Method::Put, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/canvases/{id}/shares/{user_id}`: takes a person off the canvas.
    pub async fn remove_canvas_share(
        &self,
        token: &str,
        id: &str,
        user_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/canvases/{}/shares/{}",
            plain_segment(id, "canvas id")?,
            plain_segment(user_id, "user id")?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/canvases/{id}/snapshot`: the canvas as the live room last saw it.
    pub async fn canvas_snapshot(&self, token: &str, id: &str) -> Result<Snapshot, ClientError> {
        let path = format!("/v1/canvases/{}/snapshot", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/assets`: uploads an image from the raw bytes. The file name is percent-encoded into the
    /// `X-File-Name` header. Returns the stored asset.
    pub async fn upload_canvas_asset(
        &self,
        token: &str,
        id: &str,
        file_name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<AssetView, ClientError> {
        let path = format!("/v1/canvases/{}/assets", plain_segment(id, "canvas id")?);
        let raw = RawBody {
            content_type: content_type.to_string(),
            bytes,
            file_name: Some(encode_query_value(file_name)),
        };
        let reply = self.send_raw(Method::Post, &path, Some(token), raw).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/assets/from-url`: the server fetches the image at `url` and stores it as an asset.
    pub async fn import_canvas_asset(
        &self,
        token: &str,
        id: &str,
        url: &str,
    ) -> Result<AssetView, ClientError> {
        let path = format!(
            "/v1/canvases/{}/assets/from-url",
            plain_segment(id, "canvas id")?
        );
        let body = ImportUrl {
            url: url.to_owned(),
        };
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/canvases/{id}/assets`: the canvas's images, each with how many live items use it.
    pub async fn canvas_assets(
        &self,
        token: &str,
        id: &str,
    ) -> Result<Vec<AssetUsage>, ClientError> {
        let path = format!("/v1/canvases/{}/assets", plain_segment(id, "canvas id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: AssetUsageList = parse_body(&reply.body)?;
        Ok(list.assets)
    }

    /// `DELETE /v1/canvases/{id}/assets/{asset}`: deletes an image. The server refuses with 409 while an item uses it.
    pub async fn delete_canvas_asset(
        &self,
        token: &str,
        id: &str,
        asset_id: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/canvases/{}/assets/{}",
            plain_segment(id, "canvas id")?,
            plain_segment(asset_id, "asset id")?
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }

    /// `GET /v1/canvases/{id}/assets/{asset_id}`: the image's bytes.
    pub async fn canvas_asset_bytes(
        &self,
        token: &str,
        id: &str,
        asset_id: &str,
    ) -> Result<Vec<u8>, ClientError> {
        let path = format!(
            "/v1/canvases/{}/assets/{}",
            plain_segment(id, "canvas id")?,
            plain_segment(asset_id, "asset id")?
        );
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        Ok(reply.bytes)
    }

    /// `GET /v1/canvases/{id}/assets/{asset_id}/thumbs/{px}`: a thumbnail's bytes. `px` must be 256 or 1024; any other
    /// size is refused without a request.
    pub async fn canvas_thumb_bytes(
        &self,
        token: &str,
        id: &str,
        asset_id: &str,
        px: u32,
    ) -> Result<Vec<u8>, ClientError> {
        let px = thumb_size(px)?;
        let path = format!(
            "/v1/canvases/{}/assets/{}/thumbs/{px}",
            plain_segment(id, "canvas id")?,
            plain_segment(asset_id, "asset id")?
        );
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        Ok(reply.bytes)
    }

    /// `PUT /v1/canvases/{id}/assets/{asset_id}/thumbs/{px}`: stores a WebP thumbnail. `px` must be 256 or 1024.
    pub async fn upload_canvas_thumb(
        &self,
        token: &str,
        id: &str,
        asset_id: &str,
        px: u32,
        webp: Vec<u8>,
    ) -> Result<AssetView, ClientError> {
        let px = thumb_size(px)?;
        let path = format!(
            "/v1/canvases/{}/assets/{}/thumbs/{px}",
            plain_segment(id, "canvas id")?,
            plain_segment(asset_id, "asset id")?
        );
        let raw = RawBody {
            content_type: "image/webp".to_string(),
            bytes: webp,
            file_name: None,
        };
        let reply = self.send_raw(Method::Put, &path, Some(token), raw).await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/me/quota`: the caller's canvas storage quota.
    pub async fn my_canvas_quota(&self, token: &str) -> Result<Quota, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/me/quota", Some(token), None)
            .await?;
        parse_body(&reply.body)
    }

    /// `GET /v1/projects/{key}/quota`: a project's canvas storage quota.
    pub async fn project_canvas_quota(&self, token: &str, key: &str) -> Result<Quota, ClientError> {
        let path = format!("/v1/projects/{}/quota", project_segment(key)?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/live-ticket`: a short-lived ticket for joining the canvas's live room (no body).
    pub async fn canvas_live_ticket(
        &self,
        token: &str,
        id: &str,
    ) -> Result<LiveTicket, ClientError> {
        let path = format!(
            "/v1/canvases/{}/live-ticket",
            plain_segment(id, "canvas id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/uploads`: starts a video upload. The answer is either the existing asset (the canvas
    /// already holds the file) or the upload id, part size and part count to send the parts with.
    pub async fn start_video_upload(
        &self,
        token: &str,
        canvas_id: &str,
        video: &VideoUpload,
    ) -> Result<VideoUploadStart, ClientError> {
        let path = format!(
            "/v1/canvases/{}/uploads",
            plain_segment(canvas_id, "canvas id")?
        );
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(video)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `PUT /v1/canvases/{id}/uploads/{upload_id}/parts/{n}`: sends one part as raw bytes. `sha256` and `mime` are the
    /// upload's own values, sent in the query. Returns the part's entity tag.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_video_part(
        &self,
        token: &str,
        canvas_id: &str,
        upload_id: &str,
        sha256: &str,
        mime: &str,
        part: u32,
        bytes: Vec<u8>,
    ) -> Result<UploadedPart, ClientError> {
        let path = format!(
            "/v1/canvases/{}/uploads/{}/parts/{part}{}",
            plain_segment(canvas_id, "canvas id")?,
            plain_segment(upload_id, "upload id")?,
            join_query(&[("sha256", sha256), ("mime", mime)])
        );
        let raw = RawBody {
            content_type: "application/octet-stream".to_string(),
            bytes,
            file_name: None,
        };
        let reply = self.send_raw(Method::Put, &path, Some(token), raw).await?;
        parse_body(&reply.body)
    }

    /// `POST /v1/canvases/{id}/uploads/{upload_id}/complete`: finishes a video upload with the upload's fields and
    /// every part. Returns the stored asset.
    pub async fn complete_video_upload(
        &self,
        token: &str,
        canvas_id: &str,
        upload_id: &str,
        body: &VideoUploadComplete,
    ) -> Result<AssetView, ClientError> {
        let path = format!(
            "/v1/canvases/{}/uploads/{}/complete",
            plain_segment(canvas_id, "canvas id")?,
            plain_segment(upload_id, "upload id")?
        );
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse_body(&reply.body)
    }

    /// `DELETE /v1/canvases/{id}/uploads/{upload_id}`: drops an unfinished video upload.
    pub async fn abort_video_upload(
        &self,
        token: &str,
        canvas_id: &str,
        upload_id: &str,
        sha256: &str,
        mime: &str,
    ) -> Result<(), ClientError> {
        let path = format!(
            "/v1/canvases/{}/uploads/{}{}",
            plain_segment(canvas_id, "canvas id")?,
            plain_segment(upload_id, "upload id")?,
            join_query(&[("sha256", sha256), ("mime", mime)])
        );
        self.send(Method::Delete, &path, Some(token), None)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::api::HttpResponse;
    use crate::testing::{ScriptedTransport, bytes_response, json_response};
    use serde_json::json;

    fn client(
        replies: Vec<Result<HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("https://api.test/", transport.clone());
        (api, transport)
    }

    const CANVAS: &str = r#"{"id":"c1","title":"Refs","home":"personal","owner_id":"u1","project_key":null,"created_by":"u1","seq":3,"created_at":"2026-11-19T14:53:50.522Z","updated_at":"2026-11-19T14:53:50.522Z","archived":false,"my_access":"manage"}"#;

    const ASSET: &str = r#"{"id":"a1","sha256":"abc123","mime":"image/png","bytes":5,"width":2,"height":2,"frame_count":1,"file_name":"Q4 notes é.png","source_url":"","thumbs":[256],"url":"/v1/canvases/c1/assets/a1","created_at":"2026-11-19T14:53:50.522Z"}"#;

    #[test]
    fn canvases_lists_a_project_scope_with_archived() {
        let list = format!(r#"{{"canvases":[{CANVAS},{CANVAS}]}}"#);
        let (api, transport) = client(vec![json_response(200, &list)]);
        let found =
            pollster::block_on(api.canvases("tok", &CanvasScope::Project("EMB".to_string()), true))
                .expect("list");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Refs");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Get);
        assert_eq!(
            sent.url,
            "https://api.test/v1/canvases?scope=project&project=EMB&archived=true"
        );
        assert_eq!(sent.bearer.as_deref(), Some("tok"));
    }

    #[test]
    fn canvases_refuses_a_bad_project_key_without_a_request() {
        let (api, transport) = client(vec![]);
        let result = pollster::block_on(api.canvases(
            "tok",
            &CanvasScope::Project("../x".to_string()),
            false,
        ));
        assert!(result.is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn create_canvas_posts_only_the_title() {
        let (api, transport) = client(vec![json_response(201, CANVAS)]);
        let body = CreateCanvas {
            title: Some("Refs".to_string()),
            ..Default::default()
        };
        let created = pollster::block_on(api.create_canvas("tok", &body)).expect("created");
        assert_eq!(created.id, "c1");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/canvases");
        assert_eq!(sent.json_body.as_deref(), Some(r#"{"title":"Refs"}"#));
    }

    #[test]
    fn delete_canvas_accepts_an_empty_204() {
        let (api, transport) = client(vec![json_response(204, "")]);
        pollster::block_on(api.delete_canvas("tok", "c1")).expect("deleted");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Delete);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c1");
    }

    #[test]
    fn upload_canvas_asset_sends_raw_bytes_with_encoded_name() {
        let (api, transport) = client(vec![json_response(201, ASSET)]);
        let asset = pollster::block_on(api.upload_canvas_asset(
            "tok",
            "c1",
            "Q4 notes é.png",
            "image/png",
            b"hello".to_vec(),
        ))
        .expect("uploaded");
        assert_eq!(asset.id, "a1");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c1/assets");
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert_eq!(raw.content_type, "image/png");
        assert_eq!(raw.bytes, b"hello".to_vec());
        assert_eq!(raw.file_name.as_deref(), Some("Q4%20notes%20%C3%A9.png"));
    }

    #[test]
    fn upload_maps_413_quota_exceeded() {
        let (api, _) = client(vec![json_response(
            413,
            r#"{"error":"quota_exceeded","message":"full"}"#,
        )]);
        // AssetView's derives are not known here, so match instead of comparing the whole Result.
        match pollster::block_on(api.upload_canvas_asset(
            "tok",
            "c1",
            "a.png",
            "image/png",
            vec![1],
        )) {
            Err(ClientError::Api {
                status,
                error,
                message,
            }) => {
                assert_eq!(status, 413);
                assert_eq!(error, "quota_exceeded");
                assert_eq!(message, "full");
            }
            _ => panic!("expected a 413 API error"),
        }
    }

    #[test]
    fn thumb_bytes_refuse_other_sizes_without_a_request() {
        let (api, transport) = client(vec![]);
        let result = pollster::block_on(api.canvas_thumb_bytes("tok", "c1", "a1", 512));
        assert_eq!(
            result,
            Err(ClientError::Protocol(
                "not a thumbnail size: 512".to_string()
            ))
        );
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn thumb_bytes_fetch_the_stored_size() {
        let (api, transport) = client(vec![bytes_response(200, b"webp")]);
        let bytes =
            pollster::block_on(api.canvas_thumb_bytes("tok", "c1", "a1", 256)).expect("thumb");
        assert_eq!(bytes, b"webp".to_vec());
        assert_eq!(
            transport.requests()[0].url,
            "https://api.test/v1/canvases/c1/assets/a1/thumbs/256"
        );
    }

    #[test]
    fn upload_thumb_puts_webp() {
        let (api, transport) = client(vec![json_response(200, ASSET)]);
        pollster::block_on(api.upload_canvas_thumb("tok", "c1", "a1", 1024, b"wb".to_vec()))
            .expect("thumb uploaded");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Put);
        assert_eq!(
            sent.url,
            "https://api.test/v1/canvases/c1/assets/a1/thumbs/1024"
        );
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert_eq!(raw.content_type, "image/webp");
        assert_eq!(raw.file_name, None);
    }

    #[test]
    fn canvas_assets_lists_each_image_with_its_uses() {
        let list = r#"{"assets":[{"id":"a1","sha256":"abc123","mime":"image/png","bytes":5,"width":2,"height":2,"frame_count":1,"file_name":"a.png","source_url":"","thumbs":[],"url":"/v1/canvases/c1/assets/a1","created_at":"2026-11-19T14:53:50.522Z","uses":2}]}"#;
        let (api, transport) = client(vec![json_response(200, list)]);
        let found = pollster::block_on(api.canvas_assets("tok", "c1")).expect("assets");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].asset.id, "a1");
        assert_eq!(found[0].uses, 2);
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Get);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c1/assets");
    }

    #[test]
    fn delete_canvas_asset_accepts_204() {
        let (api, transport) = client(vec![json_response(204, "")]);
        pollster::block_on(api.delete_canvas_asset("tok", "c1", "a1")).expect("deleted");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Delete);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c1/assets/a1");
    }

    #[test]
    fn delete_canvas_asset_maps_409_in_use() {
        let (api, _) = client(vec![json_response(
            409,
            r#"{"error":"in_use","message":"This image is still used on the canvas."}"#,
        )]);
        match pollster::block_on(api.delete_canvas_asset("tok", "c1", "a1")) {
            Err(ClientError::Api { status, error, .. }) => {
                assert_eq!(status, 409);
                assert_eq!(error, "in_use");
            }
            _ => panic!("expected a 409 API error"),
        }
    }

    #[test]
    fn live_ticket_parses_and_posts_without_body() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"ticket":"t","expires_in_s":60,"path":"/v1/canvases/c/live?ticket=t"}"#,
        )]);
        let ticket = pollster::block_on(api.canvas_live_ticket("tok", "c")).expect("ticket");
        assert_eq!(ticket.ticket, "t");
        assert_eq!(ticket.expires_in_s, 60);
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c/live-ticket");
        assert_eq!(sent.json_body, None);
    }

    #[test]
    fn ids_with_path_characters_are_refused_without_a_request() {
        let (api, transport) = client(vec![]);
        assert!(pollster::block_on(api.canvas("tok", "../x")).is_err());
        assert!(pollster::block_on(api.delete_canvas("tok", "c1?x")).is_err());
        assert!(pollster::block_on(api.canvas_asset_bytes("tok", "c1", "a/b")).is_err());
        assert!(transport.requests().is_empty());
    }

    fn sample_video() -> VideoUpload {
        VideoUpload {
            sha256: "abc".into(),
            mime: "video/mp4".into(),
            bytes: 20,
            file_name: "clip.mp4".into(),
            width: 640,
            height: 360,
            duration_ms: 5000,
            has_audio: true,
        }
    }

    #[test]
    fn start_video_upload_posts_the_fields() {
        let (api, transport) = client(vec![json_response(
            201,
            r#"{"upload_id":"up1","part_bytes":8,"parts":3}"#,
        )]);
        pollster::block_on(api.start_video_upload("tok", "c1", &sample_video())).expect("start");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(sent.url, "https://api.test/v1/canvases/c1/uploads");
        let body: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(
            body,
            json!({"sha256": "abc", "mime": "video/mp4", "bytes": 20, "file_name": "clip.mp4",
                   "width": 640, "height": 360, "duration_ms": 5000, "has_audio": true})
        );
    }

    #[test]
    fn start_video_upload_parses_an_existing_asset() {
        let (api, _) = client(vec![json_response(200, &format!(r#"{{"asset":{ASSET}}}"#))]);
        let start = pollster::block_on(api.start_video_upload("tok", "c1", &sample_video()))
            .expect("start");
        assert_eq!(start.asset.map(|a| a.id), Some("a1".to_string()));
        assert_eq!(start.upload_id, None);
        assert_eq!(start.part_bytes, None);
        assert_eq!(start.parts, None);
    }

    #[test]
    fn start_video_upload_parses_the_part_plan() {
        let (api, _) = client(vec![json_response(
            201,
            r#"{"upload_id":"up1","part_bytes":8,"parts":3}"#,
        )]);
        let start = pollster::block_on(api.start_video_upload("tok", "c1", &sample_video()))
            .expect("start");
        assert!(start.asset.is_none());
        assert_eq!(start.upload_id.as_deref(), Some("up1"));
        assert_eq!(start.part_bytes, Some(8));
        assert_eq!(start.parts, Some(3));
    }

    #[test]
    fn upload_video_part_sends_raw_bytes_with_the_query() {
        let (api, transport) = client(vec![json_response(200, r#"{"part":2,"etag":"e2"}"#)]);
        let part = pollster::block_on(api.upload_video_part(
            "tok",
            "c1",
            "up1",
            "abc",
            "video/mp4",
            2,
            b"chunk".to_vec(),
        ))
        .expect("part");
        assert_eq!(
            part,
            UploadedPart {
                part: 2,
                etag: "e2".into()
            }
        );
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Put);
        assert_eq!(
            sent.url,
            "https://api.test/v1/canvases/c1/uploads/up1/parts/2?sha256=abc&mime=video%2Fmp4"
        );
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert_eq!(raw.content_type, "application/octet-stream");
        assert_eq!(raw.bytes, b"chunk".to_vec());
        assert_eq!(raw.file_name, None);
    }

    #[test]
    fn complete_video_upload_posts_flattened_fields_and_parts() {
        let body = VideoUploadComplete {
            video: sample_video(),
            parts: vec![UploadedPart {
                part: 1,
                etag: "e1".into(),
            }],
        };
        let (api, transport) = client(vec![json_response(201, ASSET)]);
        let asset = pollster::block_on(api.complete_video_upload("tok", "c1", "up1", &body))
            .expect("complete");
        assert_eq!(asset.id, "a1");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(
            sent.url,
            "https://api.test/v1/canvases/c1/uploads/up1/complete"
        );
        let sent_body: serde_json::Value =
            serde_json::from_str(sent.json_body.as_deref().expect("body")).expect("json");
        assert_eq!(sent_body["sha256"], json!("abc"));
        assert_eq!(sent_body["duration_ms"], json!(5000));
        assert_eq!(sent_body["has_audio"], json!(true));
        assert_eq!(sent_body["parts"], json!([{"part": 1, "etag": "e1"}]));
    }

    #[test]
    fn abort_video_upload_deletes_with_the_query() {
        let (api, transport) = client(vec![json_response(204, "")]);
        pollster::block_on(api.abort_video_upload("tok", "c1", "up1", "abc", "video/mp4"))
            .expect("aborted");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Delete);
        assert_eq!(
            sent.url,
            "https://api.test/v1/canvases/c1/uploads/up1?sha256=abc&mime=video%2Fmp4"
        );
    }
}
