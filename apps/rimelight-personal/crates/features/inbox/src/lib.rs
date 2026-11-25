//! The Inbox page controller (ported from `src/components/inbox/` on the web and from `crates/app/src/inbox.rs`).
//!
//! Plain Rust with no Slint. The controller holds the page state (mailboxes, folders, the paged threaded list with
//! search, the open conversation, compose, settings, mailbox and folder forms) and calls the `/api/v1/*` endpoints
//! through [`InboxApi`]. Every state change is reported as an [`InboxView`]. The shell's glue turns views into Slint
//! models.
//!
//! Threading: API calls return at once and answer later on a worker thread. The lock is never held across a call,
//! and `notify` runs after the lock is released. Mail bodies are shown as structured text ([`html`]); the original
//! HTML opens in the system browser from a temporary file ([`InboxController::view_original`]).

pub mod html;
mod local_time;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use html::{Block, blocks_for};
use idantity_core::inbox::{
    AttachmentUpload, Config, DraftBody, DraftSaved, DraftSend, Email, EmailPage, EmailPatch,
    Folder, Mailbox, MailboxSettingsPatch, MailboxSettingsWrite, SearchParams, SendEmail,
    SignatureWrite, attachment_upload, parse_search, reply_all_cc, total_pages,
};
use idantity_core::{ApiClient, ApiError};
use rimelight_picture::{Picture, load_bytes};
use web_time::{SystemTime, UNIX_EPOCH};

/// The page's Slint markup, for the shell's build script.
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The web saves a draft after the person stops typing for this long (seconds).
pub const AUTOSAVE_DELAY: f64 = 4.0;
/// Attachments above this size are refused before upload (the web's limit).
pub const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
/// Images in mail above this size are not decoded.
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// Text attachments show at most this much.
const MAX_TEXT_PREVIEW_BYTES: usize = 256 * 1024;

/// A file name safe to write into the temporary folder: no separators and no leading dots.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_start_matches('.').trim();
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}
/// The move targets offered by the move menu, before the custom folders.
const MOVE_TARGETS: [(&str, &str); 4] = [
    ("inbox", "Inbox"),
    ("archive", "Archive"),
    ("spam", "Spam"),
    ("trash", "Trash"),
];

/// A boxed completion callback, run on the API's worker thread.
pub type Done<T> = Box<dyn FnOnce(Result<T, ApiError>) + Send>;

/// The API boundary: the `/api/v1` calls the page makes. [`ApiClient`] implements it over HTTP.
pub trait InboxApi: Send + Sync {
    fn config(&self, on_done: Done<Config>);
    fn mailboxes(&self, on_done: Done<Vec<Mailbox>>);
    fn mailbox(&self, id: &str, on_done: Done<Mailbox>);
    fn create_mailbox(&self, email: &str, name: &str, on_done: Done<()>);
    fn update_mailbox(&self, id: &str, patch: &MailboxSettingsPatch<'_>, on_done: Done<()>);
    fn delete_mailbox(&self, id: &str, on_done: Done<()>);
    fn folders(&self, mailbox: &str, on_done: Done<Vec<Folder>>);
    fn create_folder(&self, mailbox: &str, name: &str, on_done: Done<Folder>);
    fn update_folder(&self, mailbox: &str, folder_id: &str, name: &str, on_done: Done<Folder>);
    fn delete_folder(&self, mailbox: &str, folder_id: &str, on_done: Done<()>);
    fn emails(&self, mailbox: &str, folder: &str, page: u32, on_done: Done<EmailPage>);
    fn search(&self, mailbox: &str, params: &SearchParams, page: u32, on_done: Done<EmailPage>);
    fn thread(&self, mailbox: &str, thread_id: &str, on_done: Done<Vec<Email>>);
    fn mark_thread_read(&self, mailbox: &str, thread_id: &str, on_done: Done<()>);
    fn patch_email(&self, mailbox: &str, id: &str, patch: &EmailPatch, on_done: Done<()>);
    fn move_email(&self, mailbox: &str, id: &str, folder_id: &str, on_done: Done<()>);
    fn delete_email(&self, mailbox: &str, id: &str, on_done: Done<()>);
    fn send_email(&self, mailbox: &str, message: &SendEmail, on_done: Done<()>);
    fn reply(&self, mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>);
    fn forward(&self, mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>);
    fn send_draft(&self, mailbox: &str, draft_id: &str, draft: &DraftSend<'_>, on_done: Done<()>);
    fn save_draft(&self, mailbox: &str, draft: &DraftBody<'_>, on_done: Done<DraftSaved>);
    /// Absolute URL of an attachment. The browser sends the session cookie when it opens it.
    fn attachment_url(&self, mailbox: &str, email_id: &str, attachment_id: &str) -> String;
    /// Downloads the bytes at `url` with no session token: remote images and attachments. Used for images inside
    /// mail, which the web loads automatically.
    fn fetch(&self, url: &str, on_done: Done<Vec<u8>>);
}

impl InboxApi for ApiClient {
    fn config(&self, on_done: Done<Config>) {
        ApiClient::config(self, on_done);
    }
    fn mailboxes(&self, on_done: Done<Vec<Mailbox>>) {
        ApiClient::mailboxes(self, on_done);
    }
    fn mailbox(&self, id: &str, on_done: Done<Mailbox>) {
        ApiClient::mailbox(self, id, on_done);
    }
    fn create_mailbox(&self, email: &str, name: &str, on_done: Done<()>) {
        ApiClient::create_mailbox(self, email, name, on_done);
    }
    fn update_mailbox(&self, id: &str, patch: &MailboxSettingsPatch<'_>, on_done: Done<()>) {
        ApiClient::update_mailbox(self, id, patch, on_done);
    }
    fn delete_mailbox(&self, id: &str, on_done: Done<()>) {
        ApiClient::delete_mailbox(self, id, on_done);
    }
    fn folders(&self, mailbox: &str, on_done: Done<Vec<Folder>>) {
        ApiClient::folders(self, mailbox, on_done);
    }
    fn create_folder(&self, mailbox: &str, name: &str, on_done: Done<Folder>) {
        ApiClient::create_folder(self, mailbox, name, on_done);
    }
    fn update_folder(&self, mailbox: &str, folder_id: &str, name: &str, on_done: Done<Folder>) {
        ApiClient::update_folder(self, mailbox, folder_id, name, on_done);
    }
    fn delete_folder(&self, mailbox: &str, folder_id: &str, on_done: Done<()>) {
        ApiClient::delete_folder(self, mailbox, folder_id, on_done);
    }
    fn emails(&self, mailbox: &str, folder: &str, page: u32, on_done: Done<EmailPage>) {
        ApiClient::emails(self, mailbox, folder, page, on_done);
    }
    fn search(&self, mailbox: &str, params: &SearchParams, page: u32, on_done: Done<EmailPage>) {
        ApiClient::search(self, mailbox, params, page, on_done);
    }
    fn thread(&self, mailbox: &str, thread_id: &str, on_done: Done<Vec<Email>>) {
        ApiClient::thread(self, mailbox, thread_id, on_done);
    }
    fn mark_thread_read(&self, mailbox: &str, thread_id: &str, on_done: Done<()>) {
        ApiClient::mark_thread_read(self, mailbox, thread_id, on_done);
    }
    fn patch_email(&self, mailbox: &str, id: &str, patch: &EmailPatch, on_done: Done<()>) {
        ApiClient::patch_email(self, mailbox, id, patch, on_done);
    }
    fn move_email(&self, mailbox: &str, id: &str, folder_id: &str, on_done: Done<()>) {
        ApiClient::move_email(self, mailbox, id, folder_id, on_done);
    }
    fn delete_email(&self, mailbox: &str, id: &str, on_done: Done<()>) {
        ApiClient::delete_email(self, mailbox, id, on_done);
    }
    fn send_email(&self, mailbox: &str, message: &SendEmail, on_done: Done<()>) {
        ApiClient::send_email(self, mailbox, message, on_done);
    }
    fn reply(&self, mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>) {
        ApiClient::reply(self, mailbox, original_id, message, on_done);
    }
    fn forward(&self, mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>) {
        ApiClient::forward(self, mailbox, original_id, message, on_done);
    }
    fn send_draft(&self, mailbox: &str, draft_id: &str, draft: &DraftSend<'_>, on_done: Done<()>) {
        ApiClient::send_draft(self, mailbox, draft_id, draft, on_done);
    }
    fn save_draft(&self, mailbox: &str, draft: &DraftBody<'_>, on_done: Done<DraftSaved>) {
        ApiClient::save_draft(self, mailbox, draft, on_done);
    }
    fn attachment_url(&self, mailbox: &str, email_id: &str, attachment_id: &str) -> String {
        ApiClient::attachment_url(self, mailbox, email_id, attachment_id)
    }
    fn fetch(&self, url: &str, on_done: Done<Vec<u8>>) {
        // No bearer token: these are other hosts (remote images) or the attachment route with the browser's cookie.
        ehttp::fetch(ehttp::Request::get(url), move |result| {
            on_done(match result {
                Err(e) => Err(ApiError::Transport(e)),
                Ok(resp) if !resp.ok => Err(ApiError::Status {
                    code: resp.status,
                    body: String::from_utf8_lossy(&resp.bytes).into_owned(),
                }),
                Ok(resp) => Ok(resp.bytes),
            });
        });
    }
}

/// The live API for `api_base`, for example `https://personal.idantity.me/api`.
pub fn http_api(api_base: impl Into<String>) -> Arc<dyn InboxApi> {
    Arc::new(ApiClient::new(api_base))
}

/// The things the page asks the system for: opening an address, and the file dialog for attachments.
pub trait Shell: Send + Sync {
    /// Opens a web address, or a plain local file path, with the system. True when the system took it.
    fn open(&self, target: &str) -> bool;
    /// The files the person picks. Empty when they cancel.
    fn pick_files(&self) -> Vec<PathBuf>;
    /// Asks where to save `name`. `None` when the person cancels.
    fn save_file(&self, name: &str) -> Option<PathBuf>;
}

/// The real shell: `rimelight_platform` for the browser, the file dialogs and the system app.
pub struct SystemShell;

impl Shell for SystemShell {
    /// `rimelight_platform::open_uri` calls `ShellExecuteW` with the verb `open`. That takes a web address and also a
    /// plain local file path (which the system opens with its associated app). It does not take a `file:///` address,
    /// so callers pass the path itself, as the file's `PathBuf` text.
    fn open(&self, target: &str) -> bool {
        rimelight_platform::open_uri(target)
    }

    fn pick_files(&self) -> Vec<PathBuf> {
        rimelight_platform::pick_files(&[], true)
    }

    fn save_file(&self, name: &str) -> Option<PathBuf> {
        rimelight_platform::save_file_dialog(name, &[])
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Seconds since the Unix epoch, now.
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The list's date, as the web shows it: `Nov 22` (month and day, in the local zone). The raw text when it does not
/// parse.
pub fn short_date(value: &str) -> String {
    local_time::month_day(value).unwrap_or_else(|| value.to_owned())
}

/// The message header's date, as the web shows it: `Nov 22, 10:00 AM` (local zone, en-US form). The raw text when it
/// does not parse.
pub fn detail_date(value: &str) -> String {
    local_time::month_day_time(value).unwrap_or_else(|| value.to_owned())
}

/// Sender shown in lists and headers.
pub fn display_sender(email: &Email) -> String {
    if email.sender.trim().is_empty() {
        "Unknown Sender".to_owned()
    } else {
        email.sender.clone()
    }
}

/// Subject shown in lists and headers.
pub fn display_subject(email: &Email) -> String {
    if email.subject.trim().is_empty() {
        "(No Subject)".to_owned()
    } else {
        email.subject.clone()
    }
}

fn has_prefix_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `Re: subject`, not doubled when the subject already has the prefix.
pub fn compose_subject(subject: &str) -> String {
    if has_prefix_ci(subject, "re:") || has_prefix_ci(subject, "fwd:") {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    }
}

/// `Fwd: subject`, not doubled.
pub fn forward_subject(subject: &str) -> String {
    if has_prefix_ci(subject, "fwd:") {
        subject.to_owned()
    } else {
        format!("Fwd: {subject}")
    }
}

/// `12 KB`, `3.4 MB`.
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_owned();
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Content type from the file name. Covers the types people attach; anything else is sent as a binary.
pub fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// Replaces each `marker…marker` pair with `open…close`. An unpaired marker stays as it is.
fn wrap_pairs(text: &str, marker: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        let after = &rest[start + marker.len()..];
        match after.find(marker) {
            Some(end) if end > 0 => {
                out.push_str(open);
                out.push_str(&after[..end]);
                out.push_str(close);
                rest = &after[end + marker.len()..];
            }
            _ => {
                out.push_str(marker);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Turns `[label](https://…)` into a link. Only web and mail addresses become links.
fn markdown_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let link = after.find("](").and_then(|mid| {
            let url_start = mid + 2;
            let end = after[url_start..].find(')')?;
            Some((mid, url_start, url_start + end))
        });
        match link {
            Some((mid, url_start, url_end)) => {
                let label = &after[..mid];
                let url = &after[url_start..url_end];
                if url.starts_with("https://")
                    || url.starts_with("http://")
                    || url.starts_with("mailto:")
                {
                    out.push_str(&format!("<a href=\"{url}\">{label}</a>"));
                } else {
                    out.push('[');
                    out.push_str(label);
                    out.push_str("](");
                    out.push_str(url);
                    out.push(')');
                }
                rest = &after[url_end + 1..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of compose text as HTML: escaped, with **bold**, *italic* and [link](url) markers.
fn inline_html(line: &str) -> String {
    let escaped = line
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let bold = wrap_pairs(&escaped, "**", "<strong>", "</strong>");
    let italic = wrap_pairs(&bold, "*", "<em>", "</em>");
    markdown_links(&italic)
}

/// The HTML the server receives for a compose body. Blank lines start a paragraph, a run of `- ` lines is a list,
/// and other line breaks are `<br/>`.
pub fn body_html(text: &str) -> String {
    let mut out = String::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        if lines.iter().all(|l| l.trim_start().starts_with("- ")) {
            out.push_str("<ul>");
            for line in &lines {
                let item = line.trim_start().trim_start_matches("- ");
                out.push_str(&format!("<li>{}</li>", inline_html(item)));
            }
            out.push_str("</ul>");
        } else {
            let joined: Vec<String> = lines.iter().map(|l| inline_html(l)).collect();
            out.push_str(&format!("<p>{}</p>", joined.join("<br/>")));
        }
    }
    out
}

/// Removes `<tag …>…</tag>` pairs (case-insensitive) and bare `<tag …>` openers, so a saved copy runs no script or
/// embedded frame.
pub fn strip_elements(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(found) = lower[pos..].find(&open) {
        let start = pos + found;
        out.push_str(&html[pos..start]);
        let after_open = match lower[start..].find('>') {
            Some(gt) => start + gt + 1,
            None => html.len(),
        };
        let end = match lower[after_open..].find(&close) {
            Some(c) => match lower[after_open + c..].find('>') {
                Some(gt) => after_open + c + gt + 1,
                None => html.len(),
            },
            None => after_open,
        };
        pos = end;
    }
    out.push_str(&html[pos..]);
    out
}

/// The standalone page written for "View original". Scripts, frames and embedded objects are removed, and a
/// Content-Security-Policy blocks everything else that could run. Images and styles still load.
pub fn original_document(email: &Email) -> String {
    let body = if html::looks_like_html(&email.body) {
        let mut cleaned = email.body.clone();
        for tag in ["script", "iframe", "object", "embed"] {
            cleaned = strip_elements(&cleaned, tag);
        }
        cleaned
    } else {
        format!(
            "<pre style=\"white-space: pre-wrap\">{}</pre>",
            email
                .body
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        )
    };
    let title = display_subject(email)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: https: http:; style-src 'unsafe-inline'; font-src data: https:\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title></head>\
<body>{body}</body></html>"
    )
}

/// Writes the original of `email` into `dir` (created when missing) and returns its path. The file name is made
/// from the message id, so no id can name a path outside `dir`.
pub fn write_original(dir: &Path, email: &Email) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let safe: String = email
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let name = if safe.is_empty() {
        "message".to_owned()
    } else {
        safe
    };
    let path = dir.join(format!("{name}.html"));
    std::fs::write(&path, original_document(email))?;
    Ok(path)
}

/// The temporary folder for "View original" files.
pub fn original_dir() -> PathBuf {
    std::env::temp_dir().join("idantity-inbox")
}

// ---------------------------------------------------------------------------
// The view (plain data, no Slint types)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct MailboxRow {
    pub id: String,
    pub label: String,
    pub email: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderRow {
    pub id: String,
    pub label: String,
    pub unread: u32,
    /// True for the folders the user made (they can be renamed and deleted).
    pub custom: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    pub id: String,
    pub thread_key: String,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub date: String,
    pub unread: bool,
    pub starred: bool,
    pub attachments: u32,
    pub selected: bool,
}

/// One structured line of a message body. `kind`: 0 paragraph, 1 item, 2 link, 3 rule, 4 picture (`text` is the
/// picture's key in [`InboxView::images`], `href` its source), 5 image not loaded (`text` is its alt text).
#[derive(Debug, Clone, PartialEq)]
pub struct LineView {
    pub kind: u8,
    pub depth: u32,
    pub text: String,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentView {
    pub id: String,
    pub filename: String,
    pub size: String,
    /// `/v1/…` address the browser opens (with the session cookie).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageView {
    pub id: String,
    pub sender: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub subject: String,
    pub starred: bool,
    pub read: bool,
    pub is_draft: bool,
    pub lines: Vec<LineView>,
    pub attachments: Vec<AttachmentView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderRow {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComposeView {
    /// `New message`, `Reply`, `Reply all` or `Forward`.
    pub title: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub show_cc_bcc: bool,
    pub subject: String,
    pub body: String,
    /// `Saving draft...` or `Draft saved`, empty when there is nothing to say.
    pub status: String,
    pub error: String,
    pub sending: bool,
    /// `file name · 12 KB` per attachment.
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsView {
    pub loading: bool,
    pub from_name: String,
    pub signature: String,
    pub signature_enabled: bool,
    pub saving: bool,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateView {
    pub prefix: String,
    pub domain: String,
    pub domains: Vec<String>,
    pub name: String,
    pub creating: bool,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderFormView {
    /// `None` for a new folder, the folder id for a rename.
    pub rename_of: Option<String>,
    pub name: String,
    pub error: String,
}

/// A question waiting for a yes or no. `title` and `message` are the dialog text.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmView {
    pub title: String,
    pub message: String,
}

/// Everything the page shows at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InboxView {
    pub loading: bool,
    /// The error or notice banner. Empty when none.
    pub banner: String,
    pub mailboxes: Vec<MailboxRow>,
    pub folders: Vec<FolderRow>,
    /// Move targets: system folders first, then the custom ones. `(id, label)`.
    pub move_targets: Vec<(String, String)>,
    pub folder: String,
    pub search: String,
    pub list: Vec<ListRow>,
    pub list_loading: bool,
    pub list_error: String,
    pub page: u32,
    pub pages: u32,
    pub total: u32,
    /// Thread key of the open conversation. Empty when none.
    pub thread_key: String,
    pub thread: Vec<MessageView>,
    pub thread_loading: bool,
    pub thread_error: String,
    pub compose: Option<ComposeView>,
    pub settings: Option<SettingsView>,
    pub create: Option<CreateView>,
    pub folder_form: Option<FolderFormView>,
    pub confirm: Option<ConfirmView>,
    /// Source headers of the message whose headers are open. `None` when closed.
    pub headers: Option<Vec<HeaderRow>>,
    /// Decoded pictures of the open conversation. A line of kind 4 names one by its `text` key.
    pub images: Vec<ImageView>,
    /// The attachment viewer, while open.
    pub viewer: Option<ViewerView>,
}

/// A decoded picture: `key` is the image's source in the mail (or the attachment's id), `rgba` is `w * h * 4` bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageView {
    pub key: String,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// The attachment viewer. `kind`: 0 image (`image` is set), 1 text (`text` is set), 2 media (`path` is a local file
/// the glue plays with rimelight_media).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewerView {
    pub title: String,
    pub kind: u8,
    pub text: String,
    pub path: String,
    pub image: Option<ImageView>,
}

/// What an attachment opens as inside the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Text,
    Audio,
    Video,
    Other,
}

/// Classifies an attachment by its content type, then by its extension.
pub fn attachment_kind(filename: &str, mimetype: &str) -> AttachmentKind {
    let mime = mimetype.to_ascii_lowercase();
    if mime.starts_with("image/") {
        return AttachmentKind::Image;
    }
    if mime.starts_with("video/") {
        return AttachmentKind::Video;
    }
    if mime.starts_with("audio/") {
        return AttachmentKind::Audio;
    }
    if mime.starts_with("text/") || mime == "application/json" {
        return AttachmentKind::Text;
    }
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif" => {
            AttachmentKind::Image
        }
        "mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v" => AttachmentKind::Video,
        "mp3" | "m4a" | "wav" | "ogg" | "flac" | "aac" => AttachmentKind::Audio,
        "txt" | "md" | "csv" | "json" | "log" | "xml" | "html" | "htm" => AttachmentKind::Text,
        _ => AttachmentKind::Other,
    }
}

// ---------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ReplyTo {
    forward: bool,
    original_id: String,
    thread_id: Option<String>,
    in_reply_to: Option<String>,
}

/// Compose field indexes, as the page sends them.
pub const FIELD_TO: u8 = 0;
pub const FIELD_CC: u8 = 1;
pub const FIELD_BCC: u8 = 2;
pub const FIELD_SUBJECT: u8 = 3;
pub const FIELD_BODY: u8 = 4;

#[derive(Default)]
struct Compose {
    reply: Option<ReplyTo>,
    title: String,
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    body: String,
    show_cc_bcc: bool,
    draft_id: Option<String>,
    /// When the text last changed (seconds), for the autosave delay. `None` when nothing is waiting to save.
    dirty_at: Option<f64>,
    saving: bool,
    status: String,
    sending: bool,
    error: String,
    attachments: Vec<AttachmentUpload>,
    attachment_names: Vec<String>,
}

impl Compose {
    fn has_content(&self) -> bool {
        [&self.to, &self.subject, &self.body]
            .iter()
            .any(|f| !f.trim().is_empty())
    }
}

struct SettingsForm {
    mailbox: String,
    loading: bool,
    from_name: String,
    signature: String,
    signature_enabled: bool,
    saving: bool,
    error: String,
}

struct CreateForm {
    prefix: String,
    domain: String,
    name: String,
    creating: bool,
    error: String,
}

struct FolderForm {
    rename_of: Option<String>,
    name: String,
    error: String,
}

enum Confirm {
    DeleteMailbox(String),
    DeleteFolder(String),
    DiscardDraft(String),
}

#[derive(Default)]
struct State {
    loading: bool,
    banner: String,
    domains: Vec<String>,
    mailboxes: Vec<Mailbox>,
    mailbox: Option<String>,
    folders: Vec<Folder>,
    folder: String,
    query: String,
    page: u32,
    emails: Vec<Email>,
    total: u32,
    list_loading: bool,
    list_error: String,
    list_seq: u64,
    selected: Option<String>,
    thread: Vec<Email>,
    thread_loading: bool,
    thread_error: String,
    thread_seq: u64,
    compose: Option<Compose>,
    settings: Option<SettingsForm>,
    create: Option<CreateForm>,
    folder_form: Option<FolderForm>,
    confirm: Option<Confirm>,
    headers: Option<Vec<(String, String)>>,
    /// Decoded pictures keyed by their source in the mail (`cid:`, `data:` or a web address).
    images: Vec<(String, Picture)>,
    /// Sources being downloaded, so each is requested once.
    image_pending: Vec<String>,
    viewer: Option<Viewer>,
}

/// The attachment viewer's content.
enum Viewer {
    Image { title: String, picture: Picture },
    Text { title: String, text: String },
    Media { title: String, path: String },
}

impl State {
    fn mailbox_email(&self) -> String {
        let id = self.mailbox.as_deref().unwrap_or("");
        self.mailboxes
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.email.clone())
            .unwrap_or_default()
    }

    /// A message from the open thread or the list.
    fn find_email(&self, id: &str) -> Option<Email> {
        self.thread
            .iter()
            .chain(self.emails.iter())
            .find(|e| e.id == id)
            .cloned()
    }

    fn view(&self) -> InboxView {
        let selected_mailbox = self.mailbox.clone().unwrap_or_default();
        let mailboxes = self
            .mailboxes
            .iter()
            .map(|m| MailboxRow {
                id: m.id.clone(),
                label: if m.name.trim().is_empty() {
                    m.email.clone()
                } else {
                    m.name.clone()
                },
                email: m.email.clone(),
                selected: m.id == selected_mailbox,
            })
            .collect();
        let folders = self
            .folders
            .iter()
            .map(|f| FolderRow {
                id: f.id.clone(),
                label: folder_label(f),
                unread: f.unread_count,
                custom: idantity_core::inbox::is_custom_folder(&f.id),
                selected: f.id == self.folder,
            })
            .collect();
        let mut move_targets: Vec<(String, String)> = MOVE_TARGETS
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect();
        for folder in idantity_core::inbox::custom_folders(&self.folders) {
            move_targets.push((folder.id.clone(), folder_label(folder)));
        }
        let list = self
            .emails
            .iter()
            .map(|e| ListRow {
                id: e.id.clone(),
                thread_key: e.thread_key().to_owned(),
                sender: display_sender(e),
                subject: display_subject(e),
                snippet: one_line(&e.snippet),
                date: short_date(&e.date),
                unread: !e.read,
                starred: e.starred,
                attachments: e.attachments.len() as u32,
                selected: self.selected.as_deref() == Some(e.thread_key()),
            })
            .collect();
        let thread_key = self.selected.clone().unwrap_or_default();
        let thread = self
            .thread
            .iter()
            .map(|e| message_view(e, &self.images))
            .collect();
        let images = self
            .images
            .iter()
            .map(|(key, pic)| ImageView {
                key: key.clone(),
                w: pic.w,
                h: pic.h,
                rgba: pic.rgba.clone(),
            })
            .collect();
        let viewer = self.viewer.as_ref().map(|v| match v {
            Viewer::Image { title, picture } => ViewerView {
                title: title.clone(),
                kind: 0,
                text: String::new(),
                path: String::new(),
                image: Some(ImageView {
                    key: title.clone(),
                    w: picture.w,
                    h: picture.h,
                    rgba: picture.rgba.clone(),
                }),
            },
            Viewer::Text { title, text } => ViewerView {
                title: title.clone(),
                kind: 1,
                text: text.clone(),
                path: String::new(),
                image: None,
            },
            Viewer::Media { title, path } => ViewerView {
                title: title.clone(),
                kind: 2,
                text: String::new(),
                path: path.clone(),
                image: None,
            },
        });
        let compose = self.compose.as_ref().map(|c| ComposeView {
            title: c.title.clone(),
            to: c.to.clone(),
            cc: c.cc.clone(),
            bcc: c.bcc.clone(),
            show_cc_bcc: c.show_cc_bcc,
            subject: c.subject.clone(),
            body: c.body.clone(),
            status: if c.saving {
                "Saving draft...".to_owned()
            } else {
                c.status.clone()
            },
            error: c.error.clone(),
            sending: c.sending,
            attachments: c
                .attachments
                .iter()
                .map(|a| format!("{} · {}", a.filename, format_bytes(a.size)))
                .collect(),
        });
        let settings = self.settings.as_ref().map(|s| SettingsView {
            loading: s.loading,
            from_name: s.from_name.clone(),
            signature: s.signature.clone(),
            signature_enabled: s.signature_enabled,
            saving: s.saving,
            error: s.error.clone(),
        });
        let create = self.create.as_ref().map(|c| CreateView {
            prefix: c.prefix.clone(),
            domain: c.domain.clone(),
            domains: self.domains.clone(),
            name: c.name.clone(),
            creating: c.creating,
            error: c.error.clone(),
        });
        let folder_form = self.folder_form.as_ref().map(|f| FolderFormView {
            rename_of: f.rename_of.clone(),
            name: f.name.clone(),
            error: f.error.clone(),
        });
        let confirm = self.confirm.as_ref().map(|c| match c {
            Confirm::DeleteMailbox(_) => ConfirmView {
                title: "Delete mailbox".to_owned(),
                message: "Delete this mailbox and everything in it? This cannot be undone."
                    .to_owned(),
            },
            Confirm::DeleteFolder(_) => ConfirmView {
                title: "Delete folder".to_owned(),
                message: "Delete this folder? Messages in it are not deleted.".to_owned(),
            },
            Confirm::DiscardDraft(_) => ConfirmView {
                title: "Discard draft".to_owned(),
                message: "Discard this draft? This cannot be undone.".to_owned(),
            },
        });
        let headers = self.headers.as_ref().map(|pairs| {
            pairs
                .iter()
                .map(|(key, value)| HeaderRow {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect()
        });
        InboxView {
            loading: self.loading,
            banner: self.banner.clone(),
            mailboxes,
            folders,
            move_targets,
            folder: self.folder.clone(),
            search: self.query.clone(),
            list,
            list_loading: self.list_loading,
            list_error: self.list_error.clone(),
            page: self.page.max(1),
            pages: total_pages(self.total).max(1),
            total: self.total,
            thread_key,
            thread,
            thread_loading: self.thread_loading,
            thread_error: self.thread_error.clone(),
            compose,
            settings,
            create,
            folder_form,
            confirm,
            headers,
            images,
            viewer,
        }
    }
}

fn folder_label(folder: &Folder) -> String {
    let system = match folder.id.as_str() {
        "inbox" => Some("Inbox"),
        "sent" => Some("Sent"),
        "draft" => Some("Drafts"),
        "archive" => Some("Archive"),
        "trash" => Some("Trash"),
        _ => None,
    };
    match system {
        Some(name) => name.to_owned(),
        None if folder.name.trim().is_empty() => folder.id.clone(),
        None => folder.name.clone(),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn message_view(e: &Email, images: &[(String, Picture)]) -> MessageView {
    let lines = blocks_for(&e.body)
        .into_iter()
        .map(|block| match block {
            Block::Image { src, alt, depth } => {
                let loaded = images.iter().any(|(key, _)| *key == src);
                if loaded {
                    LineView {
                        kind: 4,
                        depth: u32::from(depth),
                        text: src.clone(),
                        href: src,
                    }
                } else {
                    LineView {
                        kind: 5,
                        depth: u32::from(depth),
                        text: if alt.is_empty() {
                            "[image]".to_owned()
                        } else {
                            alt
                        },
                        href: src,
                    }
                }
            }
            Block::Paragraph { text, depth } => LineView {
                kind: 0,
                depth: u32::from(depth),
                text,
                href: String::new(),
            },
            Block::Item {
                marker,
                text,
                depth,
            } => LineView {
                kind: 1,
                depth: u32::from(depth),
                text: format!("{marker} {text}"),
                href: String::new(),
            },
            Block::Link { label, href, depth } => LineView {
                kind: 2,
                depth: u32::from(depth),
                text: label,
                href,
            },
            Block::Rule => LineView {
                kind: 3,
                depth: 0,
                text: String::new(),
                href: String::new(),
            },
        })
        .collect();
    MessageView {
        id: e.id.clone(),
        sender: display_sender(e),
        to: e.recipient.clone(),
        cc: e.cc.clone().unwrap_or_default(),
        date: detail_date(&e.date),
        subject: display_subject(e),
        starred: e.starred,
        read: e.read,
        is_draft: e.folder_id == "draft",
        lines,
        attachments: e
            .attachments
            .iter()
            .map(|a| AttachmentView {
                id: a.id.clone(),
                filename: a.filename.clone(),
                size: format_bytes(a.size),
                url: String::new(),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------

struct Shared {
    api: Arc<dyn InboxApi>,
    shell: Arc<dyn Shell>,
    original_dir: PathBuf,
    state: Mutex<State>,
    notify: Arc<dyn Fn(InboxView) + Send + Sync>,
}

/// The Inbox page state and actions. Cheap to clone; clones share one state.
#[derive(Clone)]
pub struct InboxController {
    shared: Arc<Shared>,
}

/// A compose text field, its new value, and when it changed.
fn touch(c: &mut Compose, now: f64) {
    c.dirty_at = Some(now);
    c.status.clear();
}

impl InboxController {
    /// `notify` receives the new view after every state change, possibly on a worker thread. `original_dir` is where
    /// "View original" writes its files.
    pub fn new(
        api: Arc<dyn InboxApi>,
        shell: Arc<dyn Shell>,
        original_dir: PathBuf,
        notify: impl Fn(InboxView) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                api,
                shell,
                original_dir,
                state: Mutex::new(State {
                    folder: "inbox".to_owned(),
                    page: 1,
                    ..State::default()
                }),
                notify: Arc::new(notify),
            }),
        }
    }

    /// The current view, without notifying.
    pub fn view(&self) -> InboxView {
        self.lock().view()
    }

    /// Loads the configuration, the mailboxes, the first mailbox's folders and its first page.
    pub fn load(&self) {
        self.change(|s| s.loading = true);
        let me = self.clone();
        self.shared.api.config(Box::new(move |result| match result {
            Ok(config) => me.change(|s| s.domains = config.domains),
            Err(e) => me.fail("Could not load the inbox settings", e),
        }));
        self.reload_mailboxes();
    }

    /// Sends the view again. The glue calls this on the autosave timer.
    pub fn tick(&self) {
        self.tick_at(now_secs());
    }

    /// Saves the compose draft when the text has been still for [`AUTOSAVE_DELAY`] seconds at `now`.
    pub fn tick_at(&self, now: f64) {
        let due = {
            let s = self.lock();
            s.compose.as_ref().is_some_and(|c| {
                !c.saving
                    && c.dirty_at.is_some_and(|t| now - t >= AUTOSAVE_DELAY)
                    && c.has_content()
            })
        };
        if due {
            self.save_draft();
        } else {
            self.change(|_| {});
        }
    }

    pub fn dismiss_banner(&self) {
        self.change(|s| s.banner.clear());
    }

    fn reload_mailboxes(&self) {
        let me = self.clone();
        self.shared
            .api
            .mailboxes(Box::new(move |result| match result {
                Ok(list) => {
                    me.change(|s| {
                        s.loading = false;
                        let keep = s
                            .mailbox
                            .as_ref()
                            .is_some_and(|id| list.iter().any(|m| &m.id == id));
                        s.mailboxes = list;
                        if !keep {
                            s.mailbox = s.mailboxes.first().map(|m| m.id.clone());
                        }
                    });
                    me.reload_folders();
                    me.reload_list();
                }
                Err(e) => {
                    me.change(|s| s.loading = false);
                    me.fail("Could not load mailboxes", e);
                }
            }));
    }

    fn reload_folders(&self) {
        let Some(mailbox) = self.lock().mailbox.clone() else {
            return;
        };
        let me = self.clone();
        let requested = mailbox.clone();
        self.shared.api.folders(
            &mailbox,
            Box::new(move |result| match result {
                Ok(folders) => me.change(|s| {
                    if s.mailbox.as_deref() == Some(requested.as_str()) {
                        s.folders = folders;
                    }
                }),
                Err(e) => me.fail("Could not load folders", e),
            }),
        );
    }

    /// Fetches the list for the current folder, or for the search when the box has text. A response for an older
    /// request is dropped.
    fn reload_list(&self) {
        let (mailbox, seq, query, folder, page) = {
            let mut s = self.lock();
            let Some(mailbox) = s.mailbox.clone() else {
                return;
            };
            s.list_seq += 1;
            s.list_loading = true;
            s.list_error.clear();
            (
                mailbox,
                s.list_seq,
                s.query.trim().to_owned(),
                s.folder.clone(),
                s.page.max(1),
            )
        };
        self.change(|_| {});
        let me = self.clone();
        let on_done: Done<EmailPage> = Box::new(move |result| {
            me.change(|s| {
                if s.list_seq != seq {
                    return;
                }
                s.list_loading = false;
                match result {
                    Ok(p) => {
                        s.emails = p.emails;
                        s.total = p.total_count;
                    }
                    Err(e) => {
                        s.emails.clear();
                        s.total = 0;
                        s.list_error = format!("Could not load messages: {e}");
                    }
                }
            });
        });
        if query.is_empty() {
            self.shared.api.emails(&mailbox, &folder, page, on_done);
        } else {
            let params = parse_search(&query);
            self.shared.api.search(&mailbox, &params, page, on_done);
        }
    }

    /// Chooses a mailbox. Its inbox is shown.
    pub fn select_mailbox(&self, id: &str) -> bool {
        let known = self.lock().mailboxes.iter().any(|m| m.id == id);
        if !known {
            return false;
        }
        self.change(|s| {
            s.mailbox = Some(id.to_owned());
            s.folder = "inbox".to_owned();
            s.page = 1;
            s.emails.clear();
            s.total = 0;
            s.selected = None;
            s.thread.clear();
        });
        self.reload_folders();
        self.reload_list();
        true
    }

    /// Chooses a folder (by id). Clears the search and the open conversation.
    pub fn select_folder(&self, folder: &str) {
        self.change(|s| {
            s.folder = folder.to_owned();
            s.query.clear();
            s.page = 1;
            s.selected = None;
            s.thread.clear();
        });
        self.reload_list();
    }

    /// Sets the search text. An empty search goes back to the folder.
    pub fn search(&self, query: &str) {
        self.change(|s| {
            s.query = query.to_owned();
            s.page = 1;
        });
        self.reload_list();
    }

    /// Goes to a page of the list. Pages outside the range are refused.
    pub fn go_page(&self, page: u32) -> bool {
        let pages = total_pages(self.lock().total).max(1);
        if page < 1 || page > pages {
            return false;
        }
        self.change(|s| s.page = page);
        self.reload_list();
        true
    }

    /// Opens a conversation by its thread key: loads it and marks it read.
    pub fn select_thread(&self, thread_key: &str) {
        let Some(mailbox) = self.lock().mailbox.clone() else {
            return;
        };
        let seq = {
            let mut s = self.lock();
            s.selected = Some(thread_key.to_owned());
            s.thread_seq += 1;
            s.thread_loading = true;
            s.thread_error.clear();
            s.thread.clear();
            s.thread_seq
        };
        self.change(|_| {});
        let me = self.clone();
        let key = thread_key.to_owned();
        self.shared.api.thread(
            &mailbox,
            thread_key,
            Box::new(move |result| match result {
                Ok(messages) => {
                    let unread = messages.iter().any(|m| !m.read);
                    let for_images = messages.clone();
                    me.change(|s| {
                        if s.thread_seq != seq {
                            return;
                        }
                        s.thread_loading = false;
                        s.thread = messages;
                        for e in s.thread.iter_mut() {
                            e.read = true;
                        }
                        for e in s.emails.iter_mut().filter(|e| e.thread_key() == key) {
                            e.read = true;
                        }
                    });
                    me.load_images(&for_images);
                    if unread {
                        if let Some(mb) = me.lock().mailbox.clone() {
                            let fail_me = me.clone();
                            me.shared.api.mark_thread_read(
                                &mb,
                                &key,
                                Box::new(move |r| {
                                    if let Err(e) = r {
                                        fail_me.fail("Could not mark the conversation read", e);
                                    }
                                }),
                            );
                        }
                        me.reload_folders();
                    }
                }
                Err(e) => me.change(|s| {
                    if s.thread_seq == seq {
                        s.thread_loading = false;
                        s.thread_error = format!("Could not load the conversation: {e}");
                    }
                }),
            }),
        );
    }

    /// Closes the open conversation.
    pub fn clear_selection(&self) {
        self.change(|s| {
            s.selected = None;
            s.thread.clear();
            s.thread_error.clear();
        });
    }

    /// Stars or unstars a message.
    pub fn set_starred(&self, id: &str, starred: bool) {
        self.patch(
            id,
            EmailPatch {
                read: None,
                starred: Some(starred),
            },
            move |e| e.starred = starred,
        );
    }

    /// Marks a message read or unread.
    pub fn set_read(&self, id: &str, read: bool) {
        self.patch(
            id,
            EmailPatch {
                read: Some(read),
                starred: None,
            },
            move |e| e.read = read,
        );
    }

    fn patch(
        &self,
        id: &str,
        patch: EmailPatch,
        apply: impl Fn(&mut Email) + Copy + Send + 'static,
    ) {
        let Some(mailbox) = self.lock().mailbox.clone() else {
            return;
        };
        let me = self.clone();
        let id_owned = id.to_owned();
        self.shared.api.patch_email(
            &mailbox,
            id,
            &patch,
            Box::new(move |result| match result {
                Ok(()) => me.change(|s| {
                    for e in s
                        .emails
                        .iter_mut()
                        .chain(s.thread.iter_mut())
                        .filter(|e| e.id == id_owned)
                    {
                        apply(e);
                    }
                }),
                Err(e) => {
                    me.fail("Could not update the message", e);
                    me.reload_list();
                }
            }),
        );
    }

    /// Moves a message to a folder (inbox, archive, spam, trash or a custom folder id).
    pub fn move_message(&self, id: &str, folder_id: &str) {
        let Some(mailbox) = self.lock().mailbox.clone() else {
            return;
        };
        let me = self.clone();
        let moved = id.to_owned();
        self.shared.api.move_email(
            &mailbox,
            id,
            folder_id,
            Box::new(move |result| match result {
                Ok(()) => {
                    // As the web does: only the chosen message leaves the list and the conversation.
                    me.change(|s| {
                        s.emails.retain(|e| e.id != moved);
                        s.thread.retain(|e| e.id != moved);
                        if s.thread.is_empty() {
                            s.selected = None;
                        }
                    });
                    me.reload_list();
                    me.reload_folders();
                }
                Err(e) => {
                    me.fail("Could not move the message", e);
                    me.reload_list();
                }
            }),
        );
    }

    /// Opens the confirmation for deleting the current mailbox.
    pub fn ask_delete_mailbox(&self) -> bool {
        let id = self.lock().mailbox.clone();
        match id {
            Some(id) => {
                self.change(|s| s.confirm = Some(Confirm::DeleteMailbox(id)));
                true
            }
            None => false,
        }
    }

    /// Opens the confirmation for deleting a custom folder.
    pub fn ask_delete_folder(&self, folder_id: &str) {
        let id = folder_id.to_owned();
        self.change(|s| s.confirm = Some(Confirm::DeleteFolder(id)));
    }

    /// Opens the confirmation for discarding a draft.
    pub fn ask_discard_draft(&self, id: &str) {
        let id = id.to_owned();
        self.change(|s| s.confirm = Some(Confirm::DiscardDraft(id)));
    }

    /// Dismisses the confirmation without doing anything.
    pub fn confirm_no(&self) {
        self.change(|s| s.confirm = None);
    }

    /// Does the confirmed action.
    pub fn confirm_yes(&self) {
        let Some(confirm) = self.lock().confirm.take() else {
            return;
        };
        self.change(|_| {});
        match confirm {
            Confirm::DeleteMailbox(id) => {
                let me = self.clone();
                let deleted = id.clone();
                self.shared.api.delete_mailbox(
                    &id,
                    Box::new(move |result| match result {
                        Ok(()) => {
                            me.change(|s| {
                                if s.mailbox.as_deref() == Some(deleted.as_str()) {
                                    s.mailbox = None;
                                    s.emails.clear();
                                    s.thread.clear();
                                    s.selected = None;
                                }
                            });
                            me.reload_mailboxes();
                        }
                        Err(e) => me.fail("Could not delete the mailbox", e),
                    }),
                );
            }
            Confirm::DeleteFolder(id) => {
                let Some(mailbox) = self.lock().mailbox.clone() else {
                    return;
                };
                let me = self.clone();
                let deleted = id.clone();
                self.shared.api.delete_folder(
                    &mailbox,
                    &id,
                    Box::new(move |result| match result {
                        Ok(()) => {
                            me.change(|s| {
                                if s.folder == deleted {
                                    s.folder = "inbox".to_owned();
                                    s.page = 1;
                                }
                            });
                            me.reload_folders();
                            me.reload_list();
                        }
                        Err(e) => me.fail("Could not delete the folder", e),
                    }),
                );
            }
            Confirm::DiscardDraft(id) => {
                let Some(mailbox) = self.lock().mailbox.clone() else {
                    return;
                };
                let me = self.clone();
                self.shared.api.delete_email(
                    &mailbox,
                    &id,
                    Box::new(move |result| match result {
                        Ok(()) => {
                            me.change(|s| {
                                s.selected = None;
                                s.thread.clear();
                            });
                            me.reload_list();
                        }
                        Err(e) => me.fail("Could not discard the draft", e),
                    }),
                );
            }
        }
    }

    // ----- Message headers and original -----

    /// Opens the source headers of message `id`.
    pub fn open_headers(&self, id: &str) -> bool {
        let found = self.lock().find_email(id);
        match found {
            Some(email) => {
                let mut pairs = email.headers();
                if let Some(message_id) = email.message_id.clone().filter(|m| !m.is_empty()) {
                    pairs.insert(0, ("Message-ID".to_owned(), message_id));
                }
                pairs.insert(0, ("Subject".to_owned(), display_subject(&email)));
                pairs.insert(0, ("Date".to_owned(), email.date.clone()));
                pairs.insert(0, ("To".to_owned(), email.recipient.clone()));
                pairs.insert(0, ("From".to_owned(), display_sender(&email)));
                self.change(|s| s.headers = Some(pairs));
                true
            }
            None => false,
        }
    }

    pub fn close_headers(&self) {
        self.change(|s| s.headers = None);
    }

    /// The value of header row `index`, for the copy action.
    pub fn header_value(&self, index: usize) -> Option<String> {
        self.lock()
            .headers
            .as_ref()
            .and_then(|pairs| pairs.get(index))
            .map(|(_, value)| value.clone())
    }

    /// Writes the original of message `id` to a temporary file and opens it in the browser. The page keeps the
    /// text view; this is the only place the HTML is shown.
    pub fn view_original(&self, id: &str) -> bool {
        let email = self.lock().find_email(id);
        let Some(email) = email else { return false };
        let result = write_original(&self.shared.original_dir, &email);
        match result {
            Ok(path) => {
                // The plain path: ShellExecuteW opens local files from it (not from a file:/// address).
                if self.shared.shell.open(&path.to_string_lossy()) {
                    true
                } else {
                    self.change(|s| s.banner = "Could not open the browser".to_owned());
                    false
                }
            }
            Err(e) => {
                self.change(|s| s.banner = format!("Could not write the original: {e}"));
                false
            }
        }
    }

    /// Opens an attachment. A picture, text or audio/video file opens inside the app (the viewer, or the glue's
    /// media player on the temporary file). Anything else is saved where the person chooses, then opened with the
    /// system app. When the download fails (for example no session for the attachment route), the browser opens the
    /// address, which carries the browser's own session. Returns `false` when the attachment is unknown.
    pub fn open_attachment(&self, email_id: &str, attachment_id: &str) -> bool {
        let (mailbox, filename, mime, url) = {
            let s = self.lock();
            let Some(mailbox) = s.mailbox.clone() else {
                return false;
            };
            let Some(email) = s.find_email(email_id) else {
                return false;
            };
            let Some(att) = email.attachments.iter().find(|a| a.id == attachment_id) else {
                return false;
            };
            let url = self
                .shared
                .api
                .attachment_url(&mailbox, email_id, attachment_id);
            (mailbox, att.filename.clone(), att.mimetype.clone(), url)
        };
        let _ = mailbox;
        let kind = attachment_kind(&filename, &mime);
        let me = self.clone();
        let opened_url = url.clone();
        self.shared.api.fetch(
            &url,
            Box::new(move |result| match result {
                Ok(bytes) => me.attachment_bytes(&filename, kind, bytes),
                Err(e) => {
                    me.change(|s| {
                        s.banner =
                            format!("Could not download {filename}: {e}. Opened in the browser.")
                    });
                    me.shared.shell.open(&opened_url);
                }
            }),
        );
        true
    }

    /// Shows or saves downloaded attachment bytes, by kind. Runs on the download's thread.
    fn attachment_bytes(&self, filename: &str, kind: AttachmentKind, bytes: Vec<u8>) {
        match kind {
            AttachmentKind::Image => match load_bytes(&bytes) {
                Ok(picture) => self.change(|s| {
                    s.viewer = Some(Viewer::Image {
                        title: filename.to_owned(),
                        picture,
                    });
                }),
                Err(e) => self.change(|s| s.banner = format!("Could not show {filename}: {e}")),
            },
            AttachmentKind::Text => {
                let cut = bytes.len().min(MAX_TEXT_PREVIEW_BYTES);
                let text = String::from_utf8_lossy(&bytes[..cut]).into_owned();
                self.change(|s| {
                    s.viewer = Some(Viewer::Text {
                        title: filename.to_owned(),
                        text,
                    });
                });
            }
            AttachmentKind::Audio | AttachmentKind::Video => {
                let path = self.shared.original_dir.join(safe_file_name(filename));
                match write_bytes(&path, &bytes) {
                    Ok(()) => self.change(|s| {
                        s.viewer = Some(Viewer::Media {
                            title: filename.to_owned(),
                            path: path.to_string_lossy().into_owned(),
                        });
                    }),
                    Err(e) => self.change(|s| s.banner = format!("Could not save {filename}: {e}")),
                }
            }
            AttachmentKind::Other => {
                // Save where the person chooses, then open it with the system app (the plain path).
                let Some(target) = self.shared.shell.save_file(filename) else {
                    return;
                };
                match write_bytes(&target, &bytes) {
                    Ok(()) => {
                        self.shared.shell.open(&target.to_string_lossy());
                    }
                    Err(e) => self.change(|s| s.banner = format!("Could not save {filename}: {e}")),
                }
            }
        }
    }

    /// Closes the attachment viewer.
    pub fn close_viewer(&self) {
        self.change(|s| s.viewer = None);
    }

    /// Downloads the pictures of the open conversation that are not yet loaded: `data:` pictures are decoded at once,
    /// `cid:` pictures come from the message's attachments, and web addresses are fetched, as the web loads them.
    fn load_images(&self, messages: &[Email]) {
        let mailbox = match self.lock().mailbox.clone() {
            Some(m) => m,
            None => return,
        };
        for message in messages {
            for block in blocks_for(&message.body) {
                let Block::Image { src, .. } = block else {
                    continue;
                };
                let wanted = {
                    let mut s = self.lock();
                    if s.images.iter().any(|(k, _)| *k == src) || s.image_pending.contains(&src) {
                        false
                    } else {
                        s.image_pending.push(src.clone());
                        true
                    }
                };
                if !wanted {
                    continue;
                }
                if let Some(payload) = src.strip_prefix("data:") {
                    let bytes = payload
                        .split_once(";base64,")
                        .and_then(|(_, data)| local_time::decode_base64(data));
                    self.image_done(&src, bytes.and_then(|b| load_bytes(&b).ok()));
                } else if let Some(cid) = src.strip_prefix("cid:") {
                    let wanted_cid = cid.trim().trim_start_matches('<').trim_end_matches('>');
                    let attachment = message.attachments.iter().find(|a| {
                        a.content_id
                            .as_deref()
                            .map(|c| {
                                c.trim().trim_start_matches('<').trim_end_matches('>') == wanted_cid
                            })
                            .unwrap_or(false)
                    });
                    match attachment {
                        Some(att) => {
                            let url =
                                self.shared
                                    .api
                                    .attachment_url(&mailbox, &message.id, &att.id);
                            self.fetch_image(src.clone(), &url);
                        }
                        None => self.image_done(&src, None),
                    }
                } else if src.starts_with("https://") || src.starts_with("http://") {
                    self.fetch_image(src.clone(), &src);
                } else {
                    self.image_done(&src, None);
                }
            }
        }
    }

    fn fetch_image(&self, key: String, url: &str) {
        let me = self.clone();
        self.shared.api.fetch(
            url,
            Box::new(move |result| {
                let picture = match result {
                    Ok(bytes) if bytes.len() <= MAX_IMAGE_BYTES => load_bytes(&bytes).ok(),
                    _ => None,
                };
                me.image_done(&key, picture);
            }),
        );
    }

    /// Records a finished image load (a decoded picture, or `None` for one that could not be shown).
    fn image_done(&self, key: &str, picture: Option<Picture>) {
        let key = key.to_owned();
        self.change(|s| {
            s.image_pending.retain(|k| *k != key);
            if let Some(picture) = picture {
                s.images.push((key, picture));
            }
        });
    }

    // ----- Mailbox creation and settings -----

    pub fn open_create(&self) {
        self.change(|s| {
            let domain = s.domains.first().cloned().unwrap_or_default();
            s.create = Some(CreateForm {
                prefix: String::new(),
                domain,
                name: String::new(),
                creating: false,
                error: String::new(),
            });
        });
    }

    pub fn close_create(&self) {
        self.change(|s| s.create = None);
    }

    /// Field 0 is the address prefix, field 1 the display name.
    pub fn create_field(&self, field: u8, text: &str) {
        self.change(|s| {
            if let Some(form) = s.create.as_mut() {
                match field {
                    0 => form.prefix = text.to_owned(),
                    _ => form.name = text.to_owned(),
                }
            }
        });
    }

    pub fn create_domain(&self, index: usize) -> bool {
        let mut picked = false;
        self.change(|s| {
            let domain = s.domains.get(index).cloned();
            if let (Some(form), Some(domain)) = (s.create.as_mut(), domain) {
                form.domain = domain;
                picked = true;
            }
        });
        picked
    }

    /// Creates the mailbox `prefix@domain`. A blank prefix, a prefix with `@`, or no domain is refused in the form.
    pub fn submit_create(&self) {
        let (email, name) = {
            let mut s = self.lock();
            let Some(form) = s.create.as_mut() else {
                return;
            };
            let prefix = form.prefix.trim().to_owned();
            if prefix.is_empty() || prefix.contains('@') {
                form.error = "Enter the part before the @".to_owned();
                drop(s);
                self.change(|_| {});
                return;
            }
            if form.domain.is_empty() {
                form.error = "Choose a domain".to_owned();
                drop(s);
                self.change(|_| {});
                return;
            }
            form.creating = true;
            form.error.clear();
            let email = format!("{prefix}@{}", form.domain);
            (email, form.name.trim().to_owned())
        };
        self.change(|_| {});
        let me = self.clone();
        self.shared.api.create_mailbox(
            &email,
            &name,
            Box::new(move |result| match result {
                Ok(()) => {
                    me.change(|s| s.create = None);
                    me.reload_mailboxes();
                }
                Err(e) => me.change(|s| {
                    if let Some(form) = s.create.as_mut() {
                        form.creating = false;
                        form.error = e.to_string();
                    }
                }),
            }),
        );
    }

    /// Opens the settings of the current mailbox and loads them.
    pub fn open_settings(&self) -> bool {
        let Some(mailbox) = self.lock().mailbox.clone() else {
            return false;
        };
        self.change(|s| {
            s.settings = Some(SettingsForm {
                mailbox: mailbox.clone(),
                loading: true,
                from_name: String::new(),
                signature: String::new(),
                signature_enabled: false,
                saving: false,
                error: String::new(),
            });
        });
        let me = self.clone();
        let requested = mailbox.clone();
        self.shared.api.mailbox(
            &mailbox,
            Box::new(move |result| match result {
                Ok(m) => me.change(|s| {
                    if let Some(form) = s.settings.as_mut().filter(|f| f.mailbox == requested) {
                        form.loading = false;
                        let settings = m.settings.unwrap_or_default();
                        form.from_name = settings.from_name.unwrap_or_default();
                        match settings.signature {
                            Some(sig) => {
                                form.signature = sig.text;
                                form.signature_enabled = sig.enabled;
                            }
                            None => {
                                form.signature = String::new();
                                form.signature_enabled = false;
                            }
                        }
                    }
                }),
                Err(e) => me.change(|s| {
                    if let Some(form) = s.settings.as_mut() {
                        form.loading = false;
                        form.error = e.to_string();
                    }
                }),
            }),
        );
        true
    }

    pub fn close_settings(&self) {
        self.change(|s| s.settings = None);
    }

    /// Field 0 is the sender name, field 1 the signature text.
    pub fn settings_field(&self, field: u8, text: &str) {
        self.change(|s| {
            if let Some(form) = s.settings.as_mut() {
                match field {
                    0 => form.from_name = text.to_owned(),
                    _ => form.signature = text.to_owned(),
                }
            }
        });
    }

    pub fn settings_signature_enabled(&self, on: bool) {
        self.change(|s| {
            if let Some(form) = s.settings.as_mut() {
                form.signature_enabled = on;
            }
        });
    }

    /// Saves the mailbox settings (`PUT /v1/mailboxes/:id`).
    pub fn save_settings(&self) {
        let (mailbox, from_name, signature, enabled) = {
            let mut s = self.lock();
            let Some(form) = s.settings.as_mut() else {
                return;
            };
            if form.loading {
                return;
            }
            form.saving = true;
            form.error.clear();
            (
                form.mailbox.clone(),
                form.from_name.clone(),
                form.signature.clone(),
                form.signature_enabled,
            )
        };
        self.change(|_| {});
        let patch = MailboxSettingsPatch {
            settings: MailboxSettingsWrite {
                from_name: &from_name,
                signature: SignatureWrite {
                    enabled,
                    text: &signature,
                },
            },
        };
        let me = self.clone();
        self.shared.api.update_mailbox(
            &mailbox,
            &patch,
            Box::new(move |result| match result {
                Ok(()) => {
                    me.change(|s| s.settings = None);
                    me.reload_mailboxes();
                }
                Err(e) => me.change(|s| {
                    if let Some(form) = s.settings.as_mut() {
                        form.saving = false;
                        form.error = e.to_string();
                    }
                }),
            }),
        );
    }

    // ----- Folders -----

    pub fn open_folder_create(&self) {
        self.change(|s| {
            s.folder_form = Some(FolderForm {
                rename_of: None,
                name: String::new(),
                error: String::new(),
            });
        });
    }

    pub fn open_folder_rename(&self, folder_id: &str) -> bool {
        let found = self
            .lock()
            .folders
            .iter()
            .find(|f| f.id == folder_id)
            .map(folder_label);
        let Some(name) = found else { return false };
        let id = folder_id.to_owned();
        self.change(|s| {
            s.folder_form = Some(FolderForm {
                rename_of: Some(id),
                name,
                error: String::new(),
            });
        });
        true
    }

    pub fn close_folder_form(&self) {
        self.change(|s| s.folder_form = None);
    }

    pub fn folder_name(&self, text: &str) {
        self.change(|s| {
            if let Some(form) = s.folder_form.as_mut() {
                form.name = text.to_owned();
            }
        });
    }

    /// Creates the folder, or renames it when the form is a rename. Blank names are refused.
    pub fn submit_folder(&self) {
        let (mailbox, name, rename_of) = {
            let mut s = self.lock();
            let Some(mailbox) = s.mailbox.clone() else {
                return;
            };
            let Some(form) = s.folder_form.as_mut() else {
                return;
            };
            let name = form.name.trim().to_owned();
            if name.is_empty() {
                form.error = "Name the folder".to_owned();
                drop(s);
                self.change(|_| {});
                return;
            }
            (mailbox, name, form.rename_of.clone())
        };
        let me = self.clone();
        let done = Box::new(move |result: Result<Folder, ApiError>| match result {
            Ok(_) => {
                me.change(|s| s.folder_form = None);
                me.reload_folders();
            }
            Err(e) => me.change(|s| {
                if let Some(form) = s.folder_form.as_mut() {
                    form.error = e.to_string();
                }
            }),
        });
        match rename_of {
            Some(id) => self.shared.api.update_folder(&mailbox, &id, &name, done),
            None => self.shared.api.create_folder(&mailbox, &name, done),
        }
    }

    // ----- Compose -----

    fn next_title(reply: Option<&ReplyTo>, all: bool) -> String {
        match reply {
            None => "New message".to_owned(),
            Some(r) if r.forward => "Forward".to_owned(),
            Some(_) if all => "Reply all".to_owned(),
            Some(_) => "Reply".to_owned(),
        }
    }

    /// A blank new message.
    pub fn new_message(&self) {
        self.change(|s| {
            s.compose = Some(Compose {
                title: Self::next_title(None, false),
                ..Compose::default()
            });
        });
    }

    /// Replies to message `id`. Reply all also copies the other recipients (see `reply_all_cc`).
    pub fn reply(&self, id: &str, reply_all: bool) -> bool {
        let (email, mailbox_email) = {
            let s = self.lock();
            (s.find_email(id), s.mailbox_email())
        };
        let Some(email) = email else { return false };
        let cc = if reply_all {
            reply_all_cc(&email, &mailbox_email)
        } else {
            String::new()
        };
        let reply = ReplyTo {
            forward: false,
            original_id: email.id.clone(),
            thread_id: email.thread_id.clone(),
            in_reply_to: Some(email.reply_target_id().to_owned()),
        };
        self.change(|s| {
            s.compose = Some(Compose {
                title: Self::next_title(Some(&reply), reply_all),
                show_cc_bcc: !cc.is_empty(),
                to: display_sender(&email),
                cc,
                subject: compose_subject(&display_subject(&email)),
                reply: Some(reply),
                ..Compose::default()
            });
        });
        true
    }

    /// Forwards message `id`.
    pub fn forward(&self, id: &str) -> bool {
        let email = self.lock().find_email(id);
        let Some(email) = email else { return false };
        let reply = ReplyTo {
            forward: true,
            original_id: email.id.clone(),
            thread_id: email.thread_id.clone(),
            in_reply_to: Some(email.reply_target_id().to_owned()),
        };
        self.change(|s| {
            s.compose = Some(Compose {
                title: Self::next_title(Some(&reply), false),
                subject: forward_subject(&display_subject(&email)),
                reply: Some(reply),
                ..Compose::default()
            });
        });
        true
    }

    /// Opens a stored draft to send or edit.
    pub fn open_draft(&self, id: &str) -> bool {
        let email = self.lock().find_email(id);
        let Some(email) = email else { return false };
        self.change(|s| {
            s.compose = Some(Compose {
                title: "New message".to_owned(),
                to: email.recipient.clone(),
                cc: email.cc.clone().unwrap_or_default(),
                bcc: email.bcc.clone().unwrap_or_default(),
                show_cc_bcc: email.cc.as_deref().is_some_and(|c| !c.is_empty()),
                subject: display_subject(&email),
                body: email.snippet.clone(),
                draft_id: Some(email.id.clone()),
                ..Compose::default()
            });
        });
        true
    }

    /// Sets a compose field: 0 to, 1 cc, 2 bcc, 3 subject, 4 body. Starts the autosave delay.
    pub fn compose_field(&self, field: u8, text: &str) {
        let now = now_secs();
        self.change(|s| {
            if let Some(c) = s.compose.as_mut() {
                match field {
                    FIELD_TO => c.to = text.to_owned(),
                    FIELD_CC => c.cc = text.to_owned(),
                    FIELD_BCC => c.bcc = text.to_owned(),
                    FIELD_SUBJECT => c.subject = text.to_owned(),
                    _ => c.body = text.to_owned(),
                }
                touch(c, now);
            }
        });
    }

    pub fn compose_toggle_cc(&self) {
        self.change(|s| {
            if let Some(c) = s.compose.as_mut() {
                c.show_cc_bcc = !c.show_cc_bcc;
            }
        });
    }

    /// Appends a markup example to the body: `bold`, `italic`, `link` or `list`. The text is sent as HTML.
    pub fn compose_markup(&self, kind: &str) -> bool {
        let snippet = match kind {
            "bold" => "**bold text**",
            "italic" => "*italic text*",
            "link" => "[link text](https://)",
            "list" => "- item",
            _ => return false,
        };
        let now = now_secs();
        self.change(|s| {
            if let Some(c) = s.compose.as_mut() {
                if !c.body.is_empty() && !c.body.ends_with('\n') {
                    c.body.push('\n');
                }
                c.body.push_str(snippet);
                touch(c, now);
            }
        });
        true
    }

    /// Adds files from the system file dialog. Files over [`MAX_ATTACHMENT_BYTES`] are refused with a message.
    pub fn compose_attach(&self) -> usize {
        let paths = self.shared.shell.pick_files();
        let mut added = 0;
        let mut too_big = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".to_owned());
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.change(|s| s.banner = format!("Could not read {name}: {e}"));
                    continue;
                }
            };
            if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
                too_big.push(name);
                continue;
            }
            let upload = attachment_upload(&name, mime_for(&name), &bytes);
            let now = now_secs();
            self.change(|s| {
                if let Some(c) = s.compose.as_mut() {
                    c.attachment_names.push(name.clone());
                    c.attachments.push(upload);
                    touch(c, now);
                }
            });
            added += 1;
        }
        if !too_big.is_empty() {
            let text = format!("Too large (over 25 MB): {}", too_big.join(", "));
            self.change(|s| {
                if let Some(c) = s.compose.as_mut() {
                    c.error = text;
                }
            });
        }
        added
    }

    pub fn compose_remove_attachment(&self, index: usize) -> bool {
        let mut removed = false;
        self.change(|s| {
            if let Some(c) = s.compose.as_mut()
                && index < c.attachments.len()
            {
                c.attachments.remove(index);
                if index < c.attachment_names.len() {
                    c.attachment_names.remove(index);
                }
                removed = true;
            }
        });
        removed
    }

    /// Saves the compose content as a draft. Empty compose content is not saved.
    pub fn save_draft(&self) {
        let (mailbox, body) = {
            let mut s = self.lock();
            let Some(mailbox) = s.mailbox.clone() else {
                return;
            };
            let Some(c) = s.compose.as_mut() else { return };
            if !c.has_content() || c.saving {
                return;
            }
            c.saving = true;
            c.dirty_at = None;
            c.error.clear();
            let snapshot = (
                c.to.clone(),
                c.cc.clone(),
                c.bcc.clone(),
                c.subject.clone(),
                c.body.clone(),
                c.reply.clone(),
                c.draft_id.clone(),
            );
            (mailbox, snapshot)
        };
        let (to, cc, bcc, subject, text, reply, draft_id) = body;
        let html = body_html(&text);
        let draft = DraftBody {
            to: &to,
            cc: non_empty(&cc),
            bcc: non_empty(&bcc),
            subject: &subject,
            body: &html,
            in_reply_to: reply.as_ref().and_then(|r| r.in_reply_to.as_deref()),
            thread_id: reply.as_ref().and_then(|r| r.thread_id.as_deref()),
            draft_id: draft_id.as_deref(),
        };
        self.change(|_| {});
        let me = self.clone();
        self.shared.api.save_draft(
            &mailbox,
            &draft,
            Box::new(move |result| match result {
                Ok(saved) => me.change(|s| {
                    if let Some(c) = s.compose.as_mut() {
                        c.saving = false;
                        c.draft_id = Some(saved.id);
                        c.status = "Draft saved".to_owned();
                    }
                }),
                Err(e) => me.change(|s| {
                    if let Some(c) = s.compose.as_mut() {
                        c.saving = false;
                        c.error = format!("Could not save the draft: {e}");
                    }
                }),
            }),
        );
    }

    /// Sends the compose message. A stored draft is sent through its own endpoint; a reply or forward goes through
    /// the original's endpoint; anything else is a new message. A missing recipient is refused in the form.
    pub fn send(&self) {
        let (mailbox, message, draft, reply) = {
            let mut s = self.lock();
            let Some(mailbox) = s.mailbox.clone() else {
                return;
            };
            let from = s.mailbox_email();
            let Some(c) = s.compose.as_mut() else { return };
            if c.sending {
                return;
            }
            if c.to.trim().is_empty() {
                c.error = "Add a recipient".to_owned();
                drop(s);
                self.change(|_| {});
                return;
            }
            c.sending = true;
            c.error.clear();
            let html = body_html(&c.body);
            let message = SendEmail {
                to: c.to.trim().to_owned(),
                cc: non_empty(&c.cc).map(str::to_owned),
                bcc: non_empty(&c.bcc).map(str::to_owned),
                from,
                subject: c.subject.clone(),
                text: c.body.clone(),
                html: html.clone(),
                attachments: c.attachments.clone(),
                thread_id: c.reply.as_ref().and_then(|r| r.thread_id.clone()),
                in_reply_to: c.reply.as_ref().and_then(|r| r.in_reply_to.clone()),
            };
            let draft = match (&c.draft_id, &c.reply) {
                (Some(id), None) => {
                    Some((id.clone(), c.to.trim().to_owned(), c.subject.clone(), html))
                }
                _ => None,
            };
            (mailbox, message, draft, c.reply.clone())
        };
        let me = self.clone();
        let done: Done<()> = Box::new(move |result| match result {
            Ok(()) => {
                me.change(|s| s.compose = None);
                me.reload_list();
                me.reload_folders();
            }
            Err(e) => me.change(|s| {
                if let Some(c) = s.compose.as_mut() {
                    c.sending = false;
                    c.error = format!("Could not send: {e}");
                }
            }),
        });
        match (draft, reply) {
            (Some((draft_id, to, subject, html)), _) => {
                let send = DraftSend {
                    to: &to,
                    subject: &subject,
                    html: &html,
                };
                self.shared.api.send_draft(&mailbox, &draft_id, &send, done);
            }
            (None, Some(r)) if r.forward => {
                self.shared
                    .api
                    .forward(&mailbox, &r.original_id, &message, done)
            }
            (None, Some(r)) => self
                .shared
                .api
                .reply(&mailbox, &r.original_id, &message, done),
            (None, None) => self.shared.api.send_email(&mailbox, &message, done),
        }
    }

    /// Closes the compose form. A saved draft stays in Drafts.
    pub fn close_compose(&self) {
        self.change(|s| s.compose = None);
    }

    // ----- Internals -----

    fn fail(&self, what: &str, error: ApiError) {
        let text = format!("{what}: {error}");
        self.change(|s| s.banner = text);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `update` under the lock, then notifies with the new view (the lock is released first).
    fn change(&self, update: impl FnOnce(&mut State)) {
        let view = {
            let mut state = self.lock();
            update(&mut state);
            state.view()
        };
        (self.shared.notify)(view);
    }
}

fn non_empty(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(text.trim())
    }
}

#[cfg(test)]
mod tests;
