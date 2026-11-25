//! Notebooks API client (`/api/notebooks`). A notebook is a titled thread of entries
//! on one topic, with pin, archive, and trash flags.

use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, json_request, percent_encode};
use crate::todo::decode_ack;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub id: String,
    pub title: String,
    pub color: String,
    pub is_pinned: bool,
    pub is_archived: bool,
    pub is_trashed: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Count of entries in the thread. Only the list and create responses carry it.
    #[serde(default)]
    pub entry_count: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookEntry {
    pub id: String,
    pub notebook_id: String,
    pub content: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewNotebook<'a> {
    pub title: &'a str,
    pub content: &'a str,
}

/// Partial update. Only the fields that are `Some` are sent, as Solid's `updateNotebookDirect`.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookPatch<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_trashed: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntry<'a> {
    pub notebook_id: &'a str,
    pub content: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPatch<'a> {
    pub id: &'a str,
    pub content: &'a str,
    pub notebook_id: &'a str,
}

#[derive(Deserialize)]
struct NotebookList {
    notebooks: Vec<Notebook>,
}

#[derive(Deserialize)]
struct EntryList {
    entries: Vec<NotebookEntry>,
}

pub fn decode_notebooks(bytes: &[u8]) -> Result<Vec<Notebook>, ApiError> {
    serde_json::from_slice::<NotebookList>(bytes)
        .map(|list| list.notebooks)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_notebook(bytes: &[u8]) -> Result<Notebook, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_entries(bytes: &[u8]) -> Result<Vec<NotebookEntry>, ApiError> {
    serde_json::from_slice::<EntryList>(bytes)
        .map(|list| list.entries)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_entry(bytes: &[u8]) -> Result<NotebookEntry, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

fn to_json(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("request bodies are plain structs and always serialize")
}

impl ApiClient {
    /// `GET /api/notebooks`: every notebook, most recently updated first.
    pub fn notebooks(
        &self,
        on_done: impl FnOnce(Result<Vec<Notebook>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::new(Method::GET, self.url("/notebooks"), Headers::new(&[])),
            decode_notebooks,
            on_done,
        );
    }

    /// `POST /api/notebooks`: creates a notebook, with `content` as its first entry when non-empty.
    pub fn create_notebook(
        &self,
        title: &str,
        content: &str,
        on_done: impl FnOnce(Result<Notebook, ApiError>) + Send + 'static,
    ) {
        let body = to_json(&NewNotebook { title, content });
        self.send(
            json_request(Method::POST, self.url("/notebooks"), body),
            decode_notebook,
            on_done,
        );
    }

    /// `PATCH /api/notebooks`: applies the set fields of `patch`.
    pub fn update_notebook(
        &self,
        patch: &NotebookPatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(Method::PATCH, self.url("/notebooks"), to_json(patch)),
            decode_ack,
            on_done,
        );
    }

    /// `DELETE /api/notebooks?id=`: removes the notebook and all of its entries.
    pub fn delete_notebook(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = self.url(&format!("/notebooks?id={}", percent_encode(id)));
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ack,
            on_done,
        );
    }

    /// `GET /api/notebooks/entries?id=`: the thread's entries, oldest first.
    pub fn notebook_entries(
        &self,
        notebook_id: &str,
        on_done: impl FnOnce(Result<Vec<NotebookEntry>, ApiError>) + Send + 'static,
    ) {
        let url = self.url(&format!(
            "/notebooks/entries?id={}",
            percent_encode(notebook_id)
        ));
        self.send(
            Request::new(Method::GET, url, Headers::new(&[])),
            decode_entries,
            on_done,
        );
    }

    /// `POST /api/notebooks/entries`: appends an entry to a thread.
    pub fn create_notebook_entry(
        &self,
        notebook_id: &str,
        content: &str,
        on_done: impl FnOnce(Result<NotebookEntry, ApiError>) + Send + 'static,
    ) {
        let body = to_json(&NewEntry {
            notebook_id,
            content,
        });
        self.send(
            json_request(Method::POST, self.url("/notebooks/entries"), body),
            decode_entry,
            on_done,
        );
    }

    /// `PATCH /api/notebooks/entries`: rewrites an entry's content.
    pub fn update_notebook_entry(
        &self,
        patch: &EntryPatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::PATCH,
                self.url("/notebooks/entries"),
                to_json(patch),
            ),
            decode_ack,
            on_done,
        );
    }

    /// `DELETE /api/notebooks/entries?id=&notebookId=`: removes one entry.
    pub fn delete_notebook_entry(
        &self,
        id: &str,
        notebook_id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let url = self.url(&format!(
            "/notebooks/entries?id={}&notebookId={}",
            percent_encode(id),
            percent_encode(notebook_id)
        ));
        self.send(
            Request::new(Method::DELETE, url, Headers::new(&[])),
            decode_ack,
            on_done,
        );
    }
}
