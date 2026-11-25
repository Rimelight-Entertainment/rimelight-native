//! Turning edits into live-room ops.
//!
//! A create carries the kind, the z key and every prop. An update carries the base version, the z key only when it
//! changed, and only the props that changed (a removed prop is sent as JSON null). A delete carries the base version.

use crate::model::Item;
use serde_json::{Map, Value};
use space_proto::canvases::{ClientOp, OpAction};

/// The props that differ: changed or added keys with their new value, removed keys as JSON null.
pub fn props_patch(old: &Map<String, Value>, new: &Map<String, Value>) -> Map<String, Value> {
    let mut patch = Map::new();
    for (key, value) in new {
        if old.get(key) != Some(value) {
            patch.insert(key.clone(), value.clone());
        }
    }
    for key in old.keys() {
        if !new.contains_key(key) {
            patch.insert(key.clone(), Value::Null);
        }
    }
    patch
}

/// The op that creates an item: base 0, with kind, z and every prop.
pub fn create_op(op_id: String, item: &Item) -> ClientOp {
    ClientOp {
        op_id,
        item_id: item.id.clone(),
        base: 0,
        action: OpAction::Create,
        kind: Some(item.kind().as_str().to_string()),
        z: Some(item.z.clone()),
        props: Some(item.to_props()),
    }
}

/// The op that changes an item from `before` to `after`, based on `before.version`. None when nothing changed.
/// `z` is included only when it changed; `props` only when the patch is not empty.
pub fn update_op(op_id: String, before: &Item, after: &Item) -> Option<ClientOp> {
    let patch = props_patch(&before.to_props(), &after.to_props());
    let z = (before.z != after.z).then(|| after.z.clone());
    if patch.is_empty() && z.is_none() {
        return None;
    }
    Some(ClientOp {
        op_id,
        item_id: before.id.clone(),
        base: before.version,
        action: OpAction::Update,
        kind: None,
        z,
        props: (!patch.is_empty()).then_some(patch),
    })
}

/// The op that deletes an item: base is the item's version; no kind, z or props.
pub fn delete_op(op_id: String, item: &Item) -> ClientOp {
    ClientOp {
        op_id,
        item_id: item.id.clone(),
        base: item.version,
        action: OpAction::Delete,
        kind: None,
        z: None,
        props: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::model::ItemData;
    use serde_json::json;

    fn frame(id: &str, title: &str) -> Item {
        Item::new(
            id.to_string(),
            "a0".to_string(),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            ItemData::Frame {
                title: title.to_string(),
            },
        )
    }

    #[test]
    fn props_patch_gives_changed_keys_and_null_for_removed() {
        let old = json!({"a": 1, "b": 2})
            .as_object()
            .cloned()
            .unwrap_or_default();
        let new = json!({"a": 1, "c": 3})
            .as_object()
            .cloned()
            .unwrap_or_default();
        let patch = props_patch(&old, &new);
        assert_eq!(Value::Object(patch), json!({"b": null, "c": 3}));
    }

    #[test]
    fn update_op_is_none_when_equal() {
        let item = frame("f1", "Board");
        assert!(update_op("op".to_string(), &item, &item.clone()).is_none());
    }

    #[test]
    fn update_op_carries_only_changed_keys() {
        let mut before = frame("f1", "Board");
        before.version = 4;
        let mut after = before.clone();
        after.rect.x = 25.0;
        let op = update_op("op1".to_string(), &before, &after).expect("changed");
        assert_eq!(op.action, OpAction::Update);
        assert_eq!(op.base, 4);
        assert_eq!(op.z, None);
        assert_eq!(op.kind, None);
        let props = op.props.expect("props");
        assert_eq!(props.len(), 1);
        assert_eq!(props.get("x"), Some(&json!(25.0)));
    }

    #[test]
    fn update_op_carries_z_only_when_changed() {
        let before = frame("f1", "Board");
        let mut after = before.clone();
        after.z = "b0".to_string();
        let op = update_op("op2".to_string(), &before, &after).expect("changed");
        assert_eq!(op.z, Some("b0".to_string()));
        assert!(op.props.is_none());
    }

    #[test]
    fn create_and_delete_ops_shape() {
        let mut item = frame("f2", "Side");
        item.version = 7;
        let create = create_op("c1".to_string(), &item);
        assert_eq!(create.base, 0);
        assert_eq!(create.kind.as_deref(), Some("Frame"));
        assert_eq!(create.z.as_deref(), Some("a0"));
        assert!(create.props.is_some());

        let delete = delete_op("d1".to_string(), &item);
        assert_eq!(delete.action, OpAction::Delete);
        assert_eq!(delete.base, 7);
        assert!(delete.kind.is_none() && delete.z.is_none() && delete.props.is_none());
    }
}
