# Projects area: wiring notes for the lead (WS5 ST5.2)

The area is `areas/projects` (package `space_area_projects`). Plain logic is in `src/`, markup in `ui/`, the window glue is
`wire_projects!` in `src/lib.rs`.

## Status (read first)

- All six pages are converted: `create_project`, `new_sprint`, `overview`, `issues`, `new_issue` and `issue`. `issue` now has
  an `IssueWindow` trait (a supertrait of `NewIssueWindow`, because the edit button opens that dialog; its method names do not
  clash with it), plain view structs (`IssueView`, `Body`, `AreaChipView`, `AttachmentTileView`, `ImageView`, `IssueRefView`,
  `LinkView`, `CommentView`, `WorkView`, `AttachmentView`), and `IssueDetail<W>`. The generated rows are built in
  `wire_projects!`. Clipboard: `IssueWindow::copy_link`, implemented with the app's clipboard function passed as the macro's
  last argument.
- Nothing has been built, per the owner's rule. The first build is the one final pass.
- `src/pages/mod.rs` still has `pub mod projects;`. It must be removed by the lead (not edited here, per the brief). The folder
  `crates/space_ui/src/pages/projects/` no longer exists.

## 1. Cargo.toml (`crates/space_ui/Cargo.toml`)

Add next to the other area lines (after `space_area_management`, around line 50):

```toml
space_area_projects = { path = "../../areas/projects" }
```

`crates/space_ui_slint/Cargo.toml` needs the same line in `[build-dependencies]` (line 21 and line 31 mirror the management
entry; add it to both sections the management entry is in).

The root `Cargo.toml` needs no change: `areas/*` is already a workspace member.

## 2. build.rs (`crates/space_ui_slint/build.rs`)

In the `with_library_paths` map, after the `space_area_management` entry (around line 29-30):

```rust
(
    "space_area_projects".to_string(),
    PathBuf::from(space_area_projects::UI_DIR),
),
```

The exact tuple shape is the one the management entry uses; copy it.

## 3. Slint imports (`crates/space_ui/ui/app.slint`, `crates/space_ui/ui/main.slint`)

Old path to new path. The file names keep their hyphens (`create-project.slint`, `new-issue.slint`, ...).

| File | Line | Old | New |
|---|---|---|---|
| app.slint | 48 | `import { IssuesPage, IssueRow } from "pages/projects/issues.slint";` | `from "@space_area_projects/issues.slint";` |
| app.slint | 49 | `import { BoardColumn } from "components/kanban-column.slint";` | `from "@space_area_projects/kanban-column.slint";` |
| app.slint | 50 | `... from "pages/projects/overview.slint";` | `from "@space_area_projects/overview.slint";` |
| app.slint | 51 | `... from "pages/projects/new-issue.slint";` | `from "@space_area_projects/new-issue.slint";` |
| app.slint | 52 | `... from "pages/projects/new-sprint.slint";` | `from "@space_area_projects/new-sprint.slint";` |
| app.slint | 55 | `... from "pages/projects/create-project.slint";` | `from "@space_area_projects/create-project.slint";` |
| app.slint | 56 | `... from "pages/projects/issue.slint";` | `from "@space_area_projects/issue.slint";` |
| main.slint | 33 | `export { IssueRow } from "pages/projects/issues.slint";` | `from "@space_area_projects/issues.slint";` |
| main.slint | 34 | `export { BoardColumn } from "components/kanban-column.slint";` | `from "@space_area_projects/kanban-column.slint";` |
| main.slint | 35 | `export { BoardCard, BoardArea } from "components/issue-card.slint";` | `from "@space_area_projects/issue-card.slint";` |
| main.slint | 37 | `export { OverviewCountRow, OverviewAreaRow } from "pages/projects/overview.slint";` | `from "@space_area_projects/overview.slint";` |
| main.slint | 40 | `export { IssueDetailView, IssueRef, LinkRow, CommentRow } from "pages/projects/issue.slint";` | `from "@space_area_projects/issue.slint";` |

Also in app.slint: `WorkRow` and `AttachmentRow` (line 56) and `AttachmentItem`/`ImageViewerItem` are imported from
`issue.slint` (line 56 names `WorkRow, AttachmentRow`); the same new path applies.

Comments in app.slint and main.slint that name `src/pages/projects/...` or `ui/pages/projects/...` are cosmetic and can
be updated with the import changes.

Already done in this area (no lead action): `overview.slint` imports `@space_area_projects/issues.slint`, `issues.slint` imports
`@space_area_projects/kanban-column.slint` and `kanban-board.slint`, the kanban files import each other and `issue-card.slint` by
`@space_area_projects/...`.

`crates/space_ui/ui/stories/stories.slint` line 7 was changed by this task to `import { KanbanColumn } from
"@space_area_projects/kanban-column.slint";`. The gallery loads that file through `@app_stories`, so the lead must check the
gallery build still has the `@space_area_projects` library path (or drop the story). `stories.txt` line 4 names
`components/kanban-board|KanbanBoard`, which no longer exists at that path; update it to the new path or remove the row.

## 4. app_shell.rs call sites (`crates/space_ui/src/slint_ui/app_shell.rs`)

Imports, lines 29-34. Old to new:

```rust
use crate::pages::projects::create_project;                       // -> use space_area_projects::create_project;
use crate::pages::projects::issue::{self, IssueDetail};           // -> use space_area_projects::issue::{self, IssueDetail};
use crate::pages::projects::issues::{self, Issues, can_create};   // -> use space_area_projects::issues::{self, Issues, can_create};
use crate::pages::projects::new_issue::{self, NewIssue};          // -> use space_area_projects::new_issue::{self, NewIssue};
use crate::pages::projects::new_sprint;                           // -> use space_area_projects::new_sprint;
use crate::pages::projects::overview::{self, Overview};           // -> use space_area_projects::overview::{self, Overview};
```

Struct fields, lines 119-126. These types are now generic over the window, so spell the app's window type:

```rust
pub overview: Overview<AppWindow>,
pub issues: Issues<AppWindow>,
pub issue: IssueDetail<AppWindow>,
pub new_issue: NewIssue<AppWindow>,
```

`new_sprint` is returned as `NewSprint<AppWindow>` if stored; at lines 1130-1134 it is only called, not stored.

Call sites, lines 1130-1134. The argument lists are unchanged; the window type is inferred from `&ui`:

```rust
let overview = overview::wire(&ui, data.clone());          // Overview<AppWindow>
let issues = issues::wire(&ui, data.clone());              // Issues<AppWindow>
let new_issue = new_issue::wire(&ui, data.clone());        // NewIssue<AppWindow>
let issue = issue::wire(&ui, data.clone(), issues.keys(), new_issue.clone()); // IssueDetail<AppWindow>; same arguments
new_sprint::wire(&ui, data.clone());                       // NewSprint<AppWindow>, unused result
```

`create_project` call sites (lines 1809-1856, 1874): `pick_picture`, `save`, `set_picked`, `take_picked`, `validate`,
`error_text`, `PickOutcome` keep their names and arguments. `pick_picture`, `save` and their `done` closures are now generic
over the window type `C: ComponentHandle`; the `|ui, result|` closures infer `&AppWindow`. `Picked`, `Created`, `set_picked`,
`take_picked`, `validate`, `error_text`, `PickOutcome` are unchanged.

Other items that moved (none is called from app_shell except as listed): `issues::can_create` (line 31 import) and
`issue::ids_of` are pub and keep their names.

`wire_projects!` invocation: put it at module level in the app crate, in a module where the generated types are in scope (the
same place the `wire_management!` call goes, or in `crates/space_ui/src/slint_ui/app_shell.rs` next to the other `use`s). It
must come before any use of its `projects_glue` module, and only once per crate. The arguments are, in order:

```rust
space_area_projects::wire_projects!(
    AppWindow,
    PageLoadState,
    SelectOption,
    MultiOption,
    CalendarDay,
    IssueRow,
    BoardArea,
    BoardCard,
    BoardColumn,
    OverviewCountRow,
    OverviewAreaRow,
    AreaChip,
    IssueDetailView,
    IssueRef,
    LinkRow,
    CommentRow,
    WorkRow,
    AttachmentRow,
    AttachmentItem,
    ImageViewerItem,
    crate::slint_ui::clipboard::write_link, // path; the app's `pub(crate) fn(&AppWindow, &str, impl FnOnce(bool) + 'static)`
);
```

Argument types in order: eleven idents for the generated types (window, `PageLoadState`, `SelectOption`, `MultiOption`,
`CalendarDay`, `IssueRow`, `BoardArea`, `BoardCard`, `BoardColumn`, `OverviewCountRow`, `OverviewAreaRow`), nine idents for the
issue page's generated types (`AreaChip`, `IssueDetailView`, `IssueRef`, `LinkRow`, `CommentRow`, `WorkRow`, `AttachmentRow`,
`AttachmentItem`, `ImageViewerItem`), then one path (the clipboard function). Place the invocation once, at module level, in
`crates/space_ui_slint/src/lib.rs` (the module where those generated types are in scope, next to the `wire_management!` call).
If the generated types are not all in scope there, `use` them first. The clipboard path must be visible from that module.

These are the generated Slint types. The app crate already exports them (`crate::slint_ui::{AppWindow, BoardArea, BoardCard,
BoardColumn, IssueRow, PageLoadState, SelectOption, MultiOption, CalendarDay, OverviewCountRow, OverviewAreaRow}`, see
`crates/space_ui/src/slint_ui/glue/calendar.rs` and the old page imports). The macro generates `pub mod projects_glue` in the
invoking module (name clash risk: the app must not already have a `projects_glue` module; there is none today). The macro needs
`space_area_projects` to be a dependency of the crate that invokes it (`crates/space_ui`, item 1).

The issue page's arguments are part of the same invocation (shown above). `IssueWindow` is implemented by the macro too.
Call-site changes in `app_shell.rs` for the issue page: the import and the `IssueDetail<AppWindow>` field type (above); the
`wire` call is unchanged. Everything that used `AppWindow` methods through `IssueDetail` (`deliver_load`, `deliver_action`,
`generation`, `last_request`) keeps its name and arguments.

## 5. Tests (`crates/space_ui/tests/ui`)

Left untouched as asked. What references the moved items:

- `crates/space_ui/tests/ui/issue.rs` line 15: `use space_ui::pages::projects::issue::{IssueData, Request};` becomes
  `use space_area_projects::issue::{IssueData, Request};`. The test crate needs `space_area_projects` in its dev-dependencies
  (or `space_ui` must re-export; the owner rule forbids a shim, so add the dependency).
- `crates/space_ui/tests/ui/issues.rs` imports `space_proto` only, and drives the app through the window; it needs no path change.
- `crates/space_ui/tests/ui/new_issue.rs` uses `projects::set_list`/`set_editable` from `space_page_kit::projects` (not moved)
  and the app's own windows. No path change needed beyond what the app shell does.
- `crates/space_ui/tests/ui/main.rs` lists the test modules, including `issue`, `issues` and `new_issue`. No change, but the
  `issue` module stays broken until `issue.rs` is converted.

The `Issues`, `Overview`, `NewIssue`, `NewSprint` `wire` results are generic now, so any test that names those types must
spell `<AppWindow>`. The current tests reach them through the app, not by name.

## 6. Notes on what changed in plain logic

- Each page's window side is a trait (`NewSprintWindow`, `OverviewWindow`, `IssuesWindow`, `NewIssueWindow`). Plain
  view structs carry the values across: `SummaryView`, `CountView`, `SprintView`, `AreaView`, `IssueRowView`, `BoardColumnView`,
  `BoardCardView`, `BoardAreaView`, `QueryNote`, `ProjectOption`, `AreaOption`, `Draft`, `Form`, `Field`.
- `colour_of` (from `crate::pages::admin::types_tags`) is replaced by `issues::colour_rgb` (plain, returns RGB bytes; the
  window turns it into a Slint `Color` in `projects_glue::colour`). The admin copy is not used by this area.
- `crate::pages::admin::choice` is `space_page_kit::picker::choice` (the same function, plain).
- `crate::slint_ui::glue::calendar::calendar_cells` is replaced by plain `space_page_kit::dates::cells` +
  `month_title`, with the window's `projects_glue::days` turning the cells into `CalendarDay`.
- `create_project::save` and `pick_picture` take `Weak<C>` with `C: ComponentHandle`, not `Weak<AppWindow>`. They name no
  generated type.
- The cross-area links go through routes, as before (`invoke_issues_open`), through the window traits `open_issues`.
- `issue.rs` copies the key through `IssueWindow::copy_link(&self, link: &str)`. The macro implements it as
  `$write_link(self, link, |_| {})`, where `$write_link` is the app's `slint_ui::clipboard::write_link` (a `pub(crate)` function
  in `crates/space_ui/src/slint_ui/clipboard.rs`) passed as the last macro argument, so this crate names no app path.

## 7. Files deleted by this task (no shims left)

- `crates/space_ui/src/pages/projects/{mod,create_project,issue,issues,new_issue,new_sprint,overview}.rs` (moved with
  `git mv`, mod.rs removed)
- `crates/space_ui/ui/pages/projects/*.slint` (moved)
- `crates/space_ui/ui/components/{kanban-board,kanban-column,issue-card}.slint` (moved to `areas/projects/ui/`; the
  `app.slint` line 49 and `main.slint` lines 34-35 and `stories.slint` import them under their new path)

## 8. Open items

1. `crates/space_ui/tests/ui/issue.rs` and anything naming the issue page's old window calls must be checked at the final build.
2. `src/pages/mod.rs` (lead): remove `pub mod projects;`.
3. The gallery (`stories.slint`, `stories.txt`) and its `@app_stories` library path need the `@space_area_projects` path, or the
   story rows must be dropped.
4. Doc comments in `app.slint` and `main.slint` still mention `src/pages/projects/...`; cosmetic.
