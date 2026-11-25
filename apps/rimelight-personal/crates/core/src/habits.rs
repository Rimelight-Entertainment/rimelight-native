use serde::{Deserialize, Serialize};

use crate::api::ApiError;

/// The whole habit tracker for one user. `GET /api/habits` and every `POST /api/habits`
/// return this shape. The server stores it as one JSON blob, so the client always
/// replaces its copy with the response.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct HabitData {
    pub years: Vec<HabitYear>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HabitYear {
    pub year: i32,
    pub categories: Vec<HabitCategory>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HabitCategory {
    pub id: String,
    pub name: String,
    pub habits: Vec<Habit>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub id: String,
    pub name: String,
    /// `YYYY-MM-DD` strings.
    pub completed_dates: Vec<String>,
}

/// Body for `POST /api/habits`. The `action` tag selects the server-side mutation;
/// each variant's fields match the keys the Solid `HabitsTracker` sends.
#[derive(Debug, Serialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HabitAction<'a> {
    AddYear {
        year: i32,
    },
    AddCategory {
        year: i32,
        name: &'a str,
    },
    DeleteCategory {
        year: i32,
        cat_id: &'a str,
    },
    AddHabit {
        year: i32,
        cat_id: &'a str,
        name: &'a str,
    },
    DeleteHabit {
        year: i32,
        cat_id: &'a str,
        habit_id: &'a str,
    },
    ToggleDate {
        year: i32,
        cat_id: &'a str,
        habit_id: &'a str,
        date: &'a str,
    },
}

pub fn decode_habits(bytes: &[u8]) -> Result<HabitData, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}
