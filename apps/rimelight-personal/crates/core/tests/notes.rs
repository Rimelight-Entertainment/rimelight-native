//! Notes wire format, checked against responses captured from the local backend
//! (`fixtures/notes_list.raw`, `fixtures/note_create.raw`).

use idantity_core::ApiError;
use idantity_core::notes::{
    NewNote, NoteBatch, NoteBatchAction, NotePatch, decode_note, decode_notes,
};
use serde_json::json;

const LIST: &[u8] = include_bytes!("fixtures/notes_list.raw");
const CREATE: &[u8] = include_bytes!("fixtures/note_create.raw");

#[test]
fn list_decodes_flags_tags_and_updated_at() {
    let notes = decode_notes(LIST).expect("list fixture decodes");
    assert_eq!(notes.len(), 1);
    let note = &notes[0];
    assert_eq!(note.id, "6e12ed7f-692f-49c7-9659-ceb77189386a");
    assert_eq!(note.title, "fixture note");
    assert_eq!(note.content, "line one");
    assert_eq!(note.tags, vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(note.color, "info");
    assert!(note.is_pinned);
    assert!(!note.is_archived);
    assert!(!note.is_trashed);
    assert_eq!(note.updated_at.as_deref(), Some("2026-11-08T19:55:17.000Z"));
}

#[test]
fn create_response_without_timestamps_decodes() {
    // POST returns the row without `updatedAt`, so that field must not be required.
    let note = decode_note(CREATE).expect("create fixture decodes");
    assert_eq!(note.title, "fixture note");
    assert_eq!(note.updated_at, None);
}

#[test]
fn list_without_notes_key_is_a_decode_error() {
    // A 200 body that isn't the list must not silently empty the local list.
    assert!(matches!(
        decode_notes(br#"{"error":"boom"}"#),
        Err(ApiError::Decode(_))
    ));
}

#[test]
fn new_note_body_uses_the_keys_the_server_reads() {
    let tags = vec!["alpha".to_owned()];
    let body = NewNote {
        title: "Title",
        content: "Body",
        tags: &tags,
        color: "neutral",
        is_pinned: true,
    };
    assert_eq!(
        serde_json::to_value(&body).expect("serializes"),
        json!({
            "title": "Title",
            "content": "Body",
            "tags": ["alpha"],
            "color": "neutral",
            "isPinned": true
        })
    );
}

#[test]
fn patch_body_carries_only_the_fields_that_are_set() {
    let body = NotePatch {
        id: "n1",
        is_trashed: Some(true),
        is_pinned: Some(false),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&body).expect("serializes"),
        json!({"id": "n1", "isTrashed": true, "isPinned": false})
    );
}

#[test]
fn batch_body_sends_the_solid_action_names() {
    let ids = vec!["n1".to_owned(), "n2".to_owned()];
    let cases = [
        (NoteBatchAction::Archive, "batchArchive"),
        (NoteBatchAction::Trash, "batchTrash"),
        (NoteBatchAction::Restore, "batchRestore"),
        (NoteBatchAction::Delete, "batchDelete"),
    ];
    for (action, name) in cases {
        let body = NoteBatch { action, ids: &ids };
        assert_eq!(
            serde_json::to_value(&body).expect("serializes"),
            json!({"action": name, "ids": ["n1", "n2"]})
        );
    }
}
