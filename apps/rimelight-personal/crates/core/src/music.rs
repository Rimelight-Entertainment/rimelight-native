//! Music studio (`/api/music`). The server stores one JSON blob per user. Every save sends the
//! whole blob and the server replaces it, so the client keeps the full shape, including the
//! guitar and DJ sections that the piano page never edits.

use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Serialize};

use crate::api::{ApiClient, ApiError, json_request, percent_encode};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MusicData {
    pub piano: Instrument,
    pub guitar: Instrument,
    pub dj: DjData,
}

/// The `piano` and `guitar` sections share one shape.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instrument {
    pub routine: Vec<RoutineCategory>,
    pub exercises: Vec<Exercise>,
    pub repertoire: Vec<RepertoireItem>,
    pub repertoire_states: Vec<String>,
    pub repertoire_composers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DjData {
    pub routine: Vec<RoutineCategory>,
    pub exercises: Vec<Exercise>,
    pub mixes: Vec<DjMix>,
    pub dj_genres: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoutineCategory {
    pub id: String,
    pub name: String,
    pub items: Vec<RoutineItem>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoutineItem {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Exercise {
    pub id: String,
    pub name: String,
    /// `null` means "not set"; the UI shows 60 for it.
    pub bpm: Option<f64>,
    pub target_bpm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepertoireItem {
    pub id: String,
    pub title: String,
    pub composer: String,
    pub status: String,
    pub difficulty: i64,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    /// Either an absolute URL or a site-relative `/api/storage/files?key=…` path.
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DjMix {
    pub id: String,
    pub name: String,
    pub duration: String,
    pub genre: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The repertoire statuses the Solid app offers when the saved list is empty.
pub const DEFAULT_REPERTOIRE_STATES: [&str; 5] = [
    "Want to Learn",
    "Learning",
    "Polishing",
    "Performance Ready",
    "Archived",
];

impl MusicData {
    /// Solid `DEFAULT_MUSIC_DATA`. The page falls back to it when the fetch fails.
    pub fn seed() -> Self {
        let states: Vec<String> = DEFAULT_REPERTOIRE_STATES.map(str::to_owned).into();
        Self {
            piano: Instrument {
                routine: vec![
                    category(
                        "p1",
                        "Warmup",
                        &[
                            ("p1-1", "Hanon Exercises 1-5"),
                            ("p1-2", "Major & Minor Scales"),
                        ],
                    ),
                    category("p2", "Sight Reading", &[("p2-1", "15 min Bach Chorales")]),
                    category(
                        "p3",
                        "Repertoire",
                        &[("p3-1", "Chopin Nocturne Op. 9 No. 2")],
                    ),
                ],
                exercises: vec![
                    exercise("pe1", "C Major 4 Octaves Arpeggio", 100.0, 140.0),
                    exercise("pe2", "Hanon No. 1", 120.0, 160.0),
                ],
                repertoire: vec![
                    piece("pr1", "Clair de Lune", "Claude Debussy", "Polishing", 4),
                    piece(
                        "pr2",
                        "Gymnopédie No. 1",
                        "Erik Satie",
                        "Performance Ready",
                        2,
                    ),
                ],
                repertoire_states: states.clone(),
                repertoire_composers: [
                    "Claude Debussy",
                    "Erik Satie",
                    "Frédéric Chopin",
                    "J.S. Bach",
                    "Ludwig van Beethoven",
                ]
                .map(str::to_owned)
                .into(),
            },
            guitar: Instrument {
                routine: vec![
                    category(
                        "g1",
                        "Technique Warmup",
                        &[
                            ("g1-1", "Spider Exercise (1-2-3-4)"),
                            ("g1-2", "Sweep picking triads"),
                        ],
                    ),
                    category(
                        "g2",
                        "Fretboard & Scales",
                        &[
                            ("g2-1", "Pentatonic 5 positions"),
                            ("g2-2", "Modes of Major Scale"),
                        ],
                    ),
                    category(
                        "g3",
                        "Song Practice",
                        &[("g3-1", "Solo section rhythm sync")],
                    ),
                ],
                exercises: vec![
                    exercise("ge1", "Chromatic 16ths 1-2-3-4", 120.0, 150.0),
                    exercise("ge2", "Major 7th Arpeggio Sweeps", 90.0, 130.0),
                ],
                repertoire: vec![
                    piece("gr1", "Little Wing", "Jimi Hendrix", "Polishing", 4),
                    piece("gr2", "Neon", "John Mayer", "Learning", 5),
                ],
                repertoire_states: states,
                repertoire_composers: [
                    "Jimi Hendrix",
                    "John Mayer",
                    "Mateus Asato",
                    "Plini",
                    "Tim Henson",
                ]
                .map(str::to_owned)
                .into(),
            },
            dj: DjData {
                routine: vec![
                    category(
                        "d1",
                        "Curation",
                        &[("d1-1", "Dig for new promos & Beatport top 100")],
                    ),
                    category(
                        "d2",
                        "Set Transition Practice",
                        &[("d2-1", "Harmonic key mixing test")],
                    ),
                ],
                exercises: vec![
                    exercise("de1", "Beatmatching by Ear (No BPM display)", 126.0, 128.0),
                    exercise("de2", "Quick 32-bar transitions", 130.0, 130.0),
                ],
                mixes: vec![DjMix {
                    id: "dm1".to_owned(),
                    name: "Summer Deep House 2026".to_owned(),
                    duration: "62:15".to_owned(),
                    genre: "Deep House".to_owned(),
                    description: Some("Melodic sunset session".to_owned()),
                }],
                dj_genres: [
                    "Deep House",
                    "Melodic Techno",
                    "Drum & Bass",
                    "Progressive House",
                    "Ambient",
                ]
                .map(str::to_owned)
                .into(),
            },
        }
    }
}

fn category(id: &str, name: &str, items: &[(&str, &str)]) -> RoutineCategory {
    RoutineCategory {
        id: id.to_owned(),
        name: name.to_owned(),
        items: items
            .iter()
            .map(|(id, name)| RoutineItem {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
            })
            .collect(),
    }
}

fn exercise(id: &str, name: &str, bpm: f64, target: f64) -> Exercise {
    Exercise {
        id: id.to_owned(),
        name: name.to_owned(),
        bpm: Some(bpm),
        target_bpm: Some(target),
    }
}

fn piece(id: &str, title: &str, composer: &str, status: &str, difficulty: i64) -> RepertoireItem {
    RepertoireItem {
        id: id.to_owned(),
        title: title.to_owned(),
        composer: composer.to_owned(),
        status: status.to_owned(),
        difficulty,
        attachments: Vec::new(),
    }
}

/// `GET /api/music` and `POST /api/music` both wrap the blob as `{ data }`.
#[derive(Deserialize)]
struct Envelope {
    data: Option<MusicData>,
}

#[derive(Serialize)]
struct SaveBody<'a> {
    data: &'a MusicData,
}

#[derive(Deserialize)]
struct UploadResponse {
    url: Option<String>,
    key: Option<String>,
}

const UPLOAD_BOUNDARY: &str = "----rimelightPersonalMusicUpload8f3c2b1d";

pub fn decode_music(bytes: &[u8]) -> Result<MusicData, ApiError> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    envelope
        .data
        .ok_or_else(|| ApiError::Decode("response has no data".to_owned()))
}

/// The stored URL for an upload response, as Solid builds it: `url` when the server sends one,
/// otherwise the relative files route for `key`.
pub fn decode_upload(bytes: &[u8]) -> Result<String, ApiError> {
    let response: UploadResponse =
        serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))?;
    if let Some(url) = response.url.filter(|url| !url.is_empty()) {
        return Ok(url);
    }
    match response.key.filter(|key| !key.is_empty()) {
        Some(key) => Ok(format!("/api/storage/files?key={}", percent_encode(&key))),
        None => Err(ApiError::Decode(
            "upload response has no url or key".to_owned(),
        )),
    }
}

/// The `multipart/form-data` body Solid sends: the file, then `folder=music/repertoire`.
fn upload_body(name: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
    let safe_name: String = name
        .chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(
        format!(
            "\r\n--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; name=\"folder\"\r\n\r\nmusic/repertoire\r\n--{UPLOAD_BOUNDARY}--\r\n"
        )
        .as_bytes(),
    );
    body
}

impl ApiClient {
    pub fn music(&self, on_done: impl FnOnce(Result<MusicData, ApiError>) + Send + 'static) {
        self.send(Request::get(self.url("/music")), decode_music, on_done);
    }

    /// Replaces the whole blob on the server.
    pub fn save_music(
        &self,
        data: &MusicData,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = serde_json::to_vec(&SaveBody { data })
            .expect("music data is plain structs and always serializes");
        self.send(
            json_request(Method::POST, self.url("/music"), body),
            |_| Ok(()),
            on_done,
        );
    }

    /// Uploads a practice recording, score, or tab. The callback gets the URL to store on the
    /// attachment.
    pub fn upload_music_file(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        on_done: impl FnOnce(Result<String, ApiError>) + Send + 'static,
    ) {
        let content_type = format!("multipart/form-data; boundary={UPLOAD_BOUNDARY}");
        let mut request = Request::new(
            Method::POST,
            self.url("/storage/upload"),
            Headers::new(&[("Content-Type", content_type.as_str())]),
        );
        request.body = upload_body(name, mime, bytes);
        self.send(request, decode_upload, on_done);
    }

    /// The absolute form of a stored attachment URL. Solid stores `/api/...` paths for its own
    /// uploads, so those resolve against the origin that serves the API.
    pub fn resolve_url(&self, stored: &str) -> String {
        if stored.starts_with('/') && !stored.starts_with("//") {
            let base = self.url("");
            let origin = base.strip_suffix("/api").unwrap_or("");
            format!("{origin}{stored}")
        } else {
            stored.to_owned()
        }
    }
}
