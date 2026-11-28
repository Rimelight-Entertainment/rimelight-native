# Feature crates: the page pattern

One crate per page, named `rimelight_personal_feature_<page>` and placed at `crates/features/<page>`. The Todos page
(`crates/features/todos`, package `rimelight_personal_feature_todos`) is the reference. Copy it and change the names.

## Why the split

Slint components cannot be nested across crates that each compile their own Slint markup. So only `apps/desktop`
compiles Slint. A feature crate has no `slint` dependency and no `build.rs`. It ships:

- its markup, `ui/<page>.slint`, which the desktop build compiles;
- plain Rust, `src/lib.rs`, which owns the page state and the calls to `rimelight_personal_core`.

## File layout

```
crates/features/<page>/
  Cargo.toml          package rimelight_personal_feature_<page>; [lints] workspace = true; deps: rimelight_personal_core (and nothing slint)
  ui/<page>.slint     export component <Page>Page inherits VerticalLayout (or Rectangle); imports only @rimelight_ui/... and @rimelight_shell/templates/...
  src/lib.rs          UI_DIR const, the API trait, the controller (state + actions), unit tests with a fake API
```

Also in the workspace:

- root `Cargo.toml`: add the crate to `members`, and to `[workspace.dependencies]` as
  `rimelight_personal_feature_<page> = { path = "crates/features/<page>" }`.
- `apps/desktop/Cargo.toml`: `rimelight_personal_feature_<page>.workspace = true` under `[dependencies]`.
- `crates/rimelight_personal_nav/src/manifest.rs`: the route already exists; nothing to add there.

## The markup (`ui/<page>.slint`)

- Import tokens and text styles from `@rimelight_ui/main.slint`, and components from
  `@rimelight_ui/components/<name>.slint`. The shared page templates (ListPage, BoardPage, DetailPage) come from
  `@rimelight_shell/templates/<name>.slint`: they are shared by design, so a feature page uses them rather than copying
  their layout. Never write a colour, size, font or duration literal: use `Tokens.*`,
  `Type.*` and `Icons.*`.
- Do not import `std-widgets.slint` (its ScrollView, LineEdit and the like). Use the builtin `Flickable` for scrolling.
- Data comes in as `in property`. Actions go out as `callback`s named `<verb>-requested`.
- Export the row struct(s) the page needs, for example `export struct TodoRow { id: string, ... }`.
- Keep a text field's contents in an `in-out property` on the page, so Rust can clear it after the page accepts the input.

## The controller (`src/lib.rs`)

- `pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");`
- A trait for the API calls, for example `TodoApi`, with one method per endpoint. Each method takes a
  `Box<dyn FnOnce(Result<T, ApiError>) + Send>` callback, because ehttp answers on a worker thread. Implement the trait
  for `rimelight_personal_core::ApiClient` (a thin forwarding impl) and expose `http_api(base) -> Arc<dyn Trait>`.
- A `Controller` that holds `Arc<dyn Trait>`, a `Mutex<State>` and a `notify: Fn(View) + Send + Sync`. It is `Clone`.
  - Read methods return a plain `View` struct (rows, flags, error text), with no Slint types.
  - Action methods take plain arguments and return `bool` or nothing. They never block: they call the API and return.
  - After each state change, call `notify(view)` **after** releasing the lock.
- Never hold the lock across an API call (a fake can answer synchronously in tests).
- Unit tests in `lib.rs`: a fake `TodoApi` records calls and answers from scripted replies. Cover the state
  transitions, the refusal of blank input, the error path and the refresh after each write. No network.

## The glue (`apps/desktop/src/pages/<page>.rs`)

One module per page, declared in `apps/desktop/src/pages/mod.rs`. It has three jobs:

1. `new(ui, api)`: create the controller with a `notify` that posts the view to the event loop:
   `let weak = ui.as_weak(); move |view| { let _ = weak.upgrade_in_event_loop(move |ui| show_view(&ui, &view)); }`
   Then wire the shell's callbacks to the controller methods.
2. `show_view(ui, view)`: convert the view into the Slint structs and set the shell's properties. Build a new
   `VecModel` each time, so the Slint rows are rebuilt (a row checkbox keeps its own state after a click).
3. `activate()`: called from `show()` in `main.rs` when the route opens, which refreshes the data.

## Wiring the page into the shell

1. `apps/desktop/build.rs`: add the feature's markup folder to the library map. The Todos line is:
   `manifest.join("../../crates/features/todos/ui")`, under the key `rimelight_personal_todos`. Use the key `rimelight_personal_<page>` and
   add a `rerun-if-changed` line for the folder. Keep the `with_debug_info(true)` call.
2. `crates/rimelight_personal_ui_slint/ui/app.slint`: `import { <Page>Page, <Row> } from "@rimelight_personal_<page>/<page>.slint";`. Add the
   properties (`in property <[Row]>`, the flags, `in-out` for field text) and the forwarding callbacks. Then show the
   page with `if root.page-route == "/<page>" : <Page>Page { ... }` and hide the placeholder for that route.
3. `crates/rimelight_personal_ui_slint/ui/app.slint`: `export { <Row> } from "@rimelight_personal_<page>/<page>.slint";` so Rust gets the struct.
4. `apps/desktop/src/main.rs`: create the glue in `run()` and call its `activate()` from `show()` on the route.

## The test (`apps/desktop/src/pages/<page>.rs`, `#[cfg(test)]`)

- `init_integration_test_with_mock_time()` from `i_slint_backend_testing`. The testing backend can be installed once
  per process, so keep one test function per module, or guard it.
- Create the shell with `AppWindow::new()`, wire the glue with a recording fake API, set the route
  (`ui.set_page_route(...)`), and `ui.show()`.
- Click with `ElementHandle::find_by_accessible_label(&ui, "<label>").next().unwrap().mock_single_click(PointerEventButton::Left)`.
  Label the controls with `accessible-name` (or the button text). Keys go in as
  `ui.window().dispatch_event(WindowEvent::KeyPressed { text })` and `KeyReleased`.
- The headless backend runs no event loop, so posted views never arrive. Apply the controller's view in the test
  with the glue's `show_view(&ui, &glue.controller().view())` before asserting on the UI. Assert on the fake API's
  recorded calls and on the controller's view.
- Dev dependencies in `apps/desktop/Cargo.toml`: `i-slint-backend-testing.workspace = true`, and `rimelight_personal_core` for
  building its values.

## Checklist for a new page

- [ ] `crates/features/<page>`: Cargo.toml, `ui/<page>.slint`, `src/lib.rs` with the trait, controller and unit tests
- [ ] root `Cargo.toml`: member and workspace dependency
- [ ] `apps/desktop`: dependency, build.rs library line, app.slint properties and page, app.slint struct export,
      `pages/<page>.rs` glue and its test, `pages/mod.rs` line, `main.rs` activation
- [ ] `cargo build -p personal_desktop`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`

## Reference

- Controller and tests: `crates/features/todos/src/lib.rs`
- Markup: `crates/features/todos/ui/todos.slint`
- Glue and headless test: `apps/desktop/src/pages/todos.rs`
- The egui file a page is ported from is named by `Page::ported_from` in `crates/rimelight_personal_nav/src/manifest.rs`.

## Wiring into rimelight_personal_ui_slint (convention for every page, read before porting)

The app shell is `crates/rimelight_personal_ui_slint` (the only crate that compiles Slint). A page touches it in three files it
owns. Nothing else in the shell changes per page.

1. **Feature crate (you, `crates/features/<page>`)**
   - `pub const UI_DIR` (already the rule above). `ui/<page>.slint` exports `<Page>Page` (data in via `in property`,
     actions out via `callback <verb>-requested`) and every row struct the page needs.
   - `src/lib.rs`: `<Page>Api` trait, `<Page>Controller` (`Clone`, takes `notify: Fn(<Page>View)`), `http_api(base)`.
   - Feature crates never depend on slint, rimelight_shell, or rimelight_personal_ui_slint.

2. **Shell markup (`crates/rimelight_personal_ui_slint/ui/pages/<page>.slint`)**
   - `import { <Page>Page, <Row> } from "@rimelight_personal_feature_<page>/<page>.slint";`
   - `export global <Page>State { in-out property <...> ...; callback <verb>-requested(...); }` mirrors the page's
     properties and callbacks one for one, so Rust reaches them as `ui.global::<<Page>State>()`.
   - `export component <Page>Route inherits Rectangle` forwards: `<Page>Page { rows: <Page>State.rows; <verb>-requested(...) => { <Page>State.<verb>-requested(...); } }`.
   - `export { <Row> } from ...` only where `app.slint` needs to re-export the struct for Rust.

3. **Glue (`crates/rimelight_personal_ui_slint/src/pages/<page>.rs`)**
   - `pub struct <Page>Glue { controller: <Page>Controller }`.
   - `pub fn new(ui: &AppWindow, api: Arc<dyn <Page>Api>) -> Self`: builds the controller with a `notify` that
     posts the view with `ui.as_weak().upgrade_in_event_loop(...)` and calls `show_view`, then calls `wire`.
   - `fn show_view(ui: &AppWindow, view: &<Page>View)`: sets `ui.global::<<Page>State>()` properties. Build new
     models each time.
   - `fn wire(ui, controller)`: each `ui.global::<<Page>State>().on_<verb>_requested(...)` calls a controller method.
   - `pub fn activate(&self)`: called by the shell when the route opens (refreshes the data).
   - `#[cfg(test)]`: one `scenario()` run from `pages/mod.rs`'s single headless test (see the todos reference).

4. **Route registration (owned by the main session, not the page agent)**
   - `crates/rimelight_personal_ui_slint/ui/app.slint`: one `if route == "/<page>" : <Page>Route {}` line and an export line.
   - `crates/rimelight_personal_ui_slint/build.rs` already maps every `@rimelight_personal_feature_<page>` library. A new page adds its
     `UI_DIR` line there in the same step.

Reference implementation for the controller: `crates/features/todos`. Its glue, in the old shape, is
`apps/desktop/src/pages/todos.rs`; move it to step 3 above when the shell moves.
