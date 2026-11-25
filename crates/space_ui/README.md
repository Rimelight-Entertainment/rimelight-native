# space_ui

The Rimelight design system on Slint 1.18.1 ([decision 0007](../../PLAN.md#15-decision-log)). It holds the tokens, icons, text styles, motion rules, the component facade (Rust API and `.slint` components), the story registry for the gallery, and the screenshot helpers.

## The facade rule

Only this crate depends on `slint`. Feature code, `apps/` and the other crates import only this crate: its Rust API and its `.slint` components. Raw Slint std-widgets and colour literals stay inside this crate.

## Layout

- `ui/` Slint markup. `ui/main.slint` is the entry point and exports the shared tokens and the Rust-facing structs. `ui/components/` holds the Space-only components (shell, app pieces, see Components below). The shared components, icons and typography come from `@rimelight_ui` (rimelight-rs).
- `src/theme.rs`, `src/motion.rs`, `src/icon.rs`: theme modes, motion and icon rules, plain Rust.
- `src/slint_ui/`: the Slint side in Rust: `glue.rs` wires the shared rules of `rimelight_ui_kit` (toast, select, menu, color, theme) to the generated types, the shell models (rail, sidebar, tabs, split, palette), and the app shell.
- `tests/`: Slint interaction tests. The component gallery is the shared `rimelight_gallery` (rimelight-rs `apps/gallery`); Space's own stories and the whole-shell capture run in `apps/gallery` as `space_gallery` (see `apps/gallery/README.md`).

## Tokens and themes

- The tokens come from the shared `rimelight_design` crate in the web monorepo (`packages/design/crate`, decision D1). Its `tokens.json` is the source. Regenerate there with `node generate.ts`; the generator also runs every contrast check. Space has no token generator and no token file of its own. Slint imports `@rimelight_design/tokens.slint`, which the build scripts map to `rimelight_design::UI_DIR`. Rust reads the same values as `rimelight_design::tokens`.
- In markup use `Tokens.surface-base`, `Tokens.text-primary`, `Tokens.accent-fill`, `Tokens.radius-control`, `Tokens.space-4` and so on. Use `Type.body.size` (and the other text styles) for type and `Icons.house` (and the other Lucide names) for icons. Never write a literal colour, size, font or duration.
- Easing curves are written in the component, because Slint properties cannot hold them.
- Theme: `Tokens.theme` is `light`, `dark` or `system`. Under `system` the dark flag follows `Palette.color-scheme`, which the window backend updates when the OS theme changes. Rust sets the mode with `slint_ui::glue::theme::set_mode`.
- Reduced motion: `Tokens.reduced-motion` is a manual flag, set with `theme::set_reduced_motion`. Every motion duration is 0 ms when it is on. The OS setting is not read; reading it needs a platform call that the workspace forbids outside the audited module (decision 0008).
- Text scale: `theme::set_text_scale` takes 100, 125, 150 or 200 percent. Type sizes, line heights and icon sizes scale with it (decision 0008).
- Fonts: Noto Sans and JetBrains Mono are bundled by the shared library (rimelight-rs, `rimelight_ui::fonts`, SIL OFL 1.1). `typography.slint` there registers them, so Space's markup does not import the font files. Space has no font files of its own.

## About requirement

The Slint royalty-free licence requires the `AboutSlint` widget on an About screen reachable from the top-level menu ([licence notes](../../PLAN.md#15-decision-log), PLAN section 15.1). `about.slint` in rimelight-rs `crates/rimelight_shell` (imported as `@rimelight_shell/about.slint`) exports `AboutScreen`, which contains `AboutSlint` on a fixed light panel, so the widget reads in both themes. The app shell mounts it. Do not remove the widget.

## Components

The shared components moved to the `rimelight_ui` crate in rimelight-rs (ST2.3): every `ui/components/<name>.slint` in that list, `icons.slint`, `typography.slint` and the fonts. Space imports them as `@rimelight_ui/components/<name>.slint`, `@rimelight_ui/icons.slint` and `@rimelight_ui/typography.slint`, and the build scripts map the `rimelight_ui` library path to `rimelight_ui::UI_DIR`. Their component tables, keyboard rules and stories are in that repo now. The old Space copies are deleted.

Space keeps these pieces until ST2.4 and ST2.5 move them (the shell, the app glue and the rules that know about Space):

- Shell, in rimelight-rs `crates/rimelight_shell` since ST2.5a (`@rimelight_shell/components/...`): `rail`, `sidebar`, `tab-bar`, `split-pane`, `status-bar`, `command-palette`, `more-drawer`, `quick-action`, `connection-state`, `shortcuts-dialog`, and `about.slint`.
- Still in Space (`ui/components/`): `issue-card`, `kanban-board`, `kanban-column`. The account menu and sign-in screen moved to `rimelight_shell` (ST2.5b). In `ui/`: `app-shell.slint` and `main.slint`.
- Space's own work surfaces (`ui/components/`): `issue-card`, `kanban-board` and `kanban-column`.
- Area pages live in the `areas/*` crates (`space_area_*`). Space's app stories for the shared gallery are in `ui/stories/` (`stories.txt` and `stories.slint`, the `AppStories` component). The gallery window and the design system's stories are in rimelight-rs, not here.

The tests of the shared components moved to rimelight-rs in ST2.7: `crates/rimelight_ui/tests/ui/` (26 modules, with their markup in `tests/ui/markup/`). The decision table is `rimelight-rs/docs/st2.7-tests.md`. The tests that remain in `tests/ui/` cover Space's own surfaces, and their markup is in `tests/ui/markup/` (`profile`, `more_drawer`, `rail_badge`, `shell_1` to `shell_3`, `shell_chrome`, `sign_in`, `status_bar`, `date_b6`).

## Known gaps

The gaps below that name a moved component (Button, Table, Tabs and so on) are copied from the old Space README. They belong to the shared library now and should be checked and moved to rimelight-rs.

**Accessibility and keyboard**
- Slint 1.18.1 has no `menu`, `menu-item`, `dialog` or `link` role. The closest roles are used. Screen readers are not verified on any platform.
- Select typeahead is single-character only; the Select clear control is not a Tab stop.
- Combobox Home and End edit the filter.
- Menus do not scroll. A menu longer than the window is clipped.
- Tooltip is not drawn for IconButton yet.
- The Toast stack has no pause on focus; Tab reach into toast buttons is tested, not verified in an app.
- No keyboard routes for the rail and sidebar yet.
- Breadcrumbs collapse by item count (`max-visible`), not by window width, and the ellipsis is not clickable to expand. The focus ring is 2 px, a literal as in Link and Tag.
- Card and Section have no whole-card click target; the Card does not draw a border.
- Tabs has no panel and no vertical orientation; a change of `active` from outside does not move the keyboard focus. Tabs wrap at the ends and do not skip disabled tabs (none exist).
- Accordion has no Home, End or arrow-key moves between headers, and its bodies are plain text, so a body cannot hold controls.
- Stepper's done, current and upcoming states are shown by colour and shape only; the accessible names carry the label alone. Labels do not wrap.
- Pagination has no page-size control and no "jump to page" field; the seven-slot layout is fixed.
- Toolbar has no overflow "more" menu: items past the right edge are clipped. Not built in batch 3 because a width measurement and a menu would have doubled the component; the caller must split the items.
- Toolbar and Tree expose one Tab stop, so a screen reader hears the group, not each item (roving focus). The Toolbar's role is `button` per item; there is no toolbar role in Slint 1.18.1.
- Tree does not move the focus to the selected row on first Tab, and has no type-ahead. The focus can point past the end if the caller shrinks `rows`.
- ScrollArea's thumb is drawn, not draggable, and the area has no horizontal scrolling. Its scrolling uses ScrollView, which Slint 1.18.1 deprecates `viewport-*` names for; the area uses `content-*`.
- ResizablePanel resizes the width only (no vertical panel) and does not save the size.
- FilterBar's clear-search button is a Tab stop of SearchField, so Tab from the field reaches it before the chips.
- DescriptionList splits columns by item count, so a long value does not move an item to the other column. It has no `dl` or table semantics for screen readers.
- Stat's delta icon is a rotated chevron (Icons has no up arrow), and the caller must format the numbers.
- Timeline and ActivityFeed take their times as text; Slint 1.18.1 has no relative-time formatting. The Timeline has no grouping by day and no scroll; the caller limits the events.
- ActivityFeed's actor name is not bold (no rich text in one Text), and the feed has no per-row action or focus. Its "Load more" button is the only Tab stop.
- List and Table do not sort, select or resize by themselves: the caller applies `picked`, `sort-requested` and `column-resized`, as rimelight-rs `crates/rimelight_ui/tests/ui/tier3_b5.rs` does. The gallery stories show static rows, so their sort and resize do nothing.
- Keys do not extend a selection (no Shift+Down). Shift-click ranges are unit tested in `rimelight_ui_kit` (`table.rs`), not driven by a real Shift key event.
- Table has no horizontal scrolling, no column reordering, and Up on the first row does not move to the header. Its sort compares numbers before text, case-insensitively for text, and resets the shift anchor.
- ListView scrolls in whole rows, so the focus row is fully in view but the scroll offset is not pixel-exact.
- The 10,000-row frame time ((not kept)) is from the debug software renderer only, with no release or GPU build measured.

**Text input**
- IME is not tested on Windows or Android (Input, Textarea).
- Textarea auto-grow ignores soft-wrapped lines.

**Motion**
- Reduced motion is a manual flag, not the OS setting.
- Exit fades and the first-show slide of Sheet and Toast are not verified on screen.

**Visual**
- The software renderer draws no box shadows, so headless captures show none. The app's femtovg renderer draws them; not verified on screen.
- Dark theme: some highlights, the Avatar fill and the Progress track read near black.
- Phone-width captures with chrome off are clipped by the gallery frame.
- Some colours are placeholders or derived (`status.info`, parts of the dark, disabled and tint sets). See `PLAN.md section 8.6`.

**Testing**
- No visual regression comparison. Captures are checked by eye.
- The gallery is not built for Android or web yet.

## Testing

- `cargo test -p space_ui --features slint` runs the Slint tests. Interaction tests use real key events (`WindowEvent::KeyPressed`, `KeyReleased`). They need the mock-time backend (`init_integration_test_with_mock_time`).
- Simulated: no OS window and no OS input queue. The winit translation is not exercised.
- `cargo xtask gate test` runs both feature sets.

## Lints and unsafe code

`space_ui` spells out its lint table instead of inheriting `[workspace.lints]`, because Cargo cannot override one inherited lint. It sets `unsafe_code = "deny"`, and `src/slint_ui/mod.rs` allows the unsafe code that Slint generates. Nothing else in the crate may use `unsafe` (decision 0008).
