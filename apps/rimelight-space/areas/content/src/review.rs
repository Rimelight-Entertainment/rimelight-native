//! Drafts and reviews (CMS.6): the Drafts tab of a scope home (my drafts and the drafts I was invited to), and the reviews page
//! (every version waiting for review, across the scopes the caller can see). Approve and reject are here too.
//!
//! Both lists come from `GET /v1/content/drafts` (`list_content_drafts`): one call per filter, not one per page. The Drafts tab
//! asks for the caller's drafts and the drafts they are a member of; the reviews page asks for the drafts in review.

use std::collections::HashSet;

use space_client::api::ApiClient;
use space_client::content::{DraftRole, DraftStatusFilter, DraftSummary, NewComment};
use space_client::error::ClientError;

use crate::scope::Scope;
use crate::view::{FALLBACK_LOCALE, localized, short_time, status_label};

/// One row of the Drafts tab.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftRow {
    pub draft_id: String,
    pub page_id: String,
    pub page_title: String,
    pub status: &'static str,
    pub in_review: bool,
    pub version_id: String,
    pub mine: bool,
    pub updated: String,
}

/// One row of the reviews page.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewRow {
    pub version_id: String,
    pub draft_id: String,
    pub page_id: String,
    pub page_title: String,
    pub scope_label: String,
    pub author_id: String,
    pub updated: String,
}

/// Whether a draft is still open for work (Open, or in review). Published and discarded drafts are closed.
fn is_open(draft: &DraftSummary) -> bool {
    matches!(draft.status.as_str(), "Open" | "InReview")
}

/// Maps a draft from the list to a row. `page_title` is the page's title in `locale`.
pub fn draft_row(draft: &DraftSummary, me: &str, locale: &str) -> DraftRow {
    DraftRow {
        draft_id: draft.id.clone(),
        page_id: draft.page.id.clone(),
        page_title: localized(&draft.page.title, locale),
        status: status_label(&draft.status),
        in_review: draft.status == "InReview",
        version_id: draft.version_id.clone().unwrap_or_default(),
        mine: draft.author_id == me,
        updated: short_time(&draft.updated_at),
    }
}

/// The Drafts tab: the caller's drafts and the drafts they are a member of, across every page. Newest first.
pub async fn gather_drafts(
    api: &ApiClient,
    token: &str,
    me: &str,
    locale: &str,
) -> Result<Vec<DraftRow>, ClientError> {
    let mut listed = api
        .list_content_drafts(token, Some(DraftStatusFilter::Open), Some(DraftRole::Mine))
        .await?;
    listed.extend(
        api.list_content_drafts(
            token,
            Some(DraftStatusFilter::Open),
            Some(DraftRole::Member),
        )
        .await?,
    );
    // A draft can be both mine and a membership listing; keep the first copy.
    let mut seen = HashSet::new();
    let mut rows: Vec<DraftRow> = listed
        .iter()
        .filter(|d| is_open(d) && seen.insert(d.id.clone()))
        .map(|d| draft_row(d, me, locale))
        .collect();
    rows.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(rows)
}

/// The reviews page: every draft in review. `scopes` names the scopes the caller picked; a draft on a scope not in the list
/// is labelled by its scope kind (see [`crate::home::scope_label`]).
pub async fn gather_reviews(
    api: &ApiClient,
    token: &str,
    scopes: &[(Scope, String)],
    locale: &str,
) -> Result<Vec<ReviewRow>, ClientError> {
    let listed = api
        .list_content_drafts(
            token,
            Some(DraftStatusFilter::InReview),
            Some(DraftRole::Reviewer),
        )
        .await?;
    let mut rows = Vec::new();
    for draft in listed {
        let Some(version) = draft.version_id.clone() else {
            continue;
        };
        let scope = Scope::from_page(draft.page.scope_kind, draft.page.scope_id.as_deref());
        let label = scopes
            .iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| crate::home::scope_label(&scope));
        rows.push(ReviewRow {
            version_id: version,
            draft_id: draft.id.clone(),
            page_id: draft.page.id.clone(),
            page_title: localized(&draft.page.title, locale),
            scope_label: label,
            author_id: draft.author_id.clone(),
            updated: short_time(&draft.updated_at),
        });
    }
    rows.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(rows)
}

/// Approves a pending version. The reply's status is "pending" (more approvals needed), or "published".
pub async fn approve(
    api: &ApiClient,
    token: &str,
    version_id: &str,
) -> Result<String, ClientError> {
    let decision = api.approve_content_version(token, version_id).await?;
    Ok(decision.status)
}

/// Rejects a pending version, with the reason as a comment on its draft first (the reject route takes no text).
/// An empty reason is refused before any request.
pub async fn reject(
    api: &ApiClient,
    token: &str,
    row: &ReviewRow,
    reason: &str,
) -> Result<(), ClientError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(ClientError::Api {
            status: 400,
            error: "reason_required".to_string(),
            message: "Say why you reject it.".to_string(),
        });
    }
    api.create_content_comment(
        token,
        &row.page_id,
        &NewComment {
            draft_id: Some(row.draft_id.clone()),
            block_id: None,
            anchor: None,
            parent_id: None,
            body: format!("Rejected: {reason}"),
        },
    )
    .await?;
    api.reject_content_version(token, &row.version_id).await?;
    Ok(())
}

/// The window side of the reviews page (`/home/reviews`).
pub trait ReviewsWindow: slint::ComponentHandle + 'static {
    fn show_state(&self, state: space_page_kit::load::LoadState);
    fn show_error(&self, text: &str);
    fn show_rows(&self, rows: Vec<ReviewRow>);
    fn show_notice(&self, text: &str);
    fn navigate(&self, route: &str);
    fn ask_reload(&self);
    fn on_reload(&self, f: impl Fn() + 'static);
    fn on_open(&self, f: impl Fn(usize) + 'static);
    fn on_approve(&self, f: impl Fn(usize) + 'static);
    fn on_reject(&self, f: impl Fn(usize, String) + 'static);
}

#[derive(Default)]
struct ReviewsShared {
    generation: u64,
    rows: Vec<ReviewRow>,
}

/// The reviews page. `scopes` is the list of scopes whose pages it searches (the app gives them: company, its projects, teams
/// and departments). Clones share the state.
pub struct Reviews<W: ReviewsWindow> {
    ui: slint::Weak<W>,
    data: space_page_kit::page_data::DataCell,
    scopes: std::sync::Arc<std::sync::Mutex<Vec<(Scope, String)>>>,
    shared: std::sync::Arc<std::sync::Mutex<ReviewsShared>>,
}

impl<W: ReviewsWindow> Clone for Reviews<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            scopes: self.scopes.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the reviews page on `ui`.
pub fn wire_reviews<W: ReviewsWindow>(
    ui: &W,
    data: space_page_kit::page_data::DataCell,
) -> Reviews<W> {
    let page = Reviews {
        ui: ui.as_weak(),
        data,
        scopes: Default::default(),
        shared: Default::default(),
    };
    let p = page.clone();
    ui.on_reload(move || p.load());
    let p = page.clone();
    ui.on_open(move |i| p.open(i));
    let p = page.clone();
    ui.on_approve(move |i| p.approve(i));
    let p = page.clone();
    ui.on_reject(move |i, reason| p.reject(i, &reason));
    page
}

impl<W: ReviewsWindow> Reviews<W> {
    /// Sets the scopes to search, each with its label ("Project: Atlas").
    pub fn set_scopes(&self, scopes: Vec<(Scope, String)>) {
        *space_page_kit::lock::lock(&self.scopes) = scopes;
    }

    /// Loads the reviews (see [`gather_reviews`]). The shell calls it when the reviews route opens.
    pub fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut s = space_page_kit::lock::lock(&self.shared);
            s.generation += 1;
            ui.show_state(space_page_kit::load::LoadState::Loading);
            s.generation
        };
        let scopes = space_page_kit::lock::lock(&self.scopes).clone();
        let shared = self.shared.clone();
        let source: Option<std::sync::Arc<space_page_kit::page_data::DataSource>> =
            self.data.borrow().clone();
        let Some(source) = source else { return };
        source.fetch(
            &self.ui,
            move |api, token| async move {
                gather_reviews(&api, &token, &scopes, FALLBACK_LOCALE).await
            },
            move |ui, result| {
                let mut s = space_page_kit::lock::lock(&shared);
                if s.generation != generation {
                    return;
                }
                match result {
                    Ok(rows) => {
                        s.rows = rows.clone();
                        ui.show_state(if rows.is_empty() {
                            space_page_kit::load::LoadState::Empty
                        } else {
                            space_page_kit::load::LoadState::Loaded
                        });
                        ui.show_rows(rows);
                    }
                    Err(e) => {
                        ui.show_state(space_page_kit::load::LoadState::Error);
                        ui.show_error(&space_page_kit::page_data::error_text(&e));
                    }
                }
            },
        );
    }

    /// Opens the page a row is about (read mode; the editor's own review view is the page's).
    fn open(&self, index: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let row = space_page_kit::lock::lock(&self.shared)
            .rows
            .get(index)
            .cloned();
        let Some(row) = row else { return };
        let scope = self.scopes_for(&row);
        ui.navigate(&crate::routes::page_route(&scope, &row.page_id, false));
    }

    /// The scope a row belongs to: its label names it, so the first scope with that label wins.
    fn scopes_for(&self, row: &ReviewRow) -> Scope {
        space_page_kit::lock::lock(&self.scopes)
            .iter()
            .find(|(_, label)| *label == row.scope_label)
            .map(|(scope, _)| scope.clone())
            .unwrap_or(Scope::Company)
    }

    fn approve(&self, index: usize) {
        let row = space_page_kit::lock::lock(&self.shared)
            .rows
            .get(index)
            .cloned();
        let Some(row) = row else { return };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else { return };
        let api = source.api();
        let ui_weak = self.ui.clone();
        space_page_kit::task::spawn_task(move || async move {
            let result = approve(&api, &token, &row.version_id).await;
            space_page_kit::page_data::ui_thread(&ui_weak, move |ui| match result {
                Ok(status) => {
                    ui.show_notice(if status == "published" {
                        "Published."
                    } else {
                        "Approved."
                    });
                    ui.ask_reload();
                }
                Err(e) => ui.show_notice(&space_page_kit::page_data::error_text(&e)),
            });
        });
    }

    fn reject(&self, index: usize, reason: &str) {
        let row = space_page_kit::lock::lock(&self.shared)
            .rows
            .get(index)
            .cloned();
        let Some(row) = row else { return };
        let Some(source) = self.data.borrow().clone() else {
            return;
        };
        let Some(token) = source.token() else { return };
        let api = source.api();
        let reason = reason.to_string();
        let ui_weak = self.ui.clone();
        space_page_kit::task::spawn_task(move || async move {
            let result = reject(&api, &token, &row, &reason).await;
            space_page_kit::page_data::ui_thread(&ui_weak, move |ui| match result {
                Ok(()) => {
                    ui.show_notice("Rejected.");
                    ui.ask_reload();
                }
                Err(e) => ui.show_notice(&space_page_kit::page_data::error_text(&e)),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_client::content::{DraftPageRef, ScopeKind, Visibility};

    fn draft(author: &str, status: &str) -> DraftSummary {
        DraftSummary {
            id: "d1".into(),
            page: DraftPageRef {
                id: "p1".into(),
                title: serde_json::json!({"en": "Intro"}),
                slug: "intro".into(),
                page_type: "wiki".into(),
                scope_kind: ScopeKind::Company,
                scope_id: None,
                visibility: Visibility::Internal,
            },
            author_id: author.into(),
            author_name: None,
            status: status.into(),
            version_id: Some("v1".into()),
            members: Vec::new(),
            updated_at: "2026-11-23T14:05:09Z".into(),
        }
    }

    #[test]
    fn only_open_and_in_review_drafts_are_open() {
        assert!(is_open(&draft("me", "Open")));
        assert!(is_open(&draft("me", "InReview")));
        assert!(!is_open(&draft("me", "Published")));
        assert!(!is_open(&draft("me", "Discarded")));
    }

    #[test]
    fn draft_rows_carry_review_state() {
        let row = draft_row(&draft("me", "InReview"), "me", FALLBACK_LOCALE);
        assert!(row.in_review);
        assert!(row.mine);
        assert_eq!(row.status, "In review");
        assert_eq!(row.version_id, "v1");
        assert_eq!(row.updated, "2026-11-23 14:05");
    }
}
