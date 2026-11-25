# Canvas area: wiring for the lead (ST5.2)

Nothing here was built or tested (owner rule: one build pass at the end). The window side is now in the area too, so the
whole canvas lives in `areas/canvas` and `space_ui` keeps no canvas code.

## What moved

- Rust, to `areas/canvas/src/`: `controller.rs`, `export.rs`, `images.rs`, `video.rs` (desktop only), `popout_platform.rs`
  (earlier), and now `page.rs`, `list.rs`, `popout.rs`, `actions.rs` (moved with their logic unchanged), plus the new
  `window.rs` (the traits) and `lib.rs` (modules and `wire_canvas!`).
- Markup, to `areas/canvas/ui/`: `canvas.slint`, `canvas-item.slint`, `canvas-list.slint`, `canvas-overlays.slint`,
  `canvas-popout.slint`, `canvas-types.slint` (earlier).
- Deleted: `crates/space_ui/src/pages/canvas/` (all five files and the directory).

## How the window side works

- `window::CanvasPageProps` and `CanvasPageEvents` (generated from one table, `__canvas_state_table!`): a getter and setter per
  property, and an `on_*` per callback, of the `CanvasState` global. `CanvasPageState`: the models (guides, menu, peers,
  video controls, asset rows, item rows), the load state and the copy of page state between windows. Both the main window
  and the pop-out window implement them.
- `window::CanvasWindow` (main window: `canvas_open_route`, `canvas_close_tab`, `new_items`, `type Popout`),
  `window::PopoutWindow` (+ `PopoutProps`: `create`, the bar callbacks, overlay/pinned/locked/opacity/click-through).
- The page is held in thread-locals and cannot be generic, so it keeps its windows type-erased: `window::Ui` (a weak handle
  to the main window with `fetch`/`post`/`upgrade`), `window::UiDyn`, `window::PopoutDyn`, `window::WeakPopout`. `page::wire`
  registers the pop-out window type with `popout::register::<W::Popout>()`, so the free functions in `popout.rs` stay
  non-generic. `page::drop_hover` and `page::files_dropped` stay free functions.
- `window::CanvasListWindow` (list page). `list::CanvasList` is now `CanvasList<W>`; `list::wire` is generic over the window.
- Generated structs became plain views (`ItemView`, `GuideView`, `PeerView`, ...), mapped to the generated types inside
  `wire_canvas!` (module `canvas_glue`).

## Shared-file edits the lead must make

### 1. `crates/space_ui/Cargo.toml`

Add under `[dependencies]` (next to `space_area_management`):

```toml
space_area_canvas = { path = "../../areas/canvas" }
```

Dependencies no longer used by any `space_ui` source (unused deps do not fail the build, so removal is cleanup):

- `rimelight_picture`, `imageproc`, `ab_glyph`, `zip`, `space_canvas` (check first: only canvas code used it).
- `rfd` (check: other pages may use it, e.g. the profile photo picker in company/management are in their own crates),
  `arboard`, `open`, `rimelight_media`, `sha2`, `dirs` in the desktop blocks.

### 2. `crates/space_ui_slint/Cargo.toml`

```toml
[dependencies]
space_area_canvas = { path = "../../areas/canvas" }

[build-dependencies]
space_area_canvas = { path = "../../areas/canvas" }
```

(`[dependencies]` is needed now: `wire_canvas!` is expanded in this crate.)

### 3. `crates/space_ui_slint/build.rs`

Add to the `with_library_paths(HashMap::from([...]))` list, after the `space_area_management` entry:

```rust
(
    "space_area_canvas".to_string(),
    PathBuf::from(space_area_canvas::UI_DIR),
),
```

### 4. `crates/space_ui/ui/app-shell.slint`

```
-import { CanvasPage } from "pages/canvas/canvas.slint";
+import { CanvasPage } from "@space_area_canvas/canvas.slint";
-import { CanvasListPage } from "pages/canvas/canvas-list.slint";
+import { CanvasListPage } from "@space_area_canvas/canvas-list.slint";
```

### 5. `crates/space_ui/ui/main.slint` (lines 50 to 53)

```
-export { CanvasItemView, CanvasGuide, CanvasPalette, CanvasState, CanvasMenuPanel, CanvasMenuRow } from "pages/canvas/canvas.slint";
+export { CanvasItemView, CanvasGuide, CanvasPalette, CanvasState, CanvasMenuPanel, CanvasMenuRow } from "@space_area_canvas/canvas.slint";
-export { CanvasPeer, CanvasPeerBox, CanvasHierarchyRow, CanvasAssetRow, CanvasWindowItem, CanvasVideoControl } from "pages/canvas/canvas-types.slint";
+export { CanvasPeer, CanvasPeerBox, CanvasHierarchyRow, CanvasAssetRow, CanvasWindowItem, CanvasVideoControl } from "@space_area_canvas/canvas-types.slint";
-export { CanvasPopout } from "pages/canvas/canvas-popout.slint";
+export { CanvasPopout } from "@space_area_canvas/canvas-popout.slint";
-export { CanvasListState } from "pages/canvas/canvas-list.slint";
+export { CanvasListState } from "@space_area_canvas/canvas-list.slint";
```

Also the main.slint line 48 comment says `(src/pages/canvas/)`; it may say `areas/canvas/` now. `CanvasState`, `CanvasPopout`,
`CanvasListState`, `PageLoadState`, `TableColumn`, `TableRow` and `SelectOption` must all be exported (they are).

### 6. `crates/space_ui_slint/src/lib.rs`: the `wire_canvas!` invocation

After the `wire_admin!` invocation (the crate that generates the types):

```rust
// Canvas area window glue: the canvas page, its pop-out window and the canvas lists (`space_area_canvas`).
space_area_canvas::wire_canvas!(
    AppShell,
    CanvasPopout,
    PageLoadState,
    CanvasState,
    CanvasItemView,
    CanvasGuide,
    CanvasMenuPanel,
    CanvasMenuRow,
    CanvasHierarchyRow,
    CanvasPeer,
    CanvasPeerBox,
    CanvasVideoControl,
    CanvasAssetRow,
    CanvasWindowItem,
    CanvasListState,
    TableColumn,
    TableRow,
    SelectOption,
);
```

Argument types in order: the main window, `CanvasPopout`, `PageLoadState`, `CanvasState`, `CanvasItemView`, `CanvasGuide`,
`CanvasMenuPanel`, `CanvasMenuRow`, `CanvasHierarchyRow`, `CanvasPeer`, `CanvasPeerBox`, `CanvasVideoControl`,
`CanvasAssetRow`, `CanvasWindowItem`, `CanvasListState`, `TableColumn`, `TableRow`, `SelectOption`. It emits the module
`canvas_glue` and the trait implementations; nothing else in the app calls it.

### 7. `crates/space_ui/src/slint_ui/app_shell.rs` (call sites)

```
-use crate::pages::canvas::list::{self as canvas_list, CanvasList};
-use crate::pages::canvas::page::{self as canvas_page, CanvasPage};
+use space_area_canvas::list::{self as canvas_list, CanvasList};
+use space_area_canvas::page::{self as canvas_page, CanvasPage};
```

- Line 109: `pub canvas_list: CanvasList,` becomes `pub canvas_list: CanvasList<AppShell>,`.
- Lines 1119 and 1120 (`canvas_page::wire(&ui, data.clone())`, `canvas_list::wire(&ui, data.clone())`) are unchanged: both are
  generic over the window and infer `AppShell`.
- Lines 2022 and 2023 (`canvas_page::canvas_id_of`, `canvas_list::list_scope_of`) are unchanged.
- `CanvasPage` keeps `deliver_live`, `sent_ops`, `set_image`, `add_assets_for_test`; `CanvasList<W>` keeps `generation` and
  `deliver`.

### 8. `apps/desktop`

Already repointed in the source (done by me): `src/main.rs` lines 118, 121, 124 (`drop_hover`, `files_dropped`) and 132
(`popout_platform::install`) now use `space_area_canvas::...`, and `src/popout.rs:12` uses
`space_area_canvas::popout_platform::{self, PopoutPlatform}`. Still needed: in `apps/desktop/Cargo.toml` add

```toml
space_area_canvas = { path = "../../areas/canvas" }
```

### 9. `crates/space_ui/src/pages/mod.rs`

Remove the line `pub mod canvas;` (the directory no longer exists). Not edited, per the instruction.

### 10. Tests in `crates/space_ui/tests/ui/`

`tests/ui/canvas.rs` and `tests/ui/main.rs` use `space_ui::pages::canvas::{page, list}` (`CanvasPage`, `CanvasList`,
`canvas_page::wire`). Repoint them to `space_area_canvas::{page, list}`; the types are `CanvasPage` and `CanvasList<AppShell>`.

## Unresolved / to look at in the one build pass

- Nothing compiled. Likely spots if something fails: the `__canvas_state_table!` macro chain in `window.rs` (trait and impl are
  generated from one table; `lib.rs` `wire_canvas!` expands it for `AppShell` and `CanvasPopout`), the `Ui::fetch` type erasure
  (`window.rs`), and `Weak<W>` being `Send` for `WeakWindow<W>` (`Mutex<Weak<W>>` inside an `Arc<dyn UiBackend>`).
- `popout::window()` now returns `Option<Rc<dyn PopoutDyn>>` (was a weak `CanvasPopout`); nothing in the repo used it.
- `popout_platform` is a process-global registry; the desktop app installs it at startup (item 8).
