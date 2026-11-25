//! Health & Fitness API client. Mirrors the Solid client calls in
//! `src/components/health/*` against `/api/health/*`.

use std::collections::BTreeMap;

use ehttp::{Method, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{ApiClient, ApiError, json_request};

// ---------------------------------------------------------------------------
// Fasting
// ---------------------------------------------------------------------------

/// One fast as returned by `GET/POST /api/health/fasting`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FastEntry {
    pub id: String,
    pub start_time: String,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub original_end_time: Option<String>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewFastBody<'a> {
    start_time: &'a str,
    duration_hours: u32,
}

#[derive(Serialize)]
struct EndFastBody<'a> {
    action: &'static str,
    id: &'a str,
}

/// The list endpoint answers either a bare array or `{ fasts }` / `{ fasting }`.
pub fn decode_fasts(bytes: &[u8]) -> Result<Vec<FastEntry>, ApiError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    let list = if value.is_array() {
        value
    } else {
        value
            .get("fasts")
            .filter(|v| v.is_array())
            .or_else(|| value.get("fasting"))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()))
    };
    serde_json::from_value(list).map_err(|e| ApiError::Decode(e.to_string()))
}

pub fn decode_fast(bytes: &[u8]) -> Result<FastEntry, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

// ---------------------------------------------------------------------------
// Measurements
// ---------------------------------------------------------------------------

/// A number or a string. Solid stores readings as either.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Reading {
    Number(f64),
    Text(String),
}

impl Reading {
    /// Numeric view. Text that parses as a number counts.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Reading::Number(n) => Some(*n),
            Reading::Text(s) => s.trim().parse().ok(),
        }
    }

    /// Display form for the matrix cells.
    pub fn display(&self) -> String {
        match self {
            Reading::Number(n) => format!("{n}"),
            Reading::Text(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthValue {
    /// Zero-based month index, 0..=11.
    pub month: u8,
    pub value: Reading,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    pub id: String,
    pub name: String,
    pub unit: String,
    #[serde(default)]
    pub target: Option<Reading>,
    /// Year (as a string key) to readings.
    #[serde(default)]
    pub values: BTreeMap<String, Vec<MonthValue>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Goals {
    pub current_weight: f64,
    pub target_weight: f64,
    pub current_body_fat: f64,
    pub target_body_fat: f64,
    pub weight_unit: String,
}

impl Default for Goals {
    fn default() -> Self {
        Self {
            current_weight: 75.4,
            target_weight: 72.0,
            current_body_fat: 13.5,
            target_body_fat: 11.0,
            weight_unit: "kg".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeasurementsData {
    pub goals: Goals,
    pub measurements: Vec<Measurement>,
}

/// `GET /api/health/measurements` answers `{ data: { measurements, goals } }`.
/// The raw object is also accepted.
pub fn decode_measurements(bytes: &[u8]) -> Result<MeasurementsData, ApiError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    let inner = match value.get("data") {
        Some(data) if data.is_object() => data.clone(),
        _ => value,
    };
    serde_json::from_value(inner).map_err(|e| ApiError::Decode(e.to_string()))
}

// ---------------------------------------------------------------------------
// Workouts & stretches
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StretchItem {
    pub id: String,
    pub name: String,
    /// Seconds.
    pub duration: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StretchCategory {
    pub id: String,
    pub name: String,
    pub items: Vec<StretchItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseSet {
    pub id: String,
    pub kg: String,
    pub reps: String,
    /// Rest seconds.
    pub rest_timer: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkoutExercise {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    pub sets: Vec<ExerciseSet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkoutExerciseCategory {
    pub id: String,
    pub name: String,
    pub items: Vec<WorkoutExercise>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkoutData {
    pub stretches: Vec<StretchCategory>,
    pub exercises: Vec<WorkoutExerciseCategory>,
}

/// Decodes `GET /api/health/workouts`. Returns `None` when the server has no
/// arrays yet, so the caller can seed defaults as Solid does.
pub fn decode_workouts(bytes: &[u8]) -> Result<Option<WorkoutData>, ApiError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    let mut raw = match value.get("data") {
        Some(data) if !data.is_null() => data.clone(),
        _ => value,
    };
    if let Value::String(text) = &raw {
        raw = serde_json::from_str(text).map_err(|e| ApiError::Decode(e.to_string()))?;
    }
    let has_arrays = raw.get("stretches").is_some_and(Value::is_array)
        || raw.get("exercises").is_some_and(Value::is_array);
    if !has_arrays {
        return Ok(None);
    }
    let data: WorkoutData =
        serde_json::from_value(raw).map_err(|e| ApiError::Decode(e.to_string()))?;
    Ok(Some(data))
}

// ---------------------------------------------------------------------------
// Client methods
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct WorkoutsSaveBody<'a> {
    data: &'a WorkoutData,
}

impl ApiClient {
    /// `GET /api/health/fasting`. The list is newest first.
    pub fn fasting_list(
        &self,
        on_done: impl FnOnce(Result<Vec<FastEntry>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/health/fasting")),
            decode_fasts,
            on_done,
        );
    }

    /// `POST /api/health/fasting` with `{ startTime, durationHours }`. `start_time` is an ISO 8601 string.
    pub fn fasting_start(
        &self,
        start_time: &str,
        duration_hours: u32,
        on_done: impl FnOnce(Result<FastEntry, ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(&NewFastBody {
            start_time,
            duration_hours,
        })
        .expect("plain struct serializes");
        self.send(
            json_request(Method::POST, self.url("/health/fasting"), body),
            decode_fast,
            on_done,
        );
    }

    /// `PATCH /api/health/fasting` with `{ action: "end", id }`.
    pub fn fasting_end(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(&EndFastBody { action: "end", id })
            .expect("plain struct serializes");
        self.send(
            json_request(Method::PATCH, self.url("/health/fasting"), body),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// `GET /api/health/measurements`.
    pub fn measurements_get(
        &self,
        on_done: impl FnOnce(Result<MeasurementsData, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/health/measurements")),
            decode_measurements,
            on_done,
        );
    }

    /// `POST /api/health/measurements`. Sends the raw `{ goals, measurements }`
    /// object, matching the Solid client. The server reads `body.data`, so it
    /// stores nothing useful. See the report.
    pub fn measurements_save(
        &self,
        data: &MeasurementsData,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(data).expect("plain struct serializes");
        self.send(
            json_request(Method::POST, self.url("/health/measurements"), body),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// `GET /api/health/workouts`. `Ok(None)` means the server has no data yet.
    pub fn workouts_get(
        &self,
        on_done: impl FnOnce(Result<Option<WorkoutData>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url("/health/workouts")),
            decode_workouts,
            on_done,
        );
    }

    /// `POST /api/health/workouts` with `{ data }`.
    pub fn workouts_save(
        &self,
        data: &WorkoutData,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(&WorkoutsSaveBody { data }).expect("plain struct serializes");
        self.send(
            json_request(Method::POST, self.url("/health/workouts"), body),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }
}
