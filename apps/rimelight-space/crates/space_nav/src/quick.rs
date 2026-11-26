//! Quick action categories (OQ14, OQ16, OQ17; `PROJECT.md section 6.6` section 4). Data only: one category per area,
//! and each item names a create action that opens a dialog. No item names a route (OQ16).
//!
//! Only Personal (Home) and Chat exist. Other areas are added here as each area is built.

use crate::Area;

/// One create action. `id` is what the dialog is keyed by; `label` is the menu text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickItem {
    pub id: &'static str,
    pub label: &'static str,
}

/// A menu category of the quick action. Personal is first (OQ14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickCategory {
    /// Stable id, unique across the categories.
    pub id: &'static str,
    /// The area the category belongs to. Personal belongs to Home.
    pub area: Area,
    pub title: &'static str,
    pub items: &'static [QuickItem],
}

const PERSONAL: &[QuickItem] = &[
    QuickItem {
        id: "new_mail",
        label: "New Mail",
    },
    QuickItem {
        id: "new_event",
        label: "New Event",
    },
    QuickItem {
        id: "new_task",
        label: "New Task",
    },
    QuickItem {
        id: "new_note",
        label: "New Note",
    },
];

const CHAT: &[QuickItem] = &[
    QuickItem {
        id: "new_channel",
        label: "New Channel",
    },
    QuickItem {
        id: "new_chat",
        label: "New Chat",
    },
    QuickItem {
        id: "new_room",
        label: "New Room",
    },
    QuickItem {
        id: "new_group",
        label: "New Group",
    },
];

const PROJECTS: &[QuickItem] = &[QuickItem {
    id: "new_issue",
    label: "New issue",
}];

const CATEGORIES: &[QuickCategory] = &[
    QuickCategory {
        id: "personal",
        area: Area::Home,
        title: "Personal",
        items: PERSONAL,
    },
    QuickCategory {
        id: "chat",
        area: Area::Chat,
        title: "Chat",
        items: CHAT,
    },
    QuickCategory {
        id: "projects",
        area: Area::Projects,
        title: "Projects",
        items: PROJECTS,
    },
];

/// The quick action categories, Personal first.
pub fn categories() -> &'static [QuickCategory] {
    CATEGORIES
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn personal_is_first() {
        assert_eq!(categories()[0].id, "personal");
        assert_eq!(categories()[0].area, Area::Home);
        let titles: Vec<&str> = categories()
            .iter()
            .find(|c| c.id == "personal")
            .unwrap()
            .items
            .iter()
            .map(|i| i.label)
            .collect();
        assert_eq!(titles, ["New Mail", "New Event", "New Task", "New Note"]);
    }

    #[test]
    fn every_item_has_a_unique_id_and_label() {
        let mut ids = HashSet::new();
        let mut labels = HashSet::new();
        for category in categories() {
            assert!(!category.items.is_empty(), "{} has no items", category.id);
            for item in category.items {
                assert!(!item.id.is_empty() && !item.label.is_empty());
                assert!(ids.insert(item.id), "duplicate item id {}", item.id);
                assert!(labels.insert(item.label), "duplicate label {}", item.label);
            }
        }
    }

    #[test]
    fn one_category_per_area_and_unique_category_ids() {
        let mut areas = HashSet::new();
        let mut ids = HashSet::new();
        for category in categories() {
            assert!(
                areas.insert(category.area),
                "two categories for {:?}",
                category.area
            );
            assert!(
                ids.insert(category.id),
                "duplicate category {}",
                category.id
            );
        }
    }

    #[test]
    fn chat_category_matches_the_design() {
        let chat = categories().iter().find(|c| c.id == "chat").unwrap();
        let labels: Vec<&str> = chat.items.iter().map(|i| i.label).collect();
        assert_eq!(labels, ["New Channel", "New Chat", "New Room", "New Group"]);
    }
}
