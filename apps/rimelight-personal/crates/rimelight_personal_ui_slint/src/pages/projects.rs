//! Glue between the Projects controller (`rimelight_personal_feature_projects`) and the shell's `ProjectsRoute`.
//!
//! The controller answers on a worker thread and reports each change through `notify`, which posts the view to the
//! event loop. The board's drop target (`column-at`) is answered here with `rimelight_ui_kit::board::drop_column`. The
//! three text fields are two-way bound to `ProjectsState`, so each write action first hands the controller the field's
//! text.

use std::sync::Arc;

use rimelight_personal_feature_projects::{ProjectsApi, ProjectsController, ProjectsView};
use rimelight_ui_kit::board::drop_column;
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};

use crate::AppWindow;
use crate::generated::{BoardItem, BoardLane, ProjectBoardTab, ProjectsState};

pub struct Projects {
    controller: ProjectsController,
}

impl Projects {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Projects::activate`].
    pub fn new(ui: &AppWindow, api: Arc<dyn ProjectsApi>) -> Self {
        let weak = ui.as_weak();
        let controller = ProjectsController::new(api, move |view: ProjectsView| {
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Projects route is shown.
    pub fn activate(&self) {
        self.controller.refresh();
    }
}

fn show_view(ui: &AppWindow, view: &ProjectsView) {
    let state = ui.global::<ProjectsState>();
    let boards: Vec<ProjectBoardTab> = view
        .boards
        .iter()
        .map(|b| ProjectBoardTab {
            id: b.id.as_str().into(),
            title: b.title.as_str().into(),
            active: b.active,
        })
        .collect();
    state.set_boards(ModelRc::new(VecModel::from(boards)));
    state.set_has_active_board(view.has_active_board);
    let lanes: Vec<BoardLane> = view
        .lanes
        .iter()
        .map(|lane| {
            let cards: Vec<BoardItem> = lane
                .cards
                .iter()
                .map(|card| BoardItem {
                    key: card.key.as_str().into(),
                    title: card.title.as_str().into(),
                    detail: card.detail.as_str().into(),
                    ..Default::default()
                })
                .collect();
            BoardLane {
                name: lane.name.as_str().into(),
                cards: ModelRc::new(VecModel::from(cards)),
            }
        })
        .collect();
    state.set_lanes(ModelRc::new(VecModel::from(lanes)));
    state.set_adding_lane(view.adding_lane.map_or(-1, |i| i as i32));
    state.set_loading(view.loading);
    state.set_error(view.error.clone().unwrap_or_default().into());
}

fn wire(ui: &AppWindow, controller: &ProjectsController) {
    let state = ui.global::<ProjectsState>();

    let c = controller.clone();
    state.on_refresh_requested(move || c.refresh());

    let c = controller.clone();
    state.on_select_board_requested(move |id| c.select_board(&id));

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_create_board_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        c.set_new_board_title(&ui.global::<ProjectsState>().get_new_board_title());
        if c.create_board() {
            ui.global::<ProjectsState>()
                .set_new_board_title(SharedString::new());
        }
    });

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_create_list_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        c.set_new_list_title(&ui.global::<ProjectsState>().get_new_list_title());
        if c.create_list() {
            ui.global::<ProjectsState>()
                .set_new_list_title(SharedString::new());
        }
    });

    // The lane index comes from the board; the controller takes the lane's list id.
    let c = controller.clone();
    state.on_start_card_requested(move |lane| {
        if let Some(list_id) = c.list_id_at(lane.max(0) as usize) {
            c.start_card(&list_id);
        }
    });

    let c = controller.clone();
    state.on_cancel_card_requested(move || c.cancel_card());

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_submit_card_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        c.set_card_title(&ui.global::<ProjectsState>().get_card_title());
        if c.submit_card() {
            ui.global::<ProjectsState>()
                .set_card_title(SharedString::new());
        }
    });

    // A drop, a keyboard step or a moved card: the board reports the lane index it goes to.
    let c = controller.clone();
    state.on_move_card_requested(move |card, lane| {
        c.move_card(&card, lane.max(0) as usize);
    });

    let c = controller.clone();
    state.on_delete_card_requested(move |card| {
        c.delete_card(&card);
    });

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());

    state.on_column_at(|x, width, gap, count| drop_column(x, width, gap, count.max(0) as usize));
}
