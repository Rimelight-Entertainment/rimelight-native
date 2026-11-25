//! The Chat sidebar tree (SF.4 scaffold, Teams-style). The rows of the Chat area's sidebar: three collapsible sections
//! (Channels, Chats, Rooms). Each has a Pinned subsection, then user groups, then ungrouped conversations. Every row starts
//! expanded. The data is mock: the constants below. Swap [`SECTIONS`] for real data and the rows follow.
//!
//! The collapse state is kept here (a thread-local set of collapsed row keys). A click flips one key and the shell rebuilds
//! the rows with [`rows`]. The actions of the last build are kept too, so a click on row `index` finds its action with
//! [`action`]. The route of a conversation is `/chat/<segment>/<id>`; the nav crate may not parse these yet, so a click may
//! not open a page.

use std::cell::RefCell;
use std::collections::HashSet;

use std::path::Path;

use slint::Image;

use crate::slint_ui::shell::icon;
use crate::slint_ui::{Icons, SidebarItem, SidebarKind, SidebarLeading};

/// One conversation: its route id, the label, its unread count, whether it is private (a private channel), its picture (a
/// file path, `None` for the initials or first letter) and a room's member count (0 for the others).
struct Conv {
    id: &'static str,
    label: &'static str,
    unread: i32,
    private: bool,
    image: Option<&'static str>,
    people: i32,
}

const fn conv(id: &'static str, label: &'static str, unread: i32, private: bool) -> Conv {
    Conv {
        id,
        label,
        unread,
        private,
        image: None,
        people: 0,
    }
}

/// A room: no picture, and a member count shown on its row.
const fn room(id: &'static str, label: &'static str, unread: i32, people: i32) -> Conv {
    Conv {
        id,
        label,
        unread,
        private: false,
        image: None,
        people,
    }
}

/// A user group inside a section: its key (for the collapse state) and label, and its conversations.
struct Group {
    key: &'static str,
    label: &'static str,
    items: &'static [Conv],
}

/// One top section. `segment` is the route segment (`channels`, `chats`, `rooms`). A heading has no icon (option A).
struct Section {
    key: &'static str,
    label: &'static str,
    segment: &'static str,
    pinned: &'static [Conv],
    groups: &'static [Group],
    ungrouped: &'static [Conv],
}

/// The mock data. One entry per section, in display order.
const SECTIONS: &[Section] = &[
    Section {
        key: "channels",
        label: "Channels",
        segment: "channels",
        pinned: &[
            conv("general", "general", 0, false),
            conv("announcements", "announcements", 0, false),
        ],
        groups: &[Group {
            key: "engineering",
            label: "Engineering",
            items: &[
                conv("eng-backend", "eng-backend", 3, false),
                conv("eng-frontend", "eng-frontend", 0, false),
            ],
        }],
        ungrouped: &[
            conv("random", "random", 0, false),
            conv("leadership", "leadership", 0, true),
        ],
    },
    Section {
        key: "chats",
        label: "Chats",
        segment: "chats",
        pinned: &[conv("alex-rivera", "Alex Rivera", 2, false)],
        groups: &[Group {
            key: "project-atlas",
            label: "Project Atlas",
            items: &[
                conv("sam-chen", "Sam Chen", 0, false),
                conv("priya-patel", "Priya Patel", 1, false),
            ],
        }],
        ungrouped: &[conv("jordan-lee", "Jordan Lee", 0, false)],
    },
    Section {
        key: "rooms",
        label: "Rooms",
        segment: "rooms",
        pinned: &[room("launch-war-room", "Launch war room", 0, 12)],
        groups: &[],
        ungrouped: &[
            room("design-review", "Design review", 0, 5),
            room("offsite-planning", "Offsite planning", 4, 8),
        ],
    },
];

/// What a clicked row does. A header toggles its collapse key; a conversation opens its route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Toggle(String),
    Open(String),
}

/// The rows of the tree, the action of each row, and the index of the row whose route is the current page (-1 for none).
pub struct Rows {
    pub items: Vec<SidebarItem>,
    pub actions: Vec<Action>,
    pub selected: i32,
}

thread_local! {
    /// The keys of the collapsed rows. Every row starts expanded, so this starts empty.
    static COLLAPSED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// The actions of the last built rows, indexed like the rows the sidebar shows.
    static LAST: RefCell<Vec<Action>> = const { RefCell::new(Vec::new()) };
}

/// Builds the tree for the Chat area. `current` is the page route; its row is selected. Also keeps the row actions for
/// [`action`].
pub fn rows(icons: &Icons<'_>, current: &str) -> Rows {
    let collapsed = COLLAPSED.with(|set| set.borrow().clone());
    let mut out = Rows {
        items: Vec::new(),
        actions: Vec::new(),
        selected: -1,
    };
    for section in SECTIONS {
        let open = !collapsed.contains(section.key);
        let total = section_total(section);
        push_header(
            &mut out,
            icons,
            section.label,
            "",
            SidebarKind::Heading,
            open,
            total,
            section.key,
        );
        if !open {
            continue;
        }
        if !section.pinned.is_empty() {
            let key = format!("{}/pinned", section.key);
            let open = !collapsed.contains(&key);
            let total = sum(section.pinned);
            push_header(
                &mut out,
                icons,
                "Pinned",
                "pin",
                SidebarKind::Pinned,
                open,
                total,
                &key,
            );
            if open {
                for conversation in section.pinned {
                    push_conversation(
                        &mut out,
                        icons,
                        section.segment,
                        conversation,
                        current,
                        true,
                    );
                }
            }
        }
        for group in section.groups {
            let key = format!("{}/{}", section.key, group.key);
            let open = !collapsed.contains(&key);
            let icon_name = if open { "folder-open" } else { "folder" };
            push_header(
                &mut out,
                icons,
                group.label,
                icon_name,
                SidebarKind::Group,
                open,
                sum(group.items),
                &key,
            );
            if open {
                for conversation in group.items {
                    push_conversation(
                        &mut out,
                        icons,
                        section.segment,
                        conversation,
                        current,
                        true,
                    );
                }
            }
        }
        for conversation in section.ungrouped {
            push_conversation(
                &mut out,
                icons,
                section.segment,
                conversation,
                current,
                false,
            );
        }
    }
    LAST.with(|last| *last.borrow_mut() = out.actions.clone());
    out
}

/// The action of row `index` in the last built rows, or `None` when there is no such row.
pub fn action(index: usize) -> Option<Action> {
    LAST.with(|last| last.borrow().get(index).cloned())
}

/// Flips the collapse state of the row with key `key` (a section, pinned or group key). The caller rebuilds the rows.
pub fn toggle(key: &str) {
    COLLAPSED.with(|set| {
        let mut set = set.borrow_mut();
        if !set.remove(key) {
            set.insert(key.to_string());
        }
    });
}

fn section_total(section: &Section) -> i32 {
    sum(section.pinned)
        + section
            .groups
            .iter()
            .map(|group| sum(group.items))
            .sum::<i32>()
        + sum(section.ungrouped)
}

fn sum(items: &[Conv]) -> i32 {
    items.iter().map(|conversation| conversation.unread).sum()
}

/// True for the Rooms section, whose rows show the door-open icon at the gutter.
fn visual_is_room(segment: &str) -> bool {
    segment == "rooms"
}

/// The left visual of a conversation: a channel is a tile, a chat a round avatar, a room none (it has the door icon).
fn leading(segment: &str) -> Leading {
    match segment {
        "channels" => Leading::Tile,
        "chats" => Leading::Avatar,
        _ => Leading::None,
    }
}

/// The initials a conversation shows when it has no picture: the first letter of a channel, the initials of a chat.
fn initials(segment: &str, label: &str) -> String {
    match leading(segment) {
        Leading::Avatar => label
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase(),
        _ => label
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default(),
    }
}

#[allow(clippy::too_many_arguments)]
/// Which left visual a conversation row draws (mirrors `SidebarLeading` in the markup).
enum Leading {
    None,
    Tile,
    Avatar,
}

#[allow(clippy::too_many_arguments)]
fn push_header(
    out: &mut Rows,
    icons: &Icons<'_>,
    label: &str,
    icon_name: &str,
    kind: SidebarKind,
    expanded: bool,
    count: i32,
    key: &str,
) {
    // A top heading is uppercase (the one heading style of the rail); a Pinned or Group sub-row keeps its case.
    let is_heading = matches!(kind, SidebarKind::Heading);
    let label = if is_heading {
        label.to_uppercase()
    } else {
        label.to_string()
    };
    out.items.push(SidebarItem {
        label: label.into(),
        header: is_heading,
        icon: icon(icons, icon_name),
        kind,
        expanded,
        count,
        bold: false,
        ..Default::default()
    });
    out.actions.push(Action::Toggle(key.to_string()));
}

/// Pushes one conversation row. `indented` is true inside a Pinned or Group sub-row (one fixed step to the right).
fn push_conversation(
    out: &mut Rows,
    icons: &Icons<'_>,
    segment: &str,
    conversation: &Conv,
    current: &str,
    indented: bool,
) {
    let route = format!("/chat/{segment}/{}", conversation.id);
    if route == current {
        out.selected = out.items.len() as i32;
    }
    let visual = leading(segment);
    out.items.push(SidebarItem {
        label: conversation.label.into(),
        header: false,
        // A room has no tile or avatar, so it shows its door-open icon in the same gutter (the label lines up with the rest).
        icon: if visual_is_room(segment) {
            icon(icons, "door-open")
        } else {
            Default::default()
        },
        kind: SidebarKind::Conversation,
        count: conversation.unread,
        indented,
        bold: conversation.unread > 0,
        leading: match visual {
            Leading::None => SidebarLeading::None,
            Leading::Tile => SidebarLeading::Tile,
            Leading::Avatar => SidebarLeading::Avatar,
        },
        // A picture file is loaded when a mock sets one. No mock does, so the initials show.
        image: conversation
            .image
            .and_then(|path| Image::load_from_path(Path::new(path)).ok())
            .unwrap_or_default(),
        initials: initials(segment, conversation.label).into(),
        private: conversation.private,
        people: conversation.people,
        ..Default::default()
    });
    out.actions.push(Action::Open(route));
}
