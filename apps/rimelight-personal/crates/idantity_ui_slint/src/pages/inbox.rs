//! Glue between the Inbox controller (`idantity_feature_inbox`) and the shell's `InboxRoute`.
//!
//! The controller answers on a worker thread and reports each change through `notify`, which posts the view to the
//! event loop. Decoded pictures (RGBA) become `slint::Image`s on the event loop. Header copy goes through the window's
//! `copy-text` function. Audio and video attachments play through `rimelight_media::Player`: the viewer opens the file
//! (`sync_media`), a 16 ms timer pulls frames and the play state (`tick_media`), and the controls seek, pause and set
//! the volume.

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use idantity_feature_inbox::{
    ImageView, InboxApi, InboxController, InboxView, Shell, SystemShell, ViewerView,
};
use rimelight_media::{Player, PlayerOptions};
use slint::{
    ComponentHandle as _, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};

use crate::IdantityShell;
use crate::generated::{
    InboxAttachment, InboxChoice, InboxFolder, InboxHeader, InboxLine, InboxListRow, InboxMailbox,
    InboxMessage, InboxState,
};

rimelight_ui_kit::wire_clipboard!(crate::IdantityShell);

pub struct Inbox {
    controller: InboxController,
    /// Pulls video frames and the playback state into the window while a media attachment is open.
    _frames: Timer,
}

/// The open media attachment: its path (the viewer's key) and its player.
///
/// The player lives in a thread local: it is used only on the event loop, where the window is touched, and the glue's
/// closures cross threads only as plain views.
struct MediaSession {
    path: String,
    player: Player,
}

thread_local! {
    static MEDIA: RefCell<Option<MediaSession>> = const { RefCell::new(None) };
}

/// The frame interval for the video (about 60 frames a second). The timer runs for the life of the page.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

impl Inbox {
    /// Connects `ui` to a controller that calls `api`, with the system shell (browser, file dialogs).
    pub fn new(ui: &IdantityShell, api: Arc<dyn InboxApi>) -> Self {
        let weak = ui.as_weak();
        let notify_weak = weak.clone();
        let controller = InboxController::new(
            api,
            Arc::new(SystemShell) as Arc<dyn Shell>,
            idantity_feature_inbox::original_dir(),
            move |view: InboxView| {
                let _ = notify_weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view));
            },
        );
        wire(ui, &controller);

        let frames = Timer::default();
        frames.start(TimerMode::Repeated, FRAME_INTERVAL, move || {
            let Some(ui) = weak.upgrade() else { return };
            tick_media(&ui);
        });
        Self {
            controller,
            _frames: frames,
        }
    }

    /// The shell calls this when the Inbox route is shown: the mailbox and folder list are loaded.
    pub fn activate(&self) {
        self.controller.load();
    }

    /// Advances the controller's timers (the compose draft autosave). The shell calls this on a timer.
    pub fn tick(&self) {
        self.controller.tick();
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn text(value: &str) -> SharedString {
    SharedString::from(value)
}

/// A decoded picture as a Slint image. The RGBA bytes are copied into a shared pixel buffer.
fn to_image(picture: &ImageView) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(picture.w, picture.h);
    let bytes = buffer.make_mut_bytes();
    if bytes.len() == picture.rgba.len() {
        bytes.copy_from_slice(&picture.rgba);
    }
    Image::from_rgba8(buffer)
}

fn show_view(ui: &IdantityShell, view: &InboxView) {
    let state = ui.global::<InboxState>();
    state.set_loading(view.loading);
    state.set_banner(text(&view.banner));
    state.set_mailboxes(model(
        view.mailboxes
            .iter()
            .map(|m| InboxMailbox {
                id: text(&m.id),
                label: text(&m.label),
                email: text(&m.email),
                selected: m.selected,
            })
            .collect(),
    ));
    state.set_folders(model(
        view.folders
            .iter()
            .map(|f| InboxFolder {
                id: text(&f.id),
                label: text(&f.label),
                unread: f.unread as i32,
                custom: f.custom,
                selected: f.selected,
            })
            .collect(),
    ));
    state.set_move_targets(model(
        view.move_targets
            .iter()
            .map(|(id, label)| InboxChoice {
                id: text(id),
                label: text(label),
            })
            .collect(),
    ));
    state.set_folder(text(&view.folder));
    state.set_search(text(&view.search));
    state.set_list(model(
        view.list
            .iter()
            .map(|r| InboxListRow {
                id: text(&r.id),
                thread_key: text(&r.thread_key),
                sender: text(&r.sender),
                subject: text(&r.subject),
                snippet: text(&r.snippet),
                date: text(&r.date),
                unread: r.unread,
                starred: r.starred,
                attachments: r.attachments as i32,
                selected: r.selected,
            })
            .collect(),
    ));
    state.set_list_loading(view.list_loading);
    state.set_list_error(text(&view.list_error));
    state.set_page(view.page as i32);
    state.set_pages(view.pages as i32);
    state.set_total(view.total as i32);
    state.set_thread_key(text(&view.thread_key));
    state.set_thread_loading(view.thread_loading);
    state.set_thread_error(text(&view.thread_error));

    // Pictures are decoded once per view and shared by the lines that name them.
    let pictures: Vec<(String, Image)> = view
        .images
        .iter()
        .map(|i| (i.key.clone(), to_image(i)))
        .collect();
    let picture_for = |key: &str| {
        pictures
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, img)| img.clone())
            .unwrap_or_default()
    };
    state.set_thread(model(
        view.thread
            .iter()
            .map(|m| InboxMessage {
                id: text(&m.id),
                sender: text(&m.sender),
                to: text(&m.to),
                cc: text(&m.cc),
                date: text(&m.date),
                subject: text(&m.subject),
                starred: m.starred,
                read: m.read,
                is_draft: m.is_draft,
                lines: model(
                    m.lines
                        .iter()
                        .map(|line| InboxLine {
                            kind: i32::from(line.kind),
                            depth: line.depth as i32,
                            text: text(&line.text),
                            href: text(&line.href),
                            picture: if line.kind == 4 {
                                picture_for(&line.text)
                            } else {
                                Image::default()
                            },
                        })
                        .collect(),
                ),
                attachments: model(
                    m.attachments
                        .iter()
                        .map(|a| InboxAttachment {
                            id: text(&a.id),
                            filename: text(&a.filename),
                            size: text(&a.size),
                        })
                        .collect(),
                ),
            })
            .collect(),
    ));

    show_compose(&state, view);
    show_settings(&state, view);
    show_create(&state, view);
    show_folder_form(&state, view);
    show_confirm(&state, view);
    show_headers(&state, view);
    show_viewer(&state, view.viewer.as_ref());
    sync_media(ui, view.viewer.as_ref());
}

/// Keeps the player in step with the viewer. A media attachment (kind 2) opens its file and starts it; a different
/// file replaces the open one; leaving the viewer stops it (dropping the player).
fn sync_media(ui: &IdantityShell, viewer: Option<&ViewerView>) {
    let state = ui.global::<InboxState>();
    let wanted = viewer.filter(|v| v.kind == 2).map(|v| v.path.clone());
    MEDIA.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().map(|m| m.path.as_str()) == wanted.as_deref() {
            return;
        }
        *slot = None;
        state.set_media_playing(false);
        state.set_media_position_ms(0);
        state.set_media_duration_ms(0);
        let Some(path) = wanted else { return };
        let options = PlayerOptions {
            max_width: 960,
            max_height: 540,
            audio: true,
        };
        match Player::open(Path::new(&path), options) {
            Ok(player) => {
                player.set_volume(state.get_media_volume());
                player.play();
                *slot = Some(MediaSession { path, player });
                state.set_media_playing(true);
            }
            Err(e) => state.set_banner(format!("{e}").into()),
        }
    });
}

/// Runs on the frame timer: the newest video frame goes into the viewer, and the position, length and play state
/// follow the player. A decode or sound error shows in the banner once.
fn tick_media(ui: &IdantityShell) {
    MEDIA.with(|cell| {
        let slot = cell.borrow();
        let Some(session) = slot.as_ref() else { return };
        let state = ui.global::<InboxState>();
        if let Some(frame) = session.player.take_frame() {
            state.set_viewer_picture(frame.picture.into_slint_image());
        }
        state.set_media_position_ms(session.player.position_ms() as i32);
        state.set_media_duration_ms(session.player.duration_ms() as i32);
        state.set_media_playing(session.player.is_playing());
        if let Some(error) = session.player.error()
            && state.get_banner().is_empty()
        {
            state.set_banner(error.into());
        }
    });
}

fn show_compose(state: &InboxState<'_>, view: &InboxView) {
    state.set_compose_open(view.compose.is_some());
    let Some(c) = view.compose.as_ref() else {
        return;
    };
    state.set_compose_title(text(&c.title));
    state.set_compose_to(text(&c.to));
    state.set_compose_cc(text(&c.cc));
    state.set_compose_bcc(text(&c.bcc));
    state.set_compose_show_cc_bcc(c.show_cc_bcc);
    state.set_compose_subject(text(&c.subject));
    state.set_compose_body(text(&c.body));
    state.set_compose_status(text(&c.status));
    state.set_compose_error(text(&c.error));
    state.set_compose_sending(c.sending);
    state.set_compose_attachments(model(c.attachments.iter().map(|a| text(a)).collect()));
}

fn show_settings(state: &InboxState<'_>, view: &InboxView) {
    state.set_settings_open(view.settings.is_some());
    let s = view
        .settings
        .clone()
        .unwrap_or(idantity_feature_inbox::SettingsView {
            loading: false,
            from_name: String::new(),
            signature: String::new(),
            signature_enabled: false,
            saving: false,
            error: String::new(),
        });
    state.set_settings_loading(s.loading);
    state.set_settings_from_name(text(&s.from_name));
    state.set_settings_signature(text(&s.signature));
    state.set_settings_signature_enabled(s.signature_enabled);
    state.set_settings_saving(s.saving);
    state.set_settings_error(text(&s.error));
}

fn show_create(state: &InboxState<'_>, view: &InboxView) {
    state.set_create_open(view.create.is_some());
    let Some(c) = view.create.as_ref() else {
        return;
    };
    state.set_create_prefix(text(&c.prefix));
    state.set_create_domain(text(&c.domain));
    state.set_create_domains(model(c.domains.iter().map(|d| text(d)).collect()));
    state.set_create_name(text(&c.name));
    state.set_create_creating(c.creating);
    state.set_create_error(text(&c.error));
}

fn show_folder_form(state: &InboxState<'_>, view: &InboxView) {
    state.set_folder_form_open(view.folder_form.is_some());
    let Some(f) = view.folder_form.as_ref() else {
        return;
    };
    state.set_folder_form_rename(f.rename_of.is_some());
    state.set_folder_form_name(text(&f.name));
    state.set_folder_form_error(text(&f.error));
}

fn show_confirm(state: &InboxState<'_>, view: &InboxView) {
    state.set_confirm_open(view.confirm.is_some());
    let Some(c) = view.confirm.as_ref() else {
        return;
    };
    state.set_confirm_title(text(&c.title));
    state.set_confirm_message(text(&c.message));
}

fn show_headers(state: &InboxState<'_>, view: &InboxView) {
    state.set_headers_open(view.headers.is_some());
    let rows = view.headers.clone().unwrap_or_default();
    state.set_headers(model(
        rows.iter()
            .map(|h| InboxHeader {
                key: text(&h.key),
                value: text(&h.value),
            })
            .collect(),
    ));
}

/// The viewer: kind -1 is closed, 0 a picture, 1 text, 2 a media file at `viewer-path`.
fn show_viewer(state: &InboxState<'_>, viewer: Option<&ViewerView>) {
    let Some(v) = viewer else {
        state.set_viewer_kind(-1);
        return;
    };
    state.set_viewer_kind(i32::from(v.kind));
    state.set_viewer_title(text(&v.title));
    state.set_viewer_text(text(&v.text));
    state.set_viewer_path(text(&v.path));
    // A video's frames come from the frame timer (`tick_media`), so only pictures are set here.
    if v.kind != 2 {
        state.set_viewer_picture(v.image.as_ref().map(to_image).unwrap_or_default());
    }
}

fn wire(ui: &IdantityShell, controller: &InboxController) {
    let state = ui.global::<InboxState>();

    let c = controller.clone();
    state.on_banner_dismissed(move || c.dismiss_banner());

    let c = controller.clone();
    state.on_mailbox_requested(move |id| {
        c.select_mailbox(&id);
    });

    let c = controller.clone();
    state.on_folder_requested(move |id| c.select_folder(&id));

    let c = controller.clone();
    state.on_search_requested(move |query| c.search(&query));

    let c = controller.clone();
    state.on_page_requested(move |page| {
        c.go_page(page.max(1) as u32);
    });

    let c = controller.clone();
    state.on_thread_requested(move |key| c.select_thread(&key));

    let c = controller.clone();
    state.on_thread_closed(move || c.clear_selection());

    let c = controller.clone();
    state.on_star_requested(move |id, on| c.set_starred(&id, on));

    let c = controller.clone();
    state.on_read_requested(move |id, on| c.set_read(&id, on));

    let c = controller.clone();
    state.on_move_requested(move |id, folder| c.move_message(&id, &folder));

    let c = controller.clone();
    state.on_headers_requested(move |id| {
        c.open_headers(&id);
    });

    let c = controller.clone();
    state.on_headers_closed(move || c.close_headers());

    let c = controller.clone();
    let weak = ui.as_weak();
    state.on_header_copy_requested(move |index| {
        let (Some(ui), Some(value)) = (weak.upgrade(), c.header_value(index.max(0) as usize))
        else {
            return;
        };
        copy_text(&ui, &value);
    });

    let c = controller.clone();
    state.on_original_requested(move |id| {
        c.view_original(&id);
    });

    let c = controller.clone();
    state.on_attachment_requested(move |email, attachment| {
        c.open_attachment(&email, &attachment);
    });

    let c = controller.clone();
    state.on_new_message_requested(move || c.new_message());

    let c = controller.clone();
    state.on_reply_requested(move |id, all| {
        c.reply(&id, all);
    });

    let c = controller.clone();
    state.on_forward_requested(move |id| {
        c.forward(&id);
    });

    let c = controller.clone();
    state.on_draft_requested(move |id| {
        c.open_draft(&id);
    });

    let c = controller.clone();
    state.on_discard_draft_requested(move |id| c.ask_discard_draft(&id));

    let c = controller.clone();
    state.on_compose_field_changed(move |field, value| c.compose_field(field.max(0) as u8, &value));

    let c = controller.clone();
    state.on_compose_cc_toggled(move || c.compose_toggle_cc());

    let c = controller.clone();
    state.on_compose_markup_requested(move |kind| {
        c.compose_markup(&kind);
    });

    let c = controller.clone();
    state.on_compose_attach_requested(move || {
        c.compose_attach();
    });

    let c = controller.clone();
    state.on_compose_attachment_removed(move |index| {
        c.compose_remove_attachment(index.max(0) as usize);
    });

    let c = controller.clone();
    state.on_compose_sent(move || c.send());

    let c = controller.clone();
    state.on_compose_closed(move || c.close_compose());

    let c = controller.clone();
    state.on_settings_open_requested(move || {
        c.open_settings();
    });

    let c = controller.clone();
    state.on_settings_closed(move || c.close_settings());

    let c = controller.clone();
    state.on_settings_field_changed(move |field, value| {
        c.settings_field(field.max(0) as u8, &value)
    });

    let c = controller.clone();
    state.on_settings_signature_toggled(move |on| c.settings_signature_enabled(on));

    let c = controller.clone();
    state.on_settings_saved(move || c.save_settings());

    let c = controller.clone();
    state.on_create_open_requested(move || c.open_create());

    let c = controller.clone();
    state.on_create_closed(move || c.close_create());

    let c = controller.clone();
    state.on_create_field_changed(move |field, value| c.create_field(field.max(0) as u8, &value));

    let c = controller.clone();
    state.on_create_domain_requested(move |index| {
        c.create_domain(index.max(0) as usize);
    });

    let c = controller.clone();
    state.on_create_submitted(move || c.submit_create());

    let c = controller.clone();
    state.on_folder_create_requested(move || c.open_folder_create());

    let c = controller.clone();
    state.on_folder_rename_requested(move |id| {
        c.open_folder_rename(&id);
    });

    let c = controller.clone();
    state.on_folder_delete_requested(move |id| c.ask_delete_folder(&id));

    let c = controller.clone();
    state.on_folder_form_closed(move || c.close_folder_form());

    let c = controller.clone();
    state.on_folder_name_changed(move |text| c.folder_name(&text));

    let c = controller.clone();
    state.on_folder_submitted(move || c.submit_folder());

    let c = controller.clone();
    state.on_mailbox_delete_requested(move || {
        c.ask_delete_mailbox();
    });

    let c = controller.clone();
    state.on_confirm_yes_requested(move || c.confirm_yes());

    let c = controller.clone();
    state.on_confirm_no_requested(move || c.confirm_no());

    let c = controller.clone();
    state.on_viewer_closed(move || c.close_viewer());

    // Playback controls for the open media: play or pause (a finished file starts again), seek in ms, and volume.
    state.on_media_toggle_requested(move || {
        MEDIA.with(|cell| {
            if let Some(session) = cell.borrow().as_ref() {
                if session.player.is_playing() {
                    session.player.pause();
                } else {
                    session.player.play();
                }
            }
        });
    });
    state.on_media_seek_requested(move |ms| {
        MEDIA.with(|cell| {
            if let Some(session) = cell.borrow().as_ref() {
                session.player.seek(ms.max(0) as u64);
            }
        });
    });
    state.on_media_volume_requested(move |volume| {
        MEDIA.with(|cell| {
            if let Some(session) = cell.borrow().as_ref() {
                session.player.set_volume(volume);
            }
        });
    });
}
