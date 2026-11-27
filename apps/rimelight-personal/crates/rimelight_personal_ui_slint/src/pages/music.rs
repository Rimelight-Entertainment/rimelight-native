//! Glue between the Music controller (`rimelight_personal_feature_music`) and the shell's `MusicState` global.
//!
//! The controller calls the API on ehttp's worker thread and reports each state change through `notify`. The notify
//! callback here posts the view to the Slint event loop, and only the event loop touches the window. Two timers run on
//! the event loop: the debounce that saves typed text, and the metronome's tick while it runs.
//!
//! The page builds on every target. Native builds take the shell's platform services (opening an attachment) and the
//! window's file drops. The web build has neither OS drops nor a system opener: attachment links open in a new browser
//! tab, and the drop handling is compiled out (`crate::platform` is not compiled for wasm). The attachment picker runs
//! in the controller through `rimelight_platform::pick_files`, which returns nothing on the web.

use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use rimelight_personal_feature_music::Tab;
use rimelight_personal_feature_music::{
    ALL_STAGES, MusicApi, MusicController, MusicView, SaveStatus, Studio,
};
use slint::{ComponentHandle as _, ModelRc, Timer, TimerMode, VecModel, Weak};

use crate::generated::{
    MusicAttachmentRow, MusicBeatCell, MusicBlockItemRow, MusicBlockPair, MusicBlockRow,
    MusicExerciseRow, MusicPieceRow, MusicState, RimelightPersonalShell, SelectOption,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::platform::{FileDrop, FileDrops, Platform, Subscription};

/// Typed text is saved once the typing pauses for this long, as the egui page did.
const SAVE_DELAY: Duration = Duration::from_millis(400);
/// The metronome's highlight moves on this tick while it runs.
const METRONOME_TICK: Duration = Duration::from_millis(30);
/// "Saved" shows for this long after a save lands, then clears (egui's SAVED_FLASH).
const SAVED_FLASH: Duration = Duration::from_secs(2);
/// The banner text when a drop lands while no piece is open (egui's text).
const OPEN_PIECE_FIRST: &str = "Open a piece to attach files to it.";

/// Opens a URL: the system's handler on native, a new browser tab on the web.
type Opener = Rc<dyn Fn(&str) -> Result<(), String>>;

/// The Music page of the shell: one controller, wired to the shell's `MusicState` and its callbacks.
pub struct Music {
    controller: MusicController,
    ui: Weak<RimelightPersonalShell>,
    /// The window's file drops. Native only: the browser delivers no OS file drops.
    #[cfg(not(target_arch = "wasm32"))]
    drops: FileDrops,
    /// Held while the Music route is shown: files dropped on the window attach to the open piece. Dropping it (when
    /// another route is shown, or on [`Music::flush`]) stops the attaching.
    #[cfg(not(target_arch = "wasm32"))]
    drop_subscription: RefCell<Option<Subscription>>,
}

impl Music {
    /// Connects `ui` to a controller that calls `api`, to the platform services, and to the window's file drops.
    /// Nothing is fetched until [`Music::activate`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        ui: &RimelightPersonalShell,
        api: Arc<dyn MusicApi>,
        platform: Rc<dyn Platform>,
        drops: FileDrops,
    ) -> Self {
        let open: Opener = Rc::new(move |url: &str| platform.open_url(url));
        let (controller, weak) = Self::connect(ui, api, open);
        Self {
            controller,
            ui: weak,
            drops,
            drop_subscription: RefCell::new(None),
        }
    }

    /// The web build: there are no OS file drops, and an attachment's link opens in a new browser tab.
    #[cfg(target_arch = "wasm32")]
    pub fn new(ui: &RimelightPersonalShell, api: Arc<dyn MusicApi>) -> Self {
        let (controller, weak) = Self::connect(ui, api, Rc::new(open_in_tab));
        Self {
            controller,
            ui: weak,
        }
    }

    /// The controller and the window's weak handle, with the page wired to both.
    fn connect(
        ui: &RimelightPersonalShell,
        api: Arc<dyn MusicApi>,
        open: Opener,
    ) -> (MusicController, Weak<RimelightPersonalShell>) {
        let weak = ui.as_weak();
        let controller = MusicController::new(api, move |view: MusicView| {
            // Runs on a worker thread. The view is plain data, so it can move to the event loop.
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
        });
        // The timers are kept alive by the callbacks that start them (`wire`).
        let save_timer = Rc::new(Timer::default());
        let tick_timer = Rc::new(Timer::default());
        wire(ui, &controller, &save_timer, &tick_timer, open);
        (controller, ui.as_weak())
    }

    /// The shell calls this when a music route is shown (`/music`, `/music/piano`, `/music/guitar`, `/music/dj`). A new
    /// route resets the tab, the search and the filter, so those are set on the UI thread at once. The native page also
    /// starts taking dropped files.
    pub fn activate(&self, route: &str) {
        self.controller.activate(route);
        let Some(ui) = self.ui.upgrade() else { return };
        let view = self.controller.view();
        let state = ui.global::<MusicState>();
        state.set_search(view.search.as_str().into());
        state.set_status_filter(view.status_filter.as_str().into());
        show_view(&ui, &view);
        #[cfg(not(target_arch = "wasm32"))]
        self.subscribe_drops(&ui);
    }

    /// Starts taking the window's file drops while the page is shown.
    #[cfg(not(target_arch = "wasm32"))]
    fn subscribe_drops(&self, ui: &RimelightPersonalShell) {
        ui.global::<MusicState>()
            .set_drag_hovering(self.drops.hovering());
        if self.drop_subscription.borrow().is_none() {
            let weak = self.ui.clone();
            let controller = self.controller.clone();
            let subscription = self.drops.subscribe(move |drop| {
                if let Some(ui) = weak.upgrade() {
                    on_file_drop(&controller, &ui, drop);
                }
            });
            *self.drop_subscription.borrow_mut() = Some(subscription);
        }
    }

    /// The shell calls this when the page closes or the route changes away from music: it saves the typed text that is
    /// not saved yet, and the native page stops taking dropped files. [`Music::activate`] starts the drops again.
    pub fn flush(&self) {
        self.controller.flush();
        #[cfg(not(target_arch = "wasm32"))]
        self.drop_subscription.borrow_mut().take();
    }

    /// The controller, for tests and for callers that need to act on the page directly.
    #[cfg(test)]
    pub fn controller(&self) -> &MusicController {
        &self.controller
    }
}

/// The studio index the shell's `studio` property uses: 0 piano, 1 guitar, 2 DJ.
fn studio_index(studio: Studio) -> i32 {
    match studio {
        Studio::Piano => 0,
        Studio::Guitar => 1,
        Studio::Dj => 2,
    }
}

/// Fills the shell's Music state from a view. The lists are rebuilt only when the view's version changed (a list changed
/// shape). A save result or a metronome tick does not rebuild them, so a field the user is typing in keeps its focus.
fn show_view(ui: &RimelightPersonalShell, view: &MusicView) {
    let state = ui.global::<MusicState>();
    state.set_loaded(view.loaded);
    state.set_notice(view.notice.clone().unwrap_or_default().into());
    let (save_label, save_kind) = match shown_save_status(ui, view.save) {
        SaveStatus::Idle => ("", 0),
        SaveStatus::Saving => ("Saving...", 1),
        SaveStatus::Saved => ("Saved ✓", 2),
        SaveStatus::Error => ("Save error", 3),
    };
    state.set_save_label(save_label.into());
    state.set_save_kind(save_kind);
    state.set_tab(view.tab.index());
    state.set_tab_strip(view.tab_strip);
    state.set_studio(studio_index(view.studio));
    state.set_piece_count(view.piece_count as i32);

    let metronome = &view.metronome;
    state.set_metronome_bpm(metronome.bpm as f32);
    state.set_metronome_bpm_whole(metronome.bpm.round() as i32);
    state.set_metronome_running(metronome.running);
    state.set_metronome_beats(metronome.beats as i32);
    state.set_metronome_subdivision(metronome.subdivision as i32);
    state.set_metronome_lit(metronome.lit_beat.map_or(-1, |beat| beat as i32));
    state.set_metronome_total(metronome.total as i32);
    state.set_countdown(metronome.countdown);
    state.set_remaining_text(metronome.remaining_text.as_str().into());

    let version = view.version as i32;
    if state.get_version() == version {
        return;
    }
    state.set_version(version);
    rebuild_lists(ui, view);
}

/// Where the "Saved" flash stands. A save result starts it; it then clears after [`SAVED_FLASH`] and stays cleared until
/// the next save, although the controller's status still reads `Saved` until then.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flash {
    /// No save has landed since the last non-saved status.
    Pending,
    /// The flash is showing; its timer is running.
    Showing,
    /// The flash has cleared.
    Expired,
}

struct SaveFlash {
    state: Flash,
    timer: Timer,
}

thread_local! {
    /// The "Saved" flash. Only the event loop thread shows views, so only that thread touches this.
    static FLASH: RefCell<Option<SaveFlash>> = const { RefCell::new(None) };
}

/// The save status the page shows: `Saved` for [`SAVED_FLASH`] after a save lands, then `Idle`.
fn shown_save_status(ui: &RimelightPersonalShell, save: SaveStatus) -> SaveStatus {
    FLASH.with(|cell| {
        let mut slot = cell.borrow_mut();
        let flash = slot.get_or_insert_with(|| SaveFlash {
            state: Flash::Pending,
            timer: Timer::default(),
        });
        if save != SaveStatus::Saved {
            flash.state = Flash::Pending;
            return save;
        }
        match flash.state {
            Flash::Pending => {
                flash.state = Flash::Showing;
                let weak = ui.as_weak();
                flash
                    .timer
                    .start(TimerMode::SingleShot, SAVED_FLASH, move || {
                        FLASH.with(|cell| {
                            if let Some(flash) = cell.borrow_mut().as_mut() {
                                flash.state = Flash::Expired;
                            }
                        });
                        if let Some(ui) = weak.upgrade() {
                            ui.global::<MusicState>().set_save_label("".into());
                            ui.global::<MusicState>().set_save_kind(0);
                        }
                    });
                SaveStatus::Saved
            }
            Flash::Showing => SaveStatus::Saved,
            Flash::Expired => SaveStatus::Idle,
        }
    })
}

/// Builds the row models. A new model each time, so every row starts again from the controller's state.
fn rebuild_lists(ui: &RimelightPersonalShell, view: &MusicView) {
    let state = ui.global::<MusicState>();
    let exercises: Vec<MusicExerciseRow> = view
        .exercises
        .iter()
        .map(|row| MusicExerciseRow {
            id: row.id.as_str().into(),
            name: row.name.as_str().into(),
            bpm_text: row.bpm_text.as_str().into(),
            target_text: row.target_text.as_str().into(),
            done: row.done,
            can_up: row.can_up,
            can_down: row.can_down,
        })
        .collect();
    state.set_exercises(ModelRc::new(VecModel::from(exercises)));

    let pieces: Vec<MusicPieceRow> = view
        .pieces
        .iter()
        .map(|piece| MusicPieceRow {
            id: piece.id.as_str().into(),
            title: piece.title.as_str().into(),
            composer: piece.composer.as_str().into(),
            status: piece.status.as_str().into(),
            difficulty: piece.difficulty as i32,
            expanded: piece.expanded,
            uploading: piece.uploading,
            attachments: ModelRc::new(VecModel::from(
                piece
                    .attachments
                    .iter()
                    .map(|att| MusicAttachmentRow {
                        id: att.id.as_str().into(),
                        name: att.name.as_str().into(),
                        audio: att.audio,
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    state.set_pieces(ModelRc::new(VecModel::from(pieces)));

    let stage_options: Vec<SelectOption> = view
        .stages
        .iter()
        .map(|stage| option(stage, stage))
        .collect();
    state.set_stage_options(ModelRc::new(VecModel::from(stage_options)));
    let mut filter_options = vec![option(ALL_STAGES, "All Stages")];
    filter_options.extend(view.stages.iter().map(|stage| option(stage, stage)));
    state.set_filter_options(ModelRc::new(VecModel::from(filter_options)));

    let blocks: Vec<MusicBlockRow> = view
        .blocks
        .iter()
        .map(|block| MusicBlockRow {
            id: block.id.as_str().into(),
            name: block.name.as_str().into(),
            items: ModelRc::new(VecModel::from(
                block
                    .items
                    .iter()
                    .map(|item| MusicBlockItemRow {
                        id: item.id.as_str().into(),
                        name: item.name.as_str().into(),
                        done: item.done,
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    // Two columns, row by row: blocks 0 and 1 share the first row, 2 and 3 the second, and so on. An odd last block
    // leaves its row's right cell empty.
    let mut pairs = Vec::new();
    let mut rest = blocks.iter().cloned();
    while let Some(left) = rest.next() {
        let right = rest.next();
        pairs.push(MusicBlockPair {
            has_right: right.is_some(),
            right: right.unwrap_or_default(),
            left,
        });
    }
    state.set_block_pairs(ModelRc::new(VecModel::from(pairs)));
    state.set_blocks(ModelRc::new(VecModel::from(blocks)));

    let cells: Vec<MusicBeatCell> = view
        .metronome
        .cells
        .iter()
        .map(|cell| MusicBeatCell {
            label: cell.label.as_str().into(),
            kind: cell.kind,
        })
        .collect();
    state.set_metronome_cells(ModelRc::new(VecModel::from(cells)));
}

fn option(value: &str, label: &str) -> SelectOption {
    SelectOption {
        value: value.into(),
        label: label.into(),
        ..Default::default()
    }
}

/// Restarts the save debounce: the whole blob is saved once typing has paused.
fn schedule_save(timer: &Timer, controller: &MusicController) {
    let c = controller.clone();
    timer.start(TimerMode::SingleShot, SAVE_DELAY, move || c.save_now());
}

/// Connects the shell's Music callbacks to the controller.
fn wire(
    ui: &RimelightPersonalShell,
    controller: &MusicController,
    save: &Rc<Timer>,
    tick: &Rc<Timer>,
    open: Opener,
) {
    let state = ui.global::<MusicState>();

    let c = controller.clone();
    state.on_music_tab_requested(move |index| c.select_tab_strip(index));

    let c = controller.clone();
    state.on_music_dismiss_notice_requested(move || c.dismiss_notice());

    // Exercises.
    let c = controller.clone();
    state.on_music_exercise_add_requested(move || c.add_exercise());
    let c = controller.clone();
    state.on_music_exercise_move_requested(move |id, up| c.move_exercise(&id, up));
    let c = controller.clone();
    state.on_music_exercise_remove_requested(move |id| c.remove_exercise(&id));
    let c = controller.clone();
    state.on_music_exercise_check_changed(move |id, done| c.set_exercise_checked(&id, done));
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_exercise_name_edited(move |id, text| {
        c.edit_exercise_name(&id, &text);
        schedule_save(&s, &c);
    });
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_exercise_bpm_edited(move |id, text| {
        c.edit_exercise_bpm(&id, &text);
        schedule_save(&s, &c);
    });
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_exercise_target_edited(move |id, text| {
        c.edit_exercise_target(&id, &text);
        schedule_save(&s, &c);
    });

    // Repertoire.
    let c = controller.clone();
    state.on_music_search_edited(move |text| c.set_search(&text));
    let c = controller.clone();
    state.on_music_filter_picked(move |filter| c.set_status_filter(&filter));
    let c = controller.clone();
    state.on_music_piece_add_requested(move || c.add_piece());
    let c = controller.clone();
    state.on_music_piece_expand_requested(move |id| c.toggle_expanded(&id));
    let c = controller.clone();
    state.on_music_piece_remove_requested(move |id| c.remove_piece(&id));
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_piece_title_edited(move |id, text| {
        c.edit_piece_title(&id, &text);
        schedule_save(&s, &c);
    });
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_piece_composer_edited(move |id, text| {
        c.edit_piece_composer(&id, &text);
        schedule_save(&s, &c);
    });
    let c = controller.clone();
    state.on_music_piece_status_picked(move |id, status| c.set_piece_status(&id, &status));
    let c = controller.clone();
    state.on_music_piece_difficulty_requested(move |id, value| {
        c.set_difficulty(&id, i64::from(value))
    });
    let (c, o) = (controller.clone(), open.clone());
    state.on_music_attachment_open_requested(move |attachment| {
        open_attachment(&c, o.as_ref(), &attachment)
    });
    let c = controller.clone();
    state.on_music_attachment_remove_requested(move |piece, attachment| {
        c.remove_attachment(&piece, &attachment)
    });
    // The system file dialog; the files picked attach to the piece as a drop does.
    let c = controller.clone();
    state.on_music_attachment_pick_requested(move |piece| {
        c.pick_attachments(&piece);
    });

    // Daily routine.
    let c = controller.clone();
    state.on_music_block_add_requested(move || c.add_block());
    let c = controller.clone();
    state.on_music_block_remove_requested(move |id| c.remove_block(&id));
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_block_name_edited(move |id, text| {
        c.edit_block_name(&id, &text);
        schedule_save(&s, &c);
    });
    let c = controller.clone();
    state.on_music_block_item_add_requested(move |block| c.add_block_item(&block));
    let c = controller.clone();
    state.on_music_block_item_remove_requested(move |block, item| {
        c.remove_block_item(&block, &item)
    });
    let (c, s) = (controller.clone(), save.clone());
    state.on_music_block_item_edited(move |block, item, text| {
        c.edit_block_item(&block, &item, &text);
        schedule_save(&s, &c);
    });
    let c = controller.clone();
    state
        .on_music_block_item_check_changed(move |item, done| c.set_block_item_checked(&item, done));

    // Metronome.
    let c = controller.clone();
    state.on_music_metronome_tap_requested(move || c.metronome_tap());
    let (c, tick) = (controller.clone(), tick.clone());
    state.on_music_metronome_toggle_requested(move || {
        c.metronome_toggle();
        if c.view().metronome.running {
            let c = c.clone();
            let handle = tick.clone();
            tick.start(TimerMode::Repeated, METRONOME_TICK, move || {
                if !c.metronome_tick() {
                    handle.stop();
                }
            });
        } else {
            tick.stop();
        }
    });
    let c = controller.clone();
    state.on_music_metronome_bpm_changed(move |bpm| c.metronome_set_bpm(f64::from(bpm)));
    let c = controller.clone();
    state.on_music_metronome_step_requested(move |delta| c.metronome_step_bpm(f64::from(delta)));
    let c = controller.clone();
    state
        .on_music_metronome_beats_picked(move |beats| c.metronome_set_beats(beats.max(0) as usize));
    let c = controller.clone();
    state.on_music_metronome_subdivision_picked(move |index| {
        c.metronome_set_subdivision(index.max(0) as usize)
    });
    let c = controller.clone();
    state
        .on_music_metronome_beat_cycled(move |index| c.metronome_cycle_beat(index.max(0) as usize));
    let c = controller.clone();
    state.on_music_metronome_countdown_changed(move |on| c.metronome_set_countdown(on));
}

/// A file-drop event on the Music route, as egui handled it: the drop zones highlight while a file hovers, and a dropped
/// file attaches to the open piece on the Repertoire tab. With no piece open, the drop shows a notice.
#[cfg(not(target_arch = "wasm32"))]
fn on_file_drop(controller: &MusicController, ui: &RimelightPersonalShell, drop: &FileDrop) {
    match drop {
        FileDrop::Hovered(_) => ui.global::<MusicState>().set_drag_hovering(true),
        FileDrop::HoveredCancelled => ui.global::<MusicState>().set_drag_hovering(false),
        FileDrop::Dropped(path) => {
            ui.global::<MusicState>().set_drag_hovering(false);
            if controller.view().tab != Tab::Repertoire {
                return;
            }
            match controller.expanded_piece() {
                Some(piece) => attach_path(controller, &piece, path),
                None => controller.report(OPEN_PIECE_FIRST),
            }
        }
    }
}

/// Reads the file at `path` off the UI thread, then attaches it to the piece `piece`.
#[cfg(not(target_arch = "wasm32"))]
fn attach_path(controller: &MusicController, piece: &str, path: &Path) {
    let c = controller.clone();
    let piece = piece.to_owned();
    let path = path.to_owned();
    std::thread::spawn(move || {
        let name = path.file_name().map_or_else(
            || "attachment".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        match std::fs::read(&path) {
            Ok(bytes) => c.attach_file(&piece, &name, &bytes),
            Err(e) => c.report(&format!("Could not read {name}: {e}")),
        }
    });
}

/// Opens a stored attachment in the system, as egui's link did. A failure shows in the notice.
fn open_attachment(
    controller: &MusicController,
    open: &dyn Fn(&str) -> Result<(), String>,
    attachment: &str,
) {
    let Some(url) = controller.attachment_url(attachment) else {
        return;
    };
    if let Err(e) = open(&url) {
        controller.report(&format!("Could not open {url}: {e}"));
    }
}

/// Opens a URL in a new browser tab (the web build's opener).
#[cfg(target_arch = "wasm32")]
fn open_in_tab(url: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "no browser window".to_owned())?;
    window
        .open_with_url_and_target(url, "_blank")
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests {
    use std::sync::Mutex;

    use rimelight_personal_core::ApiError;
    use rimelight_personal_core::music::MusicData;
    use rimelight_personal_feature_music::MusicApi;
    use slint::Model as _;

    use super::*;
    use crate::platform::fake::FakePlatform;

    /// Records the calls the page makes and answers each one at once with the seeded blob.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    impl Recorder {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_owned());
        }
    }

    impl MusicApi for Recorder {
        fn load(&self, on_done: Box<dyn FnOnce(Result<MusicData, ApiError>) + Send>) {
            self.record("load");
            on_done(Ok(MusicData::seed()));
        }

        fn save(&self, _data: &MusicData, on_done: Box<dyn FnOnce(Result<(), ApiError>) + Send>) {
            self.record("save");
            on_done(Ok(()));
        }

        fn upload(
            &self,
            name: &str,
            _mime: &str,
            _bytes: &[u8],
            on_done: Box<dyn FnOnce(Result<String, ApiError>) + Send>,
        ) {
            self.record(&format!("upload:{name}"));
            on_done(Ok(format!("/api/storage/files?key={name}")));
        }

        fn resolve_url(&self, stored: &str) -> String {
            format!("https://example.test{stored}")
        }
    }

    /// Waits until the upload thread has run `upload` for `name`: a dropped file is read off the UI thread.
    fn wait_for_upload(api: &Recorder, name: &str) {
        let call = format!("upload:{name}");
        wait_until(|| api.calls().contains(&call));
    }

    /// Polls `done` until it holds, for work the page hands to a thread. Panics after five seconds.
    fn wait_until(done: impl Fn() -> bool) {
        for _ in 0..1000 {
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the background work never finished");
    }

    /// Opening the page loads the blob. Adding an exercise saves and the page shows the new row. Typed text reaches the
    /// controller without a rebuild, and the metronome's running flag follows its state. A dropped file goes to the
    /// open piece, and the platform opens the attachment's link. Run by `pages::tests::page_scenarios`, on the thread
    /// that installed the testing backend.
    pub(crate) fn scenario() {
        let ui = RimelightPersonalShell::new().expect("create the shell");
        let api = Arc::new(Recorder::default());
        let platform = Rc::new(FakePlatform::default());
        let drops = FileDrops::default();
        let music = Music::new(&ui, api.clone(), platform.clone(), drops.clone());
        ui.set_route("/music".into());

        music.activate("/music");
        show_view(&ui, &music.controller().view());
        assert_eq!(api.calls(), ["load"]);
        assert!(ui.global::<MusicState>().get_loaded());
        assert_eq!(ui.global::<MusicState>().get_exercises().row_count(), 2);

        // Exercises is the first tab. Adding an exercise saves and shows the new row.
        ui.global::<MusicState>().invoke_music_tab_requested(0);
        show_view(&ui, &music.controller().view());
        ui.global::<MusicState>()
            .invoke_music_exercise_add_requested();
        show_view(&ui, &music.controller().view());
        assert!(
            api.calls().contains(&"save".to_owned()),
            "{:?}",
            api.calls()
        );
        assert_eq!(ui.global::<MusicState>().get_exercises().row_count(), 3);
        assert_eq!(
            ui.global::<MusicState>().get_save_kind(),
            2,
            "the save shows Saved"
        );

        // Typing: the controller keeps the text, and nothing is sent until the debounce fires.
        let id = music.controller().view().exercises[0].id.clone();
        ui.global::<MusicState>()
            .invoke_music_exercise_name_edited(id.as_str().into(), "Scale draft".into());
        assert_eq!(music.controller().view().exercises[0].name, "Scale draft");

        // Metronome: Start runs the tick timer, and the page shows the running state.
        ui.global::<MusicState>().invoke_music_tab_requested(3);
        show_view(&ui, &music.controller().view());
        ui.global::<MusicState>()
            .invoke_music_metronome_toggle_requested();
        show_view(&ui, &music.controller().view());
        assert!(ui.global::<MusicState>().get_metronome_running());
        ui.global::<MusicState>()
            .invoke_music_metronome_toggle_requested();
        show_view(&ui, &music.controller().view());
        assert!(!ui.global::<MusicState>().get_metronome_running());
        assert_eq!(ui.global::<MusicState>().get_metronome_beats(), 4);

        // Daily routine: the three blocks sit in two columns; the odd third block leaves its row's right cell empty.
        ui.global::<MusicState>().invoke_music_tab_requested(2);
        show_view(&ui, &music.controller().view());
        let pairs = ui.global::<MusicState>().get_block_pairs();
        assert_eq!(pairs.row_count(), 2);
        let first = pairs.row_data(0).expect("a row");
        let second = pairs.row_data(1).expect("a row");
        assert_eq!(first.left.name.as_str(), "Warmup");
        assert!(first.has_right);
        assert_eq!(first.right.name.as_str(), "Sight Reading");
        assert_eq!(second.left.name.as_str(), "Repertoire");
        assert!(
            !second.has_right,
            "an odd block leaves its row's right cell empty"
        );

        // Repertoire: a dropped file goes to the open piece. No piece is open yet, so the drop shows the notice.
        ui.global::<MusicState>().invoke_music_tab_requested(1);
        show_view(&ui, &music.controller().view());
        let file = std::env::temp_dir().join("rimelight-personal-music-test-take-one.mp3");
        std::fs::write(&file, [1u8, 2, 3]).expect("write the dropped file");
        drops.dispatch(&FileDrop::Hovered(file.clone()));
        assert!(drops.hovering());
        show_view(&ui, &music.controller().view());
        assert!(
            ui.global::<MusicState>().get_drag_hovering(),
            "the drop zone highlights while a file hovers"
        );
        drops.dispatch(&FileDrop::HoveredCancelled);
        assert!(!drops.hovering());
        assert!(!ui.global::<MusicState>().get_drag_hovering());
        drops.dispatch(&FileDrop::Dropped(file.clone()));
        show_view(&ui, &music.controller().view());
        assert_eq!(
            ui.global::<MusicState>().get_notice().as_str(),
            OPEN_PIECE_FIRST
        );
        assert!(
            api.calls().iter().all(|call| !call.starts_with("upload")),
            "{:?}",
            api.calls()
        );

        // Open Clair de Lune, then drop the file on it: it uploads, and the attachment appears.
        ui.global::<MusicState>()
            .invoke_music_piece_expand_requested("pr1".into());
        show_view(&ui, &music.controller().view());
        drops.dispatch(&FileDrop::Dropped(file.clone()));
        wait_for_upload(&api, "rimelight-personal-music-test-take-one.mp3");
        wait_until(|| music.controller().view().save == SaveStatus::Saved);
        show_view(&ui, &music.controller().view());
        let attached = music.controller().view();
        let piece = attached
            .pieces
            .iter()
            .find(|p| p.id == "pr1")
            .expect("Clair de Lune");
        assert_eq!(piece.attachments.len(), 1);
        assert_eq!(
            piece.attachments[0].name,
            "rimelight-personal-music-test-take-one.mp3"
        );

        // The attachment's link opens its stored URL through the platform.
        ui.global::<MusicState>()
            .invoke_music_attachment_open_requested(piece.attachments[0].id.as_str().into());
        assert_eq!(
            *platform.opens.borrow(),
            [
                "https://example.test/api/storage/files?key=rimelight-personal-music-test-take-one.mp3"
            ]
        );

        // The save flash: a save shows "Saved" until two seconds pass, then clears.
        assert_eq!(
            ui.global::<MusicState>().get_save_kind(),
            2,
            "the attachment's save shows Saved"
        );
        slint::platform::update_timers_and_animations();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2100));
        slint::platform::update_timers_and_animations();
        assert_eq!(
            ui.global::<MusicState>().get_save_kind(),
            0,
            "Saved clears after two seconds"
        );
        assert_eq!(ui.global::<MusicState>().get_save_label().as_str(), "");

        // Leaving the Music route stops the drops: the next drop uploads nothing.
        music.flush();
        let calls_before = api.calls().len();
        drops.dispatch(&FileDrop::Dropped(file.clone()));
        assert_eq!(
            api.calls().len(),
            calls_before,
            "a drop off the Music route is not taken"
        );
        let _ = std::fs::remove_file(&file);
    }
}
