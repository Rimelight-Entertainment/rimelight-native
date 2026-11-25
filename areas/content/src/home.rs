//! The scope home (CMS.6): one scope's content. Collection tabs ("All" first), the page tree and list, the search box, "New page",
//! the per-page menu (open, edit, move to another scope, visibility, delete) and a Drafts tab (my drafts and the drafts I was
//! invited to).
//!
//! Routes: `/p/:key/docs`, `/company/knowledge`, `/company/teams/:id/docs`, `/company/departments/:id/docs`. The page loads on
//! the page data pattern (`space_page_kit::page_data`). Row actions that open a page navigate to its route; "Edit" navigates
//! with `?mode=edit`, and the page opens the caller's draft itself.
//!
//! This module is plain Rust. The window side is [`HomeWindow`]; `wire_content!` implements it for the generated window.

use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, Weak};
use space_client::content::{ContentCollection, CreatePage, PageSummary, PatchPage, Visibility};
use space_client::error::ClientError;
use space_page_kit::load::LoadState;
use space_page_kit::lock::lock;
use space_page_kit::page_data::{DataCell, DataSource};

use crate::review::{self, DraftRow};
use crate::routes::page_route;
use crate::scope::{
    Access, Scope, ScopeChoice, can_manage, can_move, move_targets, parse_access, scope_choices,
};
use crate::view::{
    FALLBACK_LOCALE, PageRow, TabView, localized, matches_query, page_row, slug_from_title, tabs,
    valid_slug,
};

/// The window side of the scope home. `wire_content!` implements it for the generated window.
pub trait HomeWindow: ComponentHandle + 'static {
    fn show_state(&self, state: LoadState);
    fn show_error(&self, text: &str);
    fn show_scope(&self, label: &str);
    fn show_tabs(&self, tabs: Vec<TabView>, active: usize);
    fn show_rows(&self, rows: Vec<PageRow>);
    fn show_drafts(&self, rows: Vec<DraftRow>);
    fn show_notice(&self, text: &str);
    /// The New page dialog: open or closed, its collection choices (slug, label) and its template choices (id, title).
    fn show_new_dialog(
        &self,
        open: bool,
        collections: Vec<(String, String)>,
        templates: Vec<(String, String)>,
        error: &str,
    );
    /// The move dialog: the targets (labels, in the order the picker gives) for the page with `page_id`.
    fn show_move_dialog(&self, page_id: &str, labels: Vec<String>);
    /// The caller's user id, read on the UI thread (drafts and invitations are matched on it).
    fn user_id(&self) -> String;
    /// Opens a route in the shell (the app's `navigate` callback).
    fn navigate(&self, route: &str);
    /// Asks the window to reload the page (a Slint callback the page loads on). Used after a change, from a worker's result.
    fn ask_reload(&self);
    fn on_reload(&self, f: impl Fn() + 'static);
    fn on_retry(&self, f: impl Fn() + 'static);
    fn on_tab(&self, f: impl Fn(String) + 'static);
    fn on_search(&self, f: impl Fn(String) + 'static);
    fn on_new_open(&self, f: impl Fn() + 'static);
    fn on_new_collection(&self, f: impl Fn(String) + 'static);
    fn on_new_create(&self, f: impl Fn(String, String, String, String) + 'static);
    fn on_new_cancel(&self, f: impl Fn() + 'static);
    fn on_row_open(&self, f: impl Fn(String) + 'static);
    fn on_row_edit(&self, f: impl Fn(String) + 'static);
    fn on_row_move_open(&self, f: impl Fn(String) + 'static);
    fn on_row_move(&self, f: impl Fn(String, i32) + 'static);
    fn on_row_visibility(&self, f: impl Fn(String, bool) + 'static);
    fn on_row_delete(&self, f: impl Fn(String) + 'static);
    fn on_drafts_opened(&self, f: impl Fn() + 'static);
    fn on_draft_open(&self, f: impl Fn(String, String) + 'static);
}

/// The request state of the scope home. Behind a `Mutex`, because a worker's result reads and writes it.
#[derive(Default)]
struct Shared {
    /// Bumped by every load. A load's result for an older number is dropped.
    generation: u64,
    collections: Vec<ContentCollection>,
    pages: Vec<PageSummary>,
    loaded: bool,
    error: Option<String>,
    tab: String,
    query: String,
    /// The projects the caller is in, as (key, name). Set by the app; the load adds the teams and departments.
    projects: Vec<(String, String)>,
    /// The move targets, offered to every row's move dialog: Company, the projects, then the teams and departments the caller
    /// can see (built by `load` with [`scope_choices`]).
    scope_choices: Vec<ScopeChoice>,
    /// The New page dialog.
    new_open: bool,
    new_collection: String,
    new_error: String,
}

/// The scope home page: its state and the window it draws in. Built by [`wire`]. Clones share the state.
pub struct Home<W: HomeWindow> {
    ui: Weak<W>,
    data: DataCell,
    scope: Scope,
    locale: String,
    shared: Arc<Mutex<Shared>>,
}

impl<W: HomeWindow> Clone for Home<W> {
    fn clone(&self) -> Self {
        Self {
            ui: self.ui.clone(),
            data: self.data.clone(),
            scope: self.scope.clone(),
            locale: self.locale.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Wires the scope home's callbacks on `ui` for `scope`. `data` is the slot the session driver fills; until it is set, loads
/// do nothing and the page stays in its loading state.
pub fn wire<W: HomeWindow>(ui: &W, data: DataCell, scope: Scope) -> Home<W> {
    let home = Home {
        ui: ui.as_weak(),
        data,
        scope,
        locale: FALLBACK_LOCALE.to_string(),
        shared: Arc::new(Mutex::new(Shared::default())),
    };

    let page = home.clone();
    ui.on_reload(move || page.load());
    let page = home.clone();
    ui.on_retry(move || page.load());
    let page = home.clone();
    ui.on_tab(move |slug| page.select_tab(&slug));
    let page = home.clone();
    ui.on_search(move |text| page.search(&text));
    let page = home.clone();
    ui.on_new_open(move || page.open_new());
    let page = home.clone();
    ui.on_new_collection(move |slug| page.pick_new_collection(&slug));
    let page = home.clone();
    ui.on_new_create(move |collection, title, slug, template| {
        page.create(&collection, &title, &slug, &template)
    });
    let page = home.clone();
    ui.on_new_cancel(move || page.close_new());
    let page = home.clone();
    ui.on_row_open(move |id| page.open(&id, false));
    let page = home.clone();
    ui.on_row_edit(move |id| page.open(&id, true));
    let page = home.clone();
    ui.on_row_move_open(move |id| page.open_move(&id));
    let page = home.clone();
    ui.on_row_move(move |id, index| page.move_to(&id, index));
    let page = home.clone();
    ui.on_row_visibility(move |id, public| page.set_visibility(&id, public));
    let page = home.clone();
    ui.on_row_delete(move |id| page.delete(&id));
    let page = home.clone();
    ui.on_drafts_opened(move || page.load_drafts());
    let page = home.clone();
    ui.on_draft_open(move |page_id, _draft| page.open(&page_id, true));

    home
}

impl<W: HomeWindow> Home<W> {
    /// Sets the projects the move dialog offers, as (key, name). The app calls it before `load`.
    pub fn set_projects(&self, projects: Vec<(String, String)>) {
        lock(&self.shared).projects = projects;
    }

    /// The generation of the latest load.
    pub fn generation(&self) -> u64 {
        lock(&self.shared).generation
    }

    /// Loads the collections, the scope's pages, and the teams and departments the move dialog offers. Each load starts from
    /// the loading state.
    pub fn load(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = {
            let mut guard = lock(&self.shared);
            let s = &mut *guard;
            s.generation += 1;
            s.loaded = false;
            s.error = None;
            show(&ui, &self.scope, &self.locale, s);
            s.generation
        };
        let shared = self.shared.clone();
        let fetch_scope = self.scope.clone();
        let (scope, locale) = (self.scope.clone(), self.locale.clone());
        self.run(
            move |api, token| async move {
                let collections = api.content_collections(&token).await?;
                let pages = api
                    .content_pages(&token, &fetch_scope.to_page_scope(), None)
                    .await?;
                // UNSURE: a failed teams or departments list leaves those move targets out rather than failing the page.
                let teams = api.teams(&token).await.unwrap_or_default();
                let departments = api.departments(&token).await.unwrap_or_default();
                Ok((collections, pages, teams, departments))
            },
            move |ui, result| {
                let mut guard = lock(&shared);
                let s = &mut *guard;
                if s.generation != generation {
                    return;
                }
                match result {
                    Ok((collections, pages, teams, departments)) => {
                        let team_pairs: Vec<(String, String)> =
                            teams.into_iter().map(|t| (t.id, t.name)).collect();
                        let department_pairs: Vec<(String, String)> =
                            departments.into_iter().map(|d| (d.id, d.name)).collect();
                        s.scope_choices =
                            scope_choices(&s.projects, &team_pairs, &department_pairs);
                        s.collections = collections;
                        s.pages = pages;
                        s.loaded = true;
                    }
                    Err(e) => s.error = Some(space_page_kit::page_data::error_text(&e)),
                }
                show(ui, &scope, &locale, s);
            },
        );
    }

    /// Switches the list to a collection tab ("" for All).
    fn select_tab(&self, slug: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut guard = lock(&self.shared);
        guard.tab = slug.to_string();
        show(&ui, &self.scope, &self.locale, &guard);
    }

    /// The search box: filters the rows by title, slug and collection name.
    fn search(&self, text: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mut guard = lock(&self.shared);
        guard.query = text.to_string();
        show(&ui, &self.scope, &self.locale, &guard);
    }

    /// Opens the New page dialog with the first collection selected.
    fn open_new(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        {
            let mut guard = lock(&self.shared);
            guard.new_open = true;
            guard.new_error.clear();
            guard.new_collection = guard
                .collections
                .first()
                .map(|c| c.slug.clone())
                .unwrap_or_default();
        }
        self.show_new(&ui);
        let collection = lock(&self.shared).new_collection.clone();
        if !collection.is_empty() {
            self.pick_new_collection(&collection);
        }
    }

    fn close_new(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        lock(&self.shared).new_open = false;
        self.show_new(&ui);
    }

    /// Loads the templates of a collection into the New page dialog.
    fn pick_new_collection(&self, slug: &str) {
        let Some(_ui) = self.ui.upgrade() else { return };
        lock(&self.shared).new_collection = slug.to_string();
        let collection = slug.to_string();
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move { api.content_templates(&token, &collection).await },
            move |ui, result| {
                let mut s = lock(&shared);
                match result {
                    Ok(templates) => {
                        s.new_error.clear();
                        // The first template is the default; the dialog shows (id, title) pairs.
                        let rows: Vec<(String, String)> = templates
                            .iter()
                            .map(|t| (t.id.clone(), localized(&t.title, FALLBACK_LOCALE)))
                            .collect();
                        ui.show_new_dialog(
                            s.new_open,
                            collection_choices(&s.collections),
                            rows,
                            "",
                        );
                    }
                    Err(e) => {
                        s.new_error = space_page_kit::page_data::error_text(&e);
                        ui.show_new_dialog(
                            s.new_open,
                            collection_choices(&s.collections),
                            Vec::new(),
                            &s.new_error,
                        );
                    }
                }
            },
        );
    }

    /// Creates a page in this scope and opens it in edit mode. Empty slug means "from the title". The title is required.
    /// An empty template id starts a blank page.
    fn create(&self, collection: &str, title: &str, slug: &str, template: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let title = title.trim().to_string();
        if title.is_empty() {
            lock(&self.shared).new_error = "Give the page a title.".to_string();
            self.show_new(&ui);
            return;
        }
        let slug = match slug.trim() {
            "" => slug_from_title(&title),
            typed => typed.to_string(),
        };
        if !valid_slug(&slug) {
            lock(&self.shared).new_error =
                "The address may use lowercase letters, digits and single hyphens.".to_string();
            self.show_new(&ui);
            return;
        }
        let body = CreatePage {
            scope_kind: self.scope.to_page_scope().kind,
            scope_id: self.scope.id().map(str::to_string),
            page_type: collection.to_string(),
            slug,
            title,
            template_id: (!template.is_empty()).then(|| template.to_string()),
        };
        let scope = self.scope.clone();
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move { api.create_content_page(&token, &body).await },
            move |ui, result| match result {
                Ok(created) => {
                    lock(&shared).new_open = false;
                    ui.navigate(&page_route(&scope, &created.id, true));
                }
                Err(e) => {
                    let text = space_page_kit::page_data::error_text(&e);
                    lock(&shared).new_error = text.clone();
                    ui.show_notice(&text);
                }
            },
        );
    }

    /// Opens a page (`edit` opens it in edit mode, where the page opens the caller's draft).
    fn open(&self, page_id: &str, edit: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.navigate(&page_route(&self.scope, page_id, edit));
    }

    /// Opens the move dialog for a page. The current scope is left out of the targets.
    fn open_move(&self, page_id: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let s = lock(&self.shared);
        let Some(page) = s.pages.iter().find(|p| p.id == page_id) else {
            return;
        };
        if !can_manage(parse_access(&page.my_access)) {
            return;
        }
        let labels: Vec<String> = move_targets(&self.scope, &s.scope_choices)
            .into_iter()
            .map(|c| c.label)
            .collect();
        ui.show_move_dialog(page_id, labels);
    }

    /// Moves a page to the target at `index` of the move targets (after its scope picker). Needs manage on both scopes; the
    /// target's access is not known here, so the server's answer is the check (see WIRING, UNSURE).
    fn move_to(&self, page_id: &str, index: i32) {
        let Some(_ui) = self.ui.upgrade() else { return };
        let target = {
            let s = lock(&self.shared);
            let Some(page) = s.pages.iter().find(|p| p.id == page_id) else {
                return;
            };
            let from = parse_access(&page.my_access);
            if !can_move(from, Access::Manage) {
                return;
            }
            let targets = move_targets(&self.scope, &s.scope_choices);
            match usize::try_from(index).ok().and_then(|i| targets.get(i)) {
                Some(t) => t.scope.clone(),
                None => return,
            }
        };
        let body = space_client::content::MovePage {
            scope_kind: target.to_page_scope().kind,
            scope_id: target.id().map(str::to_string),
        };
        let id = page_id.to_string();
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move { api.move_content_page(&token, &id, &body).await },
            move |ui, result| match result {
                // The page left this scope: reload the list, which no longer has it.
                Ok(_) => ui.ask_reload(),
                Err(e) => lock(&shared).error = Some(space_page_kit::page_data::error_text(&e)),
            },
        );
    }

    /// Sets a page public or internal (manage only).
    fn set_visibility(&self, page_id: &str, public: bool) {
        let visibility = if public {
            Visibility::Public
        } else {
            Visibility::Internal
        };
        let id = page_id.to_string();
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move {
                api.update_content_page(
                    &token,
                    &id,
                    &PatchPage {
                        visibility: Some(visibility),
                        slug: None,
                    },
                )
                .await
            },
            move |ui, result| match result {
                Ok(_) => ui.ask_reload(),
                Err(e) => lock(&shared).error = Some(space_page_kit::page_data::error_text(&e)),
            },
        );
    }

    /// Deletes a page (a soft delete on the server). The window asks for confirmation first.
    fn delete(&self, page_id: &str) {
        let id = page_id.to_string();
        let shared = self.shared.clone();
        self.run(
            move |api, token| async move { api.delete_content_page(&token, &id).await },
            move |ui, result| match result {
                Ok(_) => ui.ask_reload(),
                Err(e) => lock(&shared).error = Some(space_page_kit::page_data::error_text(&e)),
            },
        );
    }

    /// The Drafts tab: the caller's drafts and the drafts they were invited to, on this scope's pages. The list call is not
    /// scoped, so the rows are kept to the scope's page ids here.
    fn load_drafts(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let me = ui.user_id();
        let page_ids: std::collections::HashSet<String> = lock(&self.shared)
            .pages
            .iter()
            .map(|p| p.id.clone())
            .collect();
        self.run(
            move |api, token| async move {
                let rows = review::gather_drafts(&api, &token, &me, FALLBACK_LOCALE).await?;
                Ok::<_, ClientError>(
                    rows.into_iter()
                        .filter(|r| page_ids.contains(&r.page_id))
                        .collect::<Vec<DraftRow>>(),
                )
            },
            move |ui, result| match result {
                Ok(rows) => ui.show_drafts(rows),
                Err(e) => ui.show_notice(&space_page_kit::page_data::error_text(&e)),
            },
        );
    }

    /// Runs `work` on the session's data (a worker), then `done` on the UI thread. With no session source, nothing happens.
    fn run<T, F, Fut, D>(&self, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce(Arc<space_client::api::ApiClient>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, ClientError>>,
        D: FnOnce(&W, Result<T, ClientError>) + Send + 'static,
    {
        let source: Option<Arc<DataSource>> = self.data.borrow().clone();
        if let Some(source) = source {
            source.fetch(&self.ui, work, done);
        }
    }

    fn show_new(&self, ui: &W) {
        let s = lock(&self.shared);
        ui.show_new_dialog(
            s.new_open,
            collection_choices(&s.collections),
            Vec::new(),
            &s.new_error,
        );
    }
}

/// The collection choices of the New page dialog: (slug, label).
fn collection_choices(collections: &[ContentCollection]) -> Vec<(String, String)> {
    collections
        .iter()
        .map(|c| (c.slug.clone(), c.name.clone()))
        .collect()
}

/// Draws the page from the shared state: the scope heading, the tabs, the rows (filtered by tab and search), the error.
fn show<W: HomeWindow>(ui: &W, scope: &Scope, locale: &str, s: &Shared) {
    ui.show_scope(&scope_label(scope));
    if let Some(error) = &s.error {
        ui.show_state(LoadState::Error);
        ui.show_error(error);
        return;
    }
    if !s.loaded {
        ui.show_state(LoadState::Loading);
        return;
    }
    let rows: Vec<PageRow> = s
        .pages
        .iter()
        .map(|p| page_row(p, locale, &s.collections))
        .filter(|r| {
            let in_tab =
                s.tab.is_empty() || s.pages.iter().any(|p| p.id == r.id && p.page_type == s.tab);
            in_tab && matches_query(r, &s.query)
        })
        .collect();
    let all_tabs = tabs(&s.collections, &s.pages);
    let active = all_tabs.iter().position(|t| t.slug == s.tab).unwrap_or(0);
    ui.show_tabs(all_tabs, active);
    ui.show_state(if s.pages.is_empty() {
        LoadState::Empty
    } else {
        LoadState::Loaded
    });
    ui.show_rows(rows);
}

/// The scope's heading: "Company knowledge", or the scope's kind and id (the page header and the scope home use it).
pub fn scope_label(scope: &Scope) -> String {
    match scope {
        Scope::Company => "Company knowledge".to_string(),
        Scope::Project(id) => format!("Project docs: {id}"),
        Scope::Team(id) => format!("Team docs: {id}"),
        Scope::Department(id) => format!("Department docs: {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_name_the_scope() {
        assert_eq!(scope_label(&Scope::Company), "Company knowledge");
        assert_eq!(scope_label(&Scope::Team("t1".into())), "Team docs: t1");
    }

    #[test]
    fn collection_choices_keep_order() {
        let cols = vec![
            ContentCollection {
                slug: "wiki".into(),
                name: "Wiki".into(),
                description: None,
                icon: "book".into(),
                color: None,
                mode: "page".into(),
                fields: Vec::new(),
                features: serde_json::Value::Null,
                label_plural: None,
                routable: false,
            },
            ContentCollection {
                slug: "blog".into(),
                name: "Blog".into(),
                description: None,
                icon: "pen".into(),
                color: None,
                mode: "page".into(),
                fields: Vec::new(),
                features: serde_json::Value::Null,
                label_plural: None,
                routable: true,
            },
        ];
        assert_eq!(
            collection_choices(&cols),
            vec![
                ("wiki".into(), "Wiki".into()),
                ("blog".into(), "Blog".into())
            ]
        );
    }
}
