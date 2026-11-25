//! Per-user, local undo and redo. Other people's edits are not undone.
//!
//! A user action is a batch of [`Change`]s. Undo walks the batch in reverse and produces the [`Edit`]s that move each
//! item back to its `before` state, as the current document has it now. The moved batch (each change with before and
//! after swapped, in the order it was processed) goes on the redo stack, and redo is the same procedure run on that
//! stack.
//!
//! Undoing a delete creates a copy under a new id, because the room never re-creates a deleted id. Changes earlier in
//! the same batch are then rewritten to that id.

use crate::model::{Doc, Item};
use std::collections::HashMap;

/// One item before and after a user action. `before: None` means the action created it; `after: None` means it deleted it.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: String,
    pub before: Option<Item>,
    pub after: Option<Item>,
}

/// What to send to the room to undo or redo: a create, an update from the current item to the target, or a delete.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "edits are short-lived values returned in small lists; boxing would only add allocations"
)]
pub enum Edit {
    Create(Item),
    Update { from: Item, to: Item },
    Delete(Item),
}

/// The undo and redo stacks of one user.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
}

impl History {
    /// Most batches kept on the undo stack.
    pub const LIMIT: usize = 200;

    /// Records one user action. Clears redo. Drops the oldest batch past [`Self::LIMIT`]. Empty batches are ignored.
    pub fn record(&mut self, batch: Vec<Change>) {
        if batch.is_empty() {
            return;
        }
        self.redo.clear();
        push_bounded(&mut self.undo, batch);
    }

    /// True when there is an action to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// True when there is an action to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The edits that undo the last action against the current `doc`. The action moves to the redo stack.
    /// `new_id` makes a fresh id for an item recreated by an undo of a delete.
    pub fn undo(&mut self, doc: &Doc, new_id: &mut dyn FnMut() -> String) -> Vec<Edit> {
        let Some(batch) = self.undo.pop() else {
            return Vec::new();
        };
        let (edits, moved) = revert(doc, &batch, new_id);
        if !moved.is_empty() {
            push_bounded(&mut self.redo, moved);
        }
        edits
    }

    /// The edits that redo the last undone action against the current `doc`. The action moves back to the undo stack.
    pub fn redo(&mut self, doc: &Doc, new_id: &mut dyn FnMut() -> String) -> Vec<Edit> {
        let Some(batch) = self.redo.pop() else {
            return Vec::new();
        };
        let (edits, moved) = revert(doc, &batch, new_id);
        if !moved.is_empty() {
            push_bounded(&mut self.undo, moved);
        }
        edits
    }
}

fn push_bounded(stack: &mut Vec<Vec<Change>>, batch: Vec<Change>) {
    stack.push(batch);
    if stack.len() > History::LIMIT {
        stack.remove(0);
    }
}

/// Follows recreated ids to the newest one.
fn resolve(alias: &HashMap<String, String>, id: &str) -> String {
    let mut current = id.to_string();
    while let Some(next) = alias.get(&current) {
        current = next.clone();
    }
    current
}

/// The item with `id` set to `id`.
fn with_id(item: &Item, id: &str) -> Item {
    let mut copy = item.clone();
    copy.id = id.to_string();
    copy
}

/// Sets back every field of `current` that differs between `before` and `after` to the value in `before`.
fn restore_fields(current: &Item, before: &Item, after: &Item) -> Item {
    let mut target = current.clone();
    if before.rect != after.rect {
        target.rect = before.rect;
    }
    if before.rot != after.rot {
        target.rot = before.rot;
    }
    if before.flip_x != after.flip_x {
        target.flip_x = before.flip_x;
    }
    if before.flip_y != after.flip_y {
        target.flip_y = before.flip_y;
    }
    if before.opacity != after.opacity {
        target.opacity = before.opacity;
    }
    if before.locked != after.locked {
        target.locked = before.locked;
    }
    if before.group != after.group {
        target.group = before.group.clone();
    }
    if before.frame != after.frame {
        target.frame = before.frame.clone();
    }
    if before.z != after.z {
        target.z = before.z.clone();
    }
    if before.data != after.data {
        target.data = before.data.clone();
    }
    target
}

/// Runs a batch in reverse against `doc`. Returns the edits, and the inverse batch in processing order (which is the
/// order `revert` expects when it runs on that batch again).
fn revert(
    doc: &Doc,
    batch: &[Change],
    new_id: &mut dyn FnMut() -> String,
) -> (Vec<Edit>, Vec<Change>) {
    let mut edits = Vec::new();
    let mut moved = Vec::new();
    // Items as this batch leaves them, so that several changes to one item chain correctly.
    let mut overlay: HashMap<String, Option<Item>> = HashMap::new();
    // Old id to the id of the copy created in its place.
    let mut alias: HashMap<String, String> = HashMap::new();

    for change in batch.iter().rev() {
        let id = resolve(&alias, &change.id);
        let current = match overlay.get(&id) {
            Some(entry) => entry.clone(),
            None => doc.get(&id).cloned(),
        };
        match (&change.before, &change.after) {
            (None, None) => {}
            (None, Some(_)) => {
                // The action created the item: delete it if it is still there.
                if let Some(cur) = current {
                    edits.push(Edit::Delete(cur.clone()));
                    overlay.insert(id.clone(), None);
                    moved.push(Change {
                        id: id.clone(),
                        before: Some(cur),
                        after: None,
                    });
                }
            }
            (Some(before), None) => {
                // The action deleted the item: recreate it under a fresh id.
                let fresh = new_id();
                let copy = with_id(before, &fresh);
                edits.push(Edit::Create(copy.clone()));
                overlay.insert(fresh.clone(), Some(copy.clone()));
                alias.insert(id.clone(), fresh.clone());
                moved.push(Change {
                    id: fresh,
                    before: None,
                    after: Some(copy),
                });
            }
            (Some(before), Some(after)) => {
                // The action changed the item: set back only the fields it changed.
                let Some(cur) = current else {
                    continue;
                };
                let target = restore_fields(&cur, before, after);
                if target != cur {
                    edits.push(Edit::Update {
                        from: cur,
                        to: target.clone(),
                    });
                    overlay.insert(id.clone(), Some(target));
                }
                moved.push(Change {
                    id: id.clone(),
                    before: Some(with_id(after, &id)),
                    after: Some(with_id(before, &id)),
                });
            }
        }
    }
    (edits, moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::model::{ItemData, TextAlign};

    fn text(id: &str, x: f64, body: &str) -> Item {
        Item::new(
            id.to_string(),
            "a0".to_string(),
            Rect::new(x, 0.0, 10.0, 10.0),
            ItemData::Text {
                text: body.to_string(),
                color: "ink".to_string(),
                fill: None,
                size: 24.0,
                align: TextAlign::Left,
            },
        )
    }

    fn doc_with(items: &[Item]) -> Doc {
        let mut doc = Doc::default();
        for item in items {
            doc.put(item.clone());
        }
        doc
    }

    #[test]
    fn create_then_undo_gives_delete() {
        let item = text("t1", 0.0, "hi");
        let doc = doc_with(std::slice::from_ref(&item));
        let mut h = History::default();
        h.record(vec![Change {
            id: "t1".to_string(),
            before: None,
            after: Some(item.clone()),
        }]);
        let mut fresh = || "unused".to_string();
        let edits = h.undo(&doc, &mut fresh);
        assert_eq!(edits, vec![Edit::Delete(item)]);
        assert!(h.can_redo());
    }

    #[test]
    fn delete_then_undo_creates_new_id_and_redo_deletes_it() {
        let item = text("t2", 5.0, "bye");
        let mut h = History::default();
        h.record(vec![Change {
            id: "t2".to_string(),
            before: Some(item.clone()),
            after: None,
        }]);

        let mut fresh = || "t2-copy".to_string();
        let edits = h.undo(&Doc::default(), &mut fresh);
        let mut expected = item.clone();
        expected.id = "t2-copy".to_string();
        assert_eq!(edits, vec![Edit::Create(expected.clone())]);

        // The room now holds the copy; redo removes it.
        let doc = doc_with(std::slice::from_ref(&expected));
        let mut fresh = || "unused".to_string();
        let edits = h.redo(&doc, &mut fresh);
        assert_eq!(edits, vec![Edit::Delete(expected)]);
    }

    #[test]
    fn update_undo_restores_only_changed_field_and_keeps_others_text() {
        let before = text("t3", 0.0, "a");
        let mut after = before.clone();
        after.rect.x = 5.0;
        // Someone else changed the text since, and the user's x is still 5.
        let current = text("t3", 5.0, "b");
        let doc = doc_with(std::slice::from_ref(&current));

        let mut h = History::default();
        h.record(vec![Change {
            id: "t3".to_string(),
            before: Some(before.clone()),
            after: Some(after),
        }]);
        let mut fresh = || "unused".to_string();
        let edits = h.undo(&doc, &mut fresh);

        let mut target = current.clone();
        target.rect = before.rect;
        assert_eq!(
            edits,
            vec![Edit::Update {
                from: current,
                to: target.clone()
            }]
        );
        assert_eq!(target.rect.x, 0.0);
        assert_eq!(target.data, text("t3", 0.0, "b").data);
    }

    #[test]
    fn undo_of_missing_item_gives_nothing() {
        let before = text("t4", 0.0, "a");
        let mut after = before.clone();
        after.rect.x = 1.0;
        let mut h = History::default();
        h.record(vec![Change {
            id: "t4".to_string(),
            before: Some(before),
            after: Some(after),
        }]);
        let mut fresh = || "unused".to_string();
        assert!(h.undo(&Doc::default(), &mut fresh).is_empty());
        assert!(!h.can_redo());
    }

    #[test]
    fn limit_is_respected() {
        let item = text("t5", 0.0, "x");
        let mut h = History::default();
        for _ in 0..(History::LIMIT + 1) {
            h.record(vec![Change {
                id: "t5".to_string(),
                before: None,
                after: Some(item.clone()),
            }]);
        }
        let doc = Doc::default();
        let mut fresh = || "unused".to_string();
        let mut count = 0;
        while h.can_undo() {
            h.undo(&doc, &mut fresh);
            count += 1;
        }
        assert_eq!(count, History::LIMIT);
    }

    #[test]
    fn record_clears_redo() {
        let item = text("t6", 0.0, "x");
        let mut h = History::default();
        h.record(vec![Change {
            id: "t6".to_string(),
            before: None,
            after: Some(item.clone()),
        }]);
        let doc = doc_with(std::slice::from_ref(&item));
        let mut fresh = || "unused".to_string();
        h.undo(&doc, &mut fresh);
        assert!(h.can_redo());
        h.record(vec![Change {
            id: "t7".to_string(),
            before: None,
            after: Some(text("t7", 0.0, "y")),
        }]);
        assert!(!h.can_redo());
    }

    #[test]
    fn empty_batch_is_ignored() {
        let mut h = History::default();
        h.record(Vec::new());
        assert!(!h.can_undo());
    }
}
