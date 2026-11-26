//! Decodes responses captured from the local personal.idantity.me backend.
//! The fixtures are the source of truth for the wire format.

use rimelight_personal_core::ApiError;
use rimelight_personal_core::todo::{decode_ack, decode_todo, decode_todos};

const LIST: &[u8] = include_bytes!("fixtures/list.raw");
const CREATE: &[u8] = include_bytes!("fixtures/create.raw");
const PATCH: &[u8] = include_bytes!("fixtures/patch.raw");
const DELETE: &[u8] = include_bytes!("fixtures/delete.raw");
const CREATE_INVALID: &[u8] = include_bytes!("fixtures/create_invalid.raw");

#[test]
fn list_decodes_full_rows_and_ignores_extra_fields() {
    let todos = decode_todos(LIST).expect("list fixture decodes");
    assert_eq!(todos.len(), 1);
    let todo = &todos[0];
    assert_eq!(todo.id, "1ea13715-724f-41f9-9aae-ada36cf14772");
    assert_eq!(todo.title, "poc fixture");
    assert_eq!(todo.category, "Work");
    assert_eq!(todo.priority, "high");
    assert!(!todo.is_completed);
    assert_eq!(todo.due_date, None);
}

#[test]
fn create_response_decodes_as_single_todo() {
    let todo = decode_todo(CREATE).expect("create fixture decodes");
    assert_eq!(todo.title, "poc fixture");
    assert!(!todo.is_completed);
}

#[test]
fn patch_and_delete_acks_decode_as_success() {
    decode_ack(PATCH).expect("patch fixture is success");
    decode_ack(DELETE).expect("delete fixture is success");
}

#[test]
fn list_body_is_not_a_single_todo() {
    // Guards against accidentally decoding the wrapped list as one todo.
    assert!(matches!(decode_todo(LIST), Err(ApiError::Decode(_))));
}

#[test]
fn server_error_body_is_not_a_todo() {
    // The backend returns 500 with an `error` object for an invalid POST. The client
    // reports the status before decoding, so decoding this body must fail.
    assert!(matches!(
        decode_todo(CREATE_INVALID),
        Err(ApiError::Decode(_))
    ));
}
