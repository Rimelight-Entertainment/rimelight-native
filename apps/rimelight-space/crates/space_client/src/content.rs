//! Typed client for the Space content API (CMS.3-CMS.5, decision 0049): content types (collections) and their templates,
//! pages in a scope, drafts (open, members, discard, publish), review of pending versions, page and draft comments, the
//! media library, and the live-room tickets that `content_live` connects with.
//!
//! Every call takes the session token. Ids, slugs and collection keys are checked to be plain path segments before a
//! request is made, so `..`, slashes, `?` and spaces are refused without a request. Route paths and JSON shapes follow
//! the website's `apps/rimelight.com/src/space/content/content-*.ts` routes. Shapes the website is still adding (the
//! scope move, collection writes, media) are marked "guessed" on their functions.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::api::{ApiClient, Method, RawBody, encode_query_value, parse};
use crate::error::ClientError;

/// The scope a page lives in. Company pages have no scope id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScopeKind {
    Project,
    Team,
    Department,
    Company,
}

/// A page's scope: its kind and the id of the project, team or department (None for Company).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageScope {
    pub kind: ScopeKind,
    pub id: Option<String>,
}

/// Who a page is visible to. Internal is staff only; Public is on the website.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Internal,
    Public,
}

/// A content type (collection) as `GET /v1/content/collections` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentCollection {
    pub slug: String,
    /// The label. The server stores it per locale (`{"en": "Blog"}`); the English one (else the first) is used here.
    #[serde(deserialize_with = "localized_text")]
    pub name: String,
    #[serde(default, deserialize_with = "optional_localized_text")]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub mode: String,
    /// The field definitions, one object per field with its `name`. The server sends the CMS field schema
    /// (`{"groups": {...}, "fields": {"<name>": {...}}}`); a plain list is read as is.
    #[serde(default, deserialize_with = "field_list")]
    pub fields: Vec<Value>,
    /// The collection's feature settings, as JSON. `labelPlural` and `routable` are kept here by the server.
    #[serde(default)]
    pub features: Value,
    /// The plural label, when the server sends it as its own field.
    #[serde(rename = "labelPlural", default)]
    pub label_plural: Option<String>,
    /// Whether entries of the collection have a public route (the mention links use it). The server sends `true`,
    /// `false` or the route pattern (a string, which means routable).
    #[serde(default, deserialize_with = "routable_flag")]
    pub routable: bool,
}

/// A label stored per locale (`{"en": "...", "pt": "..."}`) or as a plain string: the English text, else the first.
fn localized_value(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("en")
            .and_then(Value::as_str)
            .or_else(|| map.values().find_map(Value::as_str))
            .map(str::to_string),
        _ => None,
    }
}

fn localized_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(localized_value(&Value::deserialize(d)?).unwrap_or_default())
}

fn optional_localized_text<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    Ok(localized_value(&Value::deserialize(d)?))
}

/// The CMS field schema (`{"fields": {"<name>": {...}}}`) or a plain list, as one object per field carrying its `name`.
fn field_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Value>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items,
        Value::Object(schema) => match schema.get("fields") {
            Some(Value::Object(fields)) => fields
                .iter()
                .map(|(name, def)| {
                    let mut field = def.as_object().cloned().unwrap_or_default();
                    field
                        .entry("name")
                        .or_insert_with(|| Value::String(name.clone()));
                    Value::Object(field)
                })
                .collect(),
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    })
}

fn routable_flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Bool(b) => b,
        Value::String(s) => !s.is_empty(),
        _ => false,
    })
}

#[derive(Debug, Deserialize)]
struct CollectionList {
    collections: Vec<ContentCollection>,
}

/// The body of `POST /v1/content/collections` and `PATCH /v1/content/collections/:slug` (guessed: the route is being added).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectionInput {
    pub slug: String,
    pub label: String,
    #[serde(rename = "labelPlural")]
    pub label_plural: String,
    pub icon: String,
    pub color: Option<String>,
    pub features: Value,
    pub fields: Value,
}

/// A page template for one collection, from `GET /v1/content/templates?collection=` (guessed: the route is being added).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentTemplate {
    pub id: String,
    pub slug: String,
    /// A localized title (`{"en": "..."}`), kept as JSON.
    pub title: Value,
    pub collection: String,
    #[serde(rename = "initialBlocks", default)]
    pub initial_blocks: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct TemplateList {
    templates: Vec<ContentTemplate>,
}

/// A page in a list or a header (no content). `title` and `description` are localized JSON, as the server stores them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageSummary {
    pub id: String,
    pub slug: String,
    #[serde(rename = "type")]
    pub page_type: String,
    pub title: Value,
    #[serde(default)]
    pub description: Option<Value>,
    pub scope_kind: ScopeKind,
    #[serde(default)]
    pub scope_id: Option<String>,
    pub visibility: Visibility,
    pub status: String,
    #[serde(default)]
    pub published_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The caller's access on the page's scope: view, edit or manage.
    pub my_access: String,
}

/// The blocks of a page as the caller may see them (`GET /v1/content/pages/:id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageContent {
    pub blocks: Vec<Value>,
    #[serde(default)]
    pub properties: Map<String, Value>,
}

/// `GET /v1/content/pages/:id`: the summary with the published content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageDetail {
    #[serde(flatten)]
    pub page: PageSummary,
    pub content: PageContent,
}

#[derive(Debug, Deserialize)]
struct PageList {
    pages: Vec<PageSummary>,
}

/// The body of `POST /v1/content/pages`: a new Internal draft page, optionally started from a template.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatePage {
    #[serde(rename = "scopeKind")]
    pub scope_kind: ScopeKind,
    #[serde(rename = "scopeId", skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(rename = "type")]
    pub page_type: String,
    pub slug: String,
    pub title: String,
    /// A template of the page's collection to start from (`GET /v1/content/templates`). Omitted when none.
    #[serde(rename = "templateId", skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

/// The body of `PATCH /v1/content/pages/:id`. Only the set fields are sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PatchPage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

/// The body of `PATCH /v1/content/pages/:id/scope` (guessed: the route is being added). A Company move sends a null scopeId.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MovePage {
    #[serde(rename = "scopeKind")]
    pub scope_kind: ScopeKind,
    #[serde(rename = "scopeId")]
    pub scope_id: Option<String>,
}

/// `{deleted, id}`, the reply of the soft deletes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deleted {
    pub deleted: bool,
    pub id: String,
}

/// A draft: the caller's fork of a page's main document (CMS.3). `status` is Open, InReview, Published, Discarded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftView {
    pub id: String,
    pub page_id: String,
    pub author_id: String,
    pub status: String,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// The ids of the people invited to the draft (besides the author). Assumed to be plain id strings.
    pub members: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub closed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DraftList {
    drafts: Vec<DraftView>,
}

/// The status a drafts list can be filtered to (`status=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftStatusFilter {
    Open,
    InReview,
}

impl DraftStatusFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::InReview => "InReview",
        }
    }
}

/// The caller's relation to the drafts a list shows (`role=`): their own, ones they are a member of, or ones they review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftRole {
    Mine,
    Member,
    Reviewer,
}

impl DraftRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mine => "mine",
            Self::Member => "member",
            Self::Reviewer => "reviewer",
        }
    }
}

/// The page a draft is on, as a drafts list shows it. On the wire the scope is nested,
/// `{"scope": {"kind": "Project", "id": "..."}}` (website `content-drafts.ts`); it is flattened here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "DraftPageWire")]
pub struct DraftPageRef {
    pub id: String,
    /// A localized title (`{"en": "..."}`), kept as JSON.
    pub title: Value,
    pub slug: String,
    #[serde(rename = "type")]
    pub page_type: String,
    #[serde(rename = "scopeKind")]
    pub scope_kind: ScopeKind,
    #[serde(rename = "scopeId", default)]
    pub scope_id: Option<String>,
    pub visibility: Visibility,
}

#[derive(Deserialize)]
struct DraftPageWire {
    id: String,
    title: Value,
    slug: String,
    #[serde(rename = "type")]
    page_type: String,
    scope: DraftScopeWire,
    visibility: Visibility,
}

#[derive(Deserialize)]
struct DraftScopeWire {
    kind: ScopeKind,
    #[serde(default)]
    id: Option<String>,
}

impl From<DraftPageWire> for DraftPageRef {
    fn from(w: DraftPageWire) -> Self {
        Self {
            id: w.id,
            title: w.title,
            slug: w.slug,
            page_type: w.page_type,
            scope_kind: w.scope.kind,
            scope_id: w.scope.id,
            visibility: w.visibility,
        }
    }
}

/// A draft in `GET /v1/content/drafts`. `members` entries are kept as JSON until the server's shape is settled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftSummary {
    pub id: String,
    pub page: DraftPageRef,
    #[serde(rename = "authorId")]
    pub author_id: String,
    #[serde(rename = "authorName", default)]
    pub author_name: Option<String>,
    pub status: String,
    #[serde(rename = "versionId", default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub members: Vec<Value>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
struct DraftSummaryList {
    drafts: Vec<DraftSummary>,
}

/// The reply of `POST /v1/content/drafts/:id/publish`: published now, or sent to review (`in_review`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishResult {
    pub status: String,
    pub version_id: String,
    pub draft_id: String,
}

/// The reply of the approve and reject routes. Approve adds the counts while waiting, or the published draft once done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionDecision {
    /// "pending" (approval recorded, more needed), "published", or "rejected".
    pub status: String,
    pub version_id: String,
    #[serde(default)]
    pub approvals: Option<u32>,
    #[serde(default)]
    pub required: Option<u32>,
    #[serde(default)]
    pub draft: Option<DraftView>,
}

/// A comment on a page, or on a draft (`draft_id` set), optionally on a block and anchored to text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommentView {
    pub id: String,
    pub page_id: String,
    #[serde(default)]
    pub draft_id: Option<String>,
    #[serde(default)]
    pub block_id: Option<String>,
    #[serde(default)]
    pub anchor: Option<Value>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub author_id: String,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub resolved: bool,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentList {
    comments: Vec<CommentView>,
}

/// A comment change, with the fields of the editor's comment request. The app maps its request onto this; space_client
/// does not depend on the editor crate. An empty `block_id` on a submit means a page-level comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentChange<'a> {
    Submit {
        block_id: &'a str,
        parent_id: Option<&'a str>,
        body: &'a str,
    },
    Resolve {
        id: &'a str,
    },
    Reopen {
        id: &'a str,
    },
}

/// A mention search hit from `GET /v1/content/mentions` (guessed route: being added). `title` is localized JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MentionResult {
    #[serde(rename = "entryId")]
    pub entry_id: String,
    pub collection: String,
    pub slug: String,
    pub title: Value,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MentionList {
    results: Vec<MentionResult>,
}

/// The body of `POST /v1/content/pages/:id/comments`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NewComment {
    #[serde(rename = "draftId", skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    #[serde(rename = "blockId", skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Value>,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub body: String,
}

/// A media library asset (guessed: the media routes are being added). `alt` may be empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaAsset {
    pub id: String,
    pub key: String,
    pub url: String,
    #[serde(default)]
    pub alt: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    pub mime: String,
    pub size: u64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
struct MediaList {
    assets: Vec<MediaAsset>,
}

#[derive(Debug, Deserialize)]
struct MediaUploaded {
    asset: MediaAsset,
}

#[derive(Debug, Serialize)]
struct MediaAltBody<'a> {
    alt: &'a str,
}

/// A single-use ticket for a live room (`POST .../live-ticket`). `path` already carries `?ticket=`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentLiveTicket {
    pub ticket: String,
    pub path: String,
}

/// Joins `key=value` pairs into a query string with a leading `?`, or an empty string when there are none.
fn join_query(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in pairs {
        out.push(if out.is_empty() { '?' } else { '&' });
        out.push_str(key);
        out.push('=');
        out.push_str(&encode_query_value(value));
    }
    out
}

/// An id, slug or collection key as a path segment: non-empty ASCII letters, digits, dashes and underscores.
fn segment<'a>(value: &'a str, what: &str) -> Result<&'a str, ClientError> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Ok(value)
    } else {
        Err(ClientError::Protocol(format!(
            "not a valid {what}: {value}"
        )))
    }
}

fn to_json<T: Serialize>(body: &T) -> Result<String, ClientError> {
    serde_json::to_string(body).map_err(|e| ClientError::Protocol(e.to_string()))
}

/// Builds a `multipart/form-data` body with one file part (`file`) and optional text parts. Returns the body and its
/// boundary-bearing content type.
fn multipart_body(
    file_name: &str,
    mime: &str,
    bytes: &[u8],
    fields: &[(&str, &str)],
) -> (Vec<u8>, String) {
    use std::io::Write as _;
    let boundary = "rimelight-space-form-7f3a9c2e";
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 512);
    let safe_name = file_name.replace(['"', '\r', '\n'], "_");
    let _ = write!(
        out,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: {mime}\r\n\r\n"
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    for (name, value) in fields {
        let _ = write!(
            out,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        );
    }
    let _ = write!(out, "--{boundary}--\r\n");
    (out, format!("multipart/form-data; boundary={boundary}"))
}

impl ApiClient {
    /// `GET /v1/content/collections`: the content types pages can be created with.
    pub async fn content_collections(
        &self,
        token: &str,
    ) -> Result<Vec<ContentCollection>, ClientError> {
        let reply = self
            .send(Method::Get, "/v1/content/collections", Some(token), None)
            .await?;
        let list: CollectionList = parse(&reply.body)?;
        Ok(list.collections)
    }

    /// `POST /v1/content/collections` (guessed reply: the stored collection as JSON).
    pub async fn create_content_collection(
        &self,
        token: &str,
        input: &CollectionInput,
    ) -> Result<Value, ClientError> {
        let reply = self
            .send(
                Method::Post,
                "/v1/content/collections",
                Some(token),
                Some(to_json(input)?),
            )
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/content/collections/:slug` (guessed reply: the stored collection as JSON).
    pub async fn update_content_collection(
        &self,
        token: &str,
        slug: &str,
        input: &CollectionInput,
    ) -> Result<Value, ClientError> {
        let path = format!("/v1/content/collections/{}", segment(slug, "collection")?);
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(input)?))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/content/collections/:slug` (guessed: the reply body is ignored).
    pub async fn delete_content_collection(
        &self,
        token: &str,
        slug: &str,
    ) -> Result<(), ClientError> {
        let path = format!("/v1/content/collections/{}", segment(slug, "collection")?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `GET /v1/content/templates?collection=`: the templates a page of one collection can start from (guessed route).
    pub async fn content_templates(
        &self,
        token: &str,
        collection: &str,
    ) -> Result<Vec<ContentTemplate>, ClientError> {
        let query = join_query(&[("collection", collection)]);
        let path = format!("/v1/content/templates{query}");
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: TemplateList = parse(&reply.body)?;
        Ok(list.templates)
    }

    /// `GET /v1/content/pages?scopeKind=&scopeId=&type=`: the pages in a scope the caller can view, optionally of one type.
    pub async fn content_pages(
        &self,
        token: &str,
        scope: &PageScope,
        page_type: Option<&str>,
    ) -> Result<Vec<PageSummary>, ClientError> {
        let kind = match scope.kind {
            ScopeKind::Project => "Project",
            ScopeKind::Team => "Team",
            ScopeKind::Department => "Department",
            ScopeKind::Company => "Company",
        };
        let mut pairs: Vec<(&str, &str)> = vec![("scopeKind", kind)];
        if let Some(id) = scope.id.as_deref() {
            pairs.push(("scopeId", id));
        }
        if let Some(t) = page_type {
            pairs.push(("type", t));
        }
        let path = format!("/v1/content/pages{}", join_query(&pairs));
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: PageList = parse(&reply.body)?;
        Ok(list.pages)
    }

    /// `POST /v1/content/pages`: a new empty Internal draft page. Returns it as stored.
    pub async fn create_content_page(
        &self,
        token: &str,
        body: &CreatePage,
    ) -> Result<PageSummary, ClientError> {
        let reply = self
            .send(
                Method::Post,
                "/v1/content/pages",
                Some(token),
                Some(to_json(body)?),
            )
            .await?;
        parse(&reply.body)
    }

    /// `GET /v1/content/pages/:id`: the page with the blocks the caller may see.
    pub async fn content_page(&self, token: &str, id: &str) -> Result<PageDetail, ClientError> {
        let path = format!("/v1/content/pages/{}", segment(id, "page id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/content/pages/:id` {visibility?, slug?}. Manage only. Returns the page summary.
    pub async fn update_content_page(
        &self,
        token: &str,
        id: &str,
        body: &PatchPage,
    ) -> Result<PageSummary, ClientError> {
        let path = format!("/v1/content/pages/{}", segment(id, "page id")?);
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/content/pages/:id`: soft delete. Manage only.
    pub async fn delete_content_page(&self, token: &str, id: &str) -> Result<Deleted, ClientError> {
        let path = format!("/v1/content/pages/{}", segment(id, "page id")?);
        let reply = self.send(Method::Delete, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/content/pages/:id/scope` {scopeKind, scopeId} (guessed reply: the page summary). Manage only.
    pub async fn move_content_page(
        &self,
        token: &str,
        id: &str,
        body: &MovePage,
    ) -> Result<PageSummary, ClientError> {
        let path = format!("/v1/content/pages/{}/scope", segment(id, "page id")?);
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/pages/:id/drafts`: the caller's open draft on the page, created if there is none.
    pub async fn open_content_draft(
        &self,
        token: &str,
        page_id: &str,
    ) -> Result<DraftView, ClientError> {
        let path = format!("/v1/content/pages/{}/drafts", segment(page_id, "page id")?);
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/content/pages/:id/drafts`: the open drafts on the page that the caller can open.
    pub async fn content_drafts(
        &self,
        token: &str,
        page_id: &str,
    ) -> Result<Vec<DraftView>, ClientError> {
        let path = format!("/v1/content/pages/{}/drafts", segment(page_id, "page id")?);
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: DraftList = parse(&reply.body)?;
        Ok(list.drafts)
    }

    /// `GET /v1/content/drafts?status=&role=`: the drafts across pages that the caller relates to by `role`, optionally
    /// filtered by status. Both filters are optional and sent only when set.
    pub async fn list_content_drafts(
        &self,
        token: &str,
        status: Option<DraftStatusFilter>,
        role: Option<DraftRole>,
    ) -> Result<Vec<DraftSummary>, ClientError> {
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = status {
            pairs.push(("status", s.as_str()));
        }
        if let Some(r) = role {
            pairs.push(("role", r.as_str()));
        }
        let path = format!("/v1/content/drafts{}", join_query(&pairs));
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: DraftSummaryList = parse(&reply.body)?;
        Ok(list.drafts)
    }

    /// `POST /v1/content/drafts/:id/members` {userId}. Author only.
    pub async fn add_content_draft_member(
        &self,
        token: &str,
        draft_id: &str,
        user_id: &str,
    ) -> Result<DraftView, ClientError> {
        let path = format!(
            "/v1/content/drafts/{}/members",
            segment(draft_id, "draft id")?
        );
        let body = serde_json::json!({ "userId": user_id });
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/content/drafts/:id/members/:userId`. Author only.
    pub async fn remove_content_draft_member(
        &self,
        token: &str,
        draft_id: &str,
        user_id: &str,
    ) -> Result<DraftView, ClientError> {
        let path = format!(
            "/v1/content/drafts/{}/members/{}",
            segment(draft_id, "draft id")?,
            segment(user_id, "user id")?
        );
        let reply = self.send(Method::Delete, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/drafts/:id/discard`: closes the draft without publishing. Author only.
    pub async fn discard_content_draft(
        &self,
        token: &str,
        draft_id: &str,
    ) -> Result<DraftView, ClientError> {
        let path = format!(
            "/v1/content/drafts/{}/discard",
            segment(draft_id, "draft id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/drafts/:id/publish` {summary?}: publishes, or sends to review when the template has approval rules.
    pub async fn publish_content_draft(
        &self,
        token: &str,
        draft_id: &str,
        summary: Option<&str>,
    ) -> Result<PublishResult, ClientError> {
        let path = format!(
            "/v1/content/drafts/{}/publish",
            segment(draft_id, "draft id")?
        );
        let body = match summary {
            Some(text) => serde_json::json!({ "summary": text }),
            None => serde_json::json!({}),
        };
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/versions/:id/approve`: records the caller's approval. Manage only.
    pub async fn approve_content_version(
        &self,
        token: &str,
        version_id: &str,
    ) -> Result<VersionDecision, ClientError> {
        let path = format!(
            "/v1/content/versions/{}/approve",
            segment(version_id, "version id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/versions/:id/reject`: rejects the pending version; its draft goes back to Open. Manage only.
    pub async fn reject_content_version(
        &self,
        token: &str,
        version_id: &str,
    ) -> Result<VersionDecision, ClientError> {
        let path = format!(
            "/v1/content/versions/{}/reject",
            segment(version_id, "version id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `GET /v1/content/pages/:id/comments`.
    pub async fn content_comments(
        &self,
        token: &str,
        page_id: &str,
    ) -> Result<Vec<CommentView>, ClientError> {
        let path = format!(
            "/v1/content/pages/{}/comments",
            segment(page_id, "page id")?
        );
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: CommentList = parse(&reply.body)?;
        Ok(list.comments)
    }

    /// `POST /v1/content/pages/:id/comments`: returns the comment as stored.
    pub async fn create_content_comment(
        &self,
        token: &str,
        page_id: &str,
        body: &NewComment,
    ) -> Result<CommentView, ClientError> {
        let path = format!(
            "/v1/content/pages/{}/comments",
            segment(page_id, "page id")?
        );
        let reply = self
            .send(Method::Post, &path, Some(token), Some(to_json(body)?))
            .await?;
        parse(&reply.body)
    }

    /// `PATCH /v1/content/comments/:id` {body}. The author only.
    pub async fn edit_content_comment(
        &self,
        token: &str,
        id: &str,
        text: &str,
    ) -> Result<CommentView, ClientError> {
        let path = format!("/v1/content/comments/{}", segment(id, "comment id")?);
        let body = serde_json::json!({ "body": text });
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(&body)?))
            .await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/comments/:id/resolve`.
    pub async fn resolve_content_comment(
        &self,
        token: &str,
        id: &str,
    ) -> Result<CommentView, ClientError> {
        let path = format!(
            "/v1/content/comments/{}/resolve",
            segment(id, "comment id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/comments/:id/reopen`.
    pub async fn reopen_content_comment(
        &self,
        token: &str,
        id: &str,
    ) -> Result<CommentView, ClientError> {
        let path = format!("/v1/content/comments/{}/reopen", segment(id, "comment id")?);
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `DELETE /v1/content/comments/:id`.
    pub async fn delete_content_comment(
        &self,
        token: &str,
        id: &str,
    ) -> Result<Deleted, ClientError> {
        let path = format!("/v1/content/comments/{}", segment(id, "comment id")?);
        let reply = self.send(Method::Delete, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// Dispatches one comment change from the editor to its route: submit creates a comment on the page (or on
    /// `draft_id`), resolve and reopen return the comment as stored. Replies pass `parent_id`. Edits and deletes are
    /// separate calls (`edit_content_comment`, `delete_content_comment`).
    pub async fn apply_comment_request(
        &self,
        token: &str,
        page_id: &str,
        draft_id: Option<&str>,
        change: CommentChange<'_>,
    ) -> Result<CommentView, ClientError> {
        match change {
            CommentChange::Submit {
                block_id,
                parent_id,
                body,
            } => {
                let new = NewComment {
                    draft_id: draft_id.map(str::to_string),
                    block_id: (!block_id.is_empty()).then(|| block_id.to_string()),
                    anchor: None,
                    parent_id: parent_id.map(str::to_string),
                    body: body.to_string(),
                };
                self.create_content_comment(token, page_id, &new).await
            }
            CommentChange::Resolve { id } => self.resolve_content_comment(token, id).await,
            CommentChange::Reopen { id } => self.reopen_content_comment(token, id).await,
        }
    }

    /// `GET /v1/content/mentions?q=&limit=`: entries whose title matches `query`, for the `@` picker. `limit` is sent
    /// only when set (guessed route: being added).
    pub async fn content_mentions(
        &self,
        token: &str,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<MentionResult>, ClientError> {
        let limit_text = limit.map(|l| l.to_string());
        let mut pairs: Vec<(&str, &str)> = vec![("q", query)];
        if let Some(text) = limit_text.as_deref() {
            pairs.push(("limit", text));
        }
        let path = format!("/v1/content/mentions{}", join_query(&pairs));
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: MentionList = parse(&reply.body)?;
        Ok(list.results)
    }

    /// `GET /v1/content/media?folder=`: the media library, optionally one folder (guessed route).
    pub async fn content_media(
        &self,
        token: &str,
        folder: Option<&str>,
    ) -> Result<Vec<MediaAsset>, ClientError> {
        let pairs: Vec<(&str, &str)> = folder.map(|f| vec![("folder", f)]).unwrap_or_default();
        let path = format!("/v1/content/media{}", join_query(&pairs));
        let reply = self.send(Method::Get, &path, Some(token), None).await?;
        let list: MediaList = parse(&reply.body)?;
        Ok(list.assets)
    }

    /// `POST /v1/content/media`: multipart with the file in `file` and optional `alt` and `folder` (guessed route).
    pub async fn upload_content_media(
        &self,
        token: &str,
        file_name: &str,
        mime: &str,
        bytes: Vec<u8>,
        alt: Option<&str>,
        folder: Option<&str>,
    ) -> Result<MediaAsset, ClientError> {
        let mut fields: Vec<(&str, &str)> = Vec::new();
        if let Some(a) = alt {
            fields.push(("alt", a));
        }
        if let Some(f) = folder {
            fields.push(("folder", f));
        }
        let (body, content_type) = multipart_body(file_name, mime, &bytes, &fields);
        let raw = RawBody {
            content_type,
            bytes: body,
            file_name: None,
        };
        let reply = self
            .send_raw(Method::Post, "/v1/content/media", Some(token), raw)
            .await?;
        let uploaded: MediaUploaded = parse(&reply.body)?;
        Ok(uploaded.asset)
    }

    /// `PATCH /v1/content/media/:id` {alt} (guessed route).
    pub async fn update_content_media_alt(
        &self,
        token: &str,
        id: &str,
        alt: &str,
    ) -> Result<MediaAsset, ClientError> {
        let path = format!("/v1/content/media/{}", segment(id, "media id")?);
        let body = MediaAltBody { alt };
        let reply = self
            .send(Method::Patch, &path, Some(token), Some(to_json(&body)?))
            .await?;
        let uploaded: MediaUploaded = parse(&reply.body)?;
        Ok(uploaded.asset)
    }

    /// `DELETE /v1/content/media/:id` (guessed: the reply body is ignored).
    pub async fn delete_content_media(&self, token: &str, id: &str) -> Result<(), ClientError> {
        let path = format!("/v1/content/media/{}", segment(id, "media id")?);
        self.send(Method::Delete, &path, Some(token), None).await?;
        Ok(())
    }

    /// `POST /v1/content/drafts/:id/live-ticket`: a single-use ticket for the draft's live room (author or member, Open).
    pub async fn content_draft_live_ticket(
        &self,
        token: &str,
        draft_id: &str,
    ) -> Result<ContentLiveTicket, ClientError> {
        let path = format!(
            "/v1/content/drafts/{}/live-ticket",
            segment(draft_id, "draft id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }

    /// `POST /v1/content/pages/:id/live-ticket`: a single-use ticket for the page's main room (read-only for the socket).
    pub async fn content_page_live_ticket(
        &self,
        token: &str,
        page_id: &str,
    ) -> Result<ContentLiveTicket, ClientError> {
        let path = format!(
            "/v1/content/pages/{}/live-ticket",
            segment(page_id, "page id")?
        );
        let reply = self.send(Method::Post, &path, Some(token), None).await?;
        parse(&reply.body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::api::{HttpResponse, Method};
    use crate::testing::{ScriptedTransport, json_response};

    fn client(
        replies: Vec<Result<HttpResponse, ClientError>>,
    ) -> (ApiClient, Arc<ScriptedTransport>) {
        let transport = Arc::new(ScriptedTransport::new(replies));
        let api = ApiClient::with_transport("https://api.test/", transport.clone());
        (api, transport)
    }

    const PAGE: &str = r#"{"id":"p1","slug":"intro","type":"article","title":{"en":"Intro"},"description":null,"scope_kind":"Company","scope_id":null,"visibility":"Internal","status":"draft","published_version_id":null,"created_at":"2026-11-01T00:00:00.000Z","updated_at":"2026-11-02T00:00:00.000Z","my_access":"edit"}"#;

    #[test]
    fn pages_list_sends_scope_and_type_and_parses_summaries() {
        let body = format!(r#"{{"pages":[{PAGE}]}}"#);
        let (api, transport) = client(vec![json_response(200, &body)]);
        let scope = PageScope {
            kind: ScopeKind::Company,
            id: None,
        };
        let pages =
            pollster::block_on(api.content_pages("tok", &scope, Some("article"))).expect("pages");
        assert_eq!(pages[0].scope_kind, ScopeKind::Company);
        assert_eq!(pages[0].visibility, Visibility::Internal);
        assert_eq!(pages[0].page_type, "article");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Get);
        assert_eq!(
            sent.url,
            "https://api.test/v1/content/pages?scopeKind=Company&type=article"
        );
    }

    #[test]
    fn create_page_posts_the_scope_and_omits_a_missing_scope_id() {
        let (api, transport) = client(vec![json_response(201, PAGE)]);
        let body = CreatePage {
            scope_kind: ScopeKind::Company,
            scope_id: None,
            page_type: "article".into(),
            slug: "intro".into(),
            title: "Intro".into(),
            template_id: None,
        };
        pollster::block_on(api.create_content_page("tok", &body)).expect("created");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Post);
        assert_eq!(
            sent.json_body.as_deref(),
            Some(r#"{"scopeKind":"Company","type":"article","slug":"intro","title":"Intro"}"#)
        );
    }

    #[test]
    fn patch_page_sends_only_the_set_fields() {
        let (api, transport) = client(vec![json_response(200, PAGE)]);
        let patch = PatchPage {
            visibility: Some(Visibility::Public),
            slug: None,
        };
        pollster::block_on(api.update_content_page("tok", "p1", &patch)).expect("patched");
        let sent = &transport.requests()[0];
        assert_eq!(sent.method, Method::Patch);
        assert_eq!(
            sent.json_body.as_deref(),
            Some(r#"{"visibility":"Public"}"#)
        );
    }

    #[test]
    fn move_scope_sends_null_for_company() {
        let (api, transport) = client(vec![json_response(200, PAGE)]);
        let body = MovePage {
            scope_kind: ScopeKind::Company,
            scope_id: None,
        };
        pollster::block_on(api.move_content_page("tok", "p1", &body)).expect("moved");
        let sent = &transport.requests()[0];
        assert_eq!(sent.url, "https://api.test/v1/content/pages/p1/scope");
        assert_eq!(
            sent.json_body.as_deref(),
            Some(r#"{"scopeKind":"Company","scopeId":null}"#)
        );
    }

    #[test]
    fn publish_sends_the_summary_only_when_given() {
        let reply = r#"{"status":"published","version_id":"v1","draft_id":"d1"}"#;
        let (api, transport) = client(vec![json_response(201, reply), json_response(201, reply)]);
        let result =
            pollster::block_on(api.publish_content_draft("tok", "d1", Some("Fix"))).expect("ok");
        assert_eq!(result.status, "published");
        pollster::block_on(api.publish_content_draft("tok", "d1", None)).expect("ok");
        let sent = transport.requests();
        assert_eq!(sent[0].json_body.as_deref(), Some(r#"{"summary":"Fix"}"#));
        assert_eq!(sent[1].json_body.as_deref(), Some("{}"));
    }

    #[test]
    fn approve_reads_the_waiting_counts() {
        let reply = r#"{"status":"pending","approvals":1,"required":2,"version_id":"v1"}"#;
        let (api, _transport) = client(vec![json_response(200, reply)]);
        let decision = pollster::block_on(api.approve_content_version("tok", "v1")).expect("ok");
        assert_eq!(decision.approvals, Some(1));
        assert_eq!(decision.required, Some(2));
        assert!(decision.draft.is_none());
    }

    #[test]
    fn comments_list_parses_resolved_state() {
        let reply = r#"{"comments":[{"id":"c1","page_id":"p1","draft_id":null,"block_id":"b1","anchor":null,"parent_id":null,"author_id":"u1","author_name":"Ana","body":"Hi","created_at":"x","updated_at":"x","resolved":true,"resolved_at":"y","resolved_by":"u2"}]}"#;
        let (api, _transport) = client(vec![json_response(200, reply)]);
        let comments = pollster::block_on(api.content_comments("tok", "p1")).expect("comments");
        assert!(comments[0].resolved);
        assert_eq!(comments[0].resolved_by.as_deref(), Some("u2"));
    }

    #[test]
    fn new_comment_omits_unset_fields() {
        let (api, transport) = client(vec![json_response(201, "{}")]);
        let _ = pollster::block_on(api.create_content_comment(
            "tok",
            "p1",
            &NewComment {
                body: "Looks good".into(),
                ..NewComment::default()
            },
        ));
        let sent = &transport.requests()[0];
        assert_eq!(sent.json_body.as_deref(), Some(r#"{"body":"Looks good"}"#));
    }

    #[test]
    fn multipart_upload_carries_file_and_fields() {
        let (api, transport) = client(vec![json_response(
            201,
            r#"{"asset":{"id":"m1","key":"k","url":"u","alt":"Logo","width":10,"height":20,"mime":"image/png","size":5,"createdAt":"z"}}"#,
        )]);
        let asset = pollster::block_on(api.upload_content_media(
            "tok",
            "logo.png",
            "image/png",
            b"hello".to_vec(),
            Some("Logo"),
            None,
        ))
        .expect("uploaded");
        assert_eq!(asset.alt.as_deref(), Some("Logo"));
        let sent = &transport.requests()[0];
        let raw = sent.raw_body.as_ref().expect("raw body");
        assert!(
            raw.content_type
                .starts_with("multipart/form-data; boundary=")
        );
        let text = String::from_utf8_lossy(&raw.bytes);
        assert!(text.contains("name=\"file\"; filename=\"logo.png\""));
        assert!(text.contains("name=\"alt\"\r\n\r\nLogo"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn ids_with_path_characters_are_refused_without_a_request() {
        let (api, transport) = client(vec![]);
        assert!(pollster::block_on(api.content_page("tok", "../x")).is_err());
        assert!(pollster::block_on(api.publish_content_draft("tok", "d/1", None)).is_err());
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn live_ticket_reads_the_path_with_the_ticket() {
        let (api, transport) = client(vec![json_response(
            200,
            r#"{"ticket":"t","expires_in_s":60,"path":"/v1/content/live/draft%3Ad1?ticket=t"}"#,
        )]);
        let ticket =
            pollster::block_on(api.content_draft_live_ticket("tok", "d1")).expect("ticket");
        assert_eq!(ticket.ticket, "t");
        assert_eq!(ticket.path, "/v1/content/live/draft%3Ad1?ticket=t");
        assert_eq!(
            transport.requests()[0].url,
            "https://api.test/v1/content/drafts/d1/live-ticket"
        );
    }

    /// The shape the live server sends (owner hit "invalid type: map, expected a string" on the Knowledge Base):
    /// a per-locale name, the CMS field schema object, and a route pattern for `routable`.
    #[test]
    fn collection_parses_the_live_server_shape() {
        let body = r##"{"slug":"wiki","name":{"en":"Wiki","pt":"Wiki"},"description":{"en":"Lore"},"icon":"book","color":"#0af","mode":"document","features":{"routable":"/wiki/{slug}"},"fields":{"groups":{},"fields":{"era":{"type":"text","label":"Era"}}},"labelPlural":"Wiki","routable":"/wiki/{slug}","routablePath":"/wiki/{slug}"}"##;
        let c: ContentCollection = serde_json::from_str(body).expect("parses");
        assert_eq!(c.name, "Wiki");
        assert_eq!(c.description.as_deref(), Some("Lore"));
        assert!(c.routable);
        assert_eq!(c.fields.len(), 1);
        assert_eq!(c.fields[0]["name"], "era");
        assert_eq!(c.fields[0]["type"], "text");
    }

    #[test]
    fn collection_parses_the_new_fields_and_tolerates_their_absence() {
        let full = r##"{"collections":[{"slug":"news","name":"News","description":null,"icon":"Newspaper","color":"#ff0","mode":"Collection","fields":[{"key":"date","kind":"date"}],"features":{"labelPlural":"News items","routable":true},"labelPlural":"News items","routable":true,"extra":"ignored"}]}"##;
        let (api, _transport) = client(vec![json_response(200, full)]);
        let list = pollster::block_on(api.content_collections("tok")).expect("collections");
        assert_eq!(list[0].fields.len(), 1);
        assert_eq!(list[0].features["labelPlural"], "News items");
        assert_eq!(list[0].label_plural.as_deref(), Some("News items"));
        assert!(list[0].routable);

        let old = r#"{"collections":[{"slug":"news","name":"News","icon":"Newspaper","mode":"Collection"}]}"#;
        let (api, _transport) = client(vec![json_response(200, old)]);
        let list = pollster::block_on(api.content_collections("tok")).expect("collections");
        assert!(list[0].fields.is_empty());
        assert!(list[0].features.is_null());
        assert!(!list[0].routable);
    }

    #[test]
    fn create_page_sends_the_template_only_when_set() {
        let (api, transport) = client(vec![json_response(201, PAGE), json_response(201, PAGE)]);
        let body = CreatePage {
            scope_kind: ScopeKind::Company,
            scope_id: None,
            page_type: "article".into(),
            slug: "intro".into(),
            title: "Intro".into(),
            template_id: Some("t1".into()),
        };
        pollster::block_on(api.create_content_page("tok", &body)).expect("created");
        let plain = CreatePage {
            template_id: None,
            ..body.clone()
        };
        pollster::block_on(api.create_content_page("tok", &plain)).expect("created");
        let sent = transport.requests();
        assert_eq!(
            sent[0].json_body.as_deref(),
            Some(
                r#"{"scopeKind":"Company","type":"article","slug":"intro","title":"Intro","templateId":"t1"}"#
            )
        );
        assert!(
            !sent[1]
                .json_body
                .as_deref()
                .unwrap_or("")
                .contains("templateId")
        );
    }

    #[test]
    fn drafts_list_sends_both_filters_and_parses_the_nested_page() {
        let reply = r#"{"drafts":[{"id":"d1","page":{"id":"p1","title":{"en":"Intro"},"slug":"intro","type":"article","scope":{"kind":"Company","id":null},"visibility":"Internal"},"authorId":"u1","authorName":"Ana","status":"InReview","versionId":"v1","members":[],"updatedAt":"2026-11-02T00:00:00.000Z"}]}"#;
        let (api, transport) = client(vec![json_response(200, reply)]);
        let drafts = pollster::block_on(api.list_content_drafts(
            "tok",
            Some(DraftStatusFilter::InReview),
            Some(DraftRole::Reviewer),
        ))
        .expect("drafts");
        assert_eq!(drafts[0].page.scope_kind, ScopeKind::Company);
        assert_eq!(drafts[0].page.visibility, Visibility::Internal);
        assert_eq!(drafts[0].page.page_type, "article");
        assert_eq!(drafts[0].author_name.as_deref(), Some("Ana"));
        assert_eq!(drafts[0].version_id.as_deref(), Some("v1"));
        assert_eq!(
            transport.requests()[0].url,
            "https://api.test/v1/content/drafts?status=InReview&role=reviewer"
        );
    }

    #[test]
    fn drafts_list_without_filters_sends_no_query() {
        let (api, transport) = client(vec![json_response(200, r#"{"drafts":[]}"#)]);
        let drafts =
            pollster::block_on(api.list_content_drafts("tok", None, None)).expect("drafts");
        assert!(drafts.is_empty());
        assert_eq!(
            transport.requests()[0].url,
            "https://api.test/v1/content/drafts"
        );
    }

    #[test]
    fn mentions_send_the_query_and_limit_and_parse_results() {
        let reply = r##"{"results":[{"entryId":"e1","collection":"people","slug":"ana","title":{"en":"Ana"},"color":"#0af"}]}"##;
        let (api, transport) = client(vec![json_response(200, reply), json_response(200, reply)]);
        let hits =
            pollster::block_on(api.content_mentions("tok", "ana", Some(5))).expect("mentions");
        assert_eq!(hits[0].entry_id, "e1");
        assert_eq!(hits[0].color.as_deref(), Some("#0af"));
        pollster::block_on(api.content_mentions("tok", "ana", None)).expect("mentions");
        let sent = transport.requests();
        assert_eq!(
            sent[0].url,
            "https://api.test/v1/content/mentions?q=ana&limit=5"
        );
        assert_eq!(sent[1].url, "https://api.test/v1/content/mentions?q=ana");
    }

    #[test]
    fn comment_change_submit_posts_a_reply_on_a_draft() {
        let (api, transport) = client(vec![json_response(201, COMMENT)]);
        let change = CommentChange::Submit {
            block_id: "b1",
            parent_id: Some("c0"),
            body: "Hi",
        };
        pollster::block_on(api.apply_comment_request("tok", "p1", Some("d1"), change)).expect("ok");
        let sent = &transport.requests()[0];
        assert_eq!(sent.url, "https://api.test/v1/content/pages/p1/comments");
        assert_eq!(
            sent.json_body.as_deref(),
            Some(r#"{"draftId":"d1","blockId":"b1","parentId":"c0","body":"Hi"}"#)
        );
    }

    #[test]
    fn comment_change_submit_without_a_block_is_a_page_comment() {
        let (api, transport) = client(vec![json_response(201, COMMENT)]);
        let change = CommentChange::Submit {
            block_id: "",
            parent_id: None,
            body: "Page note",
        };
        pollster::block_on(api.apply_comment_request("tok", "p1", None, change)).expect("ok");
        assert_eq!(
            transport.requests()[0].json_body.as_deref(),
            Some(r#"{"body":"Page note"}"#)
        );
    }

    #[test]
    fn comment_change_resolve_and_reopen_hit_their_routes() {
        let (api, transport) = client(vec![
            json_response(200, COMMENT),
            json_response(200, COMMENT),
        ]);
        pollster::block_on(api.apply_comment_request(
            "tok",
            "p1",
            None,
            CommentChange::Resolve { id: "c1" },
        ))
        .expect("resolved");
        pollster::block_on(api.apply_comment_request(
            "tok",
            "p1",
            None,
            CommentChange::Reopen { id: "c1" },
        ))
        .expect("reopened");
        let sent = transport.requests();
        assert_eq!(
            sent[0].url,
            "https://api.test/v1/content/comments/c1/resolve"
        );
        assert_eq!(
            sent[1].url,
            "https://api.test/v1/content/comments/c1/reopen"
        );
    }

    const COMMENT: &str = r#"{"id":"c1","page_id":"p1","draft_id":null,"block_id":null,"anchor":null,"parent_id":null,"author_id":"u1","author_name":"Ana","body":"Hi","created_at":"x","updated_at":"x","resolved":false,"resolved_at":null,"resolved_by":null}"#;
}
