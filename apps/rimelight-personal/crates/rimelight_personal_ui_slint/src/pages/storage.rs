//! Glue between the Storage controller (`rimelight_personal_feature_storage`) and the shell's `StorageRoute`.
//!
//! The controller answers on a worker thread and reports each change through `notify`, which posts the view to the
//! event loop. Thumbnails (RGBA) become `slint::Image`s on the event loop. Uploads pick files with
//! `rimelight_platform::pick_files`. Copy goes through the window's `copy-text` function, and the "copied" mark clears
//! after two seconds.
//!
//! OS file drops: the shell calls [`Storage::files_dropped`] with the dropped paths. Finding the drop event in Slint
//! 1.18's winit backend is the shell's job (the desktop app does it in `platform::FileDrops`); see the report.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rimelight_personal_feature_storage::{
    StorageApi, StorageController, StorageView, Thumb, TypeFilter,
};
use slint::Model as _;
use slint::{
    ComponentHandle as _, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};

use crate::AppWindow;
use crate::generated::{
    SelectOption, StorageCrumb, StorageFileRow, StorageFolderCard, StorageFolderRow,
    StorageInspector, StorageState,
};

rimelight_ui_kit::wire_clipboard!(crate::AppWindow);

/// How long the "copied" mark stays on a file card.
const COPIED_FOR: Duration = Duration::from_secs(2);

pub struct Storage {
    controller: StorageController,
}

impl Storage {
    /// Connects `ui` to a controller that calls `api`. Nothing is fetched until [`Storage::activate`].
    pub fn new(ui: &AppWindow, api: Arc<dyn StorageApi>) -> Self {
        // The notify closure is built before the controller exists, so it reads the controller from this slot. The
        // slot is filled right after construction; the first view is only ever sent by a call made after that.
        let slot: Arc<OnceLock<StorageController>> = Arc::new(OnceLock::new());
        let weak = ui.as_weak();
        let notify_slot = slot.clone();
        let controller = StorageController::new(api, move |view: StorageView| {
            let slot = notify_slot.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, slot.get(), &view));
        });
        let _ = slot.set(controller.clone());
        wire(ui, &controller);
        Self { controller }
    }

    /// The shell calls this when the Storage route is shown.
    pub fn activate(&self) {
        self.controller.refresh();
    }

    /// The shell calls this with the paths of files dropped on the window.
    pub fn files_dropped(&self, paths: &[String]) -> bool {
        self.controller.files_dropped(paths)
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn text(value: &str) -> SharedString {
    SharedString::from(value)
}

/// A decoded preview as a Slint image.
fn to_image(thumb: &Thumb) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(thumb.width, thumb.height);
    let bytes = buffer.make_mut_bytes();
    if bytes.len() == thumb.rgba.len() {
        bytes.copy_from_slice(&thumb.rgba);
    }
    Image::from_rgba8(buffer)
}

fn show_view(
    ui: &AppWindow,
    controller: Option<&StorageController>,
    view: &StorageView,
) {
    let state = ui.global::<StorageState>();

    state.set_folders(model(
        view.folders
            .iter()
            .map(|f| StorageFolderRow {
                path: text(&f.path),
                label: text(&f.label),
                depth: f.depth as i32,
                count: f.count as i32,
                selected: f.selected,
            })
            .collect(),
    ));
    state.set_root_count(view.root_count as i32);
    state.set_root_selected(view.root_selected);
    state.set_crumbs(model(
        view.crumbs
            .iter()
            .map(|c| StorageCrumb {
                label: text(&c.label),
                path: text(&c.path),
            })
            .collect(),
    ));
    state.set_subfolders(model(
        view.subfolders
            .iter()
            .map(|s| StorageFolderCard {
                path: text(&s.path),
                name: text(&s.name),
                count: s.count as i32,
            })
            .collect(),
    ));
    state.set_files(model(
        view.files
            .iter()
            .map(|f| {
                let preview = if f.has_preview {
                    controller
                        .as_ref()
                        .and_then(|c| c.thumbnail(&f.id))
                        .map(|t| to_image(&t))
                } else {
                    None
                };
                StorageFileRow {
                    id: text(&f.id),
                    name: text(&f.name),
                    key: text(&f.key),
                    url: text(&f.url),
                    size: text(&f.size),
                    mime: text(&f.mime),
                    ext: text(&f.ext),
                    kind: text(&f.kind),
                    nested: f.nested,
                    selected: f.selected,
                    copied: f.copied,
                    duration: text(&f.duration),
                    has_preview: preview.is_some(),
                    preview: preview.unwrap_or_default(),
                    playing: f.playing,
                }
            })
            .collect(),
    ));
    state.set_table_view(view.table);
    state.set_filter(view.filter as i32);
    state.set_selected_count(view.selected_count as i32);
    state.set_all_visible_selected(view.all_visible_selected);
    state.set_loading(view.loading);
    state.set_uploading(view.uploading);
    state.set_error(view.error.clone().unwrap_or_default().into());
    state.set_folder_empty(view.folder_empty);
    state.set_new_folder_open(view.new_folder_open);
    state.set_confirm_message(view.confirm.clone().unwrap_or_default().into());
    state.set_query(text(&view.query));
    state.set_new_folder_name(text(&view.new_folder_name));

    match view.inspector.as_ref() {
        Some(insp) => {
            let preview = controller
                .as_ref()
                .and_then(|c| c.thumbnail(&insp.id))
                .map(|t| to_image(&t));
            state.set_inspector(StorageInspector {
                id: text(&insp.id),
                name: text(&insp.name),
                key: text(&insp.key),
                url: text(&insp.url),
                size: text(&insp.size),
                mime: text(&insp.mime),
                ext: text(&insp.ext),
                kind: text(&insp.kind),
                basename: text(&insp.basename),
                folder: text(&insp.folder),
                folder_options: model(
                    insp.folder_options
                        .iter()
                        .map(|o| SelectOption {
                            value: text(&o.value),
                            label: text(&o.label),
                            ..Default::default()
                        })
                        .collect(),
                ),
                preview_key: text(&insp.preview_key),
                saving: insp.saving,
                has_preview: preview.is_some(),
                preview: preview.unwrap_or_default(),
            });
        }
        None => state.set_inspector(StorageInspector::default()),
    }
}

fn wire(ui: &AppWindow, controller: &StorageController) {
    let state = ui.global::<StorageState>();

    let c = controller.clone();
    state.on_refresh_requested(move || c.refresh());

    let c = controller.clone();
    state.on_open_folder_requested(move |path| c.open_folder(&path));

    let c = controller.clone();
    state.on_query_changed(move |query| c.set_query(&query));

    let c = controller.clone();
    state.on_filter_requested(move |index| {
        c.set_filter(TypeFilter::from_index(index.max(0) as usize))
    });

    let c = controller.clone();
    state.on_table_requested(move |on| c.set_table(on));

    let c = controller.clone();
    state.on_toggle_select_requested(move |key| c.toggle_select(&key));

    let c = controller.clone();
    state.on_select_visible_requested(move |on| c.set_select_visible(on));

    let c = controller.clone();
    state.on_new_folder_requested(move || c.open_new_folder());

    let c = controller.clone();
    state.on_new_folder_name_changed(move |name| c.set_new_folder_name(&name));

    let c = controller.clone();
    state.on_create_folder_requested(move || {
        c.create_folder();
    });

    let c = controller.clone();
    state.on_close_new_folder_requested(move || c.close_new_folder());

    let c = controller.clone();
    state.on_upload_requested(move || {
        // The file dialog runs on the event loop, then the picked files upload through the controller.
        let paths = rimelight_platform::pick_files(&[], true);
        if !paths.is_empty() {
            c.upload_paths(&paths);
        }
    });

    let c = controller.clone();
    state.on_open_detail_requested(move |id| c.open_detail(&id));

    let c = controller.clone();
    state.on_close_detail_requested(move || c.close_detail());

    let c = controller.clone();
    state.on_detail_basename_changed(move |name| c.set_detail_basename(&name));

    let c = controller.clone();
    state.on_detail_folder_changed(move |folder| c.set_detail_folder(&folder));

    let c = controller.clone();
    state.on_save_detail_requested(move || {
        c.save_detail();
    });

    // Copy the file's URL, then clear the "copied" mark after two seconds.
    let copied = Rc::new(Timer::default());
    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_copy_requested(move |key| {
        let (Some(ui), Some(url)) = (weak.upgrade(), c.copy_url(&key)) else {
            return;
        };
        copy_text(&ui, &url);
        let clear = c.clone();
        copied.start(TimerMode::SingleShot, COPIED_FOR, move || {
            clear.clear_copied()
        });
    });

    let c = controller.clone();
    state.on_delete_requested(move |id| {
        c.ask_delete(&id);
    });

    let c = controller.clone();
    state.on_delete_selected_requested(move || {
        c.ask_delete_selected();
    });

    let c = controller.clone();
    state.on_confirm_delete_requested(move || {
        c.confirm_delete();
    });

    let c = controller.clone();
    state.on_cancel_delete_requested(move || c.cancel_delete());

    let c = controller.clone();
    state.on_dismiss_error_requested(move || c.dismiss_error());

    let c = controller.clone();
    state.on_move_requested(move |id, dest| {
        c.move_to_folder(&id, &dest);
    });

    let c = controller.clone();
    state.on_toggle_audio_requested(move |id| {
        c.toggle_audio(&id);
    });

    let c = controller.clone();
    state.on_previews_visible_requested(move |ids| {
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        c.ensure_previews(&ids);
    });
}
