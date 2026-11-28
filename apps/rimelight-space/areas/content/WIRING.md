# Content area (CMS.6): wiring status

Status of the CMS.6 wiring pass (W8). Nothing was built or tested (owner rule); the first build is the final pass. Every
`UNSURE` below is a guess to check at that build.

## Applied

- Section 1 (Cargo): `rimelight_editor` added at `v0.5.0` in the root `[workspace.dependencies]`. `rimelight_platform` bumped
  from `v0.4.0` to `v0.5.0`, because `webview::WebViews` is not in any v0.4 tag (checked: v0.4.0 and v0.4.1 lack it).
  `space_area_content` and `rimelight_editor` added to `crates/space_ui_slint` build-dependencies and to `crates/space_ui`
  dependencies, with `rimelight_platform`.
- Section 2 (build.rs): the `space_area_content` and `rimelight_editor` library paths are registered.
- Section 3 (imports): `app.slint` imports the five content pages and `BlockEditor`. `TabItem` is renamed
  `ContentTabItem` in the content markup, because `rimelight_ui`'s tab strip already exports `TabItem` and both would clash.
  `main.slint` exports the five page globals, the content structs, `EditorBridge` and the editor's structs.
- Section 3 (callbacks): `content-kind(string) -> string` (pure), `content-opened(string)`, `navigate-requested(string)`.
  `overlay-open` is now `out` so Rust can read it (the web views hide while it is true).
- Section 4 (route blocks): one `if root.content-kind(group.route) == "..." : ...Page` block per content kind, placed before
  Company > Locations. DEVIATION: the single `content-route` property in the WIRING is not used. Tile groups show several
  routes at once, so the markup asks per group (as the `is-*-route` callbacks do for the other areas).
  The EmptyState exclusion includes `root.content-kind(group.route) == ""`.
- Section 4 (`content-opened`): each content page reports its route on `init` and on a route change; Rust wires and loads the
  page for that route (`crates/space_ui/src/slint_ui/content.rs`, `open`). DEVIATION: the pages no longer subscribe to a global
  `opened()`. The `on_opened` trait methods are removed from the content modules, and each module's `load()` is called by the
  dispatcher after `wire`.
- Section 5 (routes): the Team and Department docs rows, the Admin > Content section (Collections, Media library) and the
  Home > Reviews row are in `crates/space_nav/src/manifest.rs`. The admin rows went in a new `admin.content` section.
- Section 6 (editor): `BlockEditor {}` is placed inside the content page block (no bindings). The editor is the window's
  `EditorBridge` global, so the page's binding is the window itself, not a second window. The bind is in
  `glue::block_editor` (`wire_block_editor!`); the page's `EditorHandle` is `Bound` in `content.rs`.
  DEVIATION: the binder binds afresh each time a page opens (it does not call `swap_controller`), because the host callback
  differs per page and `bind_block_editor` is what replaces it.
- Section 7 (`app_shell`): `wire_content!` is expanded in `content.rs` (the same crate as the window, next to the editor glue).
  The shell's `content-kind` and `content-opened` callbacks and the `navigate-requested` handler are wired in `app_shell.rs`.
- Section 7 (web views): `WebHost` (`crates/space_ui/src/slint_ui/webviews.rs`) implements `rimelight_editor::WebViewHost`
  over `rimelight_platform::webview::WebViews`. It keeps the window handle and the scale. It hides every view while the
  overlay or the New page dialog is open, and shows them again on a 16 ms timer (`tick`).
- Section 8 (behaviour, content area fixes):
  - `CreatePage.template_id` is sent when a template is chosen (`home.rs`, `create`).
  - The four new `ContentCollection` fields are in the test literals (`home.rs`).
  - Editing an existing collection is enabled: the list's fields and features open the form, and the update is a PATCH with
    the whole form (`collections.rs`, `open_form`, `save`, `FieldDraft::from_json`).
  - The Drafts tab and the reviews page use `list_content_drafts` (no per-page walk). The Drafts tab asks for `role` Mine and
    Member, each with status Open, and merges the two by draft id (`review.rs`, `gather_drafts`). The reviews page asks for
    status InReview with `role` Reviewer (`review.rs`, `gather_reviews`).
- Section 8 (CMS.6 gaps, this pass):
  - Scope picker: the move dialog lists Company, the projects, then "Team: <name>" and "Department: <name>" for the teams and
    departments the caller can see. The home's `load` fetches `teams` and `departments` with the collections and pages, and
    builds the list with `scope::scope_choices` (`home.rs`, `set_projects`, `load`). The New page dialog creates in the
    current scope and lists no scopes, so it is unchanged. The unit tests are in `scope.rs`.
  - Presence colour: one stable colour per user. The user id is hashed (FNV-1a, then the MurmurHash3 finaliser) into the eight
    colours of `PRESENCE_PALETTE` (`presence.rs`). The caller's own colour in the draft is `presence::colour_hex` of their id
    (`content.rs`, `open`). Tests cover stability, distinctness, 4.5:1 contrast with white, and the spread over the palette.
  - Share: a Public page copies its full public address, the API base the client uses plus the page's route
    (`routes::public_address`, `page.rs`, `share`). An Internal page copies the in-app route and shows "Internal page: the link
    opens in Space".
  - Mentions: the editor's `MentionSearch` calls `content_mentions` (limit 8) and answers through `set_mention_results`.
  - Comments: the editor's `Comment` maps to `apply_comment_request` (with the draft id in edit mode), then the comments reload.
  - Web-only blocks: the controller gets the API base as its web base (`page.rs`, `start_editor`). The editor builds the embed
    URL itself with `embed_url`.

## Not applied, or applied differently

- NOT APPLIED: the WIRING's `ContentEditorPage` as a second window. The editor's `EditorBridge` is a per-window global, so
  the page is inside `AppWindow`.
- NOT APPLIED: `space_i18n` keys (the placeholder crate, as before).

## Guesses to check at the first build (`// UNSURE:` in the source)

- The content pages' route property and `opened(string)` callback order inside `init`/`changed route`.
- `rimelight_editor::wire_block_editor!` expanded in a `pub mod` (`glue::block_editor`) without its own `ComponentHandle`
  import; the macro may need it.
- `ui.get_user_name()` on an `in property` (the getter is assumed).
- `Scope` and `DraftSummary` field names against the server's list shape.
- The public address (`api base + route`) assumes the API serves the page route under the same base; check the route the
  public site uses. The team and department lists in the move dialog are assumed to be what the caller can see.
- The presence palette's 4.5:1 contrast and the 20% spread bound in `presence.rs` are checked only at the first build.
- The `opened` removal changes the content modules' public surface (the `on_opened` trait methods are gone).

---

# Content area (CMS.6): wiring notes for the lead

The area is `areas/content` (package `space_area_content`). Plain logic is in `src/`, markup in `ui/`, the window glue is
`wire_content!` in `src/lib.rs`. Nothing was built, per the owner's rule. The first build is the one final pass.
Strings are English literals, as in the company area (see section 7).

## 0. Status

- Written: `routes.rs`, `scope.rs`, `view.rs`, `home.rs`, `page.rs`, `review.rs` (drafts, reviews page), `collections.rs`,
  `media.rs`, `lib.rs` (`wire_content!`), and the markup `ui/types.slint`, `home.slint`, `page.slint`, `collections.slint`,
  `media.slint`, `reviews.slint`.
- The editor is `rimelight_editor` (README read 2026-11-23, after the coordinator's API change). The page uses the editor's
  own emoji picker, link prompt, locale switcher, comment panel, block menu and presence strip. None is rebuilt here.
- Not done: the app wiring below (section 3 onward) and the `UNSURE` items in section 8.

## 1. Cargo (workspace and the two shells)

Root `Cargo.toml` `[workspace.dependencies]`. `rimelight_content` (v0.5.0) and `rimelight_platform` (v0.4.0) are present. Add:

```toml
rimelight_editor = { git = "https://github.com/Rimelight-Entertainment/rimelight-rs.git", tag = "v0.5.0" }
```

`rimelight_platform` is already listed at v0.4.0. The content crate only names it (the app owns the web views), so it can stay;
bump it to v0.5.0 only if `WebViews` changed in that tag (UNSURE).

`crates/space_ui/Cargo.toml`, next to the other `space_area_*` lines (after `space_area_projects`):

```toml
space_area_content = { path = "../../areas/content" }
```

`crates/space_ui_slint/Cargo.toml`: the same line in `[build-dependencies]` (where `space_area_company` and the projects line
are). The editor crate is needed by the markup, so add it to the same place:

```toml
rimelight_editor = { workspace = true }
```

`areas/content/Cargo.toml` declares `space_nav`, `space_i18n` and `rimelight_ui_kit` as dependencies, but no code uses
them yet (`space_i18n` is still a placeholder). They can be dropped if the build complains about nothing; keeping them matches
the brief.

## 2. build.rs (`crates/space_ui_slint/build.rs`)

In the `with_library_paths` map, after the `space_area_projects` entry, same tuple shape:

```rust
(
    "space_area_content".to_string(),
    PathBuf::from(space_area_content::UI_DIR),
),
(
    "rimelight_editor".to_string(),
    PathBuf::from(rimelight_editor::UI_DIR),
),
```

## 3. Slint imports (`crates/space_ui/ui/app.slint`)

```slint
import { ContentHomePage, ContentHomeState } from "@space_area_content/home.slint";
import { ContentPageView, ContentPageState } from "@space_area_content/page.slint";
import { ContentCollectionsPage, ContentCollectionsState } from "@space_area_content/collections.slint";
import { ContentMediaPage, ContentMediaState } from "@space_area_content/media.slint";
import { ContentReviewsPage, ContentReviewsState } from "@space_area_content/reviews.slint";
import { TabItem, ListRowItem, DraftRowItem, ChoiceItem, PageHeaderItem, CollectionRowItem, FieldRowItem, MediaCellItem, ReviewRowItem } from "@space_area_content/types.slint";
import { BlockEditor } from "@rimelight_editor/block-editor.slint";
```

`ui/` of the content area imports `@rimelight_ui/...` and `@rimelight_design/...` and `types.slint` by relative path; those
resolve inside the area.

App-shell properties and callbacks (new):

```slint
callback navigate-requested(string);          // Rust invokes it; the shell's handler does the navigation
in property <string> content-route: "";        // set by Rust from routes::parse (see section 4)
```

The window needs `get_user_id()` (the company profile already uses it on the same window).

## 4. The `if group.route ...` blocks (`app.slint`)

Next to the Teams page block, add one block per content page. Use the same `content-route` kind string that Rust sets:

```slint
if content-route == "home": ContentHomePage {}
if content-route == "page": ContentPageView {
    // The editor is the app's own page (section 6). Its element goes here.
}
if content-route == "collections": ContentCollectionsPage {}
if content-route == "media": ContentMediaPage {}
if content-route == "reviews": ContentReviewsPage {}
```

Rust sets `content-route` in `app_shell.rs` from `space_area_content::routes::parse(route)`: `ScopeHome(_)` gives "home",
`Page{..}` gives "page", `Collections` "collections", `Media` "media", `Reviews` "reviews". Add a small mapping function there
(`fn content_kind(route: &str) -> &'static str`) as the company area does for its team route.

EmptyState exclusion list: the shell shows an empty state for routes with no page. Add the five content kinds (`"home"`,
`"page"`, `"collections"`, `"media"`, `"reviews"`) to the exclusion list, as the company pages are excluded.

## 5. Route rows (`crates/space_nav/src/manifest.rs`)

Existing and kept: `/p/:key/docs`, `/p/:key/docs/:page` (PROJECT_DOCS), `/company/knowledge`, `/company/knowledge/:page`
(COMPANY_INFORMATION). Add:

- `COMPANY_ORG`, after the `/company/teams/:team` row (section 8, Company > Organisation), icons `file-text` and `file-pen-line`:

```rust
("/company/teams/:team/docs", "Team docs", &[T::Doc], 8, "file-text"),
("/company/teams/:team/docs/:page", "Team doc page", &[T::Doc], 8, "file-pen-line"),
("/company/departments/:dept/docs", "Department docs", &[T::Doc], 8, "file-text"),
("/company/departments/:dept/docs/:page", "Department doc page", &[T::Doc], 8, "file-pen-line"),
```

- The admin section that holds the `/admin/...` rows (find it with `grep -n "/admin/" manifest.rs`), icons `layers` and `image`:

```rust
("/admin/content/collections", "Collections", &[T::List], 9, "layers"),
("/admin/content/media", "Media library", &[T::List], 9, "image"),
```

- The Home section (near `/home/notes`), icon `clipboard-check`:

```rust
("/home/reviews", "Reviews", &[T::List], 2, "clipboard-check"),
```

The `T::` kinds and sort numbers follow the neighbouring rows; adjust the numbers to the section's order.

## 6. The editor page (`app.slint` and the app's Rust)

The editor component (the Slint page that `wire_block_editor!` binds) is an app component:

```slint
export component ContentEditorPage inherits BlockEditor {}
```

**UNSURE, the main open question.** `ContentPageView` (in `page.slint`) has `@children`, so the editor element sits inside the
page's markup in `app.slint`. A child element of `AppWindow` has no Rust handle that `bind_block_editor(&page, ...)` can
take, because `bind_block_editor` calls `page.set_rows`, `page.on_text_edited` and so on on a `&$page`. Two ways out, the lead
picks one:

1. Make the editor a root window of its own (a second `ComponentHandle` shown next to the shell). Simple to wire, but a second
   window for the page.
2. Ask rimelight_editor for a binder over an in-window property bridge (the editor's properties and callbacks forwarded through
   `AppWindow`'s properties). Needs a change in rimelight_editor.

The content crate does not depend on which: `page::PageDeps.binder` is an `EditorBinder` closure, and the page talks to the
editor only through `EditorHandle`.

App side (`crates/space_ui/src/slint_ui/content_editor.rs`, new file, expanded once in the app crate):

```rust
rimelight_editor::wire_block_editor!(
    ContentEditorPage, EditorRow, RemoteCaret, InsertItem, MentionItem, InspectorField, KeyOutcome,
    EmojiItem, BlockMenuItem, LocaleItem, CommentItem, PresenceItem
);

struct Bound(Rc<BlockEditorBinding>);
impl space_area_content::page::EditorHandle for Bound {
    fn refresh(&self) { self.0.refresh(); }
    fn set_remote_cursors(&self, cursors: Vec<RemoteCursor>) { self.0.set_remote_cursors(cursors); }
}

pub fn binder(page: ContentEditorPage) -> EditorBinder {
    Box::new(move |controller, on_host| {
        Rc::new(Bound(bind_block_editor(&page, controller, on_host)))
    })
}
```

`EditorBinder` and `EditorHandle` are the content crate's own types; `Bound` is a local type, so the impl is allowed.

## 7. `app_shell.rs` (`crates/space_ui/src/slint_ui/app_shell.rs`)

Expansion, once, at module level in the app crate (after the other `wire_*!` calls; the names are the Slint globals and structs
from section 3):

```rust
space_area_content::wire_content!(
    AppWindow,
    PageLoadState,
    ContentHomeState,
    ContentPageState,
    ContentCollectionsState,
    ContentMediaState,
    ContentReviewsState,
    TabItem,
    ListRowItem,
    DraftRowItem,
    ChoiceItem,
    PageHeaderItem,
    CollectionRowItem,
    FieldRowItem,
    MediaCellItem,
    ReviewRowItem,
    crate::slint_ui::clipboard::write_link, // the app's clipboard fn, as the projects area takes it
);
```

`PageLoadState` is the kit's enum as generated by the app (the company and projects pattern uses the same name).

Wiring, in the session's startup next to the company and projects wiring (`data` is the session's `DataCell`):

```rust
use space_area_content::{collections, media, page, review, routes, scope::{Scope, scope_choices}, home};

// Once per window:
let collections = collections::wire(&ui, data.clone());
let media = media::wire(&ui, data.clone(), None);
let reviews = review::wire_reviews(&ui, data.clone());
reviews.set_scopes(scopes_from_session()); // Company, each project, team and department the caller can see

// Per navigation (the callbacks are replaced each time, so re-wiring a page is the way to change its route):
match routes::parse(&route) {
    Some(routes::Route::ScopeHome(scope)) => {
        let home = home::wire(&ui, data.clone(), scope);
        home.set_scope_choices(scope_choices(&projects, &teams, &departments));
    }
    Some(routes::Route::Page { scope, page: id, edit }) => {
        let deps = page::PageDeps {
            binder: content_editor::binder(editor_page.clone()),
            on_web: Box::new(move |ev| web_views.borrow_mut().on_editor_web(ev)), // see below
            name: user_name.clone(),
            color: presence_colour.clone(),
        };
        page::wire(&ui, data.clone(), scope, &id, edit, deps);
    }
    _ => {}
}
```

The `scope_choices` inputs are `(id, name)` pairs from the projects, teams and departments the caller can see. The home page
keeps them for its move dialog.

`navigate-requested`: the window's handler (the same place the shell routes other callbacks) sets the route, the same way the
nav menu does: `ui.on_navigate_requested(move |route| shell.navigate(route.as_str()))`.

Copy: `copy_text` calls the `write_link` argument of `wire_content!`. The page's share copies the route path (section 8).

Web views (decision 8): the app owns the native web views. Create one `rimelight_platform::webview::WebViews` per window and
keep it in an `Rc<RefCell<_>>`. `on_web` gets `HostEvent::WebGeometry { key, x, y, w, h, url }` and calls
`WebViews::show(&window_handle, &key, x, y, w, h, scale, &url)`. A 16 ms timer re-positions them (the CMS.0 pattern). While a
Slint popup or dialog is open (the editor's menus, the New page dialog, the move dialog), call `hide_all()` and show the
block's snapshot in its place. The embed base is the API base plus `/embed/block`; `rimelight_editor::embed_url` builds the
URL. Open question: which web-view calls the editor reports `hide_all` for; the editor reports its own menus via `MenusChanged`.

## 8. Behaviour notes, and what is not in the client yet

- Routes: scope home `/p/:key/docs`, `/company/knowledge`, `/company/teams/:id/docs`, `/company/departments/:id/docs`; pages add
  `/:page` and `?mode=edit`. Collections `/admin/content/collections`, media `/admin/content/media`, reviews `/home/reviews`.
- Share: see Section 8 of the status above (Public pages copy the API base plus the route).
- Templates: the New page dialog lists templates and sends the template id to `new-create`, but `POST /v1/content/pages` takes a
  type and a title only. The template is not applied yet (UNSURE: the create route needs a template field).
- Move (`PATCH .../scope`): the UI needs manage on the page's scope; the target scope's access is not known to the list, so the
  server's answer is the check. A refusal shows as an error line.
- Drafts tab and reviews page: both use `list_content_drafts` with the filters in the status above (no walk over pages).
- Reject sends its reason as a comment on the draft, then the reject call (the reject route takes no text).
- Mentions: the `@` search has no client call; it returns no results (UNSURE).
- Comments: the editor asks for threads (`CommentRequested` sets them from the page's comments). The editor's submit, resolve and
  reopen are `ControllerEffect::Comment { request }`; the page shows "Comment changes are not saved yet." until the request
  fields are mapped to the comment routes (UNSURE: the `CommentRequest` shape).
- Catch up: reconnects the draft's room. UNSURE: no "merge main into draft" call exists in the client.
- Invite: takes a user id. The people picker is the app's (UNSURE: the company area's person picker is the model).
- Collections: the list response has no fields, so an existing collection's fields cannot be read. Saving an existing collection
  is refused in the form (a PATCH without fields would drop them). New collections are created with their fields. Field types
  are the registry vocabulary; `date` is not in it (UNSURE: the CMS's full list).
- Media: thumbnails are a placeholder label (UNSURE: decode through `space_page_kit::page_data`). Folders come from the asset keys
  of the current listing (UNSURE: no folder list in the API). The grid is a list of cells, not a wrapping grid.
- Upload uses the desktop file picker (`rfd`), as the canvas area does. Web and Android show a notice.

## 9. Unit tests (plain logic, in the modules)

- `routes`: every pattern parses, page edit mode, admin and review routes, bad paths refused, builders round-trip.
- `scope`: company first in the picker, move targets drop the current scope, move needs manage on both, access text, page scope
  round-trip.
- `view`: localized text fallback, slugs from titles, typed slug checks, status and visibility labels, times, hex colours,
  search matching.
- `review`: mine-or-invited, draft rows carry the review state.
- `home`: scope labels, collection choices keep order.
- `page`: visibility wording (initials helper removed with the presence strip).
- `collections`: field kinds round-trip, a good form validates and builds its input, bad forms refused, a 409 reads as "has
  pages".
- `media`: keys split into folder and name, sizes, mime from the extension.

No test was run (owner rule).

## 10. i18n

The company area has no `space_i18n` calls (`space_i18n` is a placeholder), and its Slint uses plain literals. This area does the
same. Every user-visible string is a literal in `ui/*.slint` or in the Rust above. UNSURE: when `space_i18n` lands, the keys go
here.

## 11. Things the first build will show (UNSURE in the source)

- `page.rs`: `Inbox` is sent across threads through `ContentEventSink` (`Arc<dyn Fn + Send + Sync>`); the sink must be `Send`.
- `review.rs` and `media.rs`: `space_page_kit::task::spawn_task` and `page_data::ui_thread` take the closures shown; check their
  exact bounds.
- `lib.rs`: `wire_content!` implements the five traits for `$AppWindow`; method names on the generated globals (`on_open_new`,
  `on_new_item`, `on_open_folder`, `on_start_edit`, `set_edit_mode`, `set_behind`, `invoke_navigate_requested`) must match the
  Slint callbacks and properties in `ui/*.slint`. Those names were chosen to avoid callback/property name clashes.
- `home.rs`: the New page dialog's template choice is passed through and ignored (section 8).
