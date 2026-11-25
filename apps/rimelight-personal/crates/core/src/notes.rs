use serde::{Deserialize, Serialize};

use crate::api::ApiError;

/// One note as returned by `GET /api/notes` or `POST /api/notes`. The create
/// response omits `updatedAt`, so it is optional. `userId`, `createdAt` and
/// `deletedAt` are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub color: String,
    pub is_pinned: bool,
    pub is_archived: bool,
    pub is_trashed: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Deserialize)]
struct NoteList {
    notes: Vec<Note>,
}

/// Body for `POST /api/notes`. `isArchived` is not sent: the server always creates
/// notes unarchived, and the Solid client does not send it either.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNote<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub tags: &'a [String],
    pub color: &'a str,
    pub is_pinned: bool,
}

/// Body for `PATCH /api/notes`. Only fields set to `Some` are sent; the server
/// leaves the rest unchanged.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_trashed: Option<bool>,
}

/// Action names the Solid client sends in batch `POST /api/notes` requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NoteBatchAction {
    #[serde(rename = "batchArchive")]
    Archive,
    #[serde(rename = "batchTrash")]
    Trash,
    #[serde(rename = "batchRestore")]
    Restore,
    #[serde(rename = "batchDelete")]
    Delete,
}

/// Body for a batch `POST /api/notes`. The server currently ignores `action` and
/// creates an "Untitled" note instead; see the batch notes in the Notes parity report.
#[derive(Debug, Serialize)]
pub struct NoteBatch<'a> {
    pub action: NoteBatchAction,
    pub ids: &'a [String],
}

pub fn decode_notes(bytes: &[u8]) -> Result<Vec<Note>, ApiError> {
    serde_json::from_slice::<NoteList>(bytes)
        .map(|list| list.notes)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_note(bytes: &[u8]) -> Result<Note, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}
