//! Checks that the shared `icons.slint` names exactly the icons the Rust lists map (decision 0007, S1). The files behind
//! those names are checked in rimelight-rs (`crates/rimelight_ui/src/lib.rs`), where the icon folder lives.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::icon::IconName;

#[cfg(test)]
/// Every `@image-url("...")` path named in the shared `icons.slint` (rimelight-rs), as file stems (`house`, `chart-column`, ...).
fn icon_stems_in_markup() -> BTreeSet<String> {
    // The icon markup is in the shared library now (ST2.3): `rimelight_ui::UI_DIR/icons.slint`.
    let text = std::fs::read_to_string(PathBuf::from(rimelight_ui::UI_DIR).join("icons.slint"))
        .expect("read the shared icons.slint");
    text.split("@image-url(\"../assets/icons/")
        .skip(1)
        .map(|rest| {
            let end = rest.find(".svg\")").expect("icon path ends in .svg\")");
            rest[..end].to_string()
        })
        .collect()
}

/// The sidebar row icons: every name in `space_nav/ICONS.txt` (the rows take `Page::icon()`). The shared `icons.slint` lists each
/// one, and `shell::icon` maps its name to an image. Kept beside `IconName` because the rows are named, not mapped to a variant.
#[cfg(test)]
const SIDEBAR_ICONS: [&str; 124] = [
    "tag",
    "activity",
    "app-window",
    "arrow-left-right",
    "at-sign",
    "ban",
    "banknote",
    "book-marked",
    "book-open",
    "book-user",
    "bookmark",
    "bot",
    "box",
    "briefcase",
    "briefcase-business",
    "building",
    "building-2",
    "calendar-check",
    "calendar-days",
    "calendar-range",
    "chart-column",
    "chart-pie",
    "circle-dot",
    "clipboard-check",
    "clipboard-list",
    "clock",
    "compass",
    "concierge-bell",
    "contact",
    "credit-card",
    "database",
    "door-open",
    "file-chart-column",
    "file-chart-line",
    "file-check",
    "file-pen",
    "file-pen-line",
    "file-signature",
    "file-spreadsheet",
    "file-text",
    "flag",
    "folder",
    "folders",
    "funnel",
    "gamepad-2",
    "gantt-chart",
    "gauge",
    "git-branch",
    "git-merge",
    "git-pull-request",
    "graduation-cap",
    "hand-coins",
    "handshake",
    "hash",
    "headset",
    "heart-handshake",
    "history",
    "id-card",
    "kanban",
    "key-round",
    "landmark",
    "languages",
    "laptop",
    "layers",
    "layout-dashboard",
    "layout-panel-top",
    "list",
    "list-checks",
    "list-todo",
    "lock",
    "mail",
    "map",
    "map-pin",
    "megaphone",
    "message-circle",
    "message-square",
    "message-square-reply",
    "network",
    "notebook",
    "notebook-pen",
    "package",
    "package-check",
    "palm-tree",
    "pen-tool",
    "percent",
    "phone-call",
    "pie-chart",
    "plane",
    "play",
    "plug",
    "plug-zap",
    "presentation",
    "receipt",
    "receipt-text",
    "repeat",
    "rocket",
    "scale",
    "school",
    "scroll-text",
    "send",
    "settings",
    "shield-check",
    "shopping-cart",
    "smartphone",
    "stamp",
    "table-properties",
    "target",
    "ticket",
    "timer",
    "truck",
    "user-check",
    "user-cog",
    "user-minus",
    "user-plus",
    "user-round",
    "user-search",
    "users",
    "users-round",
    "video",
    "wallet",
    "warehouse",
    "webhook",
    "workflow",
    "zap",
];

/// Icons that only the Slint port draws: the password show and hide glyphs (Input), the NumberInput stepper minus, and the
/// S9.3 status bar call buttons (mic, mic-off, phone-off, from the same Lucide set as the kit icons, no `IconName` variant).
/// SF.4 adds the status bar keyboard, maximize (fullscreen) and the sidebar bug and settings glyphs, also without a variant.
/// The stepper and the eye are drawn with glyphs that have no `IconName` variant (README, "Slint port" S5), so
/// `IconName` has no variant for them. They still need a bundled file. SF.4 adds the quick action menu's calendar-plus and
/// square-check (mail and notebook-pen are already listed in the sidebar set).
/// S1.5b adds the issue page's glyphs: the vote and star, the attachment and its upload, the edit, tag, link and copy-key
/// buttons. They have no `IconName` variant either.
/// WS4 adds `music` (the file-kind tile of the shared recent-files list).
const SLINT_ONLY_ICONS: [&str; 59] = [
    // The shared recent-files list (WS4).
    "music",
    // Canvas video controls (CV-V.4).
    "pause",
    "volume-2",
    "volume-x",
    // Canvas drawing tools (CV.9) and the eyedropper (CV.7).
    "square",
    "circle",
    "diamond",
    "eraser",
    "frame",
    "move-up-right",
    "slash",
    "pipette",
    // Canvas page toolbar, zoom controls and menus (CV.6).
    "mouse-pointer-2",
    "type",
    "hand",
    "image-plus",
    "image",
    "undo-2",
    "redo-2",
    "zoom-in",
    "zoom-out",
    "scan",
    "lock-open",
    "flip-horizontal-2",
    "flip-vertical-2",
    "rotate-cw",
    "trash",
    "bring-to-front",
    "send-to-back",
    "arrow-up",
    "arrow-down",
    "copy-plus",
    "log-out",
    "arrow-left",
    "arrow-right",
    "eye",
    "eye-off",
    "minus",
    "mic",
    "mic-off",
    "phone-off",
    "keyboard",
    "maximize",
    "bug",
    "settings",
    "calendar-plus",
    "square-check",
    "pin",
    "folder-open",
    "messages-square",
    "share-2",
    "star",
    "thumbs-up",
    "paperclip",
    "pencil",
    "tag",
    "link",
    "copy",
    "upload",
];

#[test]
fn every_page_icon_is_a_sidebar_icon() {
    for page in space_nav::pages() {
        assert!(
            SIDEBAR_ICONS.contains(&page.icon()),
            "{} uses icon {} that is not bundled in SIDEBAR_ICONS",
            page.route,
            page.icon()
        );
    }
}

#[test]
fn the_icon_markup_names_exactly_the_mapped_icons() {
    let mut mapped: BTreeSet<String> = IconName::ALL
        .iter()
        .map(|n| n.lucide().to_string())
        .collect();
    mapped.extend(SLINT_ONLY_ICONS.iter().map(|s| s.to_string()));
    mapped.extend(SIDEBAR_ICONS.iter().map(|s| s.to_string()));
    assert_eq!(
        icon_stems_in_markup(),
        mapped,
        "the shared icons.slint must list every IconName once and nothing else"
    );
}
