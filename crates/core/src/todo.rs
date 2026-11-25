use serde::{Deserialize, Serialize};

use crate::api::ApiError;

/// A todo as returned by the backend.
///
/// `GET /api/todos` returns full rows; `POST /api/todos` returns a subset. Both
/// decode into this type because only the fields below are read. Unknown fields
/// are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub is_completed: bool,
    pub due_date: Option<String>,
}

/// Body for `POST /api/todos`. Category and priority use server defaults.
#[derive(Debug, Serialize)]
pub struct NewTodo<'a> {
    pub title: &'a str,
}

/// Body for `POST /api/todos` from the dashboard quick add: fixed category and priority, due date set.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDatedTodo<'a> {
    pub title: &'a str,
    pub category: &'a str,
    pub priority: &'a str,
    pub due_date: &'a str,
}

/// Body for `POST /api/todos` from the global quick-create modal: chosen category and priority, not completed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewQuickTodo<'a> {
    pub title: &'a str,
    pub category: &'a str,
    pub priority: &'a str,
    pub is_completed: bool,
}

/// Body for `PATCH /api/todos`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogglePatch<'a> {
    pub id: &'a str,
    pub is_completed: bool,
}

/// `{"success": true}` returned by PATCH and DELETE.
#[derive(Debug, Deserialize)]
pub struct Ack {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
struct TodoList {
    todos: Vec<Todo>,
}

pub fn decode_todos(bytes: &[u8]) -> Result<Vec<Todo>, ApiError> {
    serde_json::from_slice::<TodoList>(bytes)
        .map(|list| list.todos)
        .map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_todo(bytes: &[u8]) -> Result<Todo, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_ack(bytes: &[u8]) -> Result<(), ApiError> {
    let ack: Ack = serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    if ack.success {
        Ok(())
    } else {
        Err(ApiError::Decode("success was false".to_owned()))
    }
}
