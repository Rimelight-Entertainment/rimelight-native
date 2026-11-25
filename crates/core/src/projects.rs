//! Projects (kanban boards) models and decoders, for `/api/projects/cards`.
//!
//! The shapes follow the web app's Drizzle schema (`src/db/schema/projects.ts`) and its Hono route
//! (`src/api/routes/projects.ts`). Timestamps arrive as ISO strings; they are kept as text, since the native pages only
//! show the server's order.

use serde::{Deserialize, Serialize};

use crate::api::ApiError;

/// A board, as `GET /api/projects/cards` returns it (the signed-in user's boards only).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    pub user_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

/// A list (column) of a board. `GET` returns every list, ordered by `order` descending, as the web API does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub id: String,
    pub board_id: String,
    pub title: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

/// A card. `POST` returns a subset of these fields; the missing ones decode to their defaults.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub list_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub custom_fields: serde_json::Value,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

/// The `GET /api/projects/cards` body.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Projects {
    pub boards: Vec<Board>,
    pub lists: Vec<List>,
    pub cards: Vec<Card>,
}

/// Body for `POST /api/projects/cards`.
///
/// TODO(inventory): the web API inserts a card for any `list_id` it is given, and does not check that the list belongs to
/// a board of the user. The inventory (WS6 ST6.1) records this. It is mirrored as it is here; the native client must not
/// rely on the server to reject a card for a foreign list. Fix the API, then this comment.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectCard<'a> {
    pub list_id: &'a str,
    pub title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    pub order: i64,
    pub custom_fields: &'a serde_json::Value,
}

/// Body for `PATCH /api/projects/cards`. A field left `None` is not sent, so the server keeps it (the web route checks
/// each field with `!== undefined`). `id` is required.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardPatch<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_fields: Option<&'a serde_json::Value>,
}

pub fn decode_projects(bytes: &[u8]) -> Result<Projects, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_card(bytes: &[u8]) -> Result<Card, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}
