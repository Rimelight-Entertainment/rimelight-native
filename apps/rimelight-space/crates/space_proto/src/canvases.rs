//! Canvases contract types (CV.4, decision 0045): the REST shapes of `/v1/canvases` and the JSON messages of the live room (`/v1/canvases/{id}/live`). Timestamps in REST bodies are ISO 8601 text; `ItemState::updated_at` is epoch milliseconds. Enum-like fields are strings so the wire format can grow.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Item kinds a canvas can hold.
pub const ITEM_KINDS: &[&str] = &["Image", "Text", "Shape", "Stroke", "Frame"];
/// Access levels on a canvas, weakest first.
pub const ACCESS_LEVELS: &[&str] = &["view", "edit", "manage"];
/// Roles a canvas can be shared with.
pub const SHARE_ROLES: &[&str] = &["Viewer", "Editor"];
/// Largest number of items a live room holds.
pub const MAX_LIVE_ITEMS: usize = 1000;
/// Thumbnail edge lengths in pixels that the server generates.
pub const THUMB_SIZES: &[u32] = &[256, 1024];
/// Largest accepted asset size in bytes (50 MiB).
pub const MAX_ASSET_BYTES: u64 = 50 * 1024 * 1024;

// ---------- REST ----------

/// A canvas as seen by the caller, returned by the canvas list and the canvas read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasView {
    /// Canvas id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Where the canvas lives: `"personal"` or `"project"`.
    pub home: String,
    /// Owner user id, set for personal canvases.
    pub owner_id: Option<String>,
    /// Project key, set for project canvases.
    pub project_key: Option<String>,
    /// User id of the creator.
    pub created_by: String,
    /// Change counter of the canvas.
    pub seq: u64,
    /// Creation time as ISO 8601 text.
    pub created_at: String,
    /// Last update time as ISO 8601 text.
    pub updated_at: String,
    /// Whether the canvas is archived.
    pub archived: bool,
    /// The caller's access level (one of `ACCESS_LEVELS`).
    pub my_access: String,
}

/// Reply of `GET /v1/canvases`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasList {
    /// Canvases the caller can see.
    pub canvases: Vec<CanvasView>,
}

/// Body of `POST /v1/canvases`. Absent fields are left out of the JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateCanvas {
    /// Title; the server picks one when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Project key; a personal canvas when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
}

/// Body of a canvas rename.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenameCanvas {
    /// The new title.
    pub title: String,
}

/// Body of a canvas move to another project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveCanvas {
    /// Key of the target project.
    pub project_key: String,
}

/// One user a canvas is shared with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Share {
    /// User id of the sharee.
    pub user_id: String,
    /// Display name of the sharee.
    pub name: String,
    /// Email of the sharee.
    pub email: String,
    /// Role (one of `SHARE_ROLES`).
    pub role: String,
}

/// Reply of the canvas share list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShareList {
    /// Current shares.
    pub shares: Vec<Share>,
}

/// Body of setting a share role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetShare {
    /// The new role (one of `SHARE_ROLES`).
    pub role: String,
}

/// An uploaded image asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetView {
    /// Asset id.
    pub id: String,
    /// SHA-256 of the content, hex.
    pub sha256: String,
    /// Media type of the content.
    pub mime: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Number of frames (1 for still images).
    pub frame_count: u32,
    /// Original file name.
    pub file_name: String,
    /// Where the image was imported from, if anywhere.
    pub source_url: String,
    /// Thumbnail edge lengths available (values from `THUMB_SIZES`).
    #[serde(default)]
    pub thumbs: Vec<u32>,
    /// Address to fetch the content from.
    pub url: String,
    /// Creation time as ISO 8601 text.
    pub created_at: String,
    /// "Image" or "Video" (CV-V). Older answers have no field and are images.
    #[serde(default = "image_media")]
    pub media: String,
    /// A video's length in milliseconds; None for images.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Whether a video has sound.
    #[serde(default)]
    pub has_audio: bool,
}

/// The media value of an answer that has no `media` field.
fn image_media() -> String {
    "Image".to_string()
}

impl AssetView {
    /// True for a video asset.
    pub fn is_video(&self) -> bool {
        self.media == "Video"
    }
}

/// What the client says about a video it uploads (the start and the completion bodies).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoUpload {
    /// SHA-256 of the content, hex.
    pub sha256: String,
    /// Media type of the content.
    pub mime: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Original file name.
    pub file_name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Length in milliseconds.
    pub duration_ms: u64,
    /// Whether the video has sound.
    pub has_audio: bool,
}

/// The answer to starting an upload: the existing asset, or where to send the parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoUploadStart {
    /// The existing asset, when the canvas already holds the file.
    #[serde(default)]
    pub asset: Option<AssetView>,
    /// Id of the upload to send parts to.
    #[serde(default)]
    pub upload_id: Option<String>,
    /// Size of each part in bytes.
    #[serde(default)]
    pub part_bytes: Option<u64>,
    /// Number of parts to send.
    #[serde(default)]
    pub parts: Option<u32>,
}

/// One uploaded part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadedPart {
    /// Part number, from 1.
    pub part: u32,
    /// Entity tag the server returned for the part.
    pub etag: String,
}

/// The completion body: the upload's fields and every part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoUploadComplete {
    /// The upload's fields, flattened into the body.
    #[serde(flatten)]
    pub video: VideoUpload,
    /// Every uploaded part.
    pub parts: Vec<UploadedPart>,
}

/// An image of a canvas together with how many live items on the canvas use it (the manage-images list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetUsage {
    /// The asset itself.
    #[serde(flatten)]
    pub asset: AssetView,
    /// Number of live items on the canvas whose image is this asset.
    #[serde(default)]
    pub uses: u32,
}

/// Reply of `GET /v1/canvases/{id}/assets`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetUsageList {
    /// The canvas's images, oldest first.
    pub assets: Vec<AssetUsage>,
}

/// Body of an import of an image from a web address.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportUrl {
    /// The address to import.
    pub url: String,
}

/// Storage usage and limit for one scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    /// Scope the quota applies to.
    pub scope: String,
    /// Id of the subject the scope refers to.
    pub subject_id: String,
    /// Bytes in use.
    pub usage_bytes: u64,
    /// Byte limit (fixed in code: 1 GB per person, 10 GB per project).
    pub limit_bytes: u64,
}

/// One item in a canvas snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotItem {
    /// Item id.
    pub id: String,
    /// Item kind (one of `ITEM_KINDS`).
    pub kind: String,
    /// Stacking order key.
    pub z: String,
    /// Change counter of the item.
    pub version: u64,
    /// Kind-specific properties.
    #[serde(default)]
    pub props: Map<String, Value>,
    /// User id of the last editor.
    pub updated_by: String,
    /// Last update time as ISO 8601 text.
    pub updated_at: String,
}

/// Reply of the canvas snapshot read: the canvas, its items and the assets they use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The canvas.
    pub canvas: CanvasView,
    /// All live items.
    pub items: Vec<SnapshotItem>,
    /// Assets referenced by the items.
    pub assets: Vec<AssetView>,
}

/// Reply of the live-room ticket request: a short-lived ticket to open the socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveTicket {
    /// The ticket to present when opening the socket.
    pub ticket: String,
    /// Seconds until the ticket expires.
    pub expires_in_s: u32,
    /// Path of the live socket to open.
    pub path: String,
}

// ---------- live room ----------

/// A point on the canvas in canvas coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
}

/// The part of the canvas a peer is looking at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// Left edge in canvas coordinates.
    pub x: f64,
    /// Top edge in canvas coordinates.
    pub y: f64,
    /// Zoom factor.
    pub zoom: f64,
    /// Visible width in pixels.
    pub w: f64,
    /// Visible height in pixels.
    pub h: f64,
}

/// The state of one item as the live room holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemState {
    /// Item id.
    pub id: String,
    /// Item kind (one of `ITEM_KINDS`).
    pub kind: String,
    /// Stacking order key.
    pub z: String,
    /// Change counter of the item.
    pub version: u64,
    /// Kind-specific properties.
    #[serde(default)]
    pub props: Map<String, Value>,
    /// Whether the item is deleted (a tombstone).
    pub deleted: bool,
    /// User id of the creator.
    pub created_by: String,
    /// User id of the last editor.
    pub updated_by: String,
    /// Last update time in epoch milliseconds.
    pub updated_at: i64,
}

/// The action of a client operation on an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpAction {
    /// Create a new item.
    Create,
    /// Change an existing item.
    Update,
    /// Delete an item.
    Delete,
}

/// One change a client sends to the live room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientOp {
    /// Client-chosen id, echoed back in the ack or reject.
    pub op_id: String,
    /// Item the operation targets.
    pub item_id: String,
    /// Version of the item the client based the change on.
    pub base: u64,
    /// What the operation does.
    pub action: OpAction,
    /// Item kind; used by a create.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<String>,
    /// Stacking order key; used by a create or an update.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub z: Option<String>,
    /// Properties to set. For an update, a key set to JSON null removes that prop.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub props: Option<Map<String, Value>>,
}

/// The caller's presence: cursor, selection and viewport. A `None` cursor or viewport is sent as `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    /// Cursor position, or `None` when the cursor is off the canvas.
    pub cursor: Option<Point>,
    /// Ids of the selected items.
    #[serde(default)]
    pub selection: Vec<String>,
    /// The visible part of the canvas.
    pub viewport: Option<Viewport>,
}

/// A client's play, pause, seek or end request for a shared video playback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaControl {
    /// Item id of the video.
    pub item_id: String,
    /// Whether the shared playback should play.
    pub playing: bool,
    /// Position in milliseconds.
    pub position_ms: u64,
    /// True to end the shared playback for everyone.
    pub ended: bool,
}

/// The shared playback state of one video item, as the room holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaState {
    /// Item id of the video.
    pub item_id: String,
    /// Whether the shared playback is playing.
    pub playing: bool,
    /// Position in milliseconds at `at`.
    pub position_ms: u64,
    /// Server time in epoch milliseconds when the position was set.
    pub at: i64,
    /// Connection id of the person who set it.
    pub by: String,
    /// True when the shared playback ended (only sent in a media message).
    #[serde(default)]
    pub ended: bool,
}

/// Messages a client sends over the live socket. The tag field is `t`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMessage {
    /// A change to an item.
    Op(ClientOp),
    /// A presence update.
    Presence(Presence),
    /// A play, pause, seek or end request for a shared video.
    Media(MediaControl),
    /// A keep-alive.
    Ping,
}

/// The caller's own identity in the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct You {
    /// Connection id of this socket.
    pub conn_id: String,
    /// User id of the caller.
    pub user_id: String,
    /// Display name of the caller.
    pub name: String,
    /// Colour assigned to the caller in the room.
    pub color: String,
    /// The caller's access level (one of `ACCESS_LEVELS`).
    pub access: String,
}

/// Another connected user in the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    /// Connection id of the peer's socket.
    pub conn_id: String,
    /// User id of the peer.
    pub user_id: String,
    /// Display name of the peer.
    pub name: String,
    /// Colour assigned to the peer.
    pub color: String,
    /// The peer's cursor, or `None` when it is off the canvas.
    pub cursor: Option<Point>,
    /// Ids of the items the peer has selected.
    #[serde(default)]
    pub selection: Vec<String>,
    /// The part of the canvas the peer is looking at.
    pub viewport: Option<Viewport>,
}

/// The canvas fields the room sends with the hello.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomCanvas {
    /// Display title.
    pub title: String,
    /// Whether the canvas is archived.
    pub archived: bool,
}

/// The first message the room sends after a socket opens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// The caller's own identity.
    pub you: You,
    /// Current change counter of the room.
    pub seq: u64,
    /// The canvas fields.
    pub canvas: RoomCanvas,
    /// All live items.
    pub items: Vec<ItemState>,
    /// Peers already in the room.
    #[serde(default)]
    pub peers: Vec<Peer>,
    /// Shared video playbacks currently running in the room.
    #[serde(default)]
    pub media: Vec<MediaState>,
}

/// Messages the live room sends to a client. The tag field is `t`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The first message after the socket opens.
    Hello(Hello),
    /// The caller's operation was applied.
    Ack {
        /// Id of the acknowledged operation.
        op_id: String,
        /// Change counter after the operation.
        seq: u64,
        /// The item after the operation.
        item: ItemState,
    },
    /// Another user's operation was applied.
    Applied {
        /// Change counter after the operation.
        seq: u64,
        /// User id of the author of the operation.
        by: String,
        /// The item after the operation.
        item: ItemState,
    },
    /// The caller's operation was refused.
    Reject {
        /// Id of the refused operation.
        op_id: String,
        /// Machine-readable reason.
        reason: String,
        /// Human-readable message.
        message: String,
        /// The current item, when the refusal concerns one.
        item: Option<ItemState>,
    },
    /// A peer's presence changed.
    Presence(Peer),
    /// A peer joined the room.
    Join {
        /// The peer that joined.
        peer: Peer,
    },
    /// A peer left the room.
    Leave {
        /// Connection id of the peer that left.
        conn_id: String,
    },
    /// A change to the canvas itself: either field may be present.
    Canvas {
        /// The new title, when it changed.
        #[serde(default)]
        title: Option<String>,
        /// The new archived flag, when it changed.
        #[serde(default)]
        archived: Option<bool>,
    },
    /// The room is closing this socket: "reconnect" (reconnect with a new ticket) or "deleted".
    Closed {
        /// Why the socket is closing.
        reason: String,
    },
    /// A shared video playback changed (or ended, with `ended: true`).
    Media(MediaState),
    /// Reply to a ping. `now` is the server time in epoch milliseconds.
    Pong {
        /// Server time in epoch milliseconds, absent from older servers.
        #[serde(default)]
        now: Option<i64>,
    },
    /// A protocol error.
    Error {
        /// Human-readable message.
        message: String,
    },
    /// Any message type this client does not know yet.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item_value() -> Value {
        json!({
            "id": "i1",
            "kind": "Text",
            "z": "a0",
            "version": 2,
            "props": {"text": "hi"},
            "deleted": false,
            "created_by": "u1",
            "updated_by": "u1",
            "updated_at": 1795000000000_i64
        })
    }

    #[test]
    fn create_op_carries_kind_z_and_props() {
        let mut props = Map::new();
        props.insert("text".into(), json!("hi"));
        let msg = ClientMessage::Op(ClientOp {
            op_id: "c1".into(),
            item_id: "i1".into(),
            base: 0,
            action: OpAction::Create,
            kind: Some("Text".into()),
            z: Some("a0".into()),
            props: Some(props),
        });
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["t"], json!("op"));
        assert_eq!(v["action"], json!("create"));
        assert_eq!(v["kind"], json!("Text"));
        assert_eq!(v["z"], json!("a0"));
        assert_eq!(v["props"], json!({"text": "hi"}));
    }

    #[test]
    fn update_op_omits_kind_and_z() {
        let msg = ClientMessage::Op(ClientOp {
            op_id: "c2".into(),
            item_id: "i1".into(),
            base: 2,
            action: OpAction::Update,
            kind: None,
            z: None,
            props: None,
        });
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["action"], json!("update"));
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("kind"));
        assert!(!obj.contains_key("z"));
    }

    #[test]
    fn ping_serializes_to_tag_only() {
        let v = serde_json::to_value(&ClientMessage::Ping).unwrap();
        assert_eq!(v, json!({"t": "ping"}));
    }

    #[test]
    fn presence_without_cursor_sends_null() {
        let msg = ClientMessage::Presence(Presence {
            cursor: None,
            selection: vec![],
            viewport: None,
        });
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["t"], json!("presence"));
        assert_eq!(v.get("cursor"), Some(&Value::Null));
    }

    #[test]
    fn hello_parses_with_one_item_and_no_peers() {
        let raw = r##"{"t":"hello","you":{"conn_id":"ab","user_id":"u1","name":"Dan","color":"#e5484d","access":"manage"},"seq":3,"canvas":{"title":"Board","archived":false},"items":[{"id":"i1","kind":"Text","z":"a0","version":2,"props":{"text":"hi"},"deleted":false,"created_by":"u1","updated_by":"u1","updated_at":1795000000000}],"peers":[]}"##;
        match serde_json::from_str::<ServerMessage>(raw).unwrap() {
            ServerMessage::Hello(h) => {
                assert_eq!(h.seq, 3);
                assert_eq!(h.you.access, "manage");
                assert_eq!(h.canvas.title, "Board");
                assert_eq!(h.items.len(), 1);
                assert_eq!(h.items[0].updated_at, 1795000000000);
                assert!(h.peers.is_empty());
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn ack_reject_canvas_closed_unknown_and_presence_parse() {
        let ack = json!({"t": "ack", "op_id": "c1", "seq": 4, "item": item_value()});
        match serde_json::from_value::<ServerMessage>(ack).unwrap() {
            ServerMessage::Ack { seq, .. } => assert_eq!(seq, 4),
            other => panic!("expected Ack, got {other:?}"),
        }

        let reject =
            json!({"t": "reject", "op_id": "c2", "reason": "exists", "message": "x", "item": null});
        match serde_json::from_value::<ServerMessage>(reject).unwrap() {
            ServerMessage::Reject { item, .. } => assert!(item.is_none()),
            other => panic!("expected Reject, got {other:?}"),
        }

        let canvas = json!({"t": "canvas", "archived": true});
        match serde_json::from_value::<ServerMessage>(canvas).unwrap() {
            ServerMessage::Canvas { title, archived } => {
                assert_eq!(title, None);
                assert_eq!(archived, Some(true));
            }
            other => panic!("expected Canvas, got {other:?}"),
        }

        let closed = json!({"t": "closed", "reason": "deleted"});
        assert!(matches!(
            serde_json::from_value::<ServerMessage>(closed).unwrap(),
            ServerMessage::Closed { reason } if reason == "deleted"
        ));

        let unknown = json!({"t": "something_new", "x": 1});
        assert_eq!(
            serde_json::from_value::<ServerMessage>(unknown).unwrap(),
            ServerMessage::Unknown
        );

        let presence = r##"{"t":"presence","conn_id":"c","user_id":"u","name":"n","color":"#000","cursor":{"x":1.0,"y":2.0},"selection":[],"viewport":null}"##;
        match serde_json::from_str::<ServerMessage>(presence).unwrap() {
            ServerMessage::Presence(p) => {
                assert_eq!(p.conn_id, "c");
                assert_eq!(p.cursor, Some(Point { x: 1.0, y: 2.0 }));
                assert!(p.viewport.is_none());
            }
            other => panic!("expected Presence, got {other:?}"),
        }
    }

    #[test]
    fn rest_shapes_parse_and_default_serializes_empty() {
        let canvas = r##"{"id":"c","title":"T","home":"personal","owner_id":"u","project_key":null,"created_by":"u","seq":0,"created_at":"2026-11-19T14:53:50.522Z","updated_at":"2026-11-19T14:53:50.522Z","archived":false,"my_access":"manage"}"##;
        let view: CanvasView = serde_json::from_str(canvas).unwrap();
        assert_eq!(view.id, "c");
        assert_eq!(view.project_key, None);
        assert_eq!(view.my_access, "manage");

        let asset = json!({
            "id": "a1",
            "sha256": "00",
            "mime": "image/png",
            "bytes": 10,
            "width": 4,
            "height": 4,
            "frame_count": 1,
            "file_name": "x.png",
            "source_url": "",
            "url": "/v1/assets/a1",
            "created_at": "2026-11-19T14:53:50.522Z"
        });
        let parsed: AssetView = serde_json::from_value(asset).unwrap();
        assert!(parsed.thumbs.is_empty());

        assert_eq!(
            serde_json::to_value(CreateCanvas::default()).unwrap(),
            json!({})
        );
    }

    #[test]
    fn asset_without_media_fields_reads_as_an_image() {
        let raw = r#"{"id":"a1","sha256":"00","mime":"image/png","bytes":10,"width":4,"height":4,"frame_count":1,"file_name":"x.png","source_url":"","url":"/v1/assets/a1","created_at":"2026-11-19T14:53:50.522Z"}"#;
        let asset: AssetView = serde_json::from_str(raw).unwrap();
        assert_eq!(asset.media, "Image");
        assert_eq!(asset.duration_ms, None);
        assert!(!asset.has_audio);
        assert!(!asset.is_video());
    }

    #[test]
    fn asset_with_video_fields_reads_them() {
        let raw = r#"{"id":"a2","sha256":"11","mime":"video/mp4","bytes":10,"width":640,"height":360,"frame_count":1,"file_name":"clip.mp4","source_url":"","url":"/v1/assets/a2","created_at":"2026-11-19T14:53:50.522Z","media":"Video","duration_ms":5000,"has_audio":true}"#;
        let asset: AssetView = serde_json::from_str(raw).unwrap();
        assert_eq!(asset.media, "Video");
        assert_eq!(asset.duration_ms, Some(5000));
        assert!(asset.has_audio);
        assert!(asset.is_video());
    }

    #[test]
    fn complete_body_flattens_the_upload_fields_and_lists_parts() {
        let body = VideoUploadComplete {
            video: VideoUpload {
                sha256: "11".into(),
                mime: "video/mp4".into(),
                bytes: 10,
                file_name: "clip.mp4".into(),
                width: 640,
                height: 360,
                duration_ms: 5000,
                has_audio: true,
            },
            parts: vec![UploadedPart {
                part: 1,
                etag: "e1".into(),
            }],
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["sha256"], json!("11"));
        assert_eq!(v["duration_ms"], json!(5000));
        assert_eq!(v["has_audio"], json!(true));
        assert_eq!(v["parts"], json!([{"part": 1, "etag": "e1"}]));
        assert!(v.get("video").is_none());
    }

    #[test]
    fn media_control_round_trips_with_the_media_tag() {
        let msg = ClientMessage::Media(MediaControl {
            item_id: "i1".into(),
            playing: true,
            position_ms: 1500,
            ended: false,
        });
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            v,
            json!({"t": "media", "item_id": "i1", "playing": true, "position_ms": 1500, "ended": false})
        );
        assert_eq!(serde_json::from_value::<ClientMessage>(v).unwrap(), msg);
    }

    #[test]
    fn media_state_message_round_trips_and_defaults_ended() {
        let msg = ServerMessage::Media(MediaState {
            item_id: "i1".into(),
            playing: false,
            position_ms: 900,
            at: 1795000000000,
            by: "a".into(),
            ended: true,
        });
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["t"], json!("media"));
        assert_eq!(v["at"], json!(1795000000000_i64));
        assert_eq!(v["by"], json!("a"));
        assert_eq!(serde_json::from_value::<ServerMessage>(v).unwrap(), msg);

        let raw = json!({"t": "media", "item_id": "i1", "playing": true, "position_ms": 0, "at": 5, "by": "a"});
        match serde_json::from_value::<ServerMessage>(raw).unwrap() {
            ServerMessage::Media(state) => assert!(!state.ended),
            other => panic!("expected Media, got {other:?}"),
        }
    }

    #[test]
    fn pong_parses_with_and_without_server_time() {
        assert_eq!(
            serde_json::from_value::<ServerMessage>(json!({"t": "pong"})).unwrap(),
            ServerMessage::Pong { now: None }
        );
        assert_eq!(
            serde_json::from_value::<ServerMessage>(json!({"t": "pong", "now": 42})).unwrap(),
            ServerMessage::Pong { now: Some(42) }
        );
    }

    #[test]
    fn hello_with_media_parses_and_hello_without_it_has_none() {
        let with_media = r##"{"t":"hello","you":{"conn_id":"a","user_id":"u","name":"N","color":"#000","access":"edit"},"seq":0,"canvas":{"title":"T","archived":false},"items":[],"peers":[],"media":[{"item_id":"i1","playing":true,"position_ms":10,"at":20,"by":"a"}]}"##;
        match serde_json::from_str::<ServerMessage>(with_media).unwrap() {
            ServerMessage::Hello(h) => {
                assert_eq!(h.media.len(), 1);
                assert_eq!(h.media[0].item_id, "i1");
                assert!(h.media[0].playing);
                assert!(!h.media[0].ended);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
        let without_media = r##"{"t":"hello","you":{"conn_id":"a","user_id":"u","name":"N","color":"#000","access":"edit"},"seq":0,"canvas":{"title":"T","archived":false},"items":[],"peers":[]}"##;
        match serde_json::from_str::<ServerMessage>(without_media).unwrap() {
            ServerMessage::Hello(h) => assert!(h.media.is_empty()),
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn asset_usage_list_parses_uses_beside_the_asset() {
        let raw = r#"{"assets":[{"id":"a1","sha256":"00","mime":"image/png","bytes":10,"width":4,"height":4,"frame_count":1,"file_name":"x.png","source_url":"","thumbs":[256],"url":"/v1/canvases/c/assets/a1","created_at":"2026-11-19T14:53:50.522Z","uses":2}]}"#;
        let list: AssetUsageList = serde_json::from_str(raw).unwrap();
        assert_eq!(list.assets.len(), 1);
        assert_eq!(list.assets[0].asset.id, "a1");
        assert_eq!(list.assets[0].asset.thumbs, vec![256]);
        assert_eq!(list.assets[0].uses, 2);
    }
}
