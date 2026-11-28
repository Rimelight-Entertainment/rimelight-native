//! Glue between the Finances controller (`rimelight_personal_feature_finances`) and the shell's `FinancesRoute`.
//!
//! The controller answers on a worker thread and reports each change through `notify`, which posts the view to the
//! event loop. Copy buttons go through the window's `copy-text` function (`wire_clipboard!`).

use std::sync::Arc;

use rimelight_personal_core::local_date::today;
use rimelight_personal_feature_finances::{
    FinanceApi, FinanceTab, FinancesController, FinancesView, FormView,
};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::AppWindow;
use crate::generated::{
    CalendarDay, FinanceChip, FinanceField, FinanceRow, FinanceSection, FinanceStat, FinancesState,
    SelectOption,
};

rimelight_ui_kit::wire_clipboard!(crate::AppWindow);

pub struct Finances {
    controller: FinancesController,
}

impl Finances {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Finances::activate`].
    pub fn new(ui: &AppWindow, api: Arc<dyn FinanceApi>) -> Self {
        let weak = ui.as_weak();
        let controller = FinancesController::new(api, move |view: FinancesView| {
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Finances route is shown: the local day is set and the data fetched.
    pub fn activate(&self) {
        let day = today();
        self.controller
            .set_today(&format!("{:04}-{:02}-{:02}", day.year, day.month, day.day));
        self.controller.refresh();
    }

    /// Opens a finances form by its key (`account`, `income`, `expense`, `bill`, `plan`), as a "new" chip does. The
    /// quick-action menu's Income, Bill, Wishlist and Account items use it.
    pub fn open_form(&self, key: &str) {
        self.controller.open_form(key);
    }
}

fn show_view(ui: &AppWindow, view: &FinancesView) {
    let state = ui.global::<FinancesState>();
    state.set_tab(view.tab.key().into());
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
    state.set_rates_missing(view.rates_missing);
    state.set_search(view.search.as_str().into());

    let stats: Vec<FinanceStat> = view
        .stats
        .iter()
        .map(|s| FinanceStat {
            label: s.label.as_str().into(),
            value: s.value.as_str().into(),
        })
        .collect();
    state.set_stats(ModelRc::new(VecModel::from(stats)));

    let chips: Vec<FinanceChip> = view
        .chips
        .iter()
        .map(|c| FinanceChip {
            group: c.group.as_str().into(),
            value: c.value.as_str().into(),
            label: c.label.as_str().into(),
            selected: c.selected,
        })
        .collect();
    state.set_chips(ModelRc::new(VecModel::from(chips)));

    let sections: Vec<FinanceSection> = view
        .sections
        .iter()
        .map(|section| {
            let rows: Vec<FinanceRow> = section
                .rows
                .iter()
                .map(|r| FinanceRow {
                    id: r.id.as_str().into(),
                    title: r.title.as_str().into(),
                    subtitle: r.subtitle.as_str().into(),
                    value: r.value.as_str().into(),
                    note: r.note.as_str().into(),
                    primary_label: r.primary_label.as_str().into(),
                    secondary_label: r.secondary_label.as_str().into(),
                    can_edit: r.can_edit,
                    can_delete: r.can_delete,
                    copy_text: r.copy_text.as_str().into(),
                })
                .collect();
            FinanceSection {
                title: section.title.as_str().into(),
                empty: section.empty.as_str().into(),
                rows: ModelRc::new(VecModel::from(rows)),
            }
        })
        .collect();
    state.set_sections(ModelRc::new(VecModel::from(sections)));

    state.set_form_open(view.form.is_some());
    let form = view.form.clone().unwrap_or_default();
    state.set_form_title(form.title.as_str().into());
    state.set_form_submit(form.submit_label.as_str().into());
    state.set_form_error(form.error.clone().unwrap_or_default().into());
    state.set_form_fields(ModelRc::new(VecModel::from(form_fields(&form))));
}

/// The open form's fields: a select shows its choices in the shared Select, a date shows the shared DatePicker.
fn form_fields(form: &FormView) -> Vec<FinanceField> {
    form.fields
        .iter()
        .map(|f| FinanceField {
            key: f.key.as_str().into(),
            label: f.label.as_str().into(),
            value: f.value.as_str().into(),
            placeholder: f.placeholder.as_str().into(),
            required: f.required,
            kind: f.kind.as_str().into(),
            options: ModelRc::new(VecModel::from(
                f.choices
                    .iter()
                    .map(|c| SelectOption {
                        value: c.value.as_str().into(),
                        label: c.label.as_str().into(),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            )),
            calendar: ModelRc::new(VecModel::from(
                f.calendar
                    .iter()
                    .map(|c| CalendarDay {
                        day: c.day,
                        in_month: c.in_month,
                        today: c.today,
                        selected: c.selected,
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            )),
            calendar_title: f.calendar_title.as_str().into(),
            weekday_labels: ModelRc::new(VecModel::from(
                f.weekday_labels
                    .iter()
                    .map(|w| SharedString::from(w.as_str()))
                    .collect::<Vec<_>>(),
            )),
        })
        .collect()
}

fn wire(ui: &AppWindow, controller: &FinancesController) {
    let state = ui.global::<FinancesState>();

    let c = controller.clone();
    state.on_tab_requested(move |key| {
        if let Some(tab) = FinanceTab::from_key(&key) {
            c.set_tab(tab);
        }
    });

    let c = controller.clone();
    state.on_refresh_requested(move || c.refresh());

    let c = controller.clone();
    state.on_search_changed(move |text| c.set_search(&text));

    let c = controller.clone();
    state.on_chip_requested(move |group, value| {
        // A "new" chip opens its form; the filter chips narrow the transactions.
        if group == "new" {
            c.open_form(&value);
        } else {
            c.choose(&group, &value);
        }
    });

    let c = controller.clone();
    state.on_field_changed(move |key, value| c.set_field(&key, &value));

    let c = controller.clone();
    state.on_date_picked(move |key, index| c.pick_date(&key, index.max(0) as usize));

    let c = controller.clone();
    state.on_date_stepped(move |key, delta| c.date_step(&key, i64::from(delta)));

    let c = controller.clone();
    state.on_submit_requested(move || {
        c.submit();
    });

    let c = controller.clone();
    state.on_cancel_requested(move || c.cancel_form());

    let c = controller.clone();
    state.on_row_primary_requested(move |id| c.primary(&id));

    let c = controller.clone();
    state.on_row_secondary_requested(move |id| c.secondary(&id));

    let c = controller.clone();
    state.on_row_edit_requested(move |id| c.edit(&id));

    let c = controller.clone();
    state.on_row_delete_requested(move |id| c.delete(&id));

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());

    let weak = ui.as_weak();
    state.on_copy_requested(move |text| {
        if let Some(ui) = weak.upgrade() {
            copy_text(&ui, &text);
        }
    });
}
