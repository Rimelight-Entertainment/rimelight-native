# Admin area (`space_area_admin`): shared-file edits for the lead

The area is moved, nothing was built (owner rule). The lead applies these edits, then runs the one build.

## 1. Cargo

- `crates/space_ui/Cargo.toml`, `[dependencies]`, next to `space_area_management` (line 50):
  `space_area_admin = { path = "../../areas/admin" }`
- `crates/space_ui_slint/Cargo.toml`, add the same line to BOTH `[dependencies]` (after line 21) and `[build-dependencies]` (after line 31),
  with the comment "# The Admin area (SP.3e): its `wire_admin!` is expanded here, and its markup has library path `space_area_admin`."
- Root `Cargo.toml`: nothing. `members` already has `areas/*`.

## 2. `crates/space_ui_slint/build.rs` (library paths, after the `space_area_management` entry, around line 31)

```rust
        (
            "space_area_admin".to_string(),
            PathBuf::from(space_area_admin::UI_DIR),
        ),
```

## 3. Slint imports and exports

`crates/space_ui/ui/app.slint`, lines 65-68 become:

```slint
import { AdminUsersPage } from "@space_area_admin/users.slint";
import { AdminOrganisationPage } from "@space_area_admin/organisation.slint";
import { AdminRolesPage } from "@space_area_admin/roles.slint";
import { AdminTypesTagsPage } from "@space_area_admin/types-tags.slint";
```

`crates/space_ui/ui/main.slint`, lines 54-60 (the comment on line 54 can say "(space_area_admin)"). `CalendarDay` stays on its `@rimelight_ui` line:

```slint
// Admin (SP.3e): the admin pages' row structs and their state globals (space_area_admin).
export { AdminUserRow, AdminUsersState } from "@space_area_admin/users.slint";
// The start date pickers of Admin > Users draw these cells (the Rust calendar rules fill them).
export { CalendarDay } from "@rimelight_ui/components/calendar-grid.slint";
export { AdminRow, AdminOrgState } from "@space_area_admin/organisation.slint";
export { RolePerson, AdminRolesState } from "@space_area_admin/roles.slint";
export { AreaRow, TypeRow, AdminTypesTagsState } from "@space_area_admin/types-tags.slint";
```

The route strings in app.slint (`"/admin/users"` and so on) do not change.

## 4. `crates/space_ui/src/slint_ui/app_shell.rs`

Imports, lines 23-26, replace with:

```rust
use space_area_admin::{organisation, roles, types_tags, users};
```

Struct fields, lines 114-118, become (the types are the pages' `Page` structs, generic over the window):

```rust
    pub admin_users: space_area_admin::users::Page<AppWindow>,
    pub admin_org: space_area_admin::organisation::Page<AppWindow>,
    pub admin_roles: space_area_admin::roles::Page<AppWindow>,
```

Call sites, lines 1124-1127, become:

```rust
    let admin_users = users::wire(&ui, data.clone());
    let admin_org = organisation::wire(&ui, data.clone());
    let admin_roles = roles::wire(&ui, data.clone());
    types_tags::wire(&ui, data.clone());
```

(The old `users::wire(&ui, ...)` took `&AppWindow`; the new one is generic, so the same expressions compile.)

Line 1139-1140 (`SelectNav` wiring) stays. `organisation::wire` used to call `wire_nav` a second time; that call is gone from the page, so this line is the only one.

`crates/space_ui/src/pages/mod.rs`: remove `pub mod admin;` (line 3) and update the header comment on line 1 to say "admin" has moved to `areas/admin`. (Not edited here, as instructed.)

## 5. `wire_admin!` invocation: `crates/space_ui_slint/src/lib.rs`

Put it right after the `wire_management!` block (around line 44). It must sit in the crate that generates the types (same reason as the other areas):

```rust
// Admin area window glue (SP.3e): Users, Organisation, Roles and Types & tags (`space_area_admin`).
space_area_admin::wire_admin!(
    AppWindow,
    PageLoadState,
    SelectOption,
    TableColumn,
    TableRow,
    CalendarDay,
    AdminRow,
    AdminOrgState,
    AdminUserRow,
    AdminUsersState,
    AdminRolesState,
    RolePerson,
    AdminTypesTagsState,
    TypeRow,
    AreaRow,
);
```

Argument order matches the macro: window, `PageLoadState`, `SelectOption`, `TableColumn`, `TableRow`, `CalendarDay`, `AdminRow`, `AdminOrgState`, `AdminUserRow`, `AdminUsersState`, `AdminRolesState`, `RolePerson`, `AdminTypesTagsState`, `TypeRow`, `AreaRow`. All are exported from `main.slint` already.

The macro also implements `AdminCompany` for `AppWindow`, using the window's `employees-state`, `profile-state`, `teams-state`, `team-state`, `dept-state` and their `invoke_*_retry` methods (the same ones the old `refresh_company` called). Nothing else in `space_ui` needs `refresh_company` any more.

## 6. Tests: `crates/space_ui/tests/ui/admin.rs` (not edited, as instructed)

Lines 14-17 reference moved items. Change the paths; the types and method names are unchanged:

- `use space_ui::pages::admin::Reference;` becomes `use space_area_admin::Reference;`
- `use space_ui::pages::admin::organisation::{Row, Saved};` becomes `use space_area_admin::organisation::{Row, Saved};`
- `use space_ui::pages::admin::roles::Change;` becomes `use space_area_admin::roles::Change;`
- `use space_ui::pages::admin::users::Written;` becomes `use space_area_admin::users::Written;`

The page methods the tests call (`generation`, `write_generation`, `deliver`, `deliver_list`, `deliver_write`, `deliver_change`, `deliver_save`, `deliver_delete`, `selected`) keep their names and signatures. `tests/ui/admin_users.rs` and `tests/ui/main.rs` need no change.

## 7. Other code that referenced the old module

- `crates/space_ui/src/pages/canvas/list.rs` imported `crate::pages::admin::choice`. I replaced it with a local `choice` helper (no lead action).

## Not resolved / check at the one build

- Nothing compiled. The first build will show any slip in the new glue.
- The window traits name methods `tab`, `areas`, `show_state`, `show_error`, `picked` and so on. If `AppWindow` has an inherent method with one of those names, the inherent one wins and the trait impl will not be called. Rename the trait method if the build shows it.
- `picker_glue!`, `load_glue!`, `table_glue!` and `calendar_glue!` are not used by name except through `load_glue!`, `picker_glue!` and `table_glue!` (inside `admin_glue`). `calendar_glue!` is not used at all: the admin macro maps `CalendarCell` to `CalendarDay` itself.
- `rimelight_ui_kit` (toast) is now a dependency of `space_area_admin` (workspace dep, same as the company area).
- The pages' markup (`areas/admin/ui/*.slint`) is unchanged except for comment paths. Its imports are all `@rimelight_*` library paths, so no sibling imports were needed.
