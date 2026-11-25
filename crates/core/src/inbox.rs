//! Inbox API: mailboxes, folders, emails, threads, search and compose. Mirrors
//! `src/workers/inbox` in the Solid app. Email rows are snake_case, while folders,
//! paging and mailbox settings are camelCase, as the server sends them.

use ehttp::{Headers, Method, Request};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::api::{ApiClient, ApiError, json_request, percent_encode as encode};

/// Emails per page in the list and per search page.
pub const PAGE_SIZE: u32 = 25;

/// Folders the server creates for every mailbox. Anything else is a custom folder.
pub const SYSTEM_FOLDER_IDS: [&str; 5] = ["inbox", "sent", "draft", "archive", "trash"];

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Mailbox {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    /// Only present on `GET /mailboxes/:id`.
    #[serde(default)]
    pub settings: Option<MailboxSettings>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSettings {
    #[serde(default)]
    pub from_name: Option<String>,
    #[serde(default)]
    pub signature: Option<Signature>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Signature {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub text: String,
}

/// Body for `PUT /mailboxes/:id`. The Solid settings modal always sends both fields.
#[derive(Debug, Serialize)]
pub struct MailboxSettingsPatch<'a> {
    pub settings: MailboxSettingsWrite<'a>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSettingsWrite<'a> {
    pub from_name: &'a str,
    pub signature: SignatureWrite<'a>,
}

#[derive(Debug, Serialize)]
pub struct SignatureWrite<'a> {
    pub enabled: bool,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Absent on rename responses.
    #[serde(default)]
    pub unread_count: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Attachment {
    pub id: String,
    #[serde(default, deserialize_with = "text")]
    pub email_id: String,
    #[serde(default, deserialize_with = "text")]
    pub filename: String,
    #[serde(default, deserialize_with = "text")]
    pub mimetype: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub content_id: Option<String>,
    #[serde(default)]
    pub disposition: Option<String>,
}

/// One message as the Durable Object returns it. Lists are threaded, so a row is
/// the latest message of a conversation; `thread_id` groups the rest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Email {
    pub id: String,
    #[serde(default, deserialize_with = "text")]
    pub folder_id: String,
    #[serde(default, deserialize_with = "text")]
    pub subject: String,
    #[serde(default, deserialize_with = "text")]
    pub sender: String,
    #[serde(default, deserialize_with = "text")]
    pub recipient: String,
    #[serde(default)]
    pub cc: Option<String>,
    #[serde(default)]
    pub bcc: Option<String>,
    #[serde(default, deserialize_with = "text")]
    pub date: String,
    #[serde(default, deserialize_with = "flag")]
    pub read: bool,
    #[serde(default, deserialize_with = "flag")]
    pub starred: bool,
    /// HTML or plain text, as received.
    #[serde(default, deserialize_with = "text")]
    pub body: String,
    #[serde(default, deserialize_with = "text")]
    pub snippet: String,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    /// JSON string of `[{key, value}]` on stored rows. Parsed in [`Email::headers`].
    #[serde(default)]
    pub raw_headers: Option<Value>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl Email {
    /// `thread_id`, falling back to the message's own id.
    pub fn thread_key(&self) -> &str {
        non_empty(self.thread_id.as_deref()).unwrap_or(&self.id)
    }

    /// `message_id`, falling back to the message's own id. Replies point at this.
    pub fn reply_target_id(&self) -> &str {
        non_empty(self.message_id.as_deref()).unwrap_or(&self.id)
    }

    /// Raw headers as ordered pairs. Accepts the stored JSON string, a parsed array of
    /// `{key, value}`, or a plain object. Anything else yields no headers.
    pub fn headers(&self) -> Vec<(String, String)> {
        let parsed = match &self.raw_headers {
            Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok(),
            Some(value) => Some(value.clone()),
            None => None,
        };
        match parsed {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("key")?.as_str()?.to_owned(),
                        item.get("value")?.as_str()?.to_owned(),
                    ))
                })
                .collect(),
            Some(Value::Object(map)) => map
                .into_iter()
                .filter_map(|(key, value)| Some((key, value.as_str()?.to_owned())))
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// One page of emails. Threaded and folder lists return `{emails, totalCount}`; the
/// unfiltered list returns a bare array, so its total is the array length.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailPage {
    pub emails: Vec<Email>,
    pub total_count: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PageWire {
    Paged {
        emails: Vec<Email>,
        #[serde(rename = "totalCount")]
        total_count: u32,
    },
    Bare(Vec<Email>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DraftSaved {
    pub id: String,
}

/// Number of pages for `total` emails at [`PAGE_SIZE`] per page.
pub fn total_pages(total: u32) -> u32 {
    total.div_ceil(PAGE_SIZE)
}

/// Folder ids the Durable Object does not return a `type` for, so the Solid split view
/// treats every non-system id as custom. Mirrored here, so `spam` is custom too.
pub fn is_custom_folder(id: &str) -> bool {
    !SYSTEM_FOLDER_IDS.contains(&id)
}

/// Custom folders in server order.
pub fn custom_folders(folders: &[Folder]) -> Vec<&Folder> {
    folders.iter().filter(|f| is_custom_folder(&f.id)).collect()
}

/// Filters for `GET /mailboxes/:id/search`, built from the parsed search box.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchParams {
    pub query: String,
    pub folder: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub is_read: Option<bool>,
    pub is_starred: Option<bool>,
    pub has_attachment: bool,
    pub date_start: String,
    pub date_end: String,
}

impl SearchParams {
    /// True when no filter is set, so the search box is effectively empty.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Query pairs for the set filters. Empty strings and unset flags are omitted.
    pub fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = Vec::new();
        for (key, value) in [
            ("query", &self.query),
            ("folder", &self.folder),
            ("from", &self.from),
            ("to", &self.to),
            ("subject", &self.subject),
            ("date_start", &self.date_start),
            ("date_end", &self.date_end),
        ] {
            if !value.is_empty() {
                pairs.push((key, value.clone()));
            }
        }
        if let Some(read) = self.is_read {
            pairs.push(("is_read", read.to_string()));
        }
        if let Some(starred) = self.is_starred {
            pairs.push(("is_starred", starred.to_string()));
        }
        if self.has_attachment {
            pairs.push(("has_attachment", "true".to_owned()));
        }
        pairs
    }
}

/// Parses the search box with the Solid grammar: `from:`, `to:`, `subject:`, `in:`,
/// `is:`, `has:`, `before:`, `after:`. A quoted value may contain spaces. Unknown or
/// malformed values are dropped, and the remaining text becomes `query`.
pub fn parse_search(input: &str) -> SearchParams {
    let chars: Vec<char> = input.chars().collect();
    let mut params = SearchParams::default();
    let mut free = String::new();
    let mut i = 0;
    while i < chars.len() {
        match operator_at(&chars, i) {
            Some((name, value, next)) => {
                apply_operator(&mut params, &name, &value);
                i = next;
            }
            None => {
                free.push(chars[i]);
                i += 1;
            }
        }
    }
    params.query = free.split_whitespace().collect::<Vec<_>>().join(" ");
    params
}

const OPERATORS: [&str; 8] = [
    "from", "to", "subject", "in", "is", "has", "before", "after",
];

/// Matches `name:value` at `start`. Returns the lowercase name, the value, and the index
/// just past the match. Mirrors `\b(from|...):(?:"([^"]*?)"|(\S+))`.
fn operator_at(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if start > 0 && is_word(chars[start - 1]) {
        return None;
    }
    let rest = &chars[start..];
    let name = OPERATORS.iter().find(|name| {
        let len = name.len();
        rest.len() > len
            && rest[len] == ':'
            && rest[..len]
                .iter()
                .zip(name.chars())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    })?;
    let value_start = start + name.len() + 1;
    let tail = &chars[value_start..];
    if tail.first() == Some(&'"')
        && let Some(close) = tail[1..].iter().position(|c| *c == '"')
    {
        let value: String = tail[1..1 + close].iter().collect();
        return Some(((*name).to_owned(), value, value_start + close + 2));
    }
    let len = tail.iter().take_while(|c| !c.is_whitespace()).count();
    if len == 0 {
        return None;
    }
    let value: String = tail[..len].iter().collect();
    Some(((*name).to_owned(), value, value_start + len))
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn apply_operator(params: &mut SearchParams, name: &str, value: &str) {
    match name {
        "from" => params.from = value.trim().to_owned(),
        "to" => params.to = value.trim().to_owned(),
        "subject" => params.subject = value.trim().to_owned(),
        "in" => params.folder = value.trim().to_lowercase(),
        "is" => match value.to_lowercase().as_str() {
            "unread" => params.is_read = Some(false),
            "read" => params.is_read = Some(true),
            "starred" => params.is_starred = Some(true),
            "unstarred" => params.is_starred = Some(false),
            _ => {}
        },
        "has" => {
            if value.eq_ignore_ascii_case("attachment") {
                params.has_attachment = true;
            }
        }
        "before" => {
            if let Some(date) = normalize_date(value) {
                params.date_end = date;
            }
        }
        "after" => {
            if let Some(date) = normalize_date(value) {
                params.date_start = date;
            }
        }
        _ => {}
    }
}

/// `new Date(value).toISOString()` for the forms users type: `YYYY-MM-DD` and
/// `YYYY-MM-DDTHH:MM[:SS]` with an optional `Z`. Other forms and offsets are rejected,
/// and a date-time without `Z` is read as UTC.
fn normalize_date(value: &str) -> Option<String> {
    let (date_part, time_part) = match value.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (value, None),
    };
    let mut parts = date_part.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    if day > days_in_month(year, month) {
        return None;
    }
    let (hour, minute, second) = match time_part {
        None => (0, 0, 0),
        Some(time) => {
            let time = time.strip_suffix('Z').unwrap_or(time);
            let mut fields = time.split(':');
            let hour: u32 = fields.next()?.parse().ok()?;
            let minute: u32 = fields.next()?.parse().ok()?;
            let second: u32 = match fields.next() {
                Some(s) => s.split('.').next()?.parse().ok()?,
                None => 0,
            };
            if fields.next().is_some() || hour > 23 || minute > 59 || second > 59 {
                return None;
            }
            (hour, minute, second)
        }
    };
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z"
    ))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Body for `POST /mailboxes/:id/emails` and `POST /emails/:id/reply|forward`.
#[derive(Debug, Clone, Serialize)]
pub struct SendEmail {
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<String>,
    pub from: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentUpload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentUpload {
    pub filename: String,
    #[serde(rename = "type")]
    pub mime_type: String,
    /// Base64 of the file bytes.
    pub content: String,
    pub size: u64,
    pub disposition: &'static str,
}

/// Builds an upload for a file's bytes.
pub fn attachment_upload(filename: &str, mime_type: &str, bytes: &[u8]) -> AttachmentUpload {
    AttachmentUpload {
        filename: filename.to_owned(),
        mime_type: mime_type.to_owned(),
        content: base64_encode(bytes),
        size: bytes.len() as u64,
        disposition: "attachment",
    }
}

/// Body for sending a stored draft (`POST /emails/:id/reply` with only these fields).
#[derive(Debug, Serialize)]
pub struct DraftSend<'a> {
    pub to: &'a str,
    pub subject: &'a str,
    pub html: &'a str,
}

/// Body for `POST /mailboxes/:id/drafts`.
#[derive(Debug, Serialize)]
pub struct DraftBody<'a> {
    pub to: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bcc: Option<&'a str>,
    pub subject: &'a str,
    pub body: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<&'a str>,
}

/// Body for `PUT /mailboxes/:id/emails/:id`. Only the fields set are sent.
#[derive(Debug, Default, Serialize)]
pub struct EmailPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
}

/// Recipients for a reply-all: the original `to` and `cc`, minus the mailbox itself and
/// the sender, deduplicated in order. Matching is a case-insensitive substring test, as
/// the Solid split view does.
pub fn reply_all_cc(email: &Email, mailbox: &str) -> String {
    let mailbox = mailbox.to_lowercase();
    let sender = email.sender.to_lowercase();
    let mut kept: Vec<String> = Vec::new();
    let cc = email.cc.as_deref().unwrap_or("");
    for raw in email.recipient.split(',').chain(cc.split(',')) {
        let address = raw.trim();
        let lower = address.to_lowercase();
        if lower.is_empty() || lower.contains(&mailbox) || lower.contains(&sender) {
            continue;
        }
        if !kept.iter().any(|k| k == address) {
            kept.push(address.to_owned());
        }
    }
    kept.join(", ")
}

#[derive(Serialize)]
struct NameBody<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct EmailMailboxBody<'a> {
    email: &'a str,
    name: &'a str,
}

#[derive(Serialize)]
struct FolderMove<'a> {
    #[serde(rename = "folderId")]
    folder_id: &'a str,
}

pub fn decode_mailboxes(bytes: &[u8]) -> Result<Vec<Mailbox>, ApiError> {
    decode(bytes)
}

pub fn decode_mailbox(bytes: &[u8]) -> Result<Mailbox, ApiError> {
    decode(bytes)
}

pub fn decode_config(bytes: &[u8]) -> Result<Config, ApiError> {
    decode(bytes)
}

pub fn decode_folders(bytes: &[u8]) -> Result<Vec<Folder>, ApiError> {
    decode(bytes)
}

pub fn decode_folder(bytes: &[u8]) -> Result<Folder, ApiError> {
    decode(bytes)
}

pub fn decode_email(bytes: &[u8]) -> Result<Email, ApiError> {
    decode(bytes)
}

pub fn decode_emails(bytes: &[u8]) -> Result<Vec<Email>, ApiError> {
    decode(bytes)
}

pub fn decode_email_page(bytes: &[u8]) -> Result<EmailPage, ApiError> {
    let wire: PageWire = decode(bytes)?;
    Ok(match wire {
        PageWire::Paged {
            emails,
            total_count,
        } => EmailPage {
            emails,
            total_count,
        },
        PageWire::Bare(emails) => {
            let total_count = emails.len() as u32;
            EmailPage {
                emails,
                total_count,
            }
        }
    })
}

pub fn decode_draft_saved(bytes: &[u8]) -> Result<DraftSaved, ApiError> {
    decode(bytes)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.is_empty())
}

fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Flag {
    Bool(bool),
    Number(i64),
}

/// Read and starred arrive as booleans from the Durable Object, but SQLite-backed
/// columns can arrive as 0 or 1.
fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(match Flag::deserialize(deserializer)? {
        Flag::Bool(value) => value,
        Flag::Number(value) => value != 0,
    })
}

fn with_query(url: String, pairs: &[(&str, String)]) -> String {
    if pairs.is_empty() {
        return url;
    }
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{url}?{query}")
}

fn json_body(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("request bodies are plain structs and always serialize")
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding, as `FileReader.readAsDataURL` produces.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(BASE64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl ApiClient {
    pub fn mailboxes(&self, on_done: impl FnOnce(Result<Vec<Mailbox>, ApiError>) + Send + 'static) {
        self.send(
            Request::get(self.url("/v1/mailboxes")),
            decode_mailboxes,
            on_done,
        );
    }

    /// Detail with settings.
    pub fn mailbox(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<Mailbox, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url(&format!("/v1/mailboxes/{}", encode(id)))),
            decode_mailbox,
            on_done,
        );
    }

    pub fn create_mailbox(
        &self,
        email: &str,
        name: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        let body = json_body(&EmailMailboxBody { email, name });
        self.send(
            json_request(Method::POST, self.url("/v1/mailboxes"), body),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn update_mailbox(
        &self,
        id: &str,
        patch: &MailboxSettingsPatch<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::PUT,
                self.url(&format!("/v1/mailboxes/{}", encode(id))),
                json_body(patch),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn delete_mailbox(
        &self,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::new(
                Method::DELETE,
                self.url(&format!("/v1/mailboxes/{}", encode(id))),
                Headers::new(&[]),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn folders(
        &self,
        mailbox: &str,
        on_done: impl FnOnce(Result<Vec<Folder>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url(&format!("/v1/mailboxes/{}/folders", encode(mailbox)))),
            decode_folders,
            on_done,
        );
    }

    pub fn create_folder(
        &self,
        mailbox: &str,
        name: &str,
        on_done: impl FnOnce(Result<Folder, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!("/v1/mailboxes/{}/folders", encode(mailbox))),
                json_body(&NameBody { name }),
            ),
            decode_folder,
            on_done,
        );
    }

    pub fn update_folder(
        &self,
        mailbox: &str,
        folder_id: &str,
        name: &str,
        on_done: impl FnOnce(Result<Folder, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::PUT,
                self.url(&format!(
                    "/v1/mailboxes/{}/folders/{}",
                    encode(mailbox),
                    encode(folder_id)
                )),
                json_body(&NameBody { name }),
            ),
            decode_folder,
            on_done,
        );
    }

    pub fn delete_folder(
        &self,
        mailbox: &str,
        folder_id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::new(
                Method::DELETE,
                self.url(&format!(
                    "/v1/mailboxes/{}/folders/{}",
                    encode(mailbox),
                    encode(folder_id)
                )),
                Headers::new(&[]),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Threaded list of one folder, newest conversation first, `page` from 1.
    pub fn emails(
        &self,
        mailbox: &str,
        folder: &str,
        page: u32,
        on_done: impl FnOnce(Result<EmailPage, ApiError>) + Send + 'static,
    ) {
        let url = with_query(
            self.url(&format!("/v1/mailboxes/{}/emails", encode(mailbox))),
            &[
                ("folder", folder.to_owned()),
                ("threaded", "true".to_owned()),
                ("page", page.to_string()),
                ("limit", PAGE_SIZE.to_string()),
            ],
        );
        self.send(Request::get(url), decode_email_page, on_done);
    }

    pub fn search(
        &self,
        mailbox: &str,
        params: &SearchParams,
        page: u32,
        on_done: impl FnOnce(Result<EmailPage, ApiError>) + Send + 'static,
    ) {
        let mut pairs = params.query_pairs();
        pairs.push(("page", page.to_string()));
        pairs.push(("limit", PAGE_SIZE.to_string()));
        let url = with_query(
            self.url(&format!("/v1/mailboxes/{}/search", encode(mailbox))),
            &pairs,
        );
        self.send(Request::get(url), decode_email_page, on_done);
    }

    pub fn email(
        &self,
        mailbox: &str,
        id: &str,
        on_done: impl FnOnce(Result<Email, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url(&format!(
                "/v1/mailboxes/{}/emails/{}",
                encode(mailbox),
                encode(id)
            ))),
            decode_email,
            on_done,
        );
    }

    /// All messages of a conversation, oldest first.
    pub fn thread(
        &self,
        mailbox: &str,
        thread_id: &str,
        on_done: impl FnOnce(Result<Vec<Email>, ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::get(self.url(&format!(
                "/v1/mailboxes/{}/threads/{}",
                encode(mailbox),
                encode(thread_id)
            ))),
            decode_emails,
            on_done,
        );
    }

    pub fn mark_thread_read(
        &self,
        mailbox: &str,
        thread_id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!(
                    "/v1/mailboxes/{}/threads/{}/read",
                    encode(mailbox),
                    encode(thread_id)
                )),
                Vec::new(),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn patch_email(
        &self,
        mailbox: &str,
        id: &str,
        patch: &EmailPatch,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::PUT,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}",
                    encode(mailbox),
                    encode(id)
                )),
                json_body(patch),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn move_email(
        &self,
        mailbox: &str,
        id: &str,
        folder_id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}/move",
                    encode(mailbox),
                    encode(id)
                )),
                json_body(&FolderMove { folder_id }),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn delete_email(
        &self,
        mailbox: &str,
        id: &str,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            Request::new(
                Method::DELETE,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}",
                    encode(mailbox),
                    encode(id)
                )),
                Headers::new(&[]),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Sends a new message. The server answers 202 with `{id, status}`.
    pub fn send_email(
        &self,
        mailbox: &str,
        message: &SendEmail,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!("/v1/mailboxes/{}/emails", encode(mailbox))),
                json_body(message),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Reply to `original_id`. The server sends it and threads it.
    pub fn reply(
        &self,
        mailbox: &str,
        original_id: &str,
        message: &SendEmail,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}/reply",
                    encode(mailbox),
                    encode(original_id)
                )),
                json_body(message),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn forward(
        &self,
        mailbox: &str,
        original_id: &str,
        message: &SendEmail,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}/forward",
                    encode(mailbox),
                    encode(original_id)
                )),
                json_body(message),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    /// Sends a stored draft by posting it as a reply to itself.
    pub fn send_draft(
        &self,
        mailbox: &str,
        draft_id: &str,
        draft: &DraftSend<'_>,
        on_done: impl FnOnce(Result<(), ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!(
                    "/v1/mailboxes/{}/emails/{}/reply",
                    encode(mailbox),
                    encode(draft_id)
                )),
                json_body(draft),
            ),
            |_: &[u8]| Ok(()),
            on_done,
        );
    }

    pub fn save_draft(
        &self,
        mailbox: &str,
        draft: &DraftBody<'_>,
        on_done: impl FnOnce(Result<DraftSaved, ApiError>) + Send + 'static,
    ) {
        self.send(
            json_request(
                Method::POST,
                self.url(&format!("/v1/mailboxes/{}/drafts", encode(mailbox))),
                json_body(draft),
            ),
            decode_draft_saved,
            on_done,
        );
    }

    /// Absolute URL of an attachment. The server answers with `Content-Disposition:
    /// attachment`, so the browser downloads it when opened.
    pub fn attachment_url(&self, mailbox: &str, email_id: &str, attachment_id: &str) -> String {
        self.url(&format!(
            "/v1/mailboxes/{}/emails/{}/attachments/{}",
            encode(mailbox),
            encode(email_id),
            encode(attachment_id)
        ))
    }

    pub fn config(&self, on_done: impl FnOnce(Result<Config, ApiError>) + Send + 'static) {
        self.send(Request::get(self.url("/v1/config")), decode_config, on_done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(json: &str) -> EmailPage {
        decode_email_page(json.as_bytes()).expect("page decodes")
    }

    #[test]
    fn threaded_page_decodes_snake_case_rows_and_null_fields() {
        let json = r#"{
            "totalCount": 30,
            "emails": [{
                "id": "m1",
                "folder_id": "inbox",
                "subject": null,
                "sender": "ana@example.com",
                "recipient": "daniel@idantity.me",
                "cc": null,
                "date": "2026-05-10T12:00:00.000Z",
                "read": 0,
                "starred": true,
                "body": "<p>Hi</p>",
                "snippet": "Hi",
                "message_id": "<abc@example.com>",
                "thread_id": null,
                "raw_headers": "[{\"key\":\"from\",\"value\":\"ana@example.com\"}]",
                "attachments": [{"id": "a1", "email_id": "m1", "filename": "x.pdf",
                                 "mimetype": "application/pdf", "size": 2048}]
            }]
        }"#;
        let page = page(json);
        assert_eq!(page.total_count, 30);
        let email = &page.emails[0];
        assert!(!email.read);
        assert!(email.starred);
        assert_eq!(email.subject, "");
        assert_eq!(email.thread_key(), "m1");
        assert_eq!(email.reply_target_id(), "<abc@example.com>");
        assert_eq!(
            email.headers(),
            vec![("from".to_owned(), "ana@example.com".to_owned())]
        );
        assert_eq!(email.attachments[0].size, 2048);
    }

    #[test]
    fn bare_array_page_counts_its_rows() {
        let json = r#"[{"id":"a","subject":"s","sender":"x","recipient":"y","date":"d","read":true,"starred":false,"body":"","thread_id":"t"}]"#;
        let page = page(json);
        assert_eq!(page.total_count, 1);
        assert_eq!(page.emails[0].thread_key(), "t");
        assert!(page.emails[0].read);
    }

    #[test]
    fn total_pages_rounds_up_by_page_size() {
        assert_eq!(total_pages(0), 0);
        assert_eq!(total_pages(25), 1);
        assert_eq!(total_pages(26), 2);
    }

    #[test]
    fn folder_filter_drops_system_folders_only() {
        let folders = vec![
            Folder {
                id: "inbox".into(),
                name: "Inbox".into(),
                unread_count: 0,
            },
            Folder {
                id: "spam".into(),
                name: "Spam".into(),
                unread_count: 0,
            },
            Folder {
                id: "receipts".into(),
                name: "Receipts".into(),
                unread_count: 2,
            },
            Folder {
                id: "trash".into(),
                name: "Trash".into(),
                unread_count: 0,
            },
        ];
        let ids: Vec<&str> = custom_folders(&folders)
            .iter()
            .map(|f| f.id.as_str())
            .collect();
        assert_eq!(ids, vec!["spam", "receipts"]);
    }

    #[test]
    fn search_params_omit_empty_filters_and_encode_values() {
        let params = parse_search("budget report from:\"Ana Lima\" is:unread");
        let pairs = params.query_pairs();
        assert_eq!(
            pairs,
            vec![
                ("query", "budget report".to_owned()),
                ("from", "Ana Lima".to_owned()),
                ("is_read", "false".to_owned()),
            ]
        );
        let url = with_query(
            "http://h/search".to_owned(),
            &[("query", "a b&c".to_owned())],
        );
        assert_eq!(url, "http://h/search?query=a%20b%26c");
        assert!(SearchParams::default().is_empty());
        assert!(SearchParams::default().query_pairs().is_empty());
    }

    #[test]
    fn parser_reads_folder_flags_and_dates_and_keeps_inline_text() {
        let params = parse_search(
            "in:Archive has:attachment before:2024-01-15 after:2024-01-01T10:30Z xfrom:bob",
        );
        assert_eq!(params.folder, "archive");
        assert!(params.has_attachment);
        assert_eq!(params.date_end, "2024-01-15T00:00:00.000Z");
        assert_eq!(params.date_start, "2024-01-01T10:30:00.000Z");
        assert_eq!(params.from, "");
        assert_eq!(params.query, "xfrom:bob");
    }

    #[test]
    fn parser_drops_impossible_dates_but_keeps_free_text_operators() {
        let params = parse_search("before:2024-02-30 from:");
        assert_eq!(params.date_end, "");
        assert_eq!(params.query, "from:");
    }

    #[test]
    fn reply_all_drops_own_mailbox_and_sender_and_duplicates() {
        let email = Email {
            id: "m".into(),
            folder_id: "inbox".into(),
            subject: String::new(),
            sender: "Ana@example.com".into(),
            recipient: "daniel@idantity.me, bo@example.com".into(),
            cc: Some("bo@example.com, Carl@example.com, ana@example.com".into()),
            bcc: None,
            date: String::new(),
            read: true,
            starred: false,
            body: String::new(),
            snippet: String::new(),
            message_id: None,
            in_reply_to: None,
            thread_id: None,
            raw_headers: None,
            attachments: Vec::new(),
        };
        assert_eq!(
            reply_all_cc(&email, "daniel@idantity.me"),
            "bo@example.com, Carl@example.com"
        );
    }

    #[test]
    fn base64_matches_standard_padding() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
    }
}
