//! Canvas page and canvas lists (CV.6). The page with a room's messages injected through the page's handle (no session), and
//! the lists with results injected through their handle: the items and their states, that a drag sends one operation, that
//! a view-only room sends nothing, that a deleted canvas shows its error, and the list rows and the empty state.

use std::time::Duration;

use serde_json::{Map, Value, json};
use slint::{ComponentHandle as _, Model, ModelRc, VecModel};
use space_client::canvas_live::LiveEvent;
use space_proto::canvases::{
    AssetView, CanvasView, Hello, ItemState, OpAction, RoomCanvas, ServerMessage, You,
};
use space_ui::slint_ui::{CanvasListState, CanvasState, PageLoadState, TableRow, app_shell};
use space_ui::theme::ThemeMode;

const CANVAS: &str = "11111111-1111-4111-8111-111111111111";
const ROUTE: &str = "/home/canvases/11111111-1111-4111-8111-111111111111";
const TEXT: &str = "33333333-3333-4333-8333-333333333333";
const PICTURE: &str = "44444444-4444-4444-8444-444444444444";
const ASSET: &str = "22222222222222222222222222222222";

/// Moves the mock clock on, so the page's first load runs (its `init`).
fn wait(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn props(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// A text item at (100, 100), 200 by 60, drawn first (bottom).
fn text_item(version: u64) -> ItemState {
    ItemState {
        id: TEXT.into(),
        kind: "Text".into(),
        z: "a0".into(),
        version,
        props: props(json!({
            "x": 100.0, "y": 100.0, "w": 200.0, "h": 60.0,
            "text": "hello", "color": "ink", "size": 24.0, "align": "left",
        })),
        deleted: false,
        created_by: "u1".into(),
        updated_by: "u1".into(),
        updated_at: 0,
    }
}

/// A picture item at (400, 100), 300 by 200, drawn second (on top of the text).
fn picture_item(version: u64) -> ItemState {
    ItemState {
        id: PICTURE.into(),
        kind: "Image".into(),
        z: "a1".into(),
        version,
        props: props(json!({
            "x": 400.0, "y": 100.0, "w": 300.0, "h": 200.0, "asset": ASSET,
        })),
        deleted: false,
        created_by: "u1".into(),
        updated_by: "u1".into(),
        updated_at: 0,
    }
}

/// The room's hello for a viewer with `access`, holding the text and the picture.
fn hello(access: &str) -> Hello {
    Hello {
        you: You {
            conn_id: "c1".into(),
            user_id: "u1".into(),
            name: "Ada".into(),
            color: "#ff0000".into(),
            access: access.into(),
        },
        seq: 2,
        canvas: RoomCanvas {
            title: "Plan".into(),
            archived: false,
        },
        items: vec![text_item(1), picture_item(1)],
        peers: vec![],
        media: vec![],
    }
}

fn canvas_view(id: &str, title: &str, updated_at: &str) -> CanvasView {
    CanvasView {
        id: id.into(),
        title: title.into(),
        home: "personal".into(),
        owner_id: Some("u1".into()),
        project_key: None,
        created_by: "u1".into(),
        seq: 0,
        created_at: "2026-11-01T09:00:00Z".into(),
        updated_at: updated_at.into(),
        archived: false,
        my_access: "owner".into(),
    }
}

/// Builds the canvas page, lets it open, and loads the room.
fn loaded_page(access: &str) -> app_shell::App {
    crate::support::init();
    let app = app_shell::build(ROUTE, ThemeMode::Light).expect("window builds");
    wait(10);
    app.canvas.deliver_live(LiveEvent::Connected(hello(access)));
    app
}

pub fn canvas_page_shows_items_from_the_room() {
    crate::support::init();
    let app = app_shell::build(ROUTE, ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);

    app.canvas.deliver_live(LiveEvent::Connected(hello("edit")));
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    assert_eq!(state.get_connection().as_str(), "live");
    assert_eq!(state.get_title().as_str(), "Plan");

    let items = state.get_items();
    assert_eq!(items.row_count(), 2, "one row per item");
    let text = items.row_data(0).expect("text row");
    assert_eq!(text.kind.as_str(), "Text");
    assert_eq!(text.text.as_str(), "hello");
    let picture = items.row_data(1).expect("picture row");
    assert_eq!(picture.kind.as_str(), "Image");
    assert!(!picture.has_image, "the picture is still loading");

    // The picture arrives: a later change redraws the row with the picture.
    app.canvas.set_image(
        ASSET,
        slint::Image::from_rgba8(slint::SharedPixelBuffer::new(1, 1)),
    );
    app.canvas
        .deliver_live(LiveEvent::Message(ServerMessage::Applied {
            seq: 3,
            by: "u1".into(),
            item: picture_item(2),
        }));
    let picture = state.get_items().row_data(1).expect("picture row");
    assert!(picture.has_image, "the picture is drawn once it is ready");
}

pub fn dragging_an_item_sends_one_op() {
    let app = loaded_page("edit");
    let state = app.ui.global::<CanvasState>();
    state.invoke_viewport(800.0, 600.0);

    // The text item's centre, in screen px, from the camera the load fitted.
    let zoom = f64::from(state.get_zoom());
    let pan_x = f64::from(state.get_pan_x());
    let pan_y = f64::from(state.get_pan_y());
    let sx = 200.0 * zoom + pan_x;
    let sy = 130.0 * zoom + pan_y;

    state.invoke_pointer(
        "down".into(),
        sx as f32,
        sy as f32,
        "left".into(),
        false,
        false,
        false,
    );
    state.invoke_pointer(
        "move".into(),
        (sx + 40.0) as f32,
        sy as f32,
        "left".into(),
        false,
        true,
        false,
    );
    state.invoke_pointer(
        "up".into(),
        (sx + 40.0) as f32,
        sy as f32,
        "left".into(),
        false,
        true,
        false,
    );

    let ops = app.canvas.sent_ops();
    assert_eq!(ops.len(), 1, "a drag sends one operation: {ops:?}");
    let op = &ops[0];
    assert_eq!(op.item_id, TEXT);
    assert!(matches!(op.action, OpAction::Update));
    let new_x = op
        .props
        .as_ref()
        .and_then(|p| p.get("x"))
        .and_then(Value::as_f64)
        .expect("the op carries x");
    assert!(
        (new_x - 100.0 - 40.0 / zoom).abs() < 1e-6,
        "x moved by 40 screen px at this zoom: {new_x}"
    );
}

pub fn read_only_room_sends_nothing() {
    let app = loaded_page("view");
    let state = app.ui.global::<CanvasState>();
    assert!(state.get_read_only(), "a view-only viewer is read-only");
    state.invoke_viewport(800.0, 600.0);

    let zoom = f64::from(state.get_zoom());
    let sx = (200.0 * zoom + f64::from(state.get_pan_x())) as f32;
    let sy = (130.0 * zoom + f64::from(state.get_pan_y())) as f32;
    state.invoke_pointer("down".into(), sx, sy, "left".into(), false, false, false);
    state.invoke_pointer(
        "move".into(),
        sx + 40.0,
        sy,
        "left".into(),
        false,
        true,
        false,
    );
    state.invoke_pointer(
        "up".into(),
        sx + 40.0,
        sy,
        "left".into(),
        false,
        true,
        false,
    );

    assert!(
        app.canvas.sent_ops().is_empty(),
        "nothing is sent from a view-only room"
    );
}

pub fn deleted_canvas_shows_an_error() {
    let app = loaded_page("edit");
    let state = app.ui.global::<CanvasState>();
    app.canvas.deliver_live(LiveEvent::Ended {
        reason: "deleted".into(),
    });
    assert_eq!(state.get_state(), PageLoadState::Error);
    assert_eq!(state.get_error().as_str(), "This canvas was deleted.");
}

pub fn canvas_list_shows_rows_and_empty() {
    crate::support::init();
    let app = app_shell::build("/home/canvases", ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasListState>();
    assert_eq!(state.get_state(), PageLoadState::Loading);
    assert!(state.get_can_create(), "a signed-in person can create");

    app.canvas_list.deliver(
        app.canvas_list.generation(),
        Ok(vec![
            canvas_view(CANVAS, "Plan", "2026-11-18T10:30:00Z"),
            canvas_view(TEXT, "Sketches", "2026-11-17T08:05:00Z"),
        ]),
    );
    assert_eq!(state.get_state(), PageLoadState::Loaded);
    let rows = state.get_table_rows();
    assert_eq!(rows.row_count(), 2);
    let first = rows.row_data(0).expect("row").cells;
    assert_eq!(first.row_data(0).expect("name").as_str(), "Plan");
    assert_eq!(
        first.row_data(1).expect("last updated").as_str(),
        "2026-11-18 10:30",
        "the time is shown without seconds"
    );

    // A later load with no canvases shows the empty state.
    app.canvas_list
        .deliver(app.canvas_list.generation(), Ok(vec![]));
    assert_eq!(state.get_state(), PageLoadState::Empty);
    assert_eq!(state.get_table_rows().row_count(), 0);
}

pub fn list_row_actions_open_the_dialogs() {
    crate::support::init();
    let app = app_shell::build("/home/canvases", ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasListState>();
    app.canvas_list.deliver(
        app.canvas_list.generation(),
        Ok(vec![
            CanvasView {
                my_access: "manage".into(),
                ..canvas_view(CANVAS, "Plan", "2026-11-18T10:30:00Z")
            },
            canvas_view(TEXT, "Sketches", "2026-11-17T08:05:00Z"),
        ]),
    );
    // Row 0 is "Plan" (the newest), row 1 is "Sketches".
    state.invoke_pick(0);
    assert_eq!(state.get_selected_id().as_str(), CANVAS);
    assert!(
        state.get_selected_can_manage(),
        "a manager's canvas can be renamed and deleted"
    );
    state.invoke_pick(1);
    assert_eq!(state.get_selected_id().as_str(), TEXT);
    assert!(
        !state.get_selected_can_manage(),
        "an owner-only row cannot be managed"
    );

    state.invoke_pick(1);
    state.invoke_row_action(state.get_selected_id(), "delete".into());
    assert!(state.get_delete_open(), "Delete opens its dialog");
    assert_eq!(state.get_delete_title().as_str(), "Sketches");

    state.invoke_pick(0);
    state.invoke_row_action(state.get_selected_id(), "rename".into());
    assert!(state.get_rename_open(), "Rename opens its dialog");
    assert_eq!(state.get_rename_title().as_str(), "Plan");

    state.invoke_toggle_archived();
    assert!(
        state.get_show_archived(),
        "the Archived toggle shows archived canvases"
    );
}

pub fn canvas_list_sorts_by_name() {
    crate::support::init();
    let app = app_shell::build("/home/canvases", ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasListState>();
    app.canvas_list.deliver(
        app.canvas_list.generation(),
        Ok(vec![
            canvas_view(CANVAS, "beta", "2026-11-17T08:05:00Z"),
            canvas_view(TEXT, "Alpha", "2026-11-19T08:05:00Z"),
            canvas_view(PICTURE, "gamma", "2026-11-18T08:05:00Z"),
        ]),
    );
    let names = |state: &CanvasListState| -> Vec<String> {
        let rows = state.get_table_rows();
        (0..rows.row_count())
            .map(|i| {
                rows.row_data(i)
                    .expect("row")
                    .cells
                    .row_data(0)
                    .expect("name")
                    .to_string()
            })
            .collect()
    };
    assert_eq!(state.get_table_rows().row_count(), 3);
    assert_eq!(
        names(&state),
        vec!["Alpha", "gamma", "beta"],
        "the default order is last updated, newest first"
    );

    state.invoke_sort_by(0);
    assert_eq!(names(&state), vec!["Alpha", "beta", "gamma"]);
    state.invoke_sort_by(0);
    assert_eq!(names(&state), vec!["gamma", "beta", "Alpha"]);
}

pub fn canvas_list_double_click_survives_pick() {
    crate::support::init();
    let app = app_shell::build("/home/canvases", ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasListState>();
    app.canvas_list.deliver(
        app.canvas_list.generation(),
        Ok(vec![
            canvas_view(CANVAS, "Plan", "2026-11-18T10:30:00Z"),
            canvas_view(TEXT, "Sketches", "2026-11-17T08:05:00Z"),
        ]),
    );
    let names_of = |rows: &ModelRc<TableRow>| -> Vec<String> {
        (0..rows.row_count())
            .map(|i| {
                rows.row_data(i)
                    .expect("row")
                    .cells
                    .row_data(0)
                    .expect("name")
                    .to_string()
            })
            .collect()
    };

    // The rows as the list draws them before the pick: row 0 is "Plan" and nothing is selected.
    let rows_before = state.get_table_rows();
    let names_before = names_of(&rows_before);
    assert_eq!(names_before, vec!["Plan", "Sketches"]);
    assert!(rows_before.row_data(0).is_some_and(|row| !row.selected));

    state.invoke_pick(1);

    // The same rows model is kept and only the selected flags change, so the Table keeps its row elements and a
    // double-click still reaches the row it started on.
    let rows_after = state.get_table_rows();
    let before_model = rows_before
        .as_any()
        .downcast_ref::<VecModel<TableRow>>()
        .expect("the rows are the page's one model");
    let after_model = rows_after
        .as_any()
        .downcast_ref::<VecModel<TableRow>>()
        .expect("the rows are the page's one model");
    assert!(
        std::ptr::eq(before_model, after_model),
        "a pick keeps the rows model"
    );
    assert_eq!(rows_after.row_count(), 2);
    assert_eq!(
        names_of(&rows_after),
        names_before,
        "the row ids are unchanged"
    );
    assert!(!rows_after.row_data(0).expect("row 0").selected);
    assert!(rows_after.row_data(1).expect("row 1").selected);
    assert_eq!(state.get_selected_id().as_str(), TEXT);
    assert_eq!(state.get_selected_index(), 1);
}

pub fn canvas_list_share_dialog_opens() {
    crate::support::init();
    let app = app_shell::build("/home/canvases", ThemeMode::Light).expect("window builds");
    wait(10);
    let state = app.ui.global::<CanvasListState>();
    app.canvas_list.deliver(
        app.canvas_list.generation(),
        Ok(vec![canvas_view(CANVAS, "Plan", "2026-11-18T10:30:00Z")]),
    );
    assert!(!state.get_share_open());
    state.invoke_row_action(CANVAS.into(), "share".into());
    assert!(state.get_share_open(), "Share opens its dialog");
    assert_eq!(state.get_share_title().as_str(), "Plan");
}

/// An uploaded picture of 400 by 300 pixels, as the server returns it.
fn picture_asset(id: &str) -> AssetView {
    AssetView {
        id: id.into(),
        sha256: "00".into(),
        mime: "image/png".into(),
        bytes: 10,
        width: 400,
        height: 300,
        frame_count: 1,
        file_name: format!("{id}.png"),
        source_url: String::new(),
        thumbs: vec![],
        url: format!("/v1/assets/{id}"),
        created_at: String::new(),
        media: "Image".into(),
        duration_ms: None,
        has_audio: false,
    }
}

pub fn several_images_are_spread_not_stacked() {
    let app = loaded_page("edit");
    let state = app.ui.global::<CanvasState>();
    state.invoke_viewport(800.0, 600.0);
    app.canvas.add_assets_for_test(vec![
        picture_asset("a1"),
        picture_asset("a2"),
        picture_asset("a3"),
    ]);

    let ops = app.canvas.sent_ops();
    let rects: Vec<[f64; 4]> = ops
        .iter()
        .filter(|op| matches!(op.action, OpAction::Create))
        .map(|op| {
            let props = op.props.as_ref().expect("a create op carries props");
            let get = |key: &str| {
                props
                    .get(key)
                    .and_then(Value::as_f64)
                    .expect("a geometry prop")
            };
            [get("x"), get("y"), get("w"), get("h")]
        })
        .collect();
    assert_eq!(rects.len(), 3, "one create op per image: {ops:?}");
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            let apart = a[0] + a[2] <= b[0]
                || b[0] + b[2] <= a[0]
                || a[1] + a[3] <= b[1]
                || b[1] + b[3] <= a[1];
            assert!(apart, "images {a:?} and {b:?} overlap");
        }
    }
}

/// An uploaded video of 640 by 360 pixels and 65 seconds, with no thumbnail yet.
fn video_asset(id: &str) -> AssetView {
    AssetView {
        id: id.into(),
        sha256: "00".into(),
        mime: "video/mp4".into(),
        bytes: 10,
        width: 640,
        height: 360,
        frame_count: 1,
        file_name: format!("{id}.mp4"),
        source_url: String::new(),
        thumbs: vec![],
        url: format!("/v1/assets/{id}"),
        created_at: String::new(),
        media: "Video".into(),
        duration_ms: Some(65_000),
        has_audio: true,
    }
}

pub fn a_video_item_shows_its_duration_and_a_stopped_control() {
    let app = loaded_page("edit");
    let state = app.ui.global::<CanvasState>();
    state.invoke_viewport(800.0, 600.0);
    app.canvas.add_assets_for_test(vec![video_asset("v1")]);

    let items = state.get_items();
    let video = (0..items.row_count())
        .filter_map(|index| items.row_data(index))
        .find(|row| row.kind.as_str() == "Video")
        .expect("the video is placed as an item");
    assert!(video.is_video, "a video item is marked as a video");
    assert_eq!(video.duration_text.as_str(), "1:05");

    let controls = state.get_video_controls();
    assert_eq!(controls.row_count(), 1, "one control per visible video");
    let control = controls.row_data(0).expect("the video control");
    assert!(!control.playing, "nothing plays until someone presses play");
}

pub fn right_click_opens_the_cascading_menu() {
    let app = loaded_page("edit");
    let state = app.ui.global::<CanvasState>();
    state.invoke_viewport(800.0, 600.0);

    // The picture item's centre (400..700, 100..300), in screen px from the camera the load fitted.
    let zoom = f64::from(state.get_zoom());
    let sx = (550.0 * zoom + f64::from(state.get_pan_x())) as f32;
    let sy = (200.0 * zoom + f64::from(state.get_pan_y())) as f32;
    state.invoke_pointer("down".into(), sx, sy, "right".into(), false, false, false);

    assert!(state.get_menu_open(), "a right press opens the menu");
    let panels = state.get_menu_panels();
    assert_eq!(panels.row_count(), 1, "the menu opens with one panel");
    let rows = state.get_menu_rows();
    assert!(rows.row_count() > 0, "the panel has rows");
    let images = (0..rows.row_count()).find(|&index| {
        rows.row_data(index)
            .is_some_and(|row| row.label.as_str() == "Images")
    });
    let images = images.expect("the menu has an Images row");

    state.invoke_menu_hover(images as i32);
    assert_eq!(
        state.get_menu_panels().row_count(),
        2,
        "hovering a submenu row opens its panel"
    );

    state.invoke_menu_dismiss();
    assert!(!state.get_menu_open(), "a dismiss closes the menu");
}
