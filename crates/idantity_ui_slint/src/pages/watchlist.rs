//! Glue between the Watchlist controller (`idantity_feature_watchlist`) and the WatchlistState global of
//! `ui/pages/watchlist.slint`.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback posts the view to the Slint event loop, and only the event loop touches the window.

use std::sync::Arc;

use idantity_feature_watchlist::{
    Choice, Column, Draft, WatchlistApi, WatchlistController, WatchlistView, form_status_choices,
    section_choices, section_from_key, status_from_key, status_key, table_status_choices,
};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::clipboard;
use crate::generated::{
    IdantityShell, PageLoadState as SlintPageLoad, SelectOption, TableColumn, TableRow,
    WatchlistCategory, WatchlistColumn, WatchlistState,
};

/// Column widths in logical pixels, in the table's order (title, creator, category, genre, rating, status, details,
/// notes). They match the widths the Watchlist markup used before the table moved to rimelight_ui.
const COLUMN_WIDTHS: [f32; 8] = [160.0, 140.0, 110.0, 100.0, 70.0, 140.0, 150.0, 160.0];

/// The Watchlist page: one controller, wired to the WatchlistState global.
pub struct Watchlist {
    controller: WatchlistController,
}

impl Watchlist {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Watchlist::activate`].
    pub fn new(ui: &IdantityShell, api: Arc<dyn WatchlistApi>) -> Self {
        let state = ui.global::<WatchlistState>();
        state.set_section_options(options(&section_choices()));
        state.set_status_options(options(&table_status_choices()));
        state.set_form_status_options(options(&form_status_choices()));

        let weak = ui.as_weak();
        let controller = WatchlistController::new(api, move |view: WatchlistView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when a Watchlist route opens. `route` picks the section (or the archive).
    pub fn activate(&self, route: &str) {
        self.controller.activate(route);
    }

    /// Opens the add form for the current year, as the page's New button does. The quick-action menu's "New Watchlist
    /// Item" calls it.
    pub fn open_add(&self, ui: &IdantityShell) {
        let draft = self.controller.open_add(current_year());
        set_draft(ui, &draft);
    }

    /// The controller, for tests.
    #[cfg(test)]
    pub fn controller(&self) -> &WatchlistController {
        &self.controller
    }
}

fn options(choices: &[Choice]) -> ModelRc<SelectOption> {
    let items: Vec<SelectOption> = choices
        .iter()
        .map(|choice| SelectOption {
            value: choice.value.as_str().into(),
            label: choice.label.as_str().into(),
            icon: Default::default(),
            disabled: false,
            group: "".into(),
            ..Default::default()
        })
        .collect();
    ModelRc::new(VecModel::from(items))
}

/// Fills the WatchlistState properties from a view. The form's draft fields are not touched here: the page owns them
/// while it is open, and [`open_draft`] fills them when a form opens.
fn show_view(ui: &IdantityShell, view: &WatchlistView) {
    let state = ui.global::<WatchlistState>();

    let columns: Vec<TableColumn> = view
        .columns
        .iter()
        .filter(|column| column.visible)
        .map(|column| TableColumn {
            title: column.label.as_str().into(),
            width: COLUMN_WIDTHS[column_index(column.column)],
            sortable: true,
        })
        .collect();
    state.set_columns(ModelRc::new(VecModel::from(columns)));
    let chips: Vec<WatchlistColumn> = view
        .columns
        .iter()
        .map(|column| WatchlistColumn {
            label: column.label.as_str().into(),
            visible: column.visible,
            sorted: match column.sorted {
                None => -1,
                Some(false) => 0,
                Some(true) => 1,
            },
        })
        .collect();
    state.set_column_chips(ModelRc::new(VecModel::from(chips)));
    state.set_sort_column(view.sort_column.map(|c| c as i32).unwrap_or(-1));
    state.set_sort_descending(view.sort_descending);

    let rows: Vec<TableRow> = view
        .rows
        .iter()
        .map(|row| TableRow {
            cells: ModelRc::new(VecModel::from(
                row.cells
                    .iter()
                    .map(|cell| SharedString::from(cell.as_str()))
                    .collect::<Vec<_>>(),
            )),
            selected: row.selected,
        })
        .collect();
    state.set_rows(ModelRc::new(VecModel::from(rows)));
    state.set_selected_count(view.selected_count as i32);
    state.set_load_state(if view.error.is_some() {
        SlintPageLoad::Error
    } else if view.empty.is_some() && view.rows.is_empty() {
        SlintPageLoad::Empty
    } else {
        SlintPageLoad::Loaded
    });

    state.set_section(view.section.key().into());
    state.set_archived(view.archived);
    state.set_heading(view.heading.as_str().into());
    state.set_count_label(view.count_label.as_str().into());
    let empty = view.empty.as_ref();
    state.set_empty_title(empty.map(|e| e.title.clone()).unwrap_or_default().into());
    state.set_empty_text(empty.map(|e| e.text.clone()).unwrap_or_default().into());
    state.set_error(view.error.clone().unwrap_or_default().into());

    state.set_categories_label(view.categories_label.as_str().into());
    state.set_category_open(view.category_open);
    state.set_category_title(view.category_title.as_str().into());
    state.set_categories_count_label(view.categories.len().to_string().into());
    let categories: Vec<WatchlistCategory> = view
        .categories
        .iter()
        .map(|category| WatchlistCategory {
            id: category.id.as_str().into(),
            label: category.label.as_str().into(),
            count_text: category.items.to_string().into(),
        })
        .collect();
    state.set_category_rows(ModelRc::new(VecModel::from(categories)));
    state.set_confirm_open(view.confirm_category);

    match &view.form {
        Some(form) => {
            state.set_form_open(true);
            state.set_form_editing(form.editing);
            state.set_form_title(form.title.as_str().into());
            state.set_form_description(form.description.as_str().into());
            state.set_form_busy(form.busy);
            state.set_form_category_options(options(
                &form
                    .categories
                    .iter()
                    .map(|c| Choice {
                        value: c.value.clone(),
                        label: c.label.clone(),
                    })
                    .collect::<Vec<_>>(),
            ));
            state.set_form_no_category_hint(form.no_category_hint);
            state.set_form_can_save(form.can_save);
            state.set_form_save_label(form.save_label.as_str().into());
            state.set_form_section(form.section.key().into());
            state.set_form_category(form.category.as_str().into());
        }
        None => state.set_form_open(false),
    }
    state.set_error(view.error.clone().unwrap_or_default().into());
}

/// The index of a column in [`Column::ALL`], which is the order the table and the chips use.
fn column_index(column: Column) -> usize {
    Column::ALL.iter().position(|c| *c == column).unwrap_or(0)
}

/// Fills the draft fields from a draft, when a form opens.
fn set_draft(ui: &IdantityShell, draft: &Draft) {
    let state = ui.global::<WatchlistState>();
    state.set_draft_title(draft.title.as_str().into());
    state.set_draft_creator(draft.creator.as_str().into());
    state.set_draft_genre(draft.genre.as_str().into());
    state.set_draft_duration(draft.duration.as_str().into());
    state.set_draft_year(draft.year.as_str().into());
    state.set_draft_rating(draft.rating.as_str().into());
    state.set_draft_status(status_key(draft.status).into());
    state.set_draft_seasons(draft.seasons.as_str().into());
    state.set_draft_episodes(draft.episodes.as_str().into());
    state.set_draft_pages(draft.pages.as_str().into());
    state.set_draft_notes(draft.notes.as_str().into());
    state.set_draft_recommended_by(draft.recommended_by.as_str().into());
}

/// The draft the form's fields hold now.
fn read_draft(ui: &IdantityShell) -> Draft {
    let state = ui.global::<WatchlistState>();
    Draft {
        title: state.get_draft_title().to_string(),
        creator: state.get_draft_creator().to_string(),
        genre: state.get_draft_genre().to_string(),
        duration: state.get_draft_duration().to_string(),
        year: state.get_draft_year().to_string(),
        rating: state.get_draft_rating().to_string(),
        status: status_from_key(&state.get_draft_status()).unwrap_or_default(),
        seasons: state.get_draft_seasons().to_string(),
        episodes: state.get_draft_episodes().to_string(),
        pages: state.get_draft_pages().to_string(),
        notes: state.get_draft_notes().to_string(),
        recommended_by: state.get_draft_recommended_by().to_string(),
    }
}

/// The current year, from the UTC clock (the year the quick-add form defaults to).
fn current_year() -> i64 {
    i64::from(idantity_feature_habits::today().0)
}

/// Connects the WatchlistState callbacks to the controller.
fn wire(ui: &IdantityShell, controller: &WatchlistController) {
    let state = ui.global::<WatchlistState>();

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_new_requested(move || {
        let draft = c.open_add(current_year());
        if let Some(ui) = weak.upgrade() {
            set_draft(&ui, &draft);
        }
    });

    let c = controller.clone();
    state.on_archive_requested(move || c.toggle_archived());

    let c = controller.clone();
    state.on_categories_requested(move || c.open_categories());

    let c = controller.clone();
    state.on_search_changed(move |text| c.set_search(&text));

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_export_requested(move || {
        let csv = c.export_csv();
        if let Some(ui) = weak.upgrade() {
            clipboard::copy_text(&ui, &csv);
        }
    });

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());

    let c = controller.clone();
    state.on_sort_requested(move |column| c.sort_requested(column.max(0) as usize));

    let c = controller.clone();
    state.on_column_toggle_requested(move |index| {
        if let Some(column) = usize::try_from(index).ok().and_then(|i| Column::ALL.get(i)) {
            c.toggle_column(*column);
        }
    });

    let c = controller.clone();
    state.on_pick_requested(move |index, shift, ctrl| c.pick(index.max(0) as usize, shift, ctrl));

    let c = controller.clone();
    state.on_bulk_complete_requested(move || c.complete_selected());

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_edit_selected_requested(move || {
        let Some((cat, item)) = c.selected_item() else {
            return;
        };
        if let (Some(draft), Some(ui)) = (c.open_edit(&cat, &item), weak.upgrade()) {
            set_draft(&ui, &draft);
        }
    });

    let c = controller.clone();
    state.on_delete_selected_requested(move || c.delete_selected());

    let c = controller.clone();
    state.on_status_requested(move |cat, item, key| {
        if let Some(status) = status_from_key(&key) {
            c.set_status(&cat, &item, status);
        }
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_edit_requested(move |cat, item| {
        if let (Some(draft), Some(ui)) = (c.open_edit(&cat, &item), weak.upgrade()) {
            set_draft(&ui, &draft);
        }
    });

    let c = controller.clone();
    state.on_delete_requested(move |cat, item| c.delete_item(&cat, &item));

    let c = controller.clone();
    state.on_category_add_requested(move |name| {
        c.add_category(&name);
    });

    let c = controller.clone();
    state.on_category_delete_requested(move |id| c.request_delete_category(&id));

    let c = controller.clone();
    state.on_category_closed(move || c.close_categories());

    let c = controller.clone();
    state.on_confirm_answered(move |answer| c.answer_delete_category(answer));

    let c = controller.clone();
    state.on_form_section_requested(move |key| {
        if let Some(section) = section_from_key(&key) {
            c.set_form_section(section);
        }
    });

    let c = controller.clone();
    state.on_form_category_requested(move |id| c.set_form_category(&id));

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_form_submit_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        let draft = read_draft(&ui);
        c.submit(&draft);
    });

    let c = controller.clone();
    state.on_form_closed(move || c.close_form());
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use idantity_core::ApiError;
    use idantity_core::watchlist::{ItemPatch, Section, WatchlistData};
    use slint::Model;

    use super::*;

    /// A server that is down: every call is recorded and answers with an error.
    #[derive(Default)]
    struct Down {
        calls: Mutex<Vec<&'static str>>,
    }

    impl Down {
        fn answer(
            &self,
            call: &'static str,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.calls.lock().unwrap().push(call);
            on_done(Err(ApiError::Transport("offline".to_owned())));
        }
    }

    impl WatchlistApi for Down {
        fn load(&self, on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>) {
            self.answer("load", on_done);
        }

        fn add_category(
            &self,
            _section: Section,
            _name: &str,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("add_category", on_done);
        }

        fn delete_category(
            &self,
            _section: Section,
            _cat_id: &str,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("delete_category", on_done);
        }

        fn add_item(
            &self,
            _section: Section,
            _cat_id: &str,
            _item: &ItemPatch,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("add_item", on_done);
        }

        fn update_item(
            &self,
            _section: Section,
            _cat_id: &str,
            _item_id: &str,
            _item: &ItemPatch,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("update_item", on_done);
        }

        fn delete_item(
            &self,
            _section: Section,
            _cat_id: &str,
            _item_id: &str,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("delete_item", on_done);
        }

        fn toggle_completed(
            &self,
            _section: Section,
            _cat_id: &str,
            _item_id: &str,
            on_done: Box<dyn FnOnce(Result<WatchlistData, ApiError>) + Send>,
        ) {
            self.answer("toggle_completed", on_done);
        }
    }

    /// Run by `pages::tests::page_scenarios`, on the thread that installed the testing backend. The section and status
    /// choices are filled at once, and a failed load puts the page in its error state.
    pub(crate) fn scenario() {
        let ui = IdantityShell::new().expect("create the shell");
        let api = Arc::new(Down::default());
        let watchlist = Watchlist::new(&ui, api.clone());
        let state = ui.global::<WatchlistState>();
        assert!(state.get_section_options().row_count() > 0);
        assert!(state.get_status_options().row_count() > 0);

        watchlist.activate("/watchlist/series");
        assert_eq!(api.calls.lock().unwrap().as_slice(), ["load"]);
        show_view(&ui, &watchlist.controller().view());
        assert!(state.get_load_state() == SlintPageLoad::Error);
        assert!(!state.get_error().is_empty());
    }
}
