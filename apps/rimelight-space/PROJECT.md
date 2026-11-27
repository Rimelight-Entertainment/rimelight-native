# Rimelight Space — Project Specification

## 1. High-Level Vision & Scope

**Rimelight Space** is the proprietary, all-in-one Enterprise Resource Planning (ERP) and operational workstation for Rimelight Entertainment. It provides a cohesive native workspace unifying company management, creative production, game development workflows, player support, and personal productivity into a single high-performance Rust application.

### Key Pillars

1. **Native Multiplatform Performance:** Built in Rust with **Slint 1.18.1**, compiling to high-performance native desktop clients (Windows, macOS, Linux) and mobile (Android, iOS), alongside a WebAssembly target.
2. **Real-Time Collaboration (Loro CRDT):** Document, project, and block editing feature character-level real-time co-editing, live cursor presence, and offline resilience powered by **Loro CRDT**.
3. **Single Source of Truth for Franchise Content:** Rimelight Space is the canonical authoring studio for all CMS content, wiki pages, screenplays, and documentation. Published content feeds directly to `rimelight.com`.
4. **Unified Operations:** Eliminates fragmented third-party SaaS tools by unifying HR, finance, supply chain, asset tracking, project management, customer relationships, and player support into one unified database and UI.

## 2. System Architecture

```mermaid
flowchart TB
    subgraph Client["Client Application (Slint 1.18.1)"]
        Shell["App Shell (rimelight_shell)"]
        UI["Design & Components (rimelight_ui, space_ui)"]
        Areas["Feature Areas (Personal, Projects, Company, CMS)"]
        CRDT["Loro CRDT Engine"]
        Shell --> UI
        UI --> Areas
        Areas --> CRDT
    end

    subgraph Backend["Cloudflare Edge Backend"]
        Workers["Space API on the rimelight.com Worker (TypeScript, Rimelight-Entertainment/rimelight)"]
        Hyperdrive["Cloudflare Hyperdrive"]
        DB[(Neon PostgreSQL)]
        Auth["Auth0 Tenant Integration"]
        Workers --> Hyperdrive --> DB
        Workers --> Auth
    end

    Client -- "Secure HTTP / WebSockets" --> Backend
```

### Core Technologies
- **Language & Toolchain:** Rust 2024 Edition, pinned compiler toolchain (`rust-toolchain.toml`).
- **UI Framework:** Slint 1.18.1 (`slint`, `slint-build`, `i-slint-backend-testing`).
- **Design Tokens:** `rimelight_design` (`tokens.json` compiled to `ui/tokens.slint` and Rust constants).
- **Component Suite:** `rimelight_ui`, `rimelight_ui_kit`, `space_ui`, `rimelight_shell`.
- **Media Engine:** `rimelight_media` and `rimelight_picture` (integrated video, audio, image decoders, and WebP pipeline).
- **Backend Infrastructure:** The Space API is TypeScript on the rimelight.com Worker (`apps/rimelight.com/src/space/` in `Rimelight-Entertainment/rimelight`; decision 0041 removed the Rust Worker), using Cloudflare Hyperdrive for pooled PostgreSQL connections and Auth0 enterprise identity abstraction.

---

## 3. Product Modules & Areas

### 3.1 Personal Workplace & Communication
- **Personal Tasks & Todos:** Focus modes, task queues, daily schedules, and personal calendar.
- **Unified Chat:** Real-time team messaging, channels, direct messages, and contextual thread discussions.
- **Personal Notes & Scratchpads:** Quick-capture rich text notes with markdown rendering.

### 3.2 Projects & Issue Tracking
- **Issue Tracking:** Kanban boards, sprint backlogs, task assignments, and priority triage.
- **Milestones & Slices:** Vertical slice tracking with verifiable criteria and gate validation.
- **Game Production Workflows:** Asset pipeline tracking linking Unreal Engine and game assets.

### 3.3 Company, People & HR
- **Directory & Org Chart:** Team rosters, departmental structures, reporting lines, and access roles.
- **Time Off & Leave:** Vacation tracking, public holiday calendars, and approval workflows.
- **Recruitment & Onboarding:** Candidate tracking, onboarding checklists, and role provisioning.

### 3.4 Finance & Operations
- **Corporate Accounting:** Expense tracking, invoice generation, balance ledgers, and transaction audit trails.
- **Hardware & Asset Inventory:** Machine allocation, licenses, devkit assignments, and office procurement.

### 3.5 Player Support & Community Operations
- **Customer CRM:** Unified support ticket inbox, game account lookups, and player feedback.
- **Moderation Tools:** In-game audit log inspection and player dispute resolution.

### 3.6 CMS Block Editor Studio
*Canonical authoring tool for wiki, docs, blog, and screenplay content.*
- **Co-Editing:** Multi-user character-level collaborative editing with cursors and presence via Loro CRDT.
- **Adaptive Layout:** Inline WYSIWYG editing with side inspector panel (or bottom sheet on mobile).
- **Block Catalogue:**
  - *Text Primitives:* Paragraph, Section (nesting 2–6 deep), Callout (with status variants), Quote, Code (with language highlighting and line numbers).
  - *Lists:* Unordered List, Ordered List, List Item (with tab-indent nesting).
  - *Media & Data:* Image (with caption, sizing, alt text), Table (cell editing, border toggle), Divider, File Tree (interactive directory hierarchy).
  - *Interactive Elements:* Tabs & Tab Item, Steps & Step Item, Cards & Card (with badges, icons, links).
  - *Screenplay Blocks:* Scene heading, action, character, dialogue, parenthetical, transition, and shot blocks for franchise narrative writing.

---

## 4. Workspace & Repository Structure

```
rimelight-space/
├── apps/
│   └── desktop/                 # Desktop entry application (Windows, macOS, Linux)
├── areas/                       # Domain functional areas
│   ├── company/                 # Company directory, organizational units, and HR
│   ├── management/              # Executive dashboards, operations, and policy
│   ├── personal/                # Personal tasks, schedule, and workspace
│   └── projects/                # Issue tracking, kanban, and sprint boards
├── backend/                     # Rust helper crates: Postgres migration SQL (db) and a jobs placeholder; the API is on rimelight.com
├── crates/                      # Internal Rust modules
│   ├── space_ui/                # Slint markup, themes, and UI widgets
│   ├── space_page_kit/          # Standard page layout shells and navigation
│   └── space_core/              # Domain models, local storage, and API clients
├── release/                     # Velopack installer configurations and release scripts
├── xtask/                       # Monorepo automation tasks (gates, formatting, checks)
├── AGENTS.md                    # Agent instructions: strict "build once" rule and gates
├── README.md                    # Local development quickstart and build commands
└── PROJECT.md                   # Comprehensive technical manual & architecture (this file)
```

---

## 5. Development Conventions & Standards

### 5.1 Rust & Slint Rules
- **No Direct Std-Widgets:** Feature code must import components from `rimelight_ui` / `space_ui`, never standard Slint widgets directly.
- **Design Token Strictness:** All colors, typography, and spacing must reference `rimelight_design` tokens. Hardcoded pixel or hex values are prohibited.
- **Strict Single-Build Workflow:** Developers and AI assistants must batch code changes across crates and execute only a single compilation and test run at the conclusion of a work batch to prevent disk thrashing.

### 5.2 Naming Conventions
- **Crates:** Prefixed with `space_*` for internal space crates.
- **Slint Files:** `PascalCase.slint` (e.g., `IssueCard.slint`, `ProjectBoard.slint`).
- **Rust Modules:** Standard `snake_case`.

---

## Status

**Status:** v3, 2026-11-15. Replaces the phase 0 step list (v2, 2026-11-09). Owner: Daniel.

Rimelight Space is the internal, proprietary ERP for Rimelight Entertainment: one Rust application covering personal work, chat, projects, company, people, finance, customers, operations, analytics and admin.

---

### 1. How agents work

1. Read sections 2 and 3 once. They are the constraints every slice works under.
2. Work on one **slice** at a time (section 4), in order. A slice is a feature built end to end, not a layer.
3. A slice is done when its **gate** passes and its row in section 4 says done. The gate is: the slice's own tests pass, `cargo xtask gate quick` passes, and the owner has seen the feature running (desktop light and dark, and phone width). Never mark a slice done on reasoning alone.
4. Build only the shared components and templates a slice needs. Extract a shared piece when a second slice needs it, not before. Components go in `rimelight_ui`, feature pages in its area modules (decision 0011).
5. If a slice forces a choice this plan does not make, add an entry to section 15 with status `proposed`. Do not silently deviate.
6. Do not expand scope. Section 5 lists what is out of scope for now.
7. Commit one local commit per finished slice, with the slice ID first in the message (for example `S1: Projects issues list`). Never push unless the owner says so.

---

### 2. Locked decisions

Answers given by the owner on 2026-11-09. Unchanged in v3.

#### Product

| Topic | Decision |
|---|---|
| Name | Rimelight Space |
| Audience | One company, internal. No tenant isolation, billing or signup |
| Scale target | 250+ users within two years. Enterprise-grade controls and audit from the start |
| Company context | Game studio. Company formally starts after the owner's move to the UK. Single country, single legal entity (UK) |
| Relation to rimelight.com | Separate database and backend, shared sign-in. rimelight.com renders the public pages (careers, support form, share links) by calling the Rimelight Space API. No access to rimelight.com's store or forum data. Space owns all CMS content and rimelight.com renders the published part (decision 0049, 2026-11-23) |
| Relation to rimelight-personal | New repo, clean slate. rimelight-personal is not a code source |
| AI | None for now. Leave a seam |
| Languages | English and Portuguese from phase 0 |
| Accessibility | Keyboard navigation and WCAG AA contrast in both themes, reduced motion, scalable text. Screen readers where the Slint toolkit supports them (decision 0007) |

#### Platform and architecture

| Topic | Decision |
|---|---|
| Frontend | Rust with Slint 1.18.1 (decision 0007) |
| UI distribution | Slint, one crate family pinned exactly (`slint =1.18.1`) |
| Component base | Our own components in `rimelight_ui` as `.slint` files, drawn from tokens (decision 0007). Feature code never imports Slint components or std-widgets directly |
| Targets | Windows, macOS, Linux, Android, iOS, and web through wasm |
| Apple targets | Compile-check only for now. Running and verifying macOS and iOS is deferred |
| Web fallback | If the web target is not viable, defer web and keep going. Native targets are never blocked by web |
| Backend | TypeScript Space API on the rimelight.com Worker (decision 0041) with hosted Postgres through Hyperdrive. Everything is Cloudflare-based |
| Identity | Auth0 now (existing tenant, see `@rimelight/auth`), behind an abstraction. Own in-app identity provider in a later phase |
| Offline | Online only, with cached reads |
| Live co-editing | Docs and whiteboards only. Everything else gets live updates without simultaneous editing |
| Permissions | Full policy engine (relationship-based, Zanzibar-style) with roles on top |
| Distribution | Direct downloads with auto-update. Push notifications on mobile, native notifications on desktop. No public app stores |

#### UX

| Topic | Decision |
|---|---|
| Visual references | JetBrains Space (primary), Artcraft launcher, Zed |
| Desktop shell | Zed-style workspace: icon rail, contextual sidebar, pane group with tabs and splits, dockable panels |
| Mobile shell | Bottom tabs plus slide-in drawer, stacked pages with back navigation |
| Themes | Light, dark, follow system. No user accent colour, density setting, theme files or org branding |
| Phase 0 pages | Every page is a wireframe with mock data |

#### Process

| Topic | Decision |
|---|---|
| Phase 0 backend | Thin real slice (sign-in, current user, org, navigation capabilities), runnable entirely on the local machine. No deployed Worker or database is required |
| Order after phase 0 | Platform core, then personal modules. Remaining order chosen by dependency and risk (section 4 of this file) |
| Verification | Automated gates per step and screenshot evidence |

---

### 3. Assumptions to confirm

All fourteen were reviewed with the owner on 2026-11-09. The table shows the confirmed answer. Items marked **changed** differ from the draft's default.

| # | Confirmed answer | Why it matters |
|---|---|---|
| A1 | **Changed.** Crate prefix is `rimelight_` (for example `rimelight_ui`), so crates can be shared with other Rimelight Rust projects | Naming across the workspace |
| A2 | Inventory lives at org level (Operations area). Each project gets an Inventory page showing the stock and equipment allocated to it | You listed inventory per project but chose "physical stock and equipment" |
| A3 | **Changed.** Agents make one local commit per finished step, with the step ID in the message. They never push, open pull requests or create remote branches unless asked | Rollback per step |
| A4 | **Changed.** Local Postgres is a native Windows install running as a service. No Docker | Section 9 |
| A5 | The code is proprietary and the repo is private on GitHub, with GitHub Actions for CI | Licence headers, CI minutes for macOS runners |
| A6 | **Changed.** Neither acknowledgement tracking nor e-signature. Policies are a document store only | People area |
| A7 | **Changed.** Three public surfaces exist, all rendered by rimelight.com (the Solid site) calling the Rimelight Space API: careers pages with an application form, a support contact form with a ticket status link, and read-only share links for docs, files, boards and roadmaps. Whiteboards are not publicly shareable. Rimelight Space itself serves no public pages | Section 7.9, phases 1, 5, 6, 8, 11 |
| A8 | Portuguese means Brazilian Portuguese (`pt-BR`) | i18n files |
| A9 | **Withdrawn 2026-11-12.** Voice and video calls were removed from Chat, so Cloudflare Realtime (SFU) is not needed for now | "Everything is Cloudflare-based" |
| A10 | Base currency is GBP with multi-currency transactions (store revenue arrives in USD and EUR). Accounting follows UK rules: VAT, Making Tax Digital, FRS 102/105 | Finance phase |
| A11 | Fixture data models a studio of about 300 people, 12 teams and 6 projects, so wireframes are judged at target scale | Phase 0 fixtures |
| A12 | CRM (players, B2B contacts, sales pipeline) sits in the Customers area. Revenue ingestion and royalties sit in Finance | Area boundaries |
| A13 | **Changed.** Full parity on phones: every page is fully usable and editable, including whiteboards, the ledger and the timeline. Layout adapts, capability does not | Templates, mobile shell, risk R17 |
| A14 | **Changed.** A documented public API covering all modules, with personal and service tokens. This reverses the earlier "no public API" answer. OAuth apps and an extension model remain out of scope | Section 7.9, phase 1 |

---

### 4. Slices

Each slice names its pages, the components it adds to `rimelight_ui`, its data (fixtures from `rimelight_fixtures`, or the mock data already in the shell), and its gate. Route IDs come from `rimelight_nav`;

**Status key:** `done`, `in progress`, `next`, `planned`.

| ID | Slice | Status | What it delivers | Gate |
|---|---|---|---|---|
| S0 | Shell baseline | done | Rail, sidebar tree, tabs, split and tile groups, status bar, quick action, palette, phone shell (group SF, milestone gate passed 2026-11-12) | `gate quick`, shell tests |
| SA | Sign-in (Auth0) | in progress | SA.2b (2026-11-15): HTTP routes in `service.rs`, `pending.rs`, Postgres store in `pg.rs`, native dev server `bin/dev_server.rs`, `.dev.vars.example`. Worker entry not built, see decision 0015 (proposed). Gate: `cargo test -p rimelight_backend_api` 75 passed, Postgres test skipped (no DATABASE_URL), clippy -D warnings clean. Approved by the owner 2026-11-15, replaces S6. Backend-brokered Auth0 sign-in through the shared `rimelight.com` application (decision 0014): `/v1/auth/login`, `/v1/auth/callback`, `/v1/auth/exchange`, `/v1/me`, `/v1/auth/signout`; employees only (`https://rimelight.com/user_type == employee`), users pre-provisioned by an admin (first user seeded), 30-day sliding session in Postgres, MFA required for employees in the Auth0 login Action. Client: system browser, loopback on desktop, custom scheme on mobile, redirect on web; session in the OS credential store. UI: sign-in screen, real user in the avatar menu, sign out, connection state in the status bar | Token and session tests; `gate quick`; owner runs it against Auth0. SA.5 (2026-11-15): failed sign-ins redirect to the app with `error=` codes (decision 0016 deviation closed); `dev_server` finds `.dev.vars` from the crate directory too; shell shortcuts off while signed out. Gate: `cargo test -p rimelight_backend_api` 79 passed, `cargo test -p rimelight_client` 28 passed, `cargo test -p rimelight_ui --test ui sign_in` 6 passed. SA.6 (2026-11-15): web sign-in in code. `rimelight_client` on wasm32: fetch transport (gloo-net), localStorage session store (`WebSessionStore`), getrandom `wasm_js` (`.cargo/config.toml`), same-tab redirect in `web_signin.rs` (verifier and state in sessionStorage, query stripped after the return). The session driver runs on the web too (`spawn_task`); `apps/web` wires it, base URL `RIMELIGHT_API_URL` or `http://localhost:8787`. Backend: `WEB_APP_ORIGINS` (empty default; example `http://localhost:8080`) is a return address and CORS origin for exchange, me, signout and health. Gate: `cargo test -p rimelight_backend_api` 87 passed, `cargo test -p rimelight_client` 34 passed, `cargo test -p rimelight_ui --lib session_driver` 3 passed, `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean, `cargo build -p rimelight_desktop` clean, clippy `-D warnings` clean (native changed crates; wasm clippy for client and web). Not run in a browser or against Auth0 (owner). Decision 0017 (proposed). SA.7 (2026-11-15): sign-in on Android in code. `rimelight_platform::android` (JNI, the only unsafe, decision 0008 (a)) opens the browser with ACTION_VIEW and reads the activity's intent; `rimelight_client::signin_mobile` keeps the pending verifier and `state` in `signin-pending.json` and checks the return `rimelight-space://auth`; `FileSessionStore` keeps the session in `session.json` (mode 0600). `session_driver::attach_mobile` and `Session::sign_in_returned` are wired in `apps/android`. Manifest intent filter for the scheme and host, and `RimelightActivity.java` keeps the newest intent. iOS keeps the stub. Gate: `cargo test -p rimelight_client` 41 passed, `cargo test -p rimelight_ui --lib session_driver` 3 passed, `cargo ndk -t arm64-v8a check -p rimelight_android` clean, `cargo ndk -t arm64-v8a clippy -p rimelight_platform -p rimelight_android -- -D warnings` clean with `missing_const_for_thread_local` allowed (pre-existing in `chat_sidebar.rs` and `projects.rs`), `cargo build -p rimelight_desktop` clean, `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean, host clippy `-D warnings` clean for the changed crates, fmt clean. Not run on a device or emulator (owner). Decision 0018 (proposed). SA.8 (2026-11-15): online only, no offline mode (owner decision 0035). `AuthState::Offline` replaced by `Unreachable` (stored session kept). Start-up shows "Can't reach Rimelight Space" with Retry, retried every 10 s. Signed in: connection lost shows "Connection lost. Reconnecting…", check every 5 s, and the `Connection` global disables every Button with `requires-online` (all 81 page and quick-action buttons). Open pages reload when the connection returns. Gate: `cargo test -p rimelight_client` 76 passed, `SLINT_EMIT_DEBUG_INFO=1 cargo test -p rimelight_ui --test ui` 177 passed (4 new SA.8 tests: start-up unreachable and retry, disconnect and reconnect), `cargo test -p rimelight_ui --lib` 257 passed, `cargo build -p rimelight_desktop` clean, `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean, fmt clean. Not run against a live server or in a window (owner). SA.9 (2026-11-15): a full-window overlay (scrim, pointer and key capture, spinner, "Connection lost", "Reconnecting…") blocks the app while the connection is lost; SA.8's per-button gating, the Connection global and the connection banner removed. Gate: `cargo test -p rimelight_client` 76 passed; `SLINT_EMIT_DEBUG_INFO=1 cargo test -p rimelight_ui --test ui` 172 passed (banner geometry and per-button tests removed); `cargo test -p rimelight_ui --lib` 257 passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean. Keyboard blocking coded, not tested. Not run in a window. |
| SP | People > Profiles, directory, org chart, user admin | in progress | SP.1 (2026-11-15): migration `backend/db/migrations/0003_people.sql` (profile and org columns on `users`, `departments`, `teams`, `users.id` default) and fixture seed `backend/db/seed/0004_fixture_people.sql` (4 departments, 12 teams, 300 people in one tree under the CEO `rimelight@rimelight.com`, `is_admin`; `test@rimelight.com` under the CEO). Seed is guarded (runs while `users` has at most 2 rows). Applied to the local `rimelight_space` database. Decision 0019 (accepted by the owner). SP.2 (2026-11-15, owner addendum included): `rimelight_proto::people` contracts; `people_store.rs` (PeopleStore trait) with Postgres in `pg_people.rs` and a memory store for tests; `people_service.rs` routes for the directory, profiles, org chart, self-edit, avatar upload and serve, admin user create and edit with cycle checks, departments (tree and CRUD), teams (detail and CRUD), positions CRUD, roles (`admin`, `hr`); `blob.rs` (BlobStore with disk and R2 via SigV4 presigned URLs); migration `0005_org_roles.sql` and additive seed `0005_org_roles_seed.sql`; dev server applies 0003 and 0005 by column check. Gate: `DATABASE_URL=... cargo test -p rimelight_backend_api -p rimelight_proto -p rimelight_backend_db` 110 library tests passed (rimelight_proto 2 and rimelight_backend_db 8 more), Postgres tests ran (not skipped), clippy `-D warnings` and fmt clean. Not run: dev server, R2 bucket, owner review. Decision 0020 (proposed). Tests for the migration and seed text; tree query (no cycles, every user but the CEO has a manager); owner review of the People pages. SP.3a part 1 (2026-11-15, in progress, UI not started): CORS on all people routes, admin self-deactivation refused (409), `rimelight_client::people` typed reads and self-edit, PATCH and PUT transport. Gate: `cargo test -p rimelight_backend_api --lib` 113 passed, `cargo test -p rimelight_client` 49 passed, clippy `-D warnings` and fmt clean, wasm check and desktop build clean. SP.3b (2026-11-15, in progress, owner review pending): Company > Employees on the real API (`/company/employees`). Page data pattern in `src/slint_ui/page_data.rs` (loader, `PageState` component for loading, empty and error with Try again, picture cache). Search debounced 300 ms, paging by cursor, avatars fetched and cached (`GET /v1/avatars/{key}`, PNG, JPEG, WebP), row opens the profile route. Gate: `cargo test -p rimelight_client` 51 passed, `cargo test -p rimelight_ui --test ui employees` 3 passed, `cargo test -p rimelight_ui --lib page_data` 2 passed, `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean, fmt clean. Not run in a window or against the server (owner). Profile, Teams and Departments pages still to do. Decision 0022 (proposed). SP.3c (2026-11-15, in progress, owner review pending): Profile at `/company/employees/{id}` and the avatar menu's Profile (own profile). Header, Work, Personal (only when returned), About, Organization (read-only org fields, reporting line, direct reports, all links). Own profile: Edit (self fields, PATCH, 422 shown in the form), Change photo via `rfd` (native, png/jpeg/webp, 5 MB limit; hidden on web and Android). Gate: `cargo test -p rimelight_ui --test ui profile` 5 passed, `cargo test -p rimelight_ui --lib` 203 passed, `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean, fmt clean. Not run in a window or against the server (owner). Decision 0023 (proposed). SP.3d (2026-11-15, in progress, owner review pending): Company > Teams (`/company/teams`, List and Tree views; Tree is the org chart from the new `GET /v1/org/tree`, nodes expand and collapse, first two levels open, Find person opens the path and highlights the match), Team detail (`/company/teams/{id}`), Company > Departments (`/company/departments`, tree with heads and member counts, selecting shows the head and teams). Page data pattern throughout; pure tree logic in `src/pages/company/org_tree.rs`. Gate: `cargo test -p rimelight_ui --test ui teams` 4 passed, `team` 8 passed, `departments` 3 passed, `profile` 8 passed, `employees` 3 passed; `cargo test -p rimelight_ui --lib` 210 passed; `cargo test -p rimelight_client` 51 passed; `DATABASE_URL=... cargo test -p rimelight_backend_api` 114 library tests passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean; fmt clean. Not run in a window or against the server (owner). Decision 0024 (proposed). SP.3e (2026-11-15, in progress, owner review pending): Admin > Users (`/admin/users`), Admin > Organisation (`/admin/organisation`, Departments, Teams and Positions tabs) and Admin > Roles (`/admin/roles`, admin and HR). Admin gate from the caller's roles in `GET /v1/me/profile`; non-admins see "You need the admin role". Users: searchable list including deactivated people (`GET /v1/admin/users`), add form (pre-provisioning), detail panel for position, department, team, manager (person picker), start date, deactivate or reactivate, grant or revoke admin and HR. Organisation: create, rename or edit, set parent or head, delete; server 409 messages shown in the form. Roles: add and remove members, the last admin's 409 shown. Pages keep state in Slint globals (`AdminUsersState`, `AdminOrgState`, `AdminRolesState`). Company pages that hold a result ask again after an admin change. Gate: `cargo test -p rimelight_ui --test ui` 132 passed (admin 5); `cargo test -p rimelight_ui --lib` 217 passed; `cargo test -p rimelight_client` 56 passed; `DATABASE_URL=... cargo test -p rimelight_backend_api` 115 library tests passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean; `cargo fmt --all -- --check` clean. Not run in a window, in a browser, or against the server or Auth0 (owner). Decision 0025 (proposed) |
| SP.5 | Positions as seats (position management) | in progress, owner review pending | SP.5 (2026-11-15): seats backend (store, Postgres, routes: `/v1/org/seats`, `/v1/admin/seats`, `/v1/admin/team-budgets`, `PUT /v1/admin/teams/{id}/budget`, cycle check, one seat per person, vacate and close rules, budget used and remaining in the budget currency). Gate: `cargo test -p rimelight_backend_api --lib` 186 passed (Postgres seat test ran), `cargo test -p rimelight_client --lib` 77 passed, `cargo test -p rimelight_ui --lib organisation` passed, `cargo test -p rimelight_ui --test ui` teams 4 passed, admin 7 passed, `cargo fmt --all -- --check` clean. Company > Teams org chart draws from seats (open and planned as placeholders). Admin > Organisation has Seats and Budgets tabs. Not run in a window or against a live server (owner). Previous SP.5a schema note follows:  Migration `backend/db/migrations/0008_seats.sql` (`seats` table with state, person, reports-to seat and salary checks; one seat per person; `teams.budget_cap` and `budget_currency`) and fixture seed `backend/db/seed/0007_fixture_seats.sql` (a filled seat per person, reporting to the manager's seat; guarded on an empty table). Not applied, not tested, no API, client or UI yet. Decision 0036 applies. Choices made here, not yet logged: the CEO's seat has no team (`seats.team_id` is nullable), and the existing `positions` table stays the job profile table. Still to do: store and Postgres implementation, cycle check on reports-to, budget used and remaining, dev server migration, routes, client, Admin > Organisation seats UI, org chart placeholders, gates. |
Owner asked 2026-11-15. A position is one seat in one team: job profile (today's `positions` rows, renamed job profiles), title, level, reports-to seat, at most one person, state Planned, Open, Filled or Closed. The org chart is built from seats; a person's manager is whoever fills the parent seat; open and planned seats show as placeholder cards in their place. Budget: min and max annual salary and currency per seat, a yearly budget cap per team with used and remaining (filled plus open seats), all money visible to admin and HR only. Admin > Organisation manages seats (add open seat, assign or vacate a person, move, close). Existing employees each get a filled seat in their team, with reports-to taken from their manager's seat. Decision 0036 (accepted, owner). |

| S1 | Projects > Issues on the real backend | in progress | Projects with members (admin, member, viewer; members only), per-project workflow states (default Open, In Progress, In Review, Done, Won't Do), issue list, board, detail, backlog, comments and activity log, sprints, subtasks and links, YouTrack-style search query. S1.1 (2026-11-15): migration `backend/db/migrations/0006_projects.sql` (projects, project_members, workflow_states, sprints, issues, issue_links, comments, activity; composite foreign keys keep states, sprints, parents and assignees inside their project) and fixture seed `backend/db/seed/0006_fixture_projects.sql` (6 projects EMB, HLD, NXS, LIVE, KLN, OPS; 118 memberships; 3 sprints each; 340 issues; 63 links; 59 comments; 976 activity rows; guarded on `projects` being empty). Both applied to the local `rimelight_space` database. `rimelight_policy`: `can_view_project`, `can_edit_issues`, `can_manage_project` with the global admin override. Gate: `cargo test -p rimelight_policy` 6 passed; `cargo test -p rimelight_backend_db` 5 passed, the seed idempotency test ran against local Postgres in a scratch schema (not skipped); `cargo fmt --all -- --check` clean; clippy `-D warnings` via the pre-commit hook. Decision 0026 (proposed). S1.2a (2026-11-15): projects API on the Postgres store and a memory store. `rimelight_proto::projects` (wire types), `backend/api/src/project_store.rs` (trait and memory store), `pg_projects.rs` (Postgres, one transaction per multi-table write), `project_service.rs` (routes and rules, `/v1/projects` and `/v1/issues`), `backlog_rank.rs` (midpoint ranks). The dev server applies 0006 with `ensure_table` and runs no seed. Gate: `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto -p rimelight_policy`: 149 passed (API), 13 (proto), 6 (policy). The Postgres round trip ran in a scratch schema that the test drops. `cargo fmt --all -- --check` clean; clippy `-D warnings` clean for the two changed crates. Decision 0027 (proposed). Not run: a live server, UI or owner review. S1.2b (2026-11-15): YouTrack-style issue search. `rimelight_proto::query` (lexer, parser, AST with spans, `ParseError`, pure so the client can reuse it), `backend/api/src/search.rs` (names resolved against the caller's scope into a predicate tree; memory evaluator and comparator), `pg_projects.rs` (parameterised SQL, values only bound, field and sort lists fixed), routes `GET /v1/issues/search?q=&cursor=&limit=` and `q` on `GET /v1/projects/{key}/issues`. Decision 0028 (proposed). Gate: `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto`: API 173 passed (including the Postgres search scenario in a scratch schema, not skipped), proto 37 passed. `cargo fmt --all -- --check` clean; clippy `-D warnings` clean for both crates. Not run: a live server, the wasm32 Worker build, UI, owner review. S1.3 (2026-11-15, part 1 of 2, in progress): the typed client `rimelight_client::projects` (projects, members, workflow, sprints, issue list with filters, sort, `q` and cursor, search, issue get, create, patch and delete, comments CRUD, links add and remove, activity) with 11 fake-transport tests; a 400 `invalid_query` now maps to `ClientError::InvalidQuery { message, start, end }` so the UI can underline the span; `UpdateIssue` no longer writes absent fields as `null` (decision 0029). Gate: `cargo test -p rimelight_client -p rimelight_proto`: client 67 passed, proto 37 passed; `cargo clippy -p rimelight_client -p rimelight_proto --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean; `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean. Not done: the Slint UI (projects picker, issues list, issue detail, new-issue form, all of brief items 2 to 5); see decision 0029. S1.3 part 2 (2026-11-15, in progress, owner away): the projects picker now lists `GET /v1/projects` (the mock list is gone), and the issues list at `/p/:key/issues` has a query bar on `q` with the refused span marked, chips that toggle their clause, sortable headers that write `sort by:`, Load more, New issue for a member or admin (create, then open the issue), and a row that opens `/p/:key/issues/:id`. Decision 0030 (proposed). Gate: `cargo test -p rimelight_ui --test ui`: 139 passed (7 new in `issues`); `cargo test -p rimelight_ui --lib`: 227 passed; `cargo build -p rimelight_desktop` clean; `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean; rustfmt clean. Not done: the issue detail page (`/p/:key/issues/:id` is still a wireframe), board, backlog, subtasks, links, comments, activity. Not run: a window, screenshots, a phone width, a live server, owner review. Not run: a live server, the desktop build, the UI test binary, owner review. S1.3c (2026-11-15, in progress, owner review pending): issue detail at `/p/:key/issues/:id` (`pages/projects/issue.rs`, `ui/pages/projects/issue.slint`). Header fields save one PATCH each (type, state, priority, assignee, sprint, estimate, due date, tags); summary and description editors with Save and Cancel; parent link, subtasks (open, add under this issue); links grouped by label (add, remove); Comments and Activity tabs (add; edit and delete own, a project admin any); Delete for a project admin after a confirm step. Viewers and non-members read-only. Decision 0031 (proposed). Gate: `cargo test -p rimelight_ui --test ui issue` 17 passed (10 new in `issue`, 7 in `issues`); `cargo test -p rimelight_ui --test ui` 148 passed; `cargo test -p rimelight_ui --lib` 234 passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean; `cargo clippy -p rimelight_ui --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean. Not run in a window, at phone width, or against a live server (owner). Board, backlog and the sprint page not in this step. Tests for filter, sort, move card, edit field; `gate quick`; owner review of desktop and phone. S1.4 (2026-11-15, in progress, owner review pending): Board at `/p/:key/board` (`pages/projects/board.rs`, `ui/pages/projects/board.slint`): one column per workflow state with its count and points, a sprint selector (the active sprint by default, or All open, which sends nothing), the query bar sent as `q`, and a "Move to" select on each card (one PATCH of the state; no drag and drop). Backlog at `/p/:key/backlog` (`backlog.rs`, `backlog.slint`): planned and active sprints as collapsible groups above the backlog, with dates and totals; a row moves up or down (POST rank with `after` and `before`), and its sprint select sends one PATCH (`none` clears it). Sprints at `/p/:key/sprints` (`sprints.rs`, `sprints.slint`): create, start (a second active sprint shows the server's 409 message), and close through an inline confirm step that picks the target (next planned sprint or backlog). Shared helpers and the request state are in `pages/projects/agile.rs`. Client: `backlog`, `rank_backlog`, `create_sprint`, `update_sprint`; `UpdateSprint` no longer writes absent fields as `null`. Decision 0032 (proposed). Gate: `cargo test -p rimelight_client -p rimelight_proto`: client 70 passed, proto 38 passed; `cargo test -p rimelight_ui --test ui`: 161 passed (13 new: `board` 4, `backlog` 4, `sprints` 5); `cargo test -p rimelight_ui --lib`: 250 passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean; `cargo clippy -p rimelight_proto -p rimelight_client -p rimelight_ui --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean. Not run in a window, at phone width, or against a live server (owner). The board's columns clip, rather than scroll sideways, when the window is narrower than their minimum width (decision 0032). |
| W1 | Worker runtime for the backend (decision 0033) | superseded (2026-11-16, decision 0041) | The Rust Worker and native backend were removed; the API now runs on the website Worker. Earlier history: owner said "lets do it" 2026-11-15. Spike (2026-11-15): `tokio-postgres` 0.7 with `default-features = false` and `getrandom` 0.4 with `wasm_js` compiles for wasm32-unknown-unknown in a scratch crate (`cargo check` passed; no run-time test). Built (2026-11-15): `backend/worker` crate (`rimelight_backend_worker`, workers-rs 0.7, `#[event(fetch)]`) with `wrangler.toml` (Hyperdrive binding `DB`, R2 binding `AVATARS` to `rimelight-dot-com`, build with `worker-build` 0.7.5). Hyperdrive adapter: `tokio-postgres` `connect_raw` over `worker::Socket`, one connection per service per request. `PgStore::from_client` added; pg modules no longer native-gated. `HttpFetch` over `worker::Fetch`; R2 avatar store with prefix `space/avatars/`. Gates: `cargo check -p rimelight_backend_worker --target wasm32-unknown-unknown` passed; `cargo test -p rimelight_backend_api` 186 passed (Postgres tests on the local database); `cargo clippy -p rimelight_backend_api -p rimelight_backend_worker --all-targets -D warnings` clean; `cargo fmt` applied. Run locally with `wrangler dev` on port 8790 against the local Postgres through the `CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_DB` variable: `GET /v1/health` 200; an unauthenticated `POST /v1/projects` 401; with a test session row, `GET /v1/me` 200, `POST /v1/projects` created a project, `GET /v1/projects` listed it, and the test rows were removed. Not done: deploy to Cloudflare, the real Hyperdrive config id (placeholder in `wrangler.toml`), the R2 avatar path on the Worker, the Auth0 sign-in round trip, removing `dev_server` (decision 0033, after parity), `cargo xtask dev --remote`, CI. Decision 0015 accepted for the Worker entry; decision 0033 governs the rest |
| S1.5 | Issue page and new-issue dialog like YouTrack | paused (decision 0049) | Owner asked 2026-11-15 with reference screenshots of a YouTrack issue page and its New issue dialog. S1.5a backend: votes, stars, spent time entries, issue attachments in the blob store (Disk or R2), created by and updated by, similar issues by summary. S1.5b UI: issue page with breadcrumb, previous and next (N of M from the list query), title row (star, edit, tags, link, more), Markdown description, Attachments strip, Comments, Spent time and History tabs, right-hand field panel (Project, Type, State, Priority, Assignee, Estimation, Spent time, Due date, Area from tags, Sprint). S1.5c UI: New issue as a dialog with summary, Markdown editor, attachments, similar issues, the same field panel, Create, Cancel and View in full page; New issue is also an entry in the quick action button (owner asked 2026-11-15). Out of scope: per-issue visibility groups (no permission model for them). S1.5a (2026-11-15): backend and client for the YouTrack-style issue page. Migration `backend/db/migrations/0007_issue_extras.sql` (`issue_votes`, `issue_stars`, `issue_work_items`, `issue_attachments`, `issues.updated_by`), applied by the dev server with `ensure_table` on `issue_votes`. `rimelight_proto::projects`: `Issue` gains `updated_by`, `votes`, `voted_by_me`, `starred_by_me`, `spent_minutes`, `attachments` (serde defaults); new `Attachment`, `WorkItem`, `CreateWorkItem`, `VoteState`, `StarState`. Routes in `project_service.rs` address issues by key, as the other issue routes do (the brief's `{id}` is the key here): `PUT`/`DELETE` vote and star (view access), `GET`/`POST` work-items and `DELETE` by id (edit access), `GET`/`POST` attachments and `GET`/`DELETE` by id (edit access to write; 10 MB limit; bytes in the blob store through a fourth generic on `ProjectService`), `GET /v1/projects/{key}/issues/similar?summary=` (top 5 by word overlap). Each real vote, star, time entry and attachment change writes a `field_changed` activity row (fields `vote`, `star`, `work_item`, `attachment`); `PATCH` sets `updated_by`. Attachment blob keys are `attachment-{issue}-{nonce}.bin`; the R2 store still prefixes them `space/avatars/`, which the owner should review. Client: `set_vote`, `set_star`, `work_items`, `add_work_item`, `delete_work_item`, `attachments`, `upload_attachment`, `attachment_bytes`, `delete_attachment`, `similar_issues`, with `RawBody.file_name` sent as `X-File-Name`. Gate: `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto -p rimelight_client`: API 178 passed (5 new memory service tests; the Postgres round trip covers the new SQL in a scratch schema through a new `extras_scenario` that runs inside `projects_round_trip_against_postgres`, not skipped), proto 38 passed, client 76 passed (6 new). `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean. `cargo clippy -p rimelight_backend_db -p rimelight_backend_api -p rimelight_proto -p rimelight_client -p rimelight_ui --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean. Not run: a live server, the UI test binary, owner review. Not done: the issue page UI (S1.5b) and the New issue dialog (S1.5c). S1.5b (2026-11-15): YouTrack-style issue page (breadcrumb with copy key and previous and next from the list, created and updated line, vote and star, title row actions, attachments as file tiles, plain-text description, Comments, Spent time and History tabs, right-hand field panel that moves below under 760px). Gate: `cargo test -p rimelight_ui --test ui` 164 passed; `cargo test -p rimelight_ui --lib` 254 passed (includes SP.4); `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean. Not done: image thumbnails, opening an attachment, Markdown rendering, "M" counts loaded rows only, delete gated on member not uploader. Not run in a window. UI.F1 (2026-11-15): owner-reported fixes. Root cause: the group body was a conditional (`if`) child of the group layout, which had `alignment: start`; that stopped the page's `vertical-stretch`, so each page got only its content height (104 px on a 1200 by 800 window, group 669 px). Fix: a stretched `page-slot` holds the page, each page root sizes its layout to its parent, and the route and meta lines, the template label (`Template::id`, `page-meta`, `group.meta`) are removed. The account menu shows the connection chip (dot and word, as the status bar). Gate: `SLINT_EMIT_DEBUG_INFO=1 cargo test -p rimelight_ui --test ui` 166 passed, 0 failed (new: `departments_page_body_fills_the_group`, body 529 px at 1200 by 800, and `connection_chip_shows_in_the_account_menu`); `cargo test -p rimelight_ui --lib` 254 passed; `cargo test -p rimelight_nav` 32 passed; `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean. The two `ElementHandle` tests need `SLINT_EMIT_DEBUG_INFO=1` (decision 0034, proposed). Pre-commit fmt and clippy: see the commit. Not run in a window (owner). S1.5c (2026-11-15): New issue dialog (`ui/pages/projects/new-issue.slint`, `src/pages/projects/new_issue.rs`, tests `tests/ui/new_issue.rs`) with summary, description, attachments, debounced similar issues (400 ms), the field panel, Create, Cancel and View in full page; the issues page button opens it and the inline form is removed; quick action entry "New issue" (Projects category in `rimelight_nav::quick`), disabled with a hint when no project allows creating. Simplified: "View in full page" closes the dialog and opens the project issue list, keeping the draft (no separate full-page form); Markdown editor and tags as a plain comma field. Gate: `SLINT_EMIT_DEBUG_INFO=1 cargo test -p rimelight_ui --test ui` 174 passed, 0 failed; `cargo test -p rimelight_ui --lib` 257 passed; `cargo test -p rimelight_nav` 32 passed; `cargo build -p rimelight_desktop` clean; `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean after ungating `attachment_type`; `cargo fmt --all` run. Not run in a window (owner). |
| BT | Build time | in progress | Owner asked 2026-11-15 ("Do 1–4"). Baseline: a .slint edit cost 9m16s in the hook clippy (43 MB generated Slint Rust checked in 276s, the `ui` test binary with 33 `slint!` macros in 146s, slint-build 35s, deps re-checked; 9 copies of `rimelight_ui_slint` in the build dir). BT.1: one debug setting for every build (hook env overrides removed). BT.2: `[profile.dev.build-override] opt-level = 3`. BT.3 (gallery windows in their own crate) and BT.4 (test markup out of the test binary): see decision 0037 Result 2026-11-15 after BT.1+BT.2: the same edit takes 2m06s (`rimelight_ui_slint` check 67s, `ui` test check 44s, slint-build 7.5s, no deps re-checked). One-time rebuild after the change: 18m53s BT.4: a test .rs edit takes 8s in the hook clippy, a .slint edit 72s. `cargo test -p rimelight_ui --test ui`: 168 passed, 4 failed (connection_lost x3, departments page body: ElementHandle needs Slint debug info, decision 0034, not caused by BT.4). BT.3 deferred. BT.5 (2026-11-18, decision 0043): hook, gate and agent test commands all select `--workspace`. Cold run of the command sequence 906s before, 735s after. Check run: `cargo clippy --workspace --all-targets` 56s, then `cargo test --workspace --test ui --no-run` 2s, then `cargo xtask gate test` exit 0 in 77s, with no dependency recompiled between them. BT.6 (2026-11-18, owner approved): `rust-lld.exe` linker in `.cargo/config.toml`, `[profile.dev.package."*"] debug = false`. Incremental `cargo test --workspace --test ui --no-run`: .rs edit 25s to 14s, .slint edit 153s to 102s. Hook clippy after a .slint edit 31-34s in 3 re-runs (baseline 36s; one 70s outlier not reproduced). One-time rebuild after the change: 489s test, 257s clippy. `ui`: 164 passed. BT.7 (2026-11-18, owner: "go"; decision 0044, proposed): gallery and foundations out of the compiled Slint crate, gallery on slint-interpreter. Done 2026-11-18 (decision 0044): generated Slint Rust 44.8 MB to 25.8 MB. A story or gallery .slint edit rebuilds nothing (0 s test build, 1 s hook clippy). An app .slint edit: `ui` test build 24-28 s (was 37-42 s on the same machine state), hook clippy 19-28 s (was 31-36 s). `ui`: 163 passed (the gallery test moved to apps/gallery), gallery bin tests 9 passed, lib tests all pass. Headless captures checked: a story, the foundations sheet, a whole-shell state | Timed hook clippy after touching `ui/main.slint` |
| S2 | Home > My Work | planned | Dashboard of assigned issues, today's calendar, unread count, timer | Tests for each card's empty and loaded state; owner review |
| S3 | Calendar | planned | Month and week views, day picker (the date components in progress now feed this), overlays for projects and leave | Tests for locale week start and month names; owner review |
| S4 | Chat | planned | Channel, chat and room lists, a thread view, unread badges (mock data) | Tests for unread counts and ordering; owner review |
| S5 | Projects > Overview, sprints, timeline | planned | Project overview, sprint list, timeline view | Tests; owner review |
| UI1 | Shell: profile menu, Give Feedback, Home > Performance, offline cleanup | in progress, owner review pending | Profile menu: presence mark on the avatar, no connection line. About, then Give Feedback (placeholder shell screen), Report Issue, Settings, Sign out. Profile page: Organization merged into Work; order Work, Personal, About. Home: Performance section (Metrics and Feedbacks placeholders) above Time & Pay. Home > Personal: Boards (`/home/boards`, personal whiteboards, Canvas template, placeholder) under Notes. Offline: no connection word in the status bar; the banner and its icons are removed; offline shows only the full-window overlay (decision 0040). | Gate: `cargo test -p rimelight_nav` 32 passed; `cargo test -p rimelight_ui --test ui` 171 passed; fmt and clippy via the pre-commit hook. Not yet run in the app by the owner. |
| CV | Canvases (personal and project whiteboards; clean-room PureRef and Excalidraw) | paused (decision 0049) | Owner asked and approved 2026-11-19; answers in decision 0045. CV.0 done 2026-11-19 (spikes passed, owner ran the window spike; results in decision 0045). CV.1 (2026-11-19): migration `backend/db/migrations/0013_canvases.sql`, the same file as website `drizzle/20261119120000_space_canvases/migration.sql` (canvases, canvas_shares, canvas_items, canvas_assets, storage_quotas); not applied, not syntax-checked (no local Postgres; checked when the owner says "apply it"). Routes renamed: `/home/canvases`, `/home/canvases/:id`, `/p/:key/canvases`, `/p/:key/canvases/:id`; canvases removed from the public link pages (A7). Gate: clippy `-D warnings` clean; `cargo test --workspace --lib` all passed (nav 32); `--test ui` 160 passed, 3 failed (tooltip, pointer_drag pinned tab tooltip, strip_keyboard), which fail the same on HEAD `9c4ac5d` without this change; fixed 2026-11-19 in `3f06530` (tooltips draw in a window overlay, `TooltipLayer` and `TooltipOverlay`, not a popup, so they never take focus), `--test ui` 163 passed. CV.2 (2026-11-19, website `3e5190e9`, `68e1f10a`): REST API in `src/space/canvases.ts`, `canvas-assets.ts`, `canvas-access.ts`, `canvas-image.ts`. Gate: `vp test` 43 passed (`space-canvas-image` 26, `space-canvas-access` 17), `vp check` 0 errors, `tsc --noEmit` no errors in `src/space`. Not run against the database (migration not applied) or with a live request. Choices in decision 0045. Personal canvases in Home (shareable with people as viewer or editor), project canvases in Projects (project roles), a personal canvas moves to a project. Images first (full PureRef organising set), text notes and labels, shapes and arrows, pen, frames and groups, crop, grayscale, GIF and WebP playback, eyedropper, paste from URL. Real-time co-editing from the start (Durable Object per canvas, WebSocket, item ops ordered by the server, live cursors and selections). Desktop pop-out window (frameless or with toolbar bar, pin on top, opacity, click-through with a global hotkey). Assets in R2 under `space/canvases/{canvas_id}/`, 50 MB per file, dedupe per canvas, quotas 1 GB per user and 10 GB per project. Import folder, export PNG, save images to folder, canvas backup file. Steps CV.0 to CV.12 in decision 0045. Video is a later slice (CV-V). Desktop first; web and Android compile and open canvases, touch and phone layout later. CV.7 to CV.13 and the owner's 12-point review (2026-11-19, one batch, Haiku subagents wrote the code, one build at the end). Owner fixes: New canvas at the top right of the canvas list, the toolbar Open/Rename/Archive/Delete buttons removed (row menu, right-click and double-click remain; the double-click no longer loses its row when the list refreshes), Copy copies the original file (not the thumbnail), menu rows react to clicks and nested submenus stack with the deepest on top, arrow-key nudges no longer snap back (local ops stay pending until the server acknowledges them; a reject restores the server's item), Help, Settings, New canvas and Close removed from the right-click menu. CV.7: file picker, OS drop, paste (bitmap, files, URL), folder import, WebP thumbnails at 256 and 1024 px chosen by on-screen size with hysteresis, a disk cache, GIF and WebP animation, non-destructive crop mode, greyscale per item and for the whole canvas (a session view setting), opacity menu, eyedropper. CV.8: the organising commands in the menu. CV.9: rectangle, ellipse, diamond, line, arrow, pen, eraser (removes whole strokes) and frame tools; arrow bindings deferred. CV.10: live cursors, selection outlines, avatars, follow. CV.11: share dialog, move to project, quota bars and an admin Set limit; website `7ab31b78` adds the asset list with use counts and delete of unused assets (`GET` and `DELETE /canvases/:id/assets`). CV.12: pop-out window with its own bar, pin, on bottom, on top of a chosen window, opacity, click-through, overlay selection, lock, resize to selection; global hotkey Ctrl+Alt+Shift+M turns click-through off (changed from decision 0045's Ctrl+Alt+T, which the shell uses for workspaces; fixed in code, the owner wants no setting for it). Grid (session view setting) with snapping. CV.13: export PNG (selection or canvas, drawn on the CPU), save original images to a folder, backup zip and restore. Gate: clippy `-D warnings` clean, `--lib` 605 passed, `--test ui` 174 passed, wasm32 `--lib` check passed, Android `--lib` check passed (needs `CC_aarch64_linux_android` pointing at the NDK 28.2 clang since aws-lc-sys rebuilds after `cargo xtask clean`), desktop build passed; website `vp check` passed. Not run in the app by the agent; owner review pending. Disk was full (target 140 GB); `cargo xtask clean` freed 92 GB. Owner review 2 (2026-11-19, 26 points): images showed black or as placeholders: a Slint `source-clip-width` of 0 draws nothing (now the picture's width when there is no crop), and each picture took about 16 s to decode and thumbnail because dev builds compiled the image crates at opt-level 0 (now opt-level 3 for them in `[profile.dev.package.*]`). Menu rows ignored clicks (and Copy as file and Hierarchy, reached through the menu, did nothing): every refresh replaced the menu models, which rebuilt the rows between press and release; the page now updates its models in place (`sync_model`). Toggle items no longer reserve a blank check slot; the check shows after the label when on. Toolbar moved to the bottom centre, with a Pop out button (main window); Pop out is Ctrl+Shift+P (Ctrl+Alt+Shift+N clashed with the shell's quick action). Arrows and lines bind to the items under their ends (`rimelight_canvas::bind`) and follow them when they move, resize or rotate; moving the arrow alone unbinds it. Frames are drawn and hit below every other item (so their children can be clicked and dragged out), have a blue tint, are titled "Frame" when created, and a click on the title band selects the frame. Eraser shows a circle and fades what it would erase. Ghost presence: the room now drops connections it has not heard from for 80 s (website `canvas-room.ts`). Canvas list: New canvas at the right edge; the admin Set limit and the quota override routes were removed (limits fixed in code; the `storage_quotas` table was dropped by website migration `20261119130000_space_drop_storage_quotas`, mirrored as `backend/db/migrations/0014_drop_storage_quotas.sql`, applied 2026-11-19, 27 ledger rows). Gate: clippy `-D warnings` clean, `--lib` 625 passed; `--test ui`, wasm and Android checks not run (owner asked to skip the slow test builds and check in the app). Not run in the app by the agent. Owner review 3 (2026-11-19): storage line flush left, hierarchy close button at the right edge and closing through the controller (it no longer reopens), the shared ScrollArea thumb can be dragged, lines and arrows start at width 4 with larger heads. Gate: clippy only (owner asked to verify in the app instead of waiting on test builds). | Per step, see decision 0045; owner review of desktop light and dark |
| CV-V | Video on canvases | paused (decision 0049) | CV-V.0 desktop spikes passed 2026-11-20 (decision 0046); server spikes moved to CV-V.1. CV-V.1 (2026-11-20, website `2e6a25af`): migration `20261120120000_space_canvas_video` (mirrored as `backend/db/migrations/0015_canvas_video.sql`: asset `media`, `duration_ms`, `has_audio`; video types and the 500 MB limit in the checks), video sniffing, uploads in 64 MB parts (`POST /canvases/{id}/uploads`, `PUT .../parts/{n}`, `POST .../complete`, `DELETE`), asset downloads streamed with `Range` (they were read whole into memory, which a 300 MB video would not fit). Gate: `vp test` 75 passed (`space-canvas-image` with the video helpers), `vp check` 0 errors. Migration applied 2026-11-20 (28 ledger rows; the 359 existing assets read as Image). Live test against rimelight.com (temporary personal canvas and session for the owner, both removed; the canvas delete removed its R2 objects): a 245.6 MB reference video went up in 4 parts of 64 MB at 9.7 to 14.7 MB/s, completed as a Video asset; the same file again was deduplicated; Range reads of the first 1 KB, 64 KB in the middle and the last 1000 bytes matched the file, a range past the end gave 416; the full download streamed 245.6 MB in 21.2 s and its SHA-256 matched; a text file sent as MP4 was refused at completion with no row left; abort answered 204; over 500 MB was refused at start. No request hit the Workers Free limits (no 503). CV-V.2 (2026-11-20): `vendor/ffmpeg-sys-next` (crates.io 9.1.1 with its bindings and build output checked in for x86_64-pc-windows-msvc, patched in through `[patch.crates-io]`, excluded from the workspace so the bindings are not linted): no clang or FFmpeg headers needed to build. `cargo xtask ffmpeg` fetches the pinned BtbN `autobuild-2026-11-19-12-39` LGPL shared FFmpeg n9.0.2 (SHA-256 checked) into `<target>/ffmpeg/current`; its headers are identical to the ones the bindings came from. The DLLs are copied into the profile folder, which is reported as a native search path so `cargo test` and `cargo run` put it on PATH (test binaries live in per-unit folders under `build/`). New crate `rimelight_media` (Haiku agent wrote it): `probe`, `poster`, `Player` (video and audio threads per seek generation, cpal sound as the clock or a wall clock without sound, pre-roll before the clock starts, seek, volume, mute, loop, output size, end); FFmpeg and cpal on Windows only, a stub elsewhere. Fixture `tests/fixtures/clip.mp4` (58 KB, 2 s 160x90 MPEG-4 with AAC, made with the bundled ffmpeg). Gate: clippy `-D warnings` clean, `--lib` 625 passed before this step plus the 5 `rimelight_media` tests (probe, poster, playback in order, seek while playing, ended), wasm32 and Android `--lib` checks passed. Adding cpal changed the features of the shared `windows` crate, so Slint rebuilt once. Not yet checked: sound through a real device with the crate (the spike did), and the agent noted the audio clock starts at the seek target rather than the first audio packet time. CV-V.3 (2026-11-20, two Haiku agents): proto `AssetView` gains `media`, `duration_ms`, `has_audio` (older answers read as images) and the upload types (`VideoUpload`, `VideoUploadStart`, `UploadedPart`, `VideoUploadComplete`); client `start_video_upload`, `upload_video_part`, `complete_video_upload`, `abort_video_upload`; `rimelight_ui` `pages/canvas/video.rs` (desktop): `upload_video` probes the file with `rimelight_media`, hashes it in 1 MB chunks, uploads 64 MB parts read from disk (aborting on a failed part), completes, makes the 1024 and 256 px WebP thumbnails from a frame at 10 %, and copies the file into the disk cache as its original. Drops, the file picker (Images and videos, Images, Videos) and pasted file lists send videos there without reading them into memory; the busy chip shows the upload percentage. Videos are stored as canvas assets but not placed yet (CV-V.4). Gate: clippy `-D warnings` clean; new tests 12 passed (6 client, 3 proto, 3 upload: the fixture clip uploaded in parts with both thumbnails and the cache copy, an existing video sent once, the file types); wasm32 and Android `--lib` checks passed. Not run: the full `--lib` and `--test ui` suites (owner prefers checking in the app), an upload from the app to rimelight.com. CV-V.4 (2026-11-20, two Haiku agents): `ItemKind::Video` / `ItemData::Video` (props asset, name, added, volume, muted, loop); arrows bind to videos; controller `add_videos`, `Effect::VideoToggle` (double-click, Space with one video selected, Images > Video > Play or pause), Mute and Loop as shared item props; image-only commands skip videos; export draws the video thumbnail; save to folder includes video originals (cache first). Page: uploaded videos are placed; video items show their thumbnail (never the original) with a duration chip; a controls layer above the input layer has the play button and, while playing or selected, a bar with play/pause, scrub, time, loop and mute; up to 4 `rimelight_media` players (the oldest is dropped), the file comes from the disk cache or is downloaded once; frames go into the item every 8 ms. Website: `Video` in the room item kinds and migration `20261120130000_space_canvas_video_items` (mirrored as `0016_canvas_video_items.sql`) widening `canvas_items_kind_check`. Gate: clippy `-D warnings` clean; website `vp test` 75 passed. The new Rust tests (model, controller, export, duration text, one UI test) were written but not run: the owner stopped the test build because it slowed the PC. Migration `20261120130000_space_canvas_video_items` applied 2026-11-20 (29 ledger rows) and the website deployed. Fix `6c38819`: the live room WebSocket now installs the aws-lc-rs rustls provider itself, since a second backend (ring, from velopack's ureq in the REL work) made the connect panic and canvases stayed on loading. Owner checked in the app 2026-11-20: upload, placing, playback with sound, the controls, four players, editing, export and save to folder all work. CV-V.5 (2026-11-20, three Haiku agents): play for everyone. Room (website): `media` messages `{ item_id, playing, position_ms, ended }` from editors (viewers refused, the item must exist, the position rounded) are stamped with the server time `at` and the sender and broadcast to everyone; the current shared states live in memory and come in `hello`; `pong` carries the server time `now`. Proto: `MediaControl`, `MediaState`, `ServerMessage::Media`, `Pong { now }`, `Hello.media`. Client: `send_media`, `server_now_ms` (offset from the ping round trip, a ping right after connecting). Page: a Play for everyone button on the video bar (accent while following; a dot on the play button of a shared video); followers open the player, play or pause with it and seek when more than 300 ms off; their play, pause and seek are sent; Leave and rejoin; ended when pressed while paused. Gate: clippy `-D warnings` clean; website `vp test` 85 passed (media parsing and `applyMedia`); Rust tests written (proto round trips, the clock offset) but not run. Not yet: deployed, tried with two people. Video items beside images (thumbnail until played, sound, every image feature that applies), 500 MB per file, FFmpeg and cpal in a shared `rimelight_media` crate, desktop first. Plan and steps in decision 0046 | Per step, see decision 0046; owner plays reference videos on a canvas |
| REL | Windows release pipeline and auto-update (Velopack, R2) | paused (decision 0049) | Owner asked 2026-11-20; answers and steps in decision 0047 (proposed). REL.0 part 1 done 2026-11-20: update path proven with a spike; real desktop build not yet packaged. REL.1 to REL.6 below the decision. macOS, Linux, Android and iOS pipelines are recorded in decision 0047 as later slices. | Tag build produces Setup.exe and feed on R2; an installed old version updates itself on launch |
| CMS | Content: wiki, docs, blog and franchise content move from the rimelight.com CMS into Space | next (main track) | Owner asked and answered 2026-11-23; answers, architecture and steps CMS.0 to CMS.8 in decision 0049. Per-scope (project, team, department, company) content with a Slint block editor in `rimelight-rs`, CRDT co-editing and full phone parity; the server side becomes a private TypeScript package; rimelight.com renders published content at today's URLs. S1.5, CV, CV-V and REL are paused for it. Starts with the CMS.0 spikes (web view in Slint, CRDT library, rich text input). Owner review 1 (2026-11-24): the 109 pages under `franchises/grand-tale` moved to project GT (the other 133 stay Company). UI rework checked in gallery captures (`shell/desktop-content-home`, `-new-page`, `-page`): the page list is a table (title and address, type, status, visibility, updated, actions on hover) with collection filter chips; the New page dialog sizes to its content; the page has a toolbar like the web editor (format buttons, link, mention, undo, redo, comments, panel, Share, Discard, Publish), blocks drawn by kind with a + button and a drag handle (click opens the block menu), and a Page / Block / Comments side panel (address, language, visibility, location with Move, collection properties, change summary, delete). rimelight-rs v0.5.4 (v0.5.3 was pushed on the previous commit by mistake and is unused). Gate: clippy `-D warnings` clean and tests passed in both repos. Not run in the live app by the agent. | CMS.0 spike results reviewed by the owner before CMS.1; each later batch ends in one build and test pass and an owner review |

Slices after S6 (Company, People, Finance, Customers, Operations, Analytics, Admin, Mail, the public API, the backend) are not scheduled. Each one gets its own entry here when the owner picks it, following the same format.

**Deferred shared work** (built only when a slice needs it): the Slint lint for colour and font literals and `.slint` imports (was P0.18), the page templates (was P0.19: T-LIST, T-BOARD, T-DETAIL first), the i18n layer (was P0.22; English and pt-BR strings added per slice), and persistence of layout and preferences (was P0.23).

**Gates across all slices:** `cargo xtask gate quick` before each commit. The full `cargo xtask gate test` and the light and dark captures run at the end of each group of slices, not per slice.

---

### 5. Out of scope for now

- Android, web and iOS verification beyond compile checks, until a slice needs a phone check. Phone width is checked in the desktop app; Android is checked when S1 is reviewed on a device.
- Phases 1 to 13 as written in v2. They are replaced by the slice list above and will be re-planned when reached.

---

### 6. Process notes

- The per-step evidence folders, the section 11.0 status table and the gaps tracker from v2 are retired. Status now lives in section 4 of this file, and evidence is the owner's review plus the test output.
- Owner decision (2026-11-15): plan by vertical slices; full rewrite of the plan file; v2 archived unchanged.

---

### 15. Decision log

Decisions the plan does not settle, and deliberate deviations from it, are recorded here. Each entry has a Status (`proposed`, `accepted`, `superseded`, naming its replacement), Context, Decision, Consequences and Evidence. Add a new entry as the next number in this section. Never delete an entry: supersede it. Earlier entries were separate files in `docs/adr/`, merged here on 2026-11-14. Numbers are kept so existing references still resolve.

#### 15.1 Decision 0007: UI toolkit: Slint 1.18.1

- **Status**: accepted
- **Date**: 2026-11-10 (owner decision)
- **Scope**: every UI in the workspace: `rimelight_ui` (design system), `apps/desktop`, `apps/gallery`, `apps/web`, `apps/android`, `apps/ios`

##### Context

Rimelight Space is one Rust codebase that must run on Windows, Linux, macOS, Android, iOS and the web (PLAN section 2). The UI toolkit has to:

- run on all six targets from one codebase, with desktop, Android and wasm builds from one vendor;
- let the design system own every colour, size and font, so that components take tokens and never raw values;
- support keyboard and pointer use, light and dark themes, and phone-width layouts;
- allow screenshot evidence and headless interaction tests without opening windows.

##### Decision

Use **Slint 1.18.1** as the UI toolkit. Pin it exactly: `slint =1.18.1` and the matching `slint-build`, testing and backend crates. Upgrades are one coordinated change.

##### Reasons

1. **One vendor and one crate family.** Desktop, Android, iOS (compile check only, see decision 0009) and web build from the same crates at one exact version. No git-pinned mobile layer is needed.
2. **Android text input works with a plain activity.** A soft keyboard opens on a tapped text field and typed text appears in it (checked on the emulator). No custom Java host was needed.
3. **Our tokens reach every component.** The Rust token values are generated into one exported `Tokens` global (`crates/rimelight_ui/ui/generated/tokens.slint`). Themes switch live from a bound property.
4. **Testable without a window.** Real key and pointer events go through the public `slint::Window::dispatch_event` and the mock-time testing backend. Screenshots come from `Window::take_snapshot`, so the gallery and tests run headless.
5. **The navigation manifest stays plain Rust.** `rimelight_nav`, `rimelight_proto`, `rimelight_policy` and `rimelight_i18n` do not depend on the toolkit.
6. **Web builds with stock tooling.** `trunk build --release` produces the wasm app with `wasm-bindgen`; the web build passes (about 3.2 MiB gzip for the shell, see `PROJECT.md section 11.0`).

##### Consequences

- **Own components, not std-widgets.** Slint's std-widgets (`Button`, `LineEdit`, `TextEdit`, `ComboBox`, `CheckBox`, `Switch`, `Slider`) take their colours from a fixed palette that tokens cannot reach. Each control the app uses is an own `.slint` component in `rimelight_ui`, built on `TextInput`, `TouchArea`, `FocusScope`, `PopupWindow` and layouts.
- **Facade rule.** Only `rimelight_ui` depends on `slint`. Feature code and `apps/` import only its Rust API and its `.slint` components. No colour, size or font literal appears outside the generated token global. The lint that enforces this (P0.18) must cover `.slint` files.
- **No dock.** Slint has no dock or pane group. Rail, sidebar, tab strip, split panes, command palette and status bar are own components. Saving and restoring layout is our own code.
- **Accessibility limits.** AccessKit is used on desktop. Slint 1.18.1 has no `menu`, `menu-item`, `dialog` or `link` role, so the closest role is used and the gap is recorded in `crates/rimelight_ui/README.md`. Screen-reader behaviour on each platform is not verified.
- **Renderer limit.** The software renderer used for headless captures draws no box shadows. The app's default renderer (femtovg) draws them; on-screen shadows are not verified yet.
- **Licence.** The Slint Royalty-free Desktop, Mobile and Web Applications License 2.0 applies. The `AboutSlint` widget must stay on an About screen (see section 15.1, licence research).
- **Version churn.** The exact pin means each Slint upgrade is a deliberate change with a full gate run.
- **Testing limits.** The testing backend is headless: no OS window and no OS input queue. The OS-to-Slint translation (winit) is not exercised by tests.

##### Evidence

- Evidence of the Slint work: the slice folders under (not kept) (S1 to S11, with captures and gate output per slice).
- Licence research: the subsection below ("Licence research (2026-11-10)").

##### Licence research (2026-11-10)


> This is research for the owner, not legal advice. Have a lawyer confirm the points in "Open questions" before relying on them.

###### Answer

**Yes, on the evidence found, Rimelight Space can use Slint without paying, under the Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0 (`LicenseRef-Slint-Royalty-free-2.0`), provided that:**

1. the application is a desktop, mobile, or web application (not embedded), and
2. the app shows the AboutSlint widget in an About screen or dialog (or in the splash screen if there is no About screen), **or** the Slint attribution badge is shown on a public webpage where the binaries can be downloaded, and
3. the app does not expose Slint's APIs, in whole or in part, to others, and Slint is not distributed on its own (it must be part of an application).

The royalty-free text has no revenue, headcount, or company-size limit. Internal and private distribution is not excluded by the text found. Our use (closed source, employees and contractors, desktop, mobile, and wasm) fits the grant. Caveat: the rimelight.com website is not Slint, so the badge option is only relevant if the badge is placed on a public page where the binaries are downloadable. The About-screen option is the one that fits an internal app.

**Payment trigger:** embedded systems, which we do not ship. Per the LICENSE.md summary, a commercial licence is also needed if the attribution disclosure is missing ("Without disclosure, the Commercial license applies"). That is the one condition under which payment would become required for a use we would otherwise be in.

###### Evidence

Source-access note: all URLs were read through a summarising fetch tool, not a raw browser copy. Quotes below are the wording that tool returned. Verify each one against the live URL before relying on it in a legal context.

###### 1. Licence options and versions

Source: https://github.com/slint-ui/slint/blob/master/LICENSE.md (raw: https://raw.githubusercontent.com/slint-ui/slint/master/LICENSE.md) and https://slint.dev/pricing

The framework is triple-licensed. "You may choose any one":

| Option | Identifier / file | Summary |
|---|---|---|
| Royalty-free | `LicenseRef-Slint-Royalty-free-2.0` / `LICENSES/LicenseRef-Slint-Royalty-free-2.0.md` | "Permits use in **proprietary** desktop, mobile, and web applications **at no cost**." Embedded excluded. |
| GNU GPLv3 | `GPL-3.0-only` / `LICENSES/GPL-3.0-only.txt` | Open-source use "at no cost", for desktop, mobile, web and embedded. |
| Commercial | `LicenseRef-Slint-Software-3.0` / `LICENSES/LicenseRef-Slint-Software-3.0.md` | Proprietary use, including embedded. Pricing at slint.dev/pricing. |

Pricing page option names: "Royalty-Free", "Enterprise", "Small Enterprise", "Startup & Individual", "GPLv3". Current versions (from the licence identifiers and the terms page): Royalty-free **2.0**, Commercial (Slint Software License) **3.0** (page shows version 3.0.5), GPL **3.0-only**.

Other components per LICENSE.md: documentation and examples under MIT ("in any project, including proprietary ones"); contributions under MIT-0; third-party code keeps its own licence.

###### 2. Royalty-free licence: platforms, limits, attribution, payment

Source: https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md (also https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md)

- **Title/scope:** "Slint Royalty-free Desktop, Mobile, and Web Applications License, Version 2.0."
- **Grant (Section 1):** "world-wide, royalty-free, non-exclusive license to use, reproduce, make available, modify, display, perform, distribute the Software as part of a Desktop, Mobile, or Web Application." (Quote is the returned excerpt, truncated.)
- **Platforms:**
  - Desktop: covered (Desktop Application).
  - Mobile: covered (Mobile Application). Private distribution is not addressed, so it is not excluded by the text found.
  - Web (wasm): covered (Web Application).
  - Embedded: excluded. "The License does not permit the use of the Software within Embedded Systems." (Section 3)
- **Revenue, headcount, or company-size limits:** none stated in the royalty-free text. The headcount limits in the pricing page apply only to the paid "Small Enterprise" (up to 50 staff) and "Startup & Individual" (under 10 staff) plans, not to royalty-free.
- **Proprietary / closed source:** the text does not prohibit closed-source applications. The restriction is: "The License does not permit the distribution of Application that exposes the APIs, in part or in total, of the Software." (Section 3)
- **Standalone distribution:** Slint may not be distributed alone. For that, "you may use the Software under the GNU General Public License, version 3." (Section 3)
- **Attribution (Section 2), one of:**
  - (a) "Display the AboutSlint widget in an "About" screen or dialog" accessible from the top-level menu. If there is no such screen, "display the widget in the "Splash Screen" of the Application."
  - (b) "Display the Slint attribution badge on a public webpage," preferably where the Application's binaries can be downloaded.
- **Keep licence notices** in source code intact (summary of the licence text).
- **Warranty:** provided "as is".
- **Payment conditions:**
  - Embedded systems: "A Commercial license is required for embedded systems, regardless of disclosure." Also "one-time per-device Royalties may apply for commercial Embedded Systems" per the pricing page ("from $1.00 per device").
  - Missing disclosure: "Without disclosure, the Commercial license applies" (LICENSE.md summary).

###### 3. Already-released versions if terms change

Source: https://slint.dev/terms-and-conditions (Slint Software License 3.0, version 3.0.5), section 3.

- For the **paid** licence: "The License is limited to only the versions of Software that were made available to you under the Paid License Plan." Other versions fall under GPLv3: "For all other versions, you may use the Software under either the GNU General Public License, version 3."
- For the **royalty-free** licence: the text found does **not** address whether a grant persists for a version already released when terms change. That is an open question (see below). Each version carries its own licence identifier, so a version we already use is governed by the licence it was released under, but this has not been confirmed in the text.

###### 4. Contractors and internal distribution

- The royalty-free text does not expressly address contractors or internal distribution. The closest text is the "All individual users of Slint associated with the design, development, or testing of the application" wording on the pricing page, which covers the people who work on the app.
- The grant covers "distribute the Software as part of a ... Application," which on its face includes distributing to employees.
- The Premium Support Agreement (version 2.0.1, section 3(d)) mentions "non-employee contractors of SixtyFPS" as Slint's own subcontractors. That concerns Slint's support staff, not our contractors, and is not relevant to our licence.

###### 5. Paid-only features and components

- Pricing page add-ons: "Perpetual Fallback License", "GUI Test Framework", and "Live Preview" (shown as "Available as an Add-On" in the comparison). Premium support is sold separately.
- The royalty-free licence covers the framework (runtime libraries and language tooling) per LICENSE.md. The summaries did not list crates by name, so it is **not confirmed** which crates each licence covers. Items to check before we depend on them: the GUI Test Framework, Live Preview (dev tooling), any LSP/tooling, and any renderer or backend crate we would use.
- Not verified: whether any Slint crate we would need is under a non-royalty-free licence. Must check `Cargo.toml` license fields of the exact crates in our workspace.

###### Open questions for a lawyer

1. Does the "Royalty-free" grant persist for a released version if Slint later changes the licence? The royalty-free text found does not say.
2. Does the AboutSlint option (a) fit our app's About screen, and is it acceptable to show a third-party widget in an internal app?
3. Is distribution to employees and contractors "distribution ... as part of a ... Application" under the grant? Is any internal-only distribution restriction implied?
4. Does mobile private distribution (no app stores) count as distributing a "Mobile Application"?
5. Does the disclosure requirement or its failure ("Without disclosure, the Commercial license applies") create a payment trigger that would need to be checked for every build?
6. Does the "not expose APIs" clause affect an internal plugin or scripting API we might expose to our own tools?
7. Confirm which Slint crates and tools are under which licence for our exact dependency set (check each crate's licence field).
8. Confirm the current licence texts against the live URLs and the full (non-summarised) text; the fetches here were summaries.

Sources (all read through a summarising fetch tool):
- https://slint.dev/pricing
- https://raw.githubusercontent.com/slint-ui/slint/master/LICENSE.md
- https://raw.githubusercontent.com/slint-ui/slint/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md
- https://slint.dev/terms-and-conditions
- https://slint.dev/faq (returned 404; not used)

#### 15.2 Decision 0008: Slint build, unsafe code and icon scaling

- **Status**: accepted
- **Date**: 2026-11-10
- **Decided by**: the coordinator, for the owner. The owner may revisit any item.
- **Context**: three points needed a decision: where the Slint-generated `unsafe` code may live, whether `slint-build` may be a non-optional build dependency, and whether icons scale with the text scale.

##### Decisions

**(a) Unsafe code.** `rimelight_ui` keeps `unsafe_code = "deny"` (the workspace uses `forbid`; see the crate `Cargo.toml`). `#[allow(unsafe_code)]` is limited to the Slint-generated module (`src/slint_ui/mod.rs`). Any future OS call, such as reading the Windows reduced-motion setting, goes in `crates/rimelight_platform` behind one small audited module with the same narrow allow. No other module in either crate may use `unsafe`.

**(b) Build dependency.** `slint-build` is a non-optional `[build-dependencies]` entry of `rimelight_ui`. `build.rs` reads `CARGO_FEATURE_SLINT` and compiles the markup only when the `slint` feature is on. This is accepted.

**(c) Icon scaling.** Icon sizes scale with the text scale (100, 125, 150, 200 per cent), so icons stay in step with the text next to them for users who raise the text size. The Slint `Icons` global exposes the scaled sizes. Icon sizes are the `size-icon-*` tokens multiplied by `Tokens.text-factor`.

##### Consequences

- `crates/rimelight_ui/README.md` states that icon sizes scale.
- No new dependency is added by this decision.

#### 15.3 Decision 0009: Slint 1.18 iOS status

- **Status**: accepted (owner, 2026-11-11: iOS stays compile-check only)
- **Date**: 2026-11-11
- **Step**: P0.08 (Apple and Linux compile), iOS part

##### Context

The iOS status of Slint 1.18.1 had to be known before any iOS platform work. PLAN section 2 treats iOS as compile-check only for now. decision 0007 recorded iOS as unverified: the iOS docs page returned 404 at that time, and no iOS build was possible on Windows.

This entry is research only. No cargo build was run, and no Rust target was installed.

##### Findings

**Support.** The current Slint iOS guide says a Rust Slint app "can be cross-compiled to iOS and runs on iPhones, iPads, and their respective simulators", using the Winit backend and the Skia renderer. Only Rust is supported for iOS apps at the moment (the C++ iOS PR #13580 is not confirmed for 1.18).

**Stability label.** No stable, experimental or tech-preview label for iOS was found on the current docs page, the 1.18 blog post, or the GitHub release list. The last explicit label found is "tech preview", from the 1.12 release (June 2025, blog post "Slint 1.12 Released"). The guide also says it is the "unreleased documentation for the next version of Slint", which conflicts with a "you are viewing an older version" notice on the same page. So the docs cannot be used to confirm the 1.18.1 status.

**Release history for iOS:**
- 1.12 (June 2025): first iOS support, tech preview.
- 1.15 (February 2026): safe-area and virtual-keyboard area support for Android and iOS.
- 1.18.0 (16 Sep 2026): `safe-area-insets`, `virtual-keyboard-position` and `virtual-keyboard-size` properties; the release blog post does not mention iOS in the text fetched.
- 1.18.1 (21 Sep 2026): according to a third-party PR summary, fixes iOS Metal rendering falling back to software, full refresh rate on ProMotion, and a black or empty window at startup. Not confirmed on the GitHub release page, which does not mention iOS.

**Toolchain (per the iOS guide):**
- A macOS host with an up-to-date Xcode installation (used to build, deploy and submit to the App Store).
- XcodeGen to generate the project.
- Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim` (the guide's list). Also `x86_64-apple-ios` for Intel simulators.
- The Skia renderer; `renderer-skia-opengl` is not supported on iOS, so Metal is used.
- `CADisableMinimumFrameDurationOnPhone` in the plist for full frame rate on iPhone (without it, 60 fps).
- UIScene lifecycle entry in the plist: the guide says iOS 27 requires it, and an app without it fails to launch.

**Known gaps (not covered by Slint's docs as read):**
- IME composition on iOS: not documented or tested.
- Accessibility (VoiceOver): not documented.
- App Store submission: the guide mentions it, but no review or signing notes were found.
- Safe-area behaviour: only the configuration key is documented, not the layout behaviour.

**Checked on Windows.** `rustup target list` lists the iOS targets (`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`, plus macabi variants), none installed. Nothing was installed and no iOS compile was run. A real iOS check needs a macOS host with Xcode.

##### Options considered

1. **Option A: iOS is a supported target, planned as a compile check only.** Keep PLAN section 2 (compile-check only). P0.08 adds `cargo check` for iOS on a macOS runner, with no device work. Pros: matches PLAN; no device work until the owner asks. Cons: the stability label is still unconfirmed.
2. **Option B: iOS is a shipping target now.** Build and test on a device or simulator now. Pros: finds issues early. Cons: needs a Mac, which this repo does not have on Windows; the stability label is unknown, so the risk is high.
3. **Option C: iOS is dropped for now.** Pros: smallest scope. Cons: it contradicts PLAN section 2, which the owner has not changed.

##### Decision (proposed)

Option A. iOS stays compile-check only (PLAN section 2), using a macOS host or runner with Xcode, and it is not a shipping target until a stable label is confirmed. The owner should confirm the scope.

##### Consequences

- iOS status is recorded as "docs say supported; no stable label found; compile check only". Simulator or device testing needs a Mac and is out of scope unless the owner asks.
- A follow-up is needed to confirm the status: ask Slint directly (GitHub Discussion or the Slint Discord) or check the 1.18.1 ChangeLog, which was not readable here.

##### Evidence

Sources (read 2026-11-11):
- Slint iOS guide: https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/ (states it is unreleased "next version" docs)
- Slint 1.18 release post: https://slint.dev/blog/slint-1.18-released (dated 16 Sep 2026; no iOS text in the fetched copy)
- Slint 1.12 release post (iOS tech preview): https://slint.dev/blog/slint-1.12-released
- Slint 1.15 release post (safe area, keyboard): https://slint.dev/blog/slint-1.15-released
- GitHub releases list (v1.18.0 16 Sep 2026, v1.18.1 21 Sep 2026): https://github.com/slint-ui/slint/releases
- C++ iOS PR (not confirmed for 1.18): https://github.com/slint-ui/slint/pull/13580
- Local: `rustup target list` output (iOS targets listed, none installed), decision 0007 (PLAN section 15.1).

#### 15.4 Decision 0010: Unsafe export for the Android entry point

- **Status**: accepted (owner, 2026-11-11)
- **Date**: 2026-11-11
- **Step**: P0.06 (Android hello), entry point in `apps/android`

##### Context

android-activity finds the app by the exported symbol `android_main`. Edition 2024 requires the attribute to be written `#[unsafe(no_mangle)]`. The workspace sets `unsafe_code = "forbid"`, and decision 0008 (a) allows `unsafe` only in `rimelight_ui`'s generated Slint module and in `crates/rimelight_platform`. With `forbid`, a crate cannot re-allow the lint, so `apps/android` did not compile (`cargo ndk` error: "usage of the unsafe `no_mangle` attribute"). Edition 2021 does not help: the lint also fires on `#[no_mangle]`.

##### Decision taken to continue P0.06

`apps/android` does not inherit the workspace lints. It sets `unsafe_code = "deny"`, the same as `rimelight_ui`, and puts one `#[allow(unsafe_code)]` on the single `android_main` function. The crate has no other `unsafe`. This is a deviation from decision 0008 (a) and needs the owner's decision.

##### Options for the owner

1. Accept this: add `apps/android` to decision 0008 (a) as the second place with the narrow allow, next to `rimelight_platform`.
2. Move the export into `crates/rimelight_platform` (already allowed by a section 15 entry 0008) and have `apps/android` call it. This needs the export to survive linking into the cdylib, which must be tested.
3. Keep the export in the app crate but move the whole Android entry into a small audited module in `crates/rimelight_platform`.

Until the owner decides, the code stays as described above.


#### 15.5 Decision 0011: App areas are modules in rimelight_ui, not feature crates

- **Status**: accepted (owner, 2026-11-14)
- **Date**: 2026-11-14
- **Step**: SF (shell foundation) and every skeleton-page and area step after it (P0.31 to P0.44)

##### Context

PLAN section 6 planned one crate per app area under `crates/features/`. Only `rimelight_ui` may depend on Slint (decision 0007), and every page is a `.slint` file, so a feature crate could hold only Rust glue and would still depend on `rimelight_ui`. The Slint build cost stays in `rimelight_ui` either way. An opinion from a separate review agent (2026-11-14) reached the same conclusion; it did not measure build times.

##### Decision

Each app area is a set of modules inside `rimelight_ui`: `ui/pages/<area>/*.slint` and `src/pages/<area>/`. The `crates/features/` folder and its twelve empty placeholder crates are removed. Area modules do not import each other, and cross-area links go through routes in `rimelight_nav`.

##### Revisit when

- A measured incremental rebuild of an area's view-model logic, moved into its own crate, is more than 30 percent faster than the same edit in `rimelight_ui`.
- Separate owners per area need their own review boundaries.

#### 15.6 Decision 0013: Move the generated Slint UI into its own crate

- **Status**: proposed (2026-11-15), needs owner approval
- **Date**: 2026-11-15
- **Step**: every step after SF; affects the build of `rimelight_ui` and the test loop

##### Context

Build timing on 2026-11-15 (`cargo build --timings`, `rimelight_ui` test profile): the crate took 449 to 458 seconds to compile, and the `ui` test binary took 256 seconds. The generated Slint Rust (`slint::include_modules!()` in `slint_ui/mod.rs`) is about 263,000 lines. The hand-written Rust in the crate is about 14,000 lines. Every Rust-only edit recompiles the whole crate, so the generated code is rebuilt each time. Linking is the smaller cost.

##### Options

1. **Split the `.slint` files and their generated module into a new crate** (for example `crates/rimelight_ui_slint`). `rimelight_ui` depends on it and re-exports the same names, so the rest of the code is unchanged. Rust-only edits skip the generated code. Only this crate depends on `slint`, so decision 0007 holds. Cost: one more crate and a small re-export layer.
2. **Keep one crate, lower dev optimisation for dependencies.** Cheap to try, but it does not affect the `rimelight_ui` crate's own compile time, so it is unlikely to fix the 449 seconds.
3. **Add `lld` and `debug = "line-tables-only"`.** Helps the 256-second link step only.

##### Recommendation

Option 1, then option 3 if links still dominate. Option 1 changes the decision in section 15.5 only for the Slint part; area modules stay in `rimelight_ui` as decided.

##### Owner decision needed

Approve option 1, choose another option, or defer. A prototype measurement (move the generated module, rebuild, compare times) should run before the change is committed.

#### 15.7 Decision 0014: Sign-in through the shared rimelight.com Auth0 application

**Status:** accepted by the owner 2026-11-15.

##### Decision

- Rimelight Space reuses the `rimelight.com` Regular Web Application and the `https://api.rimelight.com` API. Website, game and app use the same application.
- The client secret stays on the backend. The app opens the system browser at `/v1/auth/login`; the backend sends it to Auth0, receives the callback, exchanges the code with the secret, checks the token (RS256, issuer, audience, `user_type == employee`, user pre-provisioned), and creates a Rimelight Space session. The app receives a one-time code through its return address (loopback on desktop, custom scheme on mobile, redirect on web) and exchanges it at `/v1/auth/exchange` for its session token.
- Sessions are 30 days, sliding, stored in Postgres. MFA for employees is enforced in the Auth0 Post Login Action.
- No dev identity provider. A test account in Auth0 is used instead.
- Local tooling: Postgres 18 native install, Cloudflare `cf` CLI (beta) instead of wrangler.

##### Owner setup still needed

- Add the Space backend callback URL (`http://localhost:8787/v1/auth/callback` locally) and logout URL to the `rimelight.com` application.
- Add the MFA rule for employees to the `rimelight-claims` Action.
- Mark the owner's Auth0 user `app_metadata.user_type = "employee"`.

#### 15.8 Decision 0015: Worker entry for the sign-in API (SA.2b)

**Status:** accepted for the Worker entry (owner, 2026-11-15, "lets do it"). Tracked as slice W1 in section 4. The native dev server part is superseded by decision 0033.

##### What was checked

- `cargo check -p rimelight_backend_api --lib --target wasm32-unknown-unknown` failed first: `getrandom` 0.2 (pulled in by `rsa` through `rand_core` 0.6) needs its `js` feature on wasm32. Added `getrandom02` with `features = ["js"]` under the wasm target table. With that, the shared core (`service`, `auth0`, `pending`, `http`, `session`, `store`) compiles for wasm32.
- Not built: the `worker` crate entry (`#[event(fetch)]`), the Hyperdrive binding and the `cf` CLI config. Postgres is native only (`tokio-postgres` over `tokio::net`), so the Worker needs a socket adapter from `worker` to tokio-postgres. I did not write or test one.

##### Decision (proposed)

- SA.2b ships the native dev server (`backend/api/src/bin/dev_server.rs`, `tiny_http` on 127.0.0.1:8787, `reqwest` for Auth0) and the Postgres store (`backend/api/src/pg.rs`). No Worker entry yet.
- The Worker entry follows as its own step once the owner approves: `worker` crate, Hyperdrive socket adapter for tokio-postgres (or a Hyperdrive-compatible query path), a `HttpFetch` over `worker::Fetch`, and the `cf` CLI config.

##### Consequences

- The app's local backend is the dev server only.
- `pg.rs` is compiled for native targets only (`cfg(not(target_arch = "wasm32"))`).

#### 15.9 Decision 0016: Sign-in screen and session driver (SA.4)

**Status:** proposed (SA.4, 2026-11-15). Needs owner approval. Deviations from the SA.4 step text, recorded rather than silently taken.

##### What was built

- `rimelight_ui`: the sign-in screen (`ui/components/sign-in-screen.slint`), the status bar's Connected or Offline indicator, the avatar and profile menu reading the signed-in user, and `slint_ui/session.rs` (initials, error texts). English strings only: `rimelight_i18n` is still a placeholder.
- `rimelight_ui` (native targets only): `slint_ui/session_driver.rs` runs restore, sign-in, sign-out and the 30 s check on worker threads, and posts results to the UI thread.
- `rimelight_client`: `block_on` runs the async client on a current-thread tokio runtime (reqwest needs a reactor).
- `apps/desktop`: builds the window, shows the loading phase, attaches the driver, then runs the loop.

##### Deviations

- Mobile (`apps/android`, `apps/ios`) and web are not wired to the driver, so their sign-in button does not appear yet. The web target cannot take `rimelight_client` as built (its random source needs a wasm feature), so the driver is native-only. Android and iOS can call `session_driver::attach` in their own step.
- Not provisioned: fixed in SA.5. Once the sealed pending login opens, every sign-in failure redirects to the app's return address with `error=<code>&state=<app state>` (`not_provisioned`, `not_employee`, `access_denied`, `server_error`). The client maps these codes. Only an invalid or missing pending login keeps the HTML page. Choices: a sub conflict and a store error both send `server_error`; a failed token check sends `not_employee`; any other Auth0 error sends `server_error`.
- While signed out, the sign-in screen covers the shell. Fixed in SA.5: the shell's key handling returns at once while the session phase is not signed-in, so Ctrl+K and the other shell shortcuts do not reach it.
- Cancel ends the wait on the screen at once. The worker thread keeps the loopback listener until the client's five-minute timeout, and its late result is dropped.
- The 30 s check calls `health` and, when a session is stored, `me`. A 401 on `me` clears the session and shows the sign-in screen with "Your session has ended".

##### Consequences

- Nothing here has been run against the real backend or Auth0. The UI tests use the window's test state and the plain-Rust rules only.

#### 15.10 Decision 0017: Web sign-in and the web session driver (SA.6)

**Status:** proposed (SA.6, 2026-11-15). Needs owner approval. Choices made for the web target, recorded here.

##### What was built

- `rimelight_client` (all targets): `web_signin.rs` with the pure parts (reading the return from the query, the `state` check, the return address), tested natively. On wasm32 it adds `begin_web_sign_in` (verifier and `state` into sessionStorage under `rimelight-space.web-signin`, then the page goes to `/v1/auth/login` with its own origin and path as the return address) and `complete_web_sign_in` (on start-up: reads the return, strips the query with `history.replaceState`, takes the pending sign-in once, checks the `state`, then exchanges the code).
- `rimelight_client` (wasm32): `WebTransport` over gloo-net `fetch` (Authorization and JSON headers, the same `Transport` seam as reqwest), `WebSessionStore` in localStorage under `rimelight-space.session`.
- `.cargo/config.toml`: `getrandom_backend="wasm_js"` for wasm32-unknown-unknown. The crate feature alone is not enough for getrandom 0.3.
- `rimelight_ui` `session_driver.rs`: one set of start-up, sign-in, sign-out and check functions. `spawn_task` is a worker thread with its own runtime on native and a local future on the web. Posting to the window is `invoke_from_event_loop` on native and a direct apply on the web (already on the UI thread).
- `apps/web`: the same wiring as apps/desktop (loading phase, `ApiClient`, `WebSessionStore`, `attach`). Base URL: `RIMELIGHT_API_URL` at build time, default `http://localhost:8787`.
- `backend/api`: `WEB_APP_ORIGINS` (comma-separated http or https origins, empty by default, validated: no path, query or user info). It is an allowed return address (origin plus path) and a CORS origin. The CORS headers (echo of the one allowed `Origin`, `Vary: Origin`, preflight `OPTIONS` answered with 204 and `Authorization, Content-Type`) go on `/v1/health`, `/v1/auth/exchange`, `/v1/me` and `/v1/auth/signout` only. Login and callback are top-level navigations and get no CORS headers.

##### Choices

- Port 8080 for `apps/web` in `WEB_APP_ORIGINS=http://localhost:8080`. `apps/web` has no `Trunk.toml`, so this is trunk's default serve port. Check it when `trunk serve` is first run.
- The 30 s check uses slint's `Timer` on both targets, not gloo-timers. Slint's timer already runs in the browser, so one fewer crate.
- `BoxFuture` drops its `Send` bound on wasm32 only. The browser's fetch futures hold JS values and are not `Send`.
- The web session is in localStorage, as the SA.6 step said. A stored session therefore survives a browser restart. The backend's 30-day sliding session is the limit; the 401 check on `me` ends it.
- The web sign-in leaves the page. The "Waiting" phase is shown only until the redirect starts; Cancel does nothing on the web.

##### Deviations and open points

- Native `spawn_task`: if the worker's runtime cannot be built, the window stays on the loading phase. The old code posted an error for that case. The chance is very small; noted, not fixed.
- Not run in a browser and not run against Auth0 or a live backend. Auth0 needs no change for the web: it only ever sees the backend callback (decision 0014). The web app origin is an allowed return address on the backend only.
- The owner's real `backend/api/.dev.vars` needs `WEB_APP_ORIGINS` added. It was not read or changed here.

##### Consequences

- Only the web app and the desktop app call `session_driver::attach`. Android and iOS are unchanged (still no sign-in).

#### 15.11 Decision 0018: Android sign-in (SA.7)

**Status:** proposed (SA.7, 2026-11-15). Needs owner approval. Deviations from the plan and the choices made, recorded here.

##### What was built

- `rimelight_platform` (Android only, module `android`): `open_url` sends ACTION_VIEW through JNI, and `intent_data` reads the activity's current intent. This is the only unsafe code added, behind the narrow allow of decision 0008 (a). The crate's lints are now spelled out with `unsafe_code = "deny"` (they were the workspace `forbid`).
- `rimelight_client::signin_mobile` (native targets, no Android code): `begin` writes the PKCE verifier and `state` to `signin-pending.json` in the app's data folder, then opens `/v1/auth/login` with return address `rimelight-space://auth`. `finish_return` takes the address the app received, checks the `state` and exchanges the code.
- `rimelight_client::session_store::FileSessionStore`: the session in `session.json`, written through a temporary file with mode 0600 on unix.
- `rimelight_ui` `session_driver`: `attach_mobile` (with `Mobile` and `BrowserOpener`) and `Session::sign_in_returned`. The sign-in opens the browser and does not wait; the return finishes it. `attach` for desktop and web is unchanged.
- `apps/android`: the entry point reads the launch intent and each resume's intent, hands a new return to the driver once (`last_return` dedupes), and runs the session driver with `FileSessionStore`. API base is `RIMELIGHT_API_URL` at compile time, else `http://10.0.2.2:8787`.
- Manifest: the activity is `.RimelightActivity` and has an intent filter for `rimelight-space://auth` (VIEW, DEFAULT, BROWSABLE).

##### Deviations and choices

1. **Java host code.** The manifest and PLAN said plain NativeActivity with no Java. android-activity 0.6.1 does not forward a new intent, and a singleTask activity gets the browser's return as a new intent. Without Java the return cannot be read reliably. Added `apps/android/android/app/src/main/java/dev/rimelight/space/RimelightActivity.java`, a NativeActivity subclass whose `onNewIntent` calls `setIntent`. It was compiled with `javac` against the android-37 `android.jar`. It was not built by Gradle.
2. **Unsafe.** The JNI calls need `JavaVM::from_raw` and `JObject::from_raw`. They live in `rimelight_platform` (decision 0008 (a)), not in the app. `apps/android` keeps its one unsafe item (decision 0010) unchanged.
3. **Session storage is not encrypted.** `session.json` is in the app's internal data folder, which is private to the app, with mode 0600 on unix. Encrypting it with the Android Keystore is a later hardening step.
4. **Pending sign-in.** `signin-pending.json` is used once. A return with nothing pending is ignored, so a stale intent that Android replays does not show an error. A `state` that does not match is `StateMismatch`. Cancel does not delete the pending file; the next sign-in replaces it.
5. **Browser return.** The backend redirects the browser to `rimelight-space://auth`. Whether the browser hands that custom scheme to the app on a device is not verified.
6. **Device address.** `10.0.2.2` reaches the host's localhost from the emulator only. The backend's `PUBLIC_BASE_URL` is localhost, so a physical device needs a host it can reach. Owner decision.
7. **iOS keeps the stub.** ASWebAuthenticationSession needs Objective-C bindings, which are out of reach here. `apps/ios` is still a placeholder and has no code to change. No iOS check was run: no Apple target is installed (`rustup target list --installed`).
8. **Without a data folder** (`internal_data_path` is `None`), the app attaches the driver without the mobile path, and sign-in reports that it is not supported.

##### Checks and environment

- The Slint Android backend's build script needs a JDK. No JDK is on PATH here. `JAVA_HOME` was set to the Android Studio JBR for the commands only; nothing was installed or changed system-wide.
- Clippy with the Android target fails on `missing_const_for_thread_local` in `crates/rimelight_ui/src/slint_ui/chat_sidebar.rs` and `projects.rs`. Those files are not changed in SA.7, and the commit hook lints the host target only. Not fixed here. The Android check was run with that one lint allowed.

##### Consequences

- Nothing has run on a device or an emulator. The browser return, the intent handling, the JNI calls and the session file are unverified on Android.
- Owner setup still needed: a host the device can reach for `RIMELIGHT_API_URL`, and the Gradle build of `RimelightActivity` (not run here).

#### 15.12 Decision 0019: People profiles, org structure and user admin (SP)

**Status:** accepted (owner, 2026-11-15). Applied in SP.1.

##### Decision

- **Who edits what.** A person edits their own personal fields: photo, preferred name, pronouns, bio, phone, timezone, skills. Admins and HR edit org fields: title, department, team, manager, start date, active.
- **Visibility.** Work information is visible to all employees. Personal contact (personal phone, personal email, address, birthday, emergency contact) is visible only to the person, their manager chain, and admins and HR. The API enforces this, not SQL.
- **Photos.** Uploaded in the app and stored in the rimelight.com R2 bucket `rimelight-dot-com` under the prefix `space/avatars/`. A local-disk fallback is used in dev.
- **Seed.** The database is seeded with the fixture studio (about 300 people, 12 teams). `rimelight@rimelight.com` is at the top as CEO with `is_admin`, and `test@rimelight.com` is placed under the CEO.
- **User admin.** The user admin page is in scope of SP.

##### Choices made in SP.1

1. **Fixture source.** `crates/rimelight_fixtures` is still a placeholder and has no people or teams, so there was nothing to read. The seed is hand-written SQL (`backend/db/seed/0004_fixture_people.sql`), not a Rust function and not a generator. Python is not installed here, so no generator was run. Ids are deterministic (`f0000000-...` people, `7e000000-...` teams, `d0000000-...` departments) and the names come from fixed arrays in the SQL.
2. **Tree.** The CEO has 4 department heads. Each department has 3 teams, each led by one team lead (12 leads). The other 284 people are members of a team. Depth: CEO 0, heads 1 (plus the test account), leads 2, members 3.
3. **Guard.** The seed runs only while `users` has at most 2 rows. Re-running it prints a notice and does nothing (checked).
4. **Schema.** Departments have no head column; the head is the manager of the team leads. Personal fields `address` and `emergency_contact` are plain `text`. `users` has a check that nobody is their own manager.
5. **Dev server.** `backend/api/src/bin/dev_server.rs` still applies only 0001. 0003 and the seed were applied to the local database by hand with psql. Not changed here.

##### Checks

- Applied to `postgres://postgres:postgres@localhost:5432/rimelight_space`: `0003_people.sql`, then the seed (`ON_ERROR_STOP=1`, one transaction each).
- Counts after the seed: 302 users, 4 departments, 12 teams (all with a lead). 1 user has no manager (the CEO), 301 have one. 296 users are in a team (12 leads and 284 members). 
- Recursive CTE from the roots: 302 reachable, 0 cycle rows, 1 root, max depth 3, depth counts 1 / 5 / 12 / 284.
- No self-managed or dangling manager rows. No user whose department differs from their team's department.
- Display names repeat (for example three people named "Amelia Adams"). Emails are unique because each carries a number.

##### Consequences

- The person and admin APIs, the photo upload and the People pages are not built yet. They belong to later SP steps.
- The fixture users have no personal contact data.

##### Owner addendum (2026-11-15), built in SP.2

- **Org admin.** Departments: CRUD with parent department and head, GET as a flat list with member counts and as a tree. Teams: CRUD with department and lead, GET with members. Delete is refused (409) while people are in it; a department is also refused while it has child departments or teams.
- **Positions** (the "roles" of the org sense): a `positions` table (title, department, level, description). `users.position_id`. The profile title comes from the position, with `users.title` as the fallback.
- **Permission roles** for `/admin/roles`: built-in `admin` and `hr` in a `user_roles` table. `users.is_admin` is kept as a legacy column and copied into `user_roles` once by migration 0005. The API reads roles only. Admins assign and unassign roles. Admins and HR see personal fields of everyone. Custom roles are out of scope.
- **Migration 0005** (`backend/db/migrations/0005_org_roles.sql`) and an additive, idempotent seed (`backend/db/seed/0005_org_roles_seed.sql`): positions for the seeded people, the People department, a head for each fixture department, the CEO as admin, and the People head as HR.
- Owner-named scope not chosen by the owner, recorded as proposed in decision 0020: the People head is the test account.

#### 15.13 Decision 0020: SP.2 API, avatars and roles

**Status:** proposed (agent choices made while the owner was away, 2026-11-15). The owner reviews them.

##### Choices

1. **Routing.** `people_service::PeopleService<A, P, B>` answers the paths it `owns` (`/v1/people`, `/v1/me/profile`, `/v1/me/avatar`, `/v1/avatars`, `/v1/admin`, `/v1/departments`, `/v1/teams`, `/v1/positions`). The dev server sends those to it and everything else to the sign-in service. The people service has its own Postgres connections. CORS is not yet applied to these routes; the web app needs it before it calls them (follow-up).
2. **Visibility.** Personal fields go to the person, to anyone above them in the manager chain, and to holders of `admin` or `hr`. The check is in the service, not in SQL. Inactive people are visible to admins only.
3. **Patch semantics.** An absent field is unchanged and an empty string clears it. PATCH bodies refuse unknown fields (400), so org fields cannot be self-edited.
4. **Errors.** 422 `invalid_field`, `unknown_*`, `manager_is_self`, `manager_cycle`, `department_cycle`, `team_department_mismatch`. 409 `email_taken`, `department_has_members`, `department_in_use`, `team_has_members`, `team_in_use`, `position_in_use`, `last_admin`. Avatars: 413 over 5 MB, 415 for a type that is not PNG, JPEG or WebP by its bytes or does not match `Content-Type`. Wrong method on a known path is 405.
5. **Pagination.** Cursor is the id of the last person on the page, with keyset order on lower-cased name then id. Default limit 50, maximum 100. The directory lists active people only. A team's member list is capped at 1000.
6. **Avatars.** Keys are `{user_id}-{16 hex}.{png|jpg|webp}`, so a new upload never reuses a cached URL. The new file is stored before the row changes, and the old file is removed best effort. Photos are served by the API and need a session. On R2 the store signs SigV4 presigned URLs with `rusty-s3` (5-minute expiry, path style, region `auto`, host `<account>.r2.cloudflarestorage.com`, object keys under `space/avatars/`). Native only. The disk store is `backend/api/.data/avatars`, which `.gitignore` excludes. R2 has not been run against a real bucket (no credentials here). Its signing shape is unit-tested.
7. **R2 selection.** The dev server uses R2 when `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` are all set, else disk. It prints the choice only. The R2 lines in `.dev.vars.example` are commented out, so copying the example cannot switch to R2 with placeholder values.
8. **Roles and last admin.** Admins may revoke their own admin role if another admin exists. The last admin cannot lose the role (409). Admins may deactivate themselves; the API does not block it (open question).
9. **Migrations.** The dev server applies 0003 and 0005 when `users.is_admin` or `users.position_id` is missing. It never runs the seeds. The local `rimelight_space` database was not changed by this step.
10. **Not done here.** Audit records for every mutation (a later step; mutations write nothing else yet). Explicit session revocation on deactivation (the sessions stop working because user lookup checks `is_active`). Department and team reads for the People pages (UI, later).

##### Checks

- `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto -p rimelight_backend_db`: 110 backend library tests passed, plus 2 proto and 8 db tests, including the Postgres round trips for the store, the seeds and the migrations, each in a scratch schema that is then dropped.
- Clippy with `-D warnings` and `cargo fmt --check` are clean for the three changed packages.
- Not run: the dev server (agents do not start servers), any R2 bucket, the People UI, and the owner's review.

#### 15.14 Decision 0021: SP.3a backend fixes and the people client

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them. Supersedes choice 8 of decision 0020 (self-deactivation) and choice 1 of decision 0020 (CORS on people routes).

##### Choices

1. **CORS on all people and org routes.** `PeopleService::handle` now applies the same CORS handling as the sign-in service: `origin_allowed`, `preflight` and `add_cors_headers` moved to `service.rs` as `pub(crate)`. The people service takes the web origins through `with_web_app_origins`, which the dev server fills from `WEB_APP_ORIGINS`. Its preflight allows `GET, POST, PATCH, PUT, DELETE, OPTIONS`.
2. **Self-deactivation refused.** `PATCH /v1/admin/users/{id}` with `is_active: false` for the caller's own id returns 409 `self_deactivation`. Replaces the open question in decision 0020 choice 8.
3. **Client transport.** `Method` gains `Patch` and `Put`. `HttpRequest` gains `raw_body` (content type and bytes) for the avatar upload. The wasm transport sends binary bodies as a `js_sys::Uint8Array`, so `js-sys` is a wasm-only dependency of `rimelight_client` (already in the lock file at 0.3.107).
4. **Client surface.** `crates/rimelight_client/src/people.rs` has typed methods for the people, org and department/team/position reads, plus `PATCH /v1/me/profile` and `PUT /v1/me/avatar`. Ids are checked to be plain hex and dashes before they reach a URL. Admin reads (`/v1/admin/roles`) and the avatar GET are not in the client yet.

##### Checks

- `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api --lib`: 113 passed (3 new: CORS on people routes, self-deactivation, unknown department in `check_refs`). Postgres tests ran.
- `cargo test -p rimelight_client`: 49 passed (8 new, over the scripted transport).
- `cargo fmt --all -- --check`, `cargo clippy -p rimelight_backend_api -p rimelight_client --all-targets -- -D warnings`, `cargo clippy -p rimelight_client --target wasm32-unknown-unknown -- -D warnings`: clean.
- `cargo check -p rimelight_web --target wasm32-unknown-unknown` and `cargo build -p rimelight_desktop`: clean.
- Not checked: the wasm binary running in a browser, the dev server, the R2 bucket.

##### Not done (SP.3a UI)

The Employees, Profile, Teams and Departments pages with real data, their loading, empty and error states, the profile edit form, the photo picker (`rfd`), and the Slint tests are not started. The routes still show wireframe content. Avatar images are not fetched yet, so the pages will show initials.

#### 15.15 Decision 0022: Page data pattern and Company > Employees (SP.3b)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Page data pattern.** `src/slint_ui/page_data.rs`. A page has a `PageLoadState` (loading, loaded, empty, error), an error line, and an `opened` callback sent from its `init`. `components/page-state.slint` (`PageState`) draws the spinner, the empty message and the error with Try again. The Rust side keeps its request state behind a `Mutex` with a generation number, and a result for an older generation is dropped. `DataSource::fetch` runs the call on a worker and hands the result back to the UI thread.
2. **Data source.** `session_driver::attach` fills the window's `DataCell`. Until then a load does nothing and the page stays loading. After sign-in the page asks again (`employees-opened` is invoked from `Update::SignedIn`), so a page that failed while signed out does not stay in error.
3. **Employees.** `src/pages/company/employees.rs` and `ui/pages/company/employees.slint`. Search waits 300 ms; a timer asks only when its text is still the latest. Load more appends by cursor, page size 50. A row opens `/company/employees/{id}` in the same tab.
4. **Avatars.** `rimelight_client::people::avatar` (the path is checked before a request). Decoded with the `image` crate, with the `jpeg` and `webp` features enabled for `rimelight_ui`. Cached in memory by path.
5. **Tests.** UI tests inject results through `App.employees`. The page's `init` runs on the event loop, so the test pumps it once after building the window.

##### Checks

- `cargo test -p rimelight_client`: 51 passed (2 new: the avatar GET and refused URLs).
- `cargo test -p rimelight_ui --test ui employees`: 3 passed.
- `cargo test -p rimelight_ui --lib page_data`: 2 passed.
- `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown`: clean. `cargo fmt --check`: clean.

##### Not verified

- Not run in a desktop window, on the web, on a phone width, or against the server or R2 (owner).
- The debounce is tested with the mock clock. The first attempt showed Slint timers can fire on the mock clock in a different order than real time, so the stale-text guard is what makes the test pass, not timing alone.
- Avatars that are JPEG or WebP are decoded by the `image` crate; no real picture was loaded.

#### 15.16 Decision 0023: Profile page and the photo picker (SP.3c)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Routes.** `/company/employees/{id}` is the profile. The avatar menu's Profile row opens `/company/employees/{user-id}`, where `user-id` is the signed-in person's id (`CurrentUser.id`, set on sign-in as `AppShell.user-id`). Nothing opens before sign-in.
2. **Data.** `src/pages/company/profile.rs` (the page data pattern). A profile is `GET /v1/people/{id}` (or `GET /v1/me/profile` for the own profile, which carries the personal fields) plus `GET /v1/people/{id}/org`, loaded together. One profile is held at a time: a second profile in another tab replaces the one shown. Try again reloads the route; a sign-in also reloads it.
3. **Layout.** `ui/pages/company/profile.slint` (`ProfilePage`): header (photo, preferred name or display name, pronouns, title, team and department, manager link), Edit and Change photo (own profile only), the edit form (own profile only, self-editable fields only), then Work, Personal (only when the server returned personal fields), About (bio and skills as tags), and Organization (read-only org fields, the reporting line root first, and direct reports). Every person in the chain or the reports is a link.
4. **Edit.** The form holds drafts copied from the view when editing starts. Save sends every self-editable field in one PATCH (an empty field clears it; skills are split on commas). A refused save (422 and the rest) keeps the form open with the server's message. Cancel leaves the form, so the saved values come back.
5. **Photo.** `rfd` 0.15 as a native, non-Android dependency (`cfg(not(any(target_arch = "wasm32", target_os = "android")))`). The dialog filters png, jpg, jpeg and webp; a file over 5 MB (`AVATAR_MAX_BYTES`) is refused before the upload. The header refreshes with the new picture. On web and Android the button is hidden and a line says the photo changes are in the desktop app.
6. **Local time.** Not shown. The client has no timezone database, so the time in the profile's timezone cannot be computed without adding one. The timezone name is shown instead. Owner decision needed.

##### Checks

- `cargo test -p rimelight_ui --test ui profile`: see the status row for the count.
- `cargo test -p rimelight_ui --lib`, `cargo build -p rimelight_desktop`, `cargo check -p rimelight_web --target wasm32-unknown-unknown`: see the status row.

##### Not verified

- Not run in a desktop window, on the web, on a phone width, or against the server or R2 (owner).
- The native file dialog (`rfd`) was not opened; the picker and its upload are not exercised by any test.
- Skills chips do not wrap at a phone width (a plain row); not measured.
- Splitting a view with two profile routes: both pages show the one profile held, the last loaded (see choice 2).

#### 15.17 Decision 0024: Teams, org chart and departments (SP.3d)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Org chart endpoint.** `GET /v1/org/tree` returns every active person as `{id, name, title, avatar_url, manager_id, team}` (one list, name order, capped at 10,000). The client builds the tree from `manager_id`. A person whose manager is not active (or missing) is a root. Per-person `/org` calls would need one request per node, so the whole list is loaded once per page load.
2. **Views.** Company > Teams has List and Tree (the org chart) as two local buttons on the page (not routes). Both views come from one load (teams, departments, org tree).
3. **Org chart rendering.** Rust lists only the rows under expanded nodes (`pages/company/org_tree.rs`, `Tree::rows`). The roots and their children start expanded. The arrow toggles a node; the name opens the profile. Find person is local (no request): the first match in name order is highlighted, its ancestors open, and a note says which one. An empty box clears it. Avatars load only for listed rows.
4. **Team detail.** `/company/teams/{id}` loads the team, the departments and the org tree (names and the lead's photo). A team with no members shows an empty message inside the loaded page. A team the server does not know is an error (404 text).
5. **Departments.** Built from the flat `GET /v1/departments` (its `parent_department_id` gives the nesting), not the nested `/tree` endpoint. The first two levels start open. Selecting a row shows its head (name and photo, opens the profile), its teams (rows open the team) and its member count. Selection is not a route.
6. **Errors.** A failed load clears the lists it had (the error replaces them), so Try again starts from the loading state.

##### Checks

- See the status row for the counts.

##### Not verified

- Not run in a desktop window, on the web, or at a phone width, and not against the server (owner).
- The 300-person chart was not timed; the tests use six people. Only listed rows are built, so the cost is bounded by the open nodes, not by the company size.
- The "Find person" box searches the whole company, but a match inside a collapsed branch is shown only after the path opens (by design), so the list can grow by many rows at once.
- The Tree and List views are not a route, so a deep link cannot open the Tree view directly.
- The team lead's photo and each member's photo are fetched once each (the picture cache); a photo that fails to load keeps the initials.

#### 15.18 Decision 0025: Admin pages, user admin and org admin (SP.3e)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Admin list endpoint.** `GET /v1/admin/users` (admin only) returns the directory's paging and search with deactivated people included, as full `Profile` rows (email, roles, active). The directory itself still lists active people only. `PeopleFilter` gains `include_inactive` (false by default); the Postgres query and the memory store honour it.
2. **Admin gate.** The roles are already in `Profile.roles` on `GET /v1/me/profile`, so no roles field was added. `GET /v1/me` (the sign-in `CurrentUser`) still has no roles; the admin pages read `/v1/me/profile`. Non-admins get "You need the admin role" and no data. The API refuses the same calls with 403.
3. **Client.** `rimelight_client::people` gains typed admin calls: user list, create, update, role grant and revoke (`PUT` and `DELETE`, a new `Method::Delete` in both transports), roles, and department, team and position create, update and delete. Unknown roles and bad ids are refused before a request.
4. **State in Slint globals.** Each admin page keeps its state in a global (`AdminUsersState`, `AdminOrgState`, `AdminRolesState`) instead of window properties, so nothing is plumbed through `AppShell`. This differs from the SP.3b shape in form, not in behaviour.
5. **Company refresh.** After an admin write succeeds, each Company page whose state is not loading asks again (`refresh_company` in `pages/admin/mod.rs`). A Company page still loading is not asked again, so its result may predate the change until it is opened again. Pages not yet loaded load when shown. Sign-in also asks the admin pages again.
6. **Search is not debounced.** Each change of the admin search sends a list request; stale results are dropped by generation. The Employees page debounces at 300 ms; this page does not yet.
7. **Pickers.** Manager, head and lead use a combobox over every active person (`GET /v1/org/tree`). Positions, departments and teams use selects. Teams are labelled with their department. A position's level is free text (the server takes up to 60 characters).
8. **Users page.** The add form has email, display name, position, department, team, manager and start date (no title: the title comes from the position). Deactivate and reactivate, and grant and revoke admin and HR, act at once from the detail panel. Save sends every org field (an empty one clears it). Server errors show inline: 409 `email_taken`, `last_admin` and `self_deactivation`, 422 `manager_cycle` and the rest.
9. **Organisation page.** One form for the three tabs: name (title for positions), parent or department, head or lead (departments and teams), level and description (positions). Create when nothing is selected, save when a row is selected. Delete shows the server's 409 message (for example "Move its 9 people first.") in the form.
10. **Roles page.** Each role lists its members with Remove; Add takes the person picked in the combobox. The server's refusal of the last admin shows in the action line.
11. **Photos.** The admin lists show initials only; avatars are not fetched on these pages.
12. **Writes in tests.** Each write takes a number and only the latest write's result is shown. The window tests inject write results through the page handles (no network).

##### Checks

- `cargo test -p rimelight_ui --test ui`: 132 passed, including `admin::admin_users_states_and_the_admin_gate`, `admin_users_add_validates_then_saves`, `admin_users_edit_shows_the_server_errors`, `admin_org_tabs_create_and_the_delete_conflict` and `admin_roles_add_and_remove`.
- `cargo test -p rimelight_ui --lib`: 217 passed. `cargo test -p rimelight_client`: 56 passed. `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api`: 115 library tests passed, including the new admin list test (admin only, deactivated people listed, the directory still hides them).
- `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown`: clean. `cargo fmt --all -- --check`: clean.

##### Not verified

- Not run in a desktop window, in a browser, at a phone width, or against the running server, Auth0 or R2 (owner). The Slint layout of the three pages has not been looked at.
- Admin writes reach the server only through the client's scripted transport in tests and through the window tests, which inject results. No test sends a real admin write.
- `include_inactive` is exercised on the memory store only. The Postgres query with `include_inactive` true has not run against the database (the default path runs through the existing Postgres store tests).
- Refreshing already-loaded Company pages after an admin change is not covered by a test.

#### 15.19 Decision 0026: Projects and issues schema, policy and seed (S1.1)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Keys.** Primary keys are `uuidv7()`,. That function needs PostgreSQL 18 (the local server is 18.7). Migrations 0001 to 0005 use `gen_random_uuid()`; this one does not change them. Seed rows use fixed md5-derived uuids so the seed is deterministic.
2. **Timestamps and soft delete.** Every new table has `created_at`, `updated_at` and `deleted_at`, including the join table and the activity log, to follow 7.7 literally.
3. **Consistency in SQL.** Composite foreign keys keep an issue's state, sprint and parent inside its project, and make an assignee a project member. Consequence: a member cannot be removed while they are assigned issues; the API must reassign first.
4. **Links.** Each link is one row with a kind (`relates`, `blocks`, `duplicates`). The inverse is read from the other side. Links across projects are allowed; SQL does not forbid them.
5. **Project key.** `^[A-Z][A-Z0-9]{1,9}$`: 2 to 10 characters, one leading letter, digits allowed after it.
6. **Roles.** Membership is required to see a project. `rimelight_policy` takes an `Access` (global admin flag and the project role) and returns the three decisions. Viewers can read but not edit. Only admins manage. The seed assigns issues only to admins and members.
7. **Backlog rank.** Stored as text. The seed writes fixed-width hex ranks. The lexorank algorithm itself is not in this step.
8. **Workflow.** Five states per project in the seed, with categories (`unstarted`, `started`, `done`, `cancelled`). Creating a project does not yet insert the default workflow; that comes with the create route.
9. **Sprints.** At most one active sprint per project (partial unique index).
10. **Activity.** Fixed kinds (`created`, `field_changed`, `comment_added`, `link_added`, `link_removed`), with `old_value` and `new_value` as jsonb and a nullable actor for system events.
11. **Fixture.** Each project takes a team list. Its lead is the lead of its first team, and members are drawn from those teams in id order. The CEO is an admin of all six projects and the test account is a member of EMB and OPS. Every eleventh issue is an epic, and subtasks point at the epic of their block. Summaries combine a subject and a detail from fixed lists.
12. **Tests.** The seed idempotency test is in `backend/db/src/lib.rs`. It runs when `DATABASE_URL` is set and skips otherwise. It needed `tokio` and `tokio-postgres` as dev-dependencies of `rimelight_backend_db`, which are already in the lock.
13. **Dev server not changed.** `backend/api`'s dev server does not apply 0006 yet. Until a later step does, apply the migration and seed with psql.

##### Checks

- `cargo test -p rimelight_policy`: 6 passed. `cargo test -p rimelight_backend_db` with `DATABASE_URL` set: 5 passed, the idempotency test included and not skipped.
- Applied to `rimelight_space` with `psql -v ON_ERROR_STOP=1 -1`: migration, then seed (`NOTICE: fixture projects seeded: 6 projects, 340 issues`). Second seed run in a scratch schema is a no-op.
- Counts after the apply: projects 6, project_members 118, workflow_states 30, sprints 18, issues 340, issue_links 63, comments 59, activity 976.
- Sanity queries, all 0: issues whose state belongs to another project, assignees not members, non-contiguous issue numbers, `next_issue_number` mismatch, leads who are not admins, projects without exactly one active sprint, subtasks whose parent is in another project.

##### Not verified

- No HTTP route, client or UI uses these tables yet.
- The dev server and the Worker do not apply 0006 (choice 13).
- The composite foreign key that stops removing a member with assigned issues has not been exercised by a test. Only the schema text is checked.
- Only the local database has the seed. Nothing is pushed or deployed.

#### 15.20 Decision 0027: Projects API, store and routes (S1.2a)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Wire types.** `rimelight_proto::projects`. Timestamps are unix seconds (`i64`), dates `YYYY-MM-DD`, ids UUID text. Issue keys (`EMB-12`) address issues in URLs. `Page<T>` is re-exported from `people`. `serde_json` becomes a normal dependency of `rimelight_proto` (activity values).
2. **Store returns wire types.** `ProjectStore` returns `rimelight_proto` types, so the Postgres and memory stores build replies the same way. The memory store (test only) takes display names from `add_user`, as Postgres takes them from `users`.
3. **Access.** Non-members get 404 on a project and its issues. Viewers get 403 on writes. Global admins override membership. Creating a project needs the global admin role. Managing details, members and the workflow needs project admin. Issues, comments, links, sprints and backlog ranks need membership (member or admin). A comment is edited or deleted by its author (with edit rights) or by a project or global admin.
4. **Member removal.** A removed member is a soft delete of the membership row, so closed issues keep a reference to them. Removal with open assigned issues is 409 `member_has_open_issues`.
5. **Workflow.** Creating a project inserts the five default states and makes the creator project admin. Deleting a state renames it to free the name (the unique key covers deleted rows). Deletion is refused with 409 when issues use it (`state_in_use`) or it is the last unstarted state (`last_unstarted`). Changing the last unstarted state's category gets the same 409.
6. **Issues.** Numbers come from `projects.next_issue_number` in the same transaction. A new issue takes the default state (first unstarted by position) when none is given. Subtasks use `parent_id`; a cycle or a parent outside the project is 422. `resolved_at` is set when the state enters `done` or `cancelled` and cleared when it leaves.
7. **Activity.** A PATCH writes one `field_changed` row per changed field, with the field name, old and new JSON values. Comment edits and deletes write `field_changed` with field `comment`. Links write `link_added` or `link_removed` on both issues, with `{kind, other}`. `null` values are stored as no value.
8. **Links.** `POST /v1/issues/{key}/links` takes `{target, kind}`. `DELETE /v1/issues/{key}/links/{link_id}` (the brief did not give a shape). A target the caller cannot see is 422, the same as a missing one. A removed link can be added again.
9. **Lists.** Issue lists and the backlog page with an offset cursor: `cursor` is the next offset as text, `next_cursor` is absent on the last page. Filters: `state` (id), `assignee` (id), `type`, `priority`, `sprint` (id or `none` for the backlog), `parent` (issue key). Sort: `number` (default), `-number`, `rank`, `priority`, `updated`. A full query language is the next step.
10. **Sprints.** Creating or activating a second active sprint is 409 `active_sprint_exists`. Closing may move unfinished issues with `move_unfinished_to`: a sprint id, or `backlog`. It is accepted only with `state: closed`. Sprint routes need membership, not project admin (the brief did not say).
11. **Backlog ranks.** Hex fractions, as the seed writes them (eight digits). `backlog_rank::between` returns a midpoint and adds digits when the gap is too small. Trailing zeros are dropped, and text order matches numeric order, so SQL sorts with `COLLATE "C"`. `POST /v1/projects/{key}/backlog/rank` takes `{issue, after?, before?}`. `after` puts the issue directly after that issue, `before` directly before it, neither moves it to the top. Out-of-order neighbours are 409 `rank_conflict`.
12. **Transactions.** `PgStore` holds one connection and its methods take `&self`, so multi-table writes run as `BEGIN` ... `COMMIT`, with `ROLLBACK` on error.
13. **Timestamps.** Postgres writes the `now` the service passes in, not the database clock, so tests control time.
14. **Dev server.** Applies 0006 with `ensure_table` (checks `to_regclass('projects')`), and opens three more connections for the project service. It runs no seed. The Worker build does not include the project service.
15. **Dependencies.** `rimelight_backend_api` depends on `rimelight_policy`. `Cargo.lock` changes for those two dependency lists only.
16. **Round trip test.** Without `DATABASE_URL` it prints SKIP and passes. With `DATABASE_URL` set and the server unreachable it fails (the people tests skip instead; this one does not hide a broken database).

##### Checks

- `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto -p rimelight_policy`: API 149 passed (including 24 project service tests on the memory store, 3 Postgres project tests, and the backlog rank tests), proto 13 passed, policy 6 passed.
- The Postgres round trip ran the service end to end in a scratch schema, with migrations 0001, 0003, 0005 and 0006 and two users, and dropped the schema. It was not skipped.
- `cargo fmt --all -- --check` clean. `cargo clippy -p rimelight_backend_api -p rimelight_proto --all-targets -- -D warnings` clean.

##### Not verified

- No server was started. No HTTP request was made to the dev server. The Worker build (wasm32) was not compiled.
- The fixture seed (0006) was not loaded into the scratch schema, so the seeded ranks (eight digit hex) were not read back by the service. Ranks written by the service were checked only on the empty scratch data.
- Concurrency: a PATCH writes the whole mutable row from the value it read. Two concurrent edits to one issue can lose one of them (last write wins). No version check exists yet.
- The owner has not reviewed the routes, the error codes or the choices above.

#### 15.21 Decision 0028: Issue search query language (S1.2b)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Grammar

```
query   := (clause | sort)*
clause  := ['-' | 'not'] (field ':' values | '#' tag | phrase | word)
values  := item (',' item)*           comma is OR within one field
item    := value ['..' value]         range, for dates and estimates
value   := phrase | word+             unquoted values may hold spaces: `In Progress`
sort    := 'sort' 'by' ':' key (',' key)*
key     := sortfield ['asc' | 'desc']
```

Fields: `project`, `state` (a name, or `#Unresolved` / `#Resolved`), `assignee`, `reporter`, `type`, `priority`, `sprint` (a name, or `current`), `tag` (or `#tag`), `parent` (an issue key), `created`, `updated`, `resolved` (`YYYY-MM-DD`, `today`, `-Nd`, ranges), `estimate` (`5`, `>3`, `<=2`, `2 .. 8`), `has` (`assignee`, `estimate`, `sprint`, `subtasks`, `links`). A bare word or phrase is full text. Clauses are joined with AND. Sort fields: `priority`, `updated`, `created`, `resolved`, `estimate`, `state`, `key` (or `number`).

##### Choices

1. **Parser location.** `rimelight_proto::query` (no I/O). `ParseError` carries a span: 0-based character offsets, end exclusive. The message names 1-based columns, e.g. `... (columns 1-6)`. Unknown fields get a "Did you mean" when one is within two edits.
2. **Error body.** `400` with `{"error":"invalid_query","message","span":{"start","end"}}`. This extends the `ApiError` shape with `span`; the existing `error` and `message` fields are unchanged.
3. **Name resolution.** Unknown projects, states, sprints and people are 400 with the span of that value. Only names in the caller's scope count, so a project the caller cannot see is reported exactly like one that does not exist.
4. **Full text.** Case-insensitive substring match (`ILIKE`, with `%`, `_` and `\` escaped) on summary or description. Each bare word is its own AND term. Chosen over `to_tsvector` because it matches the memory store exactly, needs no stored column, and handles partial words. The costs: no stemming, no ranking, and a leading-wildcard scan with no index. Revisit if lists get slow.
5. **Sorting.** Default direction is ascending. For `priority`, `desc` puts critical first and ascending puts low first. A missing resolved date or estimate sorts last in either direction. Ties break on project key, then number, in both backends.
6. **Dates.** UTC calendar days. `today` is the UTC day of the request. `-7d` is the day seven days before today. A range is inclusive at both ends.
7. **Negation.** `-` before a clause and `not` both negate it. A negated test treats a missing value as no match, so `-estimate: >5` keeps unestimated issues.
8. **Assignee and reporter.** `me` is the caller. `Unassigned` is valid for assignee only. Otherwise a value matches a member's email (case-insensitive) or display name (case-insensitive, whole name). A name shared by several members matches all of them. Postgres reads email from `users`. The memory store knows emails only through the test helper `add_email`.
9. **Sprint.** `sprint: current` is the active sprint of each project in scope.
10. **Scope.** `GET /v1/issues/search` covers the caller's projects (members, any role), and every project for a global admin. `GET /v1/projects/{key}/issues?q=` is scoped to that project. A `project:` naming another project is a 400 there. On the scoped route `q` on its own: sending `q` with any of `state`, `assignee`, `type`, `priority`, `sprint`, `parent` or `sort` is a 400 `invalid_request`.
11. **Paging.** The same offset cursor as S1.2a: `cursor` is the next offset as text, `limit` is 1 to 100 with the same defaults, `next_cursor` is absent on the last page.
12. **SQL.** Values are bound, never spliced. Each leaf adds its own `$n`. Negation is `NOT COALESCE(x, FALSE)`, so NULL counts as no match before the NOT. Sort keys come from a fixed list, so nothing user-supplied reaches `ORDER BY`.

##### Checks

- `DATABASE_URL=postgres://postgres:postgres@localhost:5432/rimelight_space cargo test -p rimelight_backend_api -p rimelight_proto`: API 173 passed (the Postgres search scenario ran in a scratch schema, not skipped: it seeds two projects, six issues, memberships and one finished issue, and runs 30 search requests including error cases), proto 37 passed.
- `cargo fmt --all -- --check` clean. `cargo clippy -p rimelight_backend_api -p rimelight_proto --all-targets -- -D warnings` clean.

##### Not verified

- No live server, no HTTP request to the dev server, no UI.
- The wasm32 Worker build was not compiled. `query.rs` and `search.rs` are not gated to native, so the Worker build now includes them.
- The client's use of `rimelight_proto::query` for highlighting is not built or tested.
- Performance of the `ILIKE` scan and the `EXTRACT(EPOCH ...)` date predicates was not measured on the 340-issue seed. The seed was not loaded into the search test schema.
- Concurrency and version checks are unchanged from S1.2a.
- The owner has not reviewed the grammar, the priority sort direction, or the `-Nd` and `today` day semantics.

#### 15.22 Decision 0029: Client projects module and the S1.3 split (S1.3)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Client module.** `rimelight_client::projects`. One typed method per endpoint the brief names, with the same token and path-segment checks as `people`. Keys are checked before a request: project keys are 2 to 10 alphanumerics, issue keys contain one dash, ids are hex and dashes. A bad value is a `Protocol` error with no request sent.
2. **Query errors.** A 400 whose body has `error: "invalid_query"` maps to a new variant `ClientError::InvalidQuery { message, start, end }`, so the UI can underline the span. Other 400s stay `ClientError::Api`. The variant is additive; `Display` covers it.
3. **Absent PATCH fields.** `UpdateIssue` now skips absent fields when it is serialized. Before, `UpdateIssue::default()` wrote every field as `null`, and the server reads `estimate_points: null` as "clear", so a PATCH of one field would have wiped the estimate. Covered by a proto test and a client test. The other `Update*` bodies (`UpdateProject`, `UpdateState`, `UpdateSprint`) have the same shape but are not sent by the client yet; they should get the same attribute when their client methods are added.
4. **Shape of the lists.** The server's reply shapes are used as they are: projects, members, workflow, comments, activity and sprints are plain arrays, issue lists are `Page<IssueSummary>`. The client does not wrap them.
5. **Split of S1.3.** This commit is the client layer only. The Slint work (brief items 2 to 5: the project picker, the issues list with its query bar, chips and sort, the issue detail with its edits, subtasks, links, comments and activity) and its UI tests are not in it. The S1.3 row stays in progress until that part is built and the owner has seen it running.

##### Checks

- `cargo test -p rimelight_client -p rimelight_proto`: client 67 passed (11 of them in `projects`), proto 37 passed.
- `cargo clippy -p rimelight_client -p rimelight_proto --all-targets -- -D warnings` clean. `cargo fmt --all -- --check` clean.
- `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean.

##### Not verified

- No live server, no HTTP request to the dev server. The client was tested with a scripted transport only, so the real JSON of each reply is taken from the server's serializers and not checked against a running backend.
- The desktop build (`cargo build -p rimelight_desktop`) and the UI test binary were not run in this step, since no UI changed.
- The client does not send the `q` filter together with the structured filters; the server rejects that combination with 400 `invalid_request`, which the client passes on as `ClientError::Api`.

#### 15.23 Decision 0030: Projects picker and the issue list (S1.3, part 2)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Project list from the server.** `slint_ui/projects.rs` keeps the list `GET /v1/projects` returns (key, name), set by `set_list`. The mock list is gone; `fixture()` is kept for the headless captures and the tests only. The list loads when the person signs in (`projects-refresh`, from the session driver). The current key starts on `DEFAULT_PROJECT_KEY` and moves to the first project when the list loads and does not have it.
2. **Issue list page.** `pages/projects/issues.rs` and `ui/pages/projects/issues.slint` at `/p/:key/issues`, on the page data pattern. The query is sent as `q` on `GET /v1/projects/{key}/issues`: Enter at once, a change after 400 ms. A refused query (`InvalidQuery`) puts its message under the bar with the span split out and marked; the list shows its empty state and keeps no rows.
3. **Chips and sort.** A chip toggles its whole clause in the query text (`assignee: me`, `state: #Unresolved`, `sprint: current`). A sortable header (Key, State, Priority, Updated) sets `sort by: <field> asc`, then `desc`, then `asc` again. A sort clause is found by its keys, so clauses after it are kept.
4. **New issue.** Shown only for a role of `admin` or `member` from `GET /v1/projects/{key}`. A viewer, or a person with no role (a global admin who is not a member), sees no button. The form has summary, type and priority (the proto lists). The created issue opens at `/p/:key/issues/:key`.
5. **Row and detail route.** A row opens `/p/:key/issues/:id`, with the issue key as `:id`. The detail page is not built; that route still shows the generic placeholder.
6. **Projects sidebar.** Its rows and header follow the list when the list loads while the Projects area is shown.

##### Checks

- `cargo test -p rimelight_ui --test ui issues`: 7 passed (list states, refused query span, chips, sort, role gate, create then open, row open).
- `cargo test -p rimelight_ui --test ui project`: 3 passed (project tree and switcher, now loading the fixture list).
- `cargo test -p rimelight_ui --test ui` (the whole binary, since the project list is shared): 139 passed.
- `cargo test -p rimelight_ui --lib`: 227 passed.
- `cargo build -p rimelight_desktop`: clean. `cargo check -p rimelight_web --target wasm32-unknown-unknown`: clean.
- `rustfmt --check` on the changed Rust files: clean.

##### Not verified

- No window was opened and no screenshot taken, so the layout, the marked span, the chip and header look, and the phone width (the seven columns are fixed-width and may clip on a narrow screen) are not checked.
- The POST that creates an issue is covered by the client test `create_issue_posts_the_body_to_the_project`, not by a UI test: test windows have no session.
- No live server. The `invalid_query` span, the role response and the list JSON are taken from the server's serializers.
- The rule for a global admin with no membership (no button) is a choice, not something the owner has said.

#### 15.24 Decision 0031: Issue detail page (S1.3c)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Page and route.** `pages/projects/issue.rs` and `ui/pages/projects/issue.slint` at `/p/:key/issues/:id`, on the page data pattern. A load reads the issue, its project (role, workflow, members), the project's sprints, the comments and the activity. The route is an issue route when `:id` is a plain key; the tab's placeholder no longer shows for it.
2. **One write per change.** Each header field sends one `PATCH /v1/issues/{key}` with only that field set. A select sends at once; the estimate and due date send on Enter. An empty assignee, sprint or due date sends an empty string, which the server reads as "clear". An empty estimate sends `null` ("clear"). A bad value (not a number, not a listed choice, empty summary) is refused under the page with no request. After a write succeeds, the issue, comments and activity are loaded again.
3. **Who may do what.** A member or an admin edits, adds subtasks and links, and comments. A project admin (`my_role` `admin`) also deletes the issue, and edits or deletes any comment. A member edits and deletes only their own comments. A viewer, and a person with no role (a global admin who is not a member, the same rule as decision 0030), sees the page read-only and gets no write sent. The check runs before a write is recorded, so a hidden control cannot be used from a callback.
4. **Delete.** "Delete issue" shows an inline confirm step that names the issue ("cannot be undone"). Only "Delete" in that step sends the request. A success goes back to the project's issue list.
5. **Comments and activity.** The Comments tab lists the author's initials and name, the relative time ("edited" when the text changed) and the body. The Activity tab lists one sentence per entry: `Ana Silva changed State from Open to In Progress · 2 h ago`. Ids in the activity are shown as the names known from the project, the sprints and the members. A comment's text is not shown in the log.
6. **Links.** Links come grouped by the server's own label (`blocks`, `is blocked by`, and so on), the group label on its first row, server order kept inside a group. Adding takes an issue key (upper-cased and checked) and a kind from the server's list.
7. **Subtasks.** A subtask is created with only its summary and this issue as the parent (the server gives it the default type and state).
8. **Dependency.** `serde_json` is now a direct dependency of `rimelight_ui`, to read the activity values. It is already in the workspace through `rimelight_proto`.

##### Checks

- `cargo test -p rimelight_ui --test ui issue`: 17 passed (10 new in `issue`, 7 in `issues`).
- `cargo test -p rimelight_ui --test ui`: 148 passed. `cargo test -p rimelight_ui --lib`: 234 passed.
- `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown`: clean.
- `cargo clippy -p rimelight_ui --all-targets -- -D warnings` and `cargo fmt --all -- --check`: clean.

##### Not verified

- No window was opened and no screenshot taken: the layout, the header's column widths, the tabs, and the phone width are not checked.
- No live server. The activity values (state, assignee and sprint ids, and the field names such as `state_id`) are taken from the server's writer, not from a stored row.
- The estimate and due date fields bind their text one way, so after a save the server changes, the field shows what was typed until the page is opened again.
- A write's result is not used: the page loads the issue again after each one. A slow second load can show the first load's data for a moment. No test covers that timing.
- The rule that an admin who is not a project member is read-only is a choice (decision 0030), not something the owner has said.

#### 15.25 Decision 0032: Board, backlog and sprints (S1.4)

**Status:** proposed (agent choices, 2026-11-15, the owner away). The owner reviews them.

##### Choices

1. **Pages and routes.** The board is `/p/:key/board`, the backlog `/p/:key/backlog` and the sprints page `/p/:key/sprints` (the brief's "sprints route if one exists"). Sprint create, start and close live on the sprints page. The backlog shows the planned and active sprints as groups above its list. The shared request state (`Load<T>`), the write enum and the list helpers are in `pages/projects/agile.rs`.
2. **Whole lists.** The board and the backlog read every page of their list (up to 50 pages of 100). This lets the board filter by sprint and by open state in Rust without a request, and lets a rank use the real neighbours.
3. **Query bar and sprint selector.** The board sends its query bar as `q` alone (decision 0028 refuses `q` with structured filters). The sprint selector never sends a filter: it filters the list it has by `sprint_id`. "All open" keeps the open states, and the default is the active sprint, or All open when there is none.
4. **Moving a card.** Each card has a "Move to" select of the workflow states, which sends one PATCH of `state_id`. Drag and drop was not built. The select is the cleaner first step, and the brief allowed it.
5. **Reordering the backlog.** Each row has Up and Down buttons, not drag. A move posts the rank with `after` (the row that will sit above the issue) and `before` (the row below it): moving up puts the issue after the row two above and before the row above, and moving down puts it after the row below and before the row two below. The top row cannot move up, and the bottom row cannot move down. Nothing is sent for those.
6. **Adding to a sprint.** A backlog row's select lists the backlog and the planned and active sprints. `none` sends an empty `sprint_id`, which clears the sprint, as in decision 0031. A closed sprint is not offered.
7. **Closing a sprint.** The confirm step is an inline panel under the sprint, not a `Dialog`. `Dialog` is a PopupWindow, and it was not used in this window's pages. The target starts on the planned sprint with the earliest start date, or on the backlog. Only planned sprints and the backlog are offered. The unfinished issues move to the target and the done ones stay in the closed sprint, as the server does it.
8. **Who may do what.** A viewer, and a person with no role (a global admin who is not a member), sees the three pages read-only, as in decisions 0030 and 0031. The check runs before a write is recorded, so a hidden control cannot be used from a callback. Only one write runs at a time on each page.
9. **Client.** `backlog`, `rank_backlog`, `create_sprint` and `update_sprint` are added with fake-transport tests. `UpdateSprint` skips its absent fields when it is serialized, the same fix as `UpdateIssue` in decision 0029.
10. **Known limit.** The board's columns share the width, with a 220 px minimum. `ScrollArea` scrolls vertically only, so when the window is narrower than the columns' minimum widths the right-hand columns clip rather than scroll. A horizontal scroll needs a change to a shared component, and was left for the owner to decide.

##### Checks

- `cargo test -p rimelight_ui --test ui board`, `backlog` and `sprints`: 13 new tests pass, and the whole binary passes: 161 passed, 0 failed.
- `cargo test -p rimelight_ui --lib`: 250 passed (5 new in `agile`).
- `cargo test -p rimelight_client -p rimelight_proto`: client 70 passed (3 new), proto 38 passed (1 new, the `UpdateSprint` absent-field test).
- `cargo clippy -p rimelight_proto -p rimelight_client -p rimelight_ui --all-targets -- -D warnings` clean. `cargo fmt --all -- --check` clean.
- `cargo build -p rimelight_desktop` and `cargo check -p rimelight_web --target wasm32-unknown-unknown` clean.

##### Not verified

- No window was opened and no screenshot taken. The layout, the card and column look, the phone width (item 10), and the close panel's position are not checked.
- No live server. The response shapes are taken from the server's serializers and handlers, not from a running backend. The 409 test uses a scripted reply with the server's error text.
- The board reads at most 50 pages of 100 issues (5,000). A project larger than that is cut short with no warning.
- No drag and drop, anywhere (items 4 and 5).
- The sprint dates are sent as typed. The form checks only the `YYYY-MM-DD` shape, and the server checks the rest.

#### 15.26 Decision 0014 addendum: identity ownership (owner, 2026-11-15)

**Status:** accepted by the owner.

- Auth0 holds the login only (credentials, MFA, `app_metadata.user_type`). Rimelight Space holds the master employee record. rimelight.com holds player profiles only.
- Auth0 tenant changes made 2026-11-15 through the Auth0 MCP: Space callback and logout URLs on `rimelight.com` (local only); `rimelight-claims` v2 requires MFA for employees; `Add User Roles & Classification` v2 no longer sets `user_type`.
- Follow-ups, not scheduled: when Space creates or removes an employee it sets or clears `user_type` in Auth0 through the Management API; rimelight.com stops keeping its own `employees` table and trusts the token or the Space API; rimelight.com's game API (`/api/v1/*`) answers 403 to `employee` tokens (owner to decide whether staff may play on work accounts).

#### 15.27 Decision 0033: The Worker is the only backend runtime (owner, 2026-11-15)

Status: accepted (owner chose option B on 2026-11-15).

- The backend runs only as a `workers-rs` Worker, in production and locally (`cf dev` on `localhost:8787`). The native `dev_server` binary is removed once the Worker serves every route it serves today.
- R2 is reached through a Worker bucket binding to `rimelight-dot-com`; no R2 API token. Local development uses a remote binding to the real bucket (unverified for Rust Workers; test first).
- Postgres is reached through Hyperdrive and Worker sockets (unverified with `tokio-postgres`; test first).
- Accepted cost: slower local rebuilds through wasm, log-only debugging.
- This supersedes the native dev server part of decision 0015. It is its own slice after S1.5.

#### 15.28 Decision 0034: Slint debug info for the element-geometry UI test (UI.F1)

Status: decided (2026-11-15, owner picked (c)). The four tests that used `ElementHandle` (connection_lost x3, departments page body) now read `AppShell` test properties (`test-lost-overlay-drawn`, `test-dept-body-height`, `status-connection-text`, the quick action geometry); the overlay click test clicks the quick action button and uses the quick menu as its sentinel. No build sets `SLINT_EMIT_DEBUG_INFO`. `cargo test -p rimelight_ui --test ui`: 172 passed, 0 failed.

- `ElementHandle` (the testing backend's element search and size) only works when the Slint compiler emits debug info. The page body geometry test (`departments::departments_page_body_fills_the_group`) and the chip test need it.
- The variable is `SLINT_EMIT_DEBUG_INFO=1`. It is not set in `.cargo/config.toml` or `crates/rimelight_ui_slint/build.rs`, so a plain `cargo test -p rimelight_ui --test ui` fails those two tests with "requires the presence of debug info".
- Options for the owner: (a) set `SLINT_EMIT_DEBUG_INFO=1` in `.cargo/config.toml` `[env]` (every build pays for debug info, the cost the owner noted for `rimelight_ui` build time); (b) set it only for the test command in the gate; (c) keep the geometry check out of `ElementHandle` and read sizes from an out property.

#### 15.29 Decision 0035: Online only, no offline mode (SA.8)

**Status:** accepted (owner, 2026-11-15).

- Rimelight Space works only while online. There is no offline mode and no change sync.
- If the server cannot be reached at start-up, the shell does not open. The window shows "Can't reach Rimelight Space" with Retry, and retries every 10 s. The stored session is kept.
- While signed in and the connection is lost, the banner says "Connection lost. Reconnecting…", the server is checked every 5 s, and the shell is read-only: Buttons that set `requires-online: true` are disabled through the `Connection` global. When the connection returns, the banner clears and the open pages reload.
- Known limit: the gate is on Buttons only. Text fields stay editable, and a keyboard Enter that reaches a page callback is not blocked.

Amended 2026-11-15 (owner): a full-window overlay blocks the app while reconnecting; per-control gating removed (SA.9). The overlay covers the whole shell and takes all pointer and key input; the `requires-online` property, the `live` logic and the `Connection` global are gone, and the banner is removed. The "Known limit" above no longer applies.

#### 15.30 Decision 0036: Positions are seats (owner, 2026-11-15)

Status: accepted (owner chose each option on 2026-11-15).

- Model: a position is a seat in one team, holding at most one person, or none. Today's shared job titles become job profiles that seats pick from.
- Hierarchy: seats report to seats. A person's manager is derived from the seat above theirs. `users.manager_id` is kept in sync by the API for existing readers (proposed detail, review in SP.5).
- Budget: min and max salary with currency per seat; a yearly budget cap per team showing used and remaining. Money is visible to admin and HR only.
- States: Planned (on the plan, not approved), Open (approved, hiring), Filled (has a person), Closed (removed from the plan, kept for history).
- Out of scope: hiring pipeline stages, target dates, ATS integration.

#### 15.31 Decision 0037: Build time (BT)

Status: proposed (2026-11-15). BT.1 and BT.2 approved by the owner ("Do 1–4"); BT.3 and BT.4 need the choices below.

- BT.1: the hook no longer sets `CARGO_PROFILE_TEST_DEBUG=0` / `CARGO_PROFILE_DEV_DEBUG=0`. Those differed from `[profile.dev] debug = "line-tables-only"`, so the hook and every other build (RustRover, `cargo run`, the gate) kept separate copies of the 43 MB Slint crate and re-checked it when they alternated.
- BT.2: `[profile.dev.build-override] opt-level = 3`, so slint-build and the `slint!` macro run optimised.
- BT.3 (open): the Slint structs shared by `AppShell` and `GalleryWindow` (`RailItem`, `SidebarItem`, `PaletteRow`, ...) are filled by shared Rust (`shell.rs`, `palette.rs`). A second Slint crate makes them distinct Rust types, so that fill code must become generic or be duplicated. Story components are at least 11.7 of the 42.9 MB of generated code (by struct name; anonymous components not counted).
- BT.4 (owner: do it, 2026-11-15): the 35 test `slint!` blocks moved to `crates/rimelight_ui/tests/ui/markup/*.slint`, compiled once by the new `rimelight_ui_test_slint` crate (one module per file, parallel threads, imports resolve against the rimelight_ui crate root). BT.3 deferred by the owner.

#### 15.32 Decision 0038: One-command dev (P0.49)

Status: done in code, not yet run (2026-11-15). Owner asked for it ("yes, implement it").

- `cargo xtask dev` builds `dev_server` (`cargo build -p rimelight_backend_api --bin dev_server`), starts `target/debug/dev_server` in the background, waits until `127.0.0.1:8787` accepts connections (stops with an error if the server exits first or the port is already in use), then runs `cargo run -p rimelight_desktop` in the foreground. The server is stopped when the app exits.
- The server runs as the built binary, not through `cargo run`, so stopping it stops the server itself.
- Not checked: running it end to end (agents do not open app windows). The owner runs `cargo xtask dev` to confirm the desktop app starts against the local backend. `--android` and `--web` remain unimplemented.
- Checked: `cargo fmt -p xtask --check` and `cargo clippy -p xtask --all-targets -- -D warnings` pass.

#### 15.33 Decision 0039: Space uses the website's Hyperdrive (owner, 2026-11-16)

Status: proposed, owner choice recorded (2026-11-16). Conflicts with the "Relation to rimelight.com" row in section 1 ("Separate database and backend, shared sign-in").

- The owner says the Hyperdrive config `5889fcb4d3034e909c0f060f18975597` (already used by the website) points at the same database. `backend/worker/wrangler.toml` binds it as `DB`.
- Risk: the Space migrations (`backend/db/migrations/`) create `users` and `sessions` tables, and the seed inserts users. If the website's database already has tables with those names, migrations can fail or mix data. The Worker does not run migrations, so this must be checked before the first deploy.
- Not verified: the table names in the website's database, and whether the owner intends to keep the two data sets separate. Either a separate database (a new Hyperdrive config) or a separate schema inside this database would keep them apart.

#### 15.34 Decision 0040: Shell offline state and profile and Home changes (owner, 2026-11-16)

Status: decided by the owner, 2026-11-16.

- Offline: the app shows only the full-window "Connection lost" overlay (SA.9, decision 0034). The offline banner is removed: no banner component, no banner icons (`wifi-off`, `refresh-cw`), no banner contrast pair. The status bar shows no connection word or dot. `ConnectionState` stays, in `ui/components/connection-state.slint`, because the shell reads it for the overlay.
- Profile menu: the presence mark sits on the avatar, as in the top bar. The header's connection line is removed. The Status picker is unchanged.
- Profile menu: "Give Feedback" sits under About, above Report Issue. It opens a placeholder shell screen, as Report Issue does. The phone More drawer does not have it yet.
- Profile page: Organization is merged into Work, which now also holds the reporting line and direct reports. Section order is Work, Personal, About. Work is shown while editing too.
- Home: new section "Performance" above "Time & Pay", with `/home/performance/metrics` (Metrics, dashboard template) and `/home/performance/feedbacks` (Feedbacks, list template). Both are placeholders until built.
- Not done: the generated IA doc (`cargo xtask docs ia`) do not list the new routes yet.

#### 15.35 Decision 0041: The Space API runs on the website Worker; the Rust backend is removed (owner, 2026-11-16)

Status: accepted (owner, 2026-11-16).

- The Space API (`/v1/...` on rimelight.com) is served by the website Worker, `rimelight.com/apps/rimelight.com` (repo `Rimelight-Entertainment/rimelight`). Its code is `src/space/`: session, sign-in, people (reads and admin), projects (reads and writes), avatars and attachments. It uses the website's postgres.js client through Hyperdrive, one connection per request.
- The site middleware is skipped for `/v1` (the Space routes are registered before it in `src/fetch.ts`). The site's rate limits and redirects do not see Space traffic.
- Removed in this repo: `backend/api` (services, native dev server, tests) and `backend/worker` (the Rust Cloudflare Worker, decision 0033). `backend/db` stays: its migrations and seeds are the schema source.
- `cargo xtask dev` runs the desktop app. It uses `RIMELIGHT_API_URL` when set, and `https://rimelight.com` otherwise. The desktop, web and Android defaults point at rimelight.com.
- Reason: the Rust Worker's Postgres connection (tokio-postgres over a raw Worker socket) dropped connections on the live path. postgres.js on Hyperdrive is the pattern the website already runs.
- Consequences: the Rust API tests are gone, so the Space API is tested by running the apps against rimelight.com and by the website's checks. The Auth0 callback for the old Worker (`rimelight-space-api.idantity.workers.dev`) is to be removed once sign-in is confirmed on rimelight.com. The old Worker is to be deleted in the Cloudflare dashboard.
- Not done: local development against a local database needs a decision (see the dev options); the attachments and avatars are ported but not yet covered by any automated test.

#### 15.36 Decision 0042: Amends 0036: positions are titles, seats are slots (owner, 2026-11-16)

Status: accepted (owner, 2026-11-16). Amends decision 0036.

- Position: a job title (Recruiter, Supervisor), defined once and reused across teams and departments. It has no team. It holds the description, level, and salary range (min and max, one currency).
- Seat: one slot in one team, holding one position, and at most one person. A person holds at most one seat. A seat has no title of its own. Title, salary, and level come from its position.
- Team: belongs to one department and has a yearly budget. Filled and open seats both count against it. Filled and open seats are both costed at their position's max salary (the budget check uses the max, as agreed). The API refuses a change that takes the team over budget.
- Hierarchy: seats form one tree. A seat's parent may be any seat, including one in another team or department. The tree is global from the start, so cross-team reporting needs no migration later. A person's manager is derived from the seat above theirs (as in 0036).
- Example: a team with a budget of 1000, a junior position at 200, and a senior position at 500. Five junior seats fill the budget, and so do two senior seats.
- Replaces in 0036: "a position is a seat in one team" and "today's shared job titles become job profiles that seats pick from". The job-profile name is dropped; the title is the position.

#### 15.37 Decision 0043: Every cargo command selects the whole workspace (owner, 2026-11-18)

Status: accepted (owner, 2026-11-18).

- Problem: cargo unifies dependency features only across the packages a command selects. The hook runs `cargo clippy -p <changed packages>`, `xtask gate test` runs `--workspace --exclude rimelight_ui` and then `-p rimelight_ui`, and agents run `cargo test -p rimelight_ui --test ui`. Checked 2026-11-18 with `cargo tree -e features`: these selections build `serde_core`, `tokio`, `futures-util`, `typenum` or `windows-sys` 0.61 (under winit and Slint) with different features, so cargo keeps one dependency build per selection. Each is paid in full the first time and after every `Cargo.lock`, toolchain or low-level change. Leaving the UI crate and the apps out once recompiled `slint`, `i-slint-backend-winit` and `rimelight_ui_slint` (6 min 31 s).
- Change: every cargo command selects `--workspace` and narrows with target flags and test filters, which do not change features. No new tool or dependency.
  - Pre-commit hook: `cargo clippy --workspace --all-targets -- -D warnings`. Measured warm after one `rimelight_ui` edit: 4 s, against 17 s for `-p rimelight_ui`.
  - `xtask gate test`: one `cargo test --workspace --lib --bins --tests -- --test-threads=1` plus `cargo test --workspace --doc`, replacing the two `-p`/`--exclude` steps. This leaves out the examples, as the gate does today. Measured warm build after one edit: 10 s.
  - Agent edit loop: `cargo test --workspace --test ui -- <filter>`. Measured warm build after one edit: 15 s, against 20 s for `-p rimelight_ui --test ui`.
- Trade-offs: the hook lints every crate, not only the changed ones; warm that costs seconds. All unit tests run with `--test-threads=1`, which the gate today applies only to `rimelight_ui`; not timed.
- Check after the change: hook, gate and agent commands run one after another rebuild no dependency.

#### 15.38 Decision 0044: Gallery on the Slint interpreter (BT.7)

Status: accepted (owner, 2026-11-18, "Accept and build"). Changes the S2 gallery design (decision 0008).

- Problem: `rimelight_ui_slint` compiles `AppShell`, `GalleryWindow` and `FoundationsWindow` into one 44.8 MB generated Rust file (416k lines). On a `.slint` edit rustc spends about 75% of the `ui` test build on that crate. Story components are at least 27% of the generated code (decision 0037). A second compiled crate (old BT.3) still builds on every `--workspace` command (decision 0043) and duplicates the shared structs (`RailItem`, `SidebarItem`, `PaletteRow`, ...) as distinct Rust types.
- Change:
  1. `rimelight_ui_slint` exports `AppShell` and the structs it needs. `GalleryWindow`, `FoundationsWindow` and every `*Story` component leave the compiled crate.
  2. `apps/gallery` loads `gallery.slint` at run time with `slint-interpreter` and reloads on file change. Story edits need no cargo build. Sample data goes in as interpreter values, so no shared Rust struct types are needed.
  3. The UI tests that use `GalleryWindow` or the harness (`gallery`, `profile`, `shell`, `tier3_b5`) move to `AppShell` or to test markup in `tests/ui/markup/` (decision 0037 pattern).
- Unverified, tested by step 1 (spike, outside the repo): the interpreter compiles our markup (font imports, generated tokens, std-widgets); a story renders headless with the testing backend and the software renderer; callbacks the Rust side wires today (palette search, select and menu navigation) can be set through the interpreter.
- Trade-off: `slint-interpreter` compiles once and is cached; it rebuilds only when `apps/gallery` changes.
- Step 1 result (2026-11-18, spike crate in the agent scratchpad, not in the repo): `slint-interpreter =1.18.1` with the testing backend and software renderer loads `ui/main.slint` with 0 errors (4.7 s, debug build) and creates `GalleryWindow` (90 ms) and `FoundationsWindow` (40 ms). All 80 story indices gave non-blank 1280x800 snapshots (23 s in total). The colours story was checked by eye: Noto Sans, tokens and toolbar render as in the app. `set_callback("palette-query-changed")` works. Not yet checked: filling the gallery's Rust-filled models (story list, sidebar rows) through interpreter values, and live reload. The spike's first build of the interpreter took 104 s, once.
- Done (2026-11-18): `ui/dev.slint` is the gallery entry; `main.slint` no longer exports `GalleryWindow` or `FoundationsWindow`. `apps/gallery` has `harness.rs` (interpreter build, capture, live reload), `nav.rs` (Select, Menu, palette and shell wiring over the `rimelight_ui` rules), `registry.rs` (moved from `rimelight_ui`), `values.rs`. `examples/slint_foundations.rs` is replaced by `rimelight_gallery --foundations`. `rimelight_ui_slint/build.rs` no longer watches all of `ui/`; slint-build reports the files `main.slint` imports, so story files do not rebuild the app crate. `apps/gallery` now depends on `slint`, `slint-interpreter`, `i-slint-backend-testing` and `image` besides `rimelight_ui` (PLAN 7.2 named only `rimelight_ui`). Not checked: live reload in an open window (agents do not open windows); the owner runs `cargo run --bin rimelight_gallery` and edits a story to confirm.
- Moved (2026-11-22): the gallery window, harness and registry now live in rimelight-rs as `rimelight_gallery` (`apps/gallery` there). Space's `apps/gallery` is a thin wrapper, package and binary `space_gallery`, with Space's stories in `crates/space_ui/ui/stories/` as the app stories and the whole-shell capture (`cargo xtask screenshot shell/<state>`). The Space gallery files (`dev.slint`, `gallery.slint`, `stories.slint`, `component-stories.slint`, `shell-stories.slint`, `foundations.slint`) and the old `harness.rs`, `nav.rs`, `registry.rs`, `values.rs` are deleted. Pin: `rimelight_gallery` at the rimelight-rs rev in the root `Cargo.toml`.

#### 15.39 Decision 0045: Canvases (CV)

Status: proposed (owner answers 2026-11-19; slice plan awaiting approval).

**Owner answers (2026-11-19)**

| Topic | Answer |
|---|---|
| Name | "Canvases" everywhere. Routes become `/home/canvases`, `/p/:key/canvases`, `/p/:key/canvases/:id` (replacing `/home/boards` and `/p/:key/whiteboards`). The kanban Board at `/p/:key/board` keeps its name |
| Co-editing | Real-time from day one |
| Sync model | Item ops ordered by the server (Excalidraw-style): every item has a version; last write wins per item and field. No CRDT library. Text inside one note is not merged character by character |
| Platforms | Desktop first (Windows built and owner-tested). Web and Android compile and open canvases; touch gestures and phone layout in a later slice. A13 (phone parity) stays the long-term target |
| Pop-out | PureRef-style separate window showing only the canvas. The OS window frame is always hidden. The app draws its own top bar with icon-only buttons (pin on top, opacity, click-through, close and the other pop-out toggles); a setting chooses whether this bar is always shown or appears when the pointer hovers the top edge of the window (owner, 2026-11-19, after the CV.0 spike). Dragging the bar or empty canvas moves the window |
| Click-through | System-wide hotkey Ctrl+Alt+Shift+M (fixed in code, not a setting; owner 2026-11-19) turns it off; the main window also has a toggle per pop-out |
| Convert | Move only: personal to project. Same canvas id, it leaves Home, its bytes move from the user's quota to the project's. Project to personal is not allowed |
| Personal sharing | Owner invites employees as viewer or editor |
| Project permissions | Project viewer: view. Member: create and edit. Project admin or the canvas creator: rename, delete, move. Global admin overrides |
| Tools in the first slice | Images, text notes and labels, shapes and arrows, freehand pen and eraser, frames and groups |
| PureRef extras | Crop and grayscale (non-destructive), GIF and WebP playback, eyedropper, paste or drop an image URL (fetched by the server) |
| Video | Planned slice CV-V, decision 0046 (owner, 2026-11-19), shared with other future video needs in the app |
| Limits | 50 MB per file; duplicates on one canvas stored once (SHA-256); quota 1 GB per user (personal canvases) and 10 GB per project, shown in the UI, enforced by the server (fixed in code; the admin override was removed, owner 2026-11-19); up to 1,000 items per canvas |
| Presence | Live named cursors, each person's selection outlined in their colour, avatars in the canvas top bar, follow a person's viewport |
| Import and export | Import a folder, export PNG (selection or whole canvas), save original images to a folder, canvas backup file (zip: JSON plus images) and restore from it |

**Design (agent proposal)**

1. **Data.** Tables (Drizzle migration in the website repo, as the 2026-11-17 Space migrations): `canvases` (id, title, exactly one of `owner_user_id` or `project_id`, `created_by`, timestamps, `deleted_at`, `seq` of the last applied op), `canvas_shares` (canvas, user, `viewer` or `editor`; personal canvases only), `canvas_items` (canvas, item id UUID, `kind`, `z` rank string, `version`, `props` jsonb, `deleted`, `updated_by`, `updated_at`), `canvas_assets` (canvas, id, `sha256` unique per canvas, mime, bytes, width, height, frame count, R2 keys), `storage_quotas` (scope `user` or `project`, id, `limit_bytes`; no row means the default). Usage is the sum of asset bytes over the scope's live canvases.
2. **R2 layout** (bucket `BLOB`): `space/canvases/{canvas_id}/assets/{sha256}.{ext}` for originals, `space/canvases/{canvas_id}/thumbs/{sha256}-{px}.webp` for preview sizes (256 and 1024 px long edge). Keys use the canvas id, so a move to a project changes no key.
3. **Items.** Kinds: `image`, `text` (note with fill, or bare label), `shape` (rectangle, ellipse, diamond, line, arrow; arrow ends may bind to items), `stroke` (pen points, simplified), `frame` (titled region; items with `frame_id` move with it). Shared props: x, y, w, h, rotation, flip x and y, opacity, locked, `group_id`, `frame_id`. Image props: asset id, crop rect, grayscale. Colours on text, shapes and strokes are named palette entries mapped per theme (an "ink" stroke is dark on light and light on dark); images are never recoloured. The canvas background follows the app theme.
4. **REST** (website `src/space/canvases*.ts`): list personal, shared-with-me and project canvases; create, rename, soft delete, restore, move to project; shares CRUD; snapshot (live items plus assets); asset upload (raw body, hash dedupe, 50 MB and quota checks), asset and thumbnail download; import from URL (https only, size and content-type checks); quota read; admin quota set.
5. **Real-time.** Durable Object `CanvasRoom`, one per canvas, hibernatable WebSockets. The client gets a one-minute ticket from `POST /v1/canvases/{id}/live-ticket` (a browser cannot set headers on a WebSocket) and connects to `/v1/canvases/{id}/live?ticket=`. JSON messages: `hello` (snapshot, seq, peers), `op` (client op id, item id, base version, field patch, create or delete), `ack`, `applied` (broadcast with the new seq), `reject` (stale or forbidden, with the current item), `presence` (cursor, selection, viewport; at most 20 per second). The room applies ops in arrival order and writes them to Postgres in batches (at most 1 s late, and on the last disconnect).
6. **Client.** `rimelight_client::canvases` (REST) and `rimelight_client::canvas_live` (native `tokio-tungstenite`, wasm `web-sys` WebSocket; reconnect with backoff, resend unacknowledged ops, rebase on `reject`).
7. **Pure logic crate** `crates/rimelight_canvas` (no Slint, unit tested): item model, camera transform, hit testing, selection, snapping, undo and redo as inverse ops (per user, local), crop math, and the arrange operations: optimal arrangement (clean-room bin packing), sort by name, date added, size, colour and random, normalise height, width and area, align, distribute, stack, z-order. A new crate departs from "components in `rimelight_ui`" (decision 0011); it holds no UI.
8. **UI.** Canvas page on the Canvas template: toolbar, infinite canvas drawn through a camera transform (Slint repeater, thumbnails chosen by zoom level), selection handles, rotation, context menu, properties panel, keyboard shortcuts. Lists in Home > Canvases and Project > Canvases with quota bars. Images decoded off the UI thread; disk cache of originals and thumbnails on desktop.
9. **Pop-out.** A second Slint window per popped-out canvas (several allowed), sharing the main window's live session.

**Steps** (one commit each; CV.1 to CV.3 are in the website repo and can run beside CV.5)

| Step | Content |
|---|---|
| CV.0 | Spikes for every unverified item below, outside the repo. Results recorded here before building on them |
| CV.1 | Schema and R2 layout, Drizzle migration (applied only when the owner says "apply it"); routes renamed to Canvases in `rimelight_nav` |
| CV.2 | REST API on the website Worker |
| CV.3 | `CanvasRoom` Durable Object, tickets, live protocol, batched persistence |
| CV.4 | `rimelight_client` REST and live session |
| CV.5 | `rimelight_canvas` pure crate: model, camera, hit test, undo, arrange |
| CV.6 | Canvas page: images and text, select, move, resize, rotate, flip, z-order, lock, zoom to fit, context menu |
| CV.7 | Image intake: file picker, OS drop, clipboard paste, URL paste, folder import, thumbnails, disk cache, GIF and WebP, crop, grayscale, opacity, eyedropper |
| CV.8 | Organising commands (the CV.5 operations) in the UI |
| CV.9 | Shapes, arrows with bindings, pen and eraser, frames and groups |
| CV.10 | Presence: cursors, selections, avatars, follow |
| CV.11 | Lists, sharing, move to project, quotas (lists and Admin) |
| CV.12 | Pop-out window: frameless or toolbar, pin, opacity, click-through and global hotkey |
| CV.13 | Export PNG, save images to folder, backup file and restore |
| CV.14 | One-time move of the owner's PureRef boards (owner, 2026-11-19): the `.pur` files in the owner's Proton Drive become project canvases in the existing project. Not a product feature: a one-off command (an `xtask` or a script run by the agent) that reads each `.pur` file, uploads its images through the CV.2 asset route, and creates the items with their positions, sizes, rotation, flips, crops and text notes through the CV.3 room or a bulk route. **Unverified:** the `.pur` format is undocumented; it is read for interoperability with the owner's own files, from the file bytes, tested on the owner's boards first. Anything the reader cannot map is listed in a report, not dropped silently. Needs from the owner when scheduled: the folder path of the synced `.pur` files and the target project key. Started 2026-11-19 (owner: the boards in Proton Drive `Rimelight Entertainment`, into project GT, Grand Tale): 10 boards in `Projects/Grand Tale/References/Images` (about 260 MB). Findings from read-only analysis (scripts in the agent scratchpad): format version strings "2.1" and "2.1.3"; Qt big-endian strings; each item record holds its name and a 3x3 transform (9 doubles, partly stored with bytes ≥ 0x80 UTF-8 encoded) that decodes for 296 of 298 items (Characters) and 284 of 286 (HUD); the source paths recorded in the boards no longer exist on disk; images are embedded, but most boards store them compressed in a PureRef-specific way (the 100 MB Characters board has 298 items and only one standard PNG/JPEG); only some boards embed plain PNGs. PureRef has no command-line export. No public description of the 2.x format exists (the FyorDev parser covers 1.10/1.11). Embedded PNGs are present (for example 111 in Characters) but their chunk data does not decode as stored (stated chunk lengths are shorter than the stored data and inflate fails), so the image bytes were taken from PureRef's own export instead: the owner ran Save › Export › All Images for each board into Downloads (2026-11-19). Layout from the .pur: the first valid transform record per item name (a record is accepted only when its 3x3 transform is affine with a last column of 0, 0, 1); PureRef positions items by their centre (shown by the crop QPainterPath around ±w/2, ±h/2); rotation and flips from the matrix; z order by record order. Checked on the boards: first-record-per-name and centred gives 5 to 9 % overlap, against about 20 % for the other readings. Done 2026-11-19 (one-off Node script in the agent scratchpad, not in the repo; temporary session for the owner's account, owner authorised, removed afterwards): 10 canvases in GT, 359 images and 359 items, 246 MB: Characters, Branding, & Artstyle 102 (64 placed from the layout, 38 exported files with no matching item in a row below), HUD 91 (90 + 1), Esc Settings 5, F1 Adventure & F4 Estate 15, F2 Collection 80, F3 Treasury 9, F5 Atlas 5, F7 Compendium 12, F8 Profile 8, F9 Social 32 (19 + 13). Server hiccups during the upload: one 500 (HUD, after 76 images; the canvas was resumed) and two 503s (Atlas, retried). Limits: crops are not applied (canvases have no crop yet, CV.7), an image placed twice on a PureRef board comes across once, PureRef text notes were not imported. Not yet seen by the owner. Follow-up 2026-11-19: the Grand Tale canvas list answered 503: Cloudflare's logs show "Worker exceeded CPU time limit" (11 ms CPU); the account has no Workers Paid subscription (Cloudflare Free Plan, R2 Paid, Teams Free Base), so each request gets 10 ms of CPU. Owner chose to fit the routes into 10 ms rather than upgrade: the Space session check and the canvas routes now use `getSql()` (website `b858239a`: plain postgres.js, no Drizzle wrapper, `fetch_types: false`); the list then answered 200 five times out of five. Successful requests do not log their CPU time, so the remaining headroom is not measured; heavier routes (large uploads, room flushes) may still hit the limit as data grows. The canvas list gained Rename, Archive and Delete (with a confirmation that says it cannot be undone) in a row menu for managers, and an Archived view with Unarchive |

**Unverified (tested in CV.0)**

- Slint 1.18.1 draws up to 1,000 image items under pan and zoom smoothly on Windows (which renderer the desktop build uses has not been checked).
- A second Slint window can be frameless, always on top (`always-on-top` on `Window`), translucent, and ignore the mouse (`set_cursor_hittest(false)` through `slint::winit_030::WinitWindowAccessor`).
- Files dropped from Explorer reach Slint 1.18.1 (winit `DroppedFile`); image clipboard reads work through `arboard`.
- `global-hotkey` registers a system-wide shortcut beside Slint's event loop on Windows.
- The website's `cf/config` supports a Durable Object binding with migrations, and the Durable Object reaches Postgres through Hyperdrive.
- WebSocket clients: `tokio-tungstenite` with rustls on desktop and Android, `web-sys` WebSocket on wasm.
- PNG export: rendering the canvas off screen (Slint software renderer, or composing with `image` and `tiny-skia` plus a text rasteriser).

**CV.0 result (2026-11-19).** Spike crate in the agent scratchpad, outside the repo (a `window_spike` binary and a `ws_spike` library), built in release. Owner ran the window spike; the agent ran the self-tests with the owner's permission.

- Canvas speed: about 33 to 39 frames/s while panning 2,000 rotated image items with femtovg (OpenGL); the owner reports panning and zooming as very responsive. Slint redraws only on change, so idle counts are 0 to 2. Textures were generated (up to 800x600); real photos and memory use are checked in CV.7.
- Pop-out: `no-frame`, `resize-border-width`, `always-on-top`, winit `drag_window`, `set_minimized`, `set_maximized`, `set_window_level`, `set_cursor_hittest(false)` (click-through), content opacity, and Win32 `SetLayeredWindowAttributes` (whole-window alpha) all work. Closing and reopening a pop-out works (`--popout-test`). The custom bar works; showing it on top-edge hover needs a larger hover zone and a hide delay (CV.12). Bar icons come from the app's icon set, not text glyphs.
- File drop from Explorer: works through `on_winit_window_event` (winit `DroppedFile`); Slint does not handle OS drops itself.
- Clipboard: `arboard` 3.7 reads bitmaps (Win+Shift+S, Ctrl+V in the window) and copied files (`get().file_list()`, needed when a PNG file is copied in Explorer). Paste tries image data first, then the file list.
- Global hotkey: `global-hotkey` 0.7 with Ctrl+Alt+T toggles click-through, polled from a 50 ms `slint::Timer`; the manager is created on the main thread before the event loop.
- Snapshot: `Window::take_snapshot()` saves a correct PNG of the window. Off-screen export at any size is still open (CV.13).
- WebSocket: `tokio-tungstenite` 0.30 (rustls, webpki roots) passed a loopback op and ack test; compiles for Android arm64 (`cargo ndk`). The `web-sys` WebSocket compiles for wasm32.
- Durable Objects: `@cloudflare/config` 1.0.0-rc.0 has `exports.durableObject({ storage: "sqlite" })`. Cloudflare's docs show a Durable Object reading its Hyperdrive binding from `env`; not yet run, tested in CV.3.
- Slint 1.18 deprecates `rotation-angle`; use `transform-rotation`.
- Not part of the spike: selecting and moving single items, text, shapes (CV.5, CV.6, CV.9).

**CV.2 choices (2026-11-19).**

- Thumbnails (256 and 1024 px WebP) are made by the desktop client, which decodes every image anyway, and uploaded with `PUT /v1/canvases/{id}/assets/{asset}/thumbs/{px}`. The website's `IMAGES` binding is not used (nothing in the site uses it yet). Until a thumbnail exists the client draws the original.
- Image type, size and GIF or WebP frame count are read from the file bytes by the server (`canvas-image.ts`), not taken from the request.
- Import from a link follows redirects by hand (at most 5) and checks every hop before requesting it (https only, port 443, no private or local addresses).
- Global admins have no access to someone else's personal canvas: personal canvases are private to the owner and the people they are shared with. On project canvases a global admin has full access.
- Archive and delete (owner, 2026-11-19): archiving keeps a canvas and its images (`archived_at`; `POST .../archive`, `POST .../unarchive`); an archived canvas still opens read-only and still counts against the quota. `DELETE /v1/canvases/{id}` deletes for good: the row (items, assets and shares by cascade) and every R2 object under `space/canvases/{id}/`, which frees the quota. Single assets are never deleted by themselves (a key is shared by identical files within a canvas).
- CV.3 (2026-11-19, website `4fea6f49`, deployed by Workers Builds): Durable Object `CanvasRoom` (`src/space/canvas-room.ts`, SQLite write-ahead, alarm flush to Postgres about 1 s later and when the last socket closes, Postgres clients closed after each use), rules in `canvas-room-logic.ts`, routes in `canvas-live.ts` (`POST /v1/canvases/{id}/live-ticket`, single-use 60 s ticket; `GET /v1/canvases/{id}/live?ticket=`), REST routes notify open rooms (title, archived, unarchived, deleted, reconnect after move or share changes). Gate: `vp test` 70 passed (room rules 27), `vp check` 0 errors, `tsc --noEmit` no errors in `src/space`. Live test on rimelight.com (2026-11-19, after fix `6278ed2d`: Hyperdrive returns timestamps as text in the Worker, so `canvasView` threw and `POST /v1/canvases` answered 500): 18 of 18 passed with a temporary session for the owner's account (owner authorised; removed afterwards) and two WebSocket clients: create canvas, tickets, hello and peers, join, create acked and applied, presence relayed, update acked and applied (null removes a prop), duplicate create rejected, reused ticket refused, flushed to Postgres (version 2, seq 2), REST snapshot, archive reaches the room, ops refused while archived, delete closes both sockets (4000), canvas row gone. The website's local dev server does not start, with or without these changes ("Denied ID /__uno.css?inline" while the Cloudflare plugin reads the Worker's exports), so local testing of the Worker is not possible until that is fixed.
- CV.4 (2026-11-19): `rimelight_proto::canvases` (REST shapes and the live messages), `rimelight_client::canvases` (every canvas route on `ApiClient`, `CanvasScope`), `rimelight_client::canvas_live` (`LiveSession`: ticket, WebSocket, unacknowledged ops kept and resent after a reconnect, backoff 0.5 s doubling to 15 s, ping every 25 s, ends on deleted, unauthorized, not_found or close). Native only: on wasm32 the session reports `Ended { reason: "unsupported" }` until the web slice. WebSocket: `tokio-tungstenite` 0.30 with rustls default features off, so `aws-lc-rs` (from reqwest) stays the only crypto provider. Gate: clippy `-D warnings` clean; `cargo test --workspace --lib` all passed (client 93, of which canvases 12 and canvas_live 5 with a loopback WebSocket server; proto 45, of which canvases 7); wasm32 `--lib` check clean; `cargo ndk -t arm64-v8a check --workspace --lib` clean (JAVA_HOME = Android Studio's `jbr`). Live run of the Rust client against rimelight.com: 13 of 13 passed (list, get, upload with the size read by the server, same file stored once, download, quota, two live sessions, op acked and applied, presence, snapshot after the flush, close); test canvas deleted through the API and session removed. Note: the server keeps only ASCII in file names (`dot é.png` is stored as `dot _.png`), as the issue attachments do.
- CV.5 (2026-11-19): new crate `crates/rimelight_canvas` (no UI, no I/O; deviation from decision 0011 noted in the design above): `geom` (Vec2, Rect, RotRect, Camera with zoom at a point, fit and visible area), `zorder` (base 62 fractional ranks in byte order, to front, to back, forward, backward), `model` (typed items over the room's props; the props schema is the module doc; unknown props kept and written back; `Doc` with version-checked `apply_state`), `ops` (props patch with null for removed keys, create, update, delete ops), `undo` (per-user history of 200 actions; undo only reverts the fields the action changed, so other people's later edits stay; undoing a delete re-creates a copy under a new id because the room never re-creates a deleted id), `arrange` (optimal pack scored on area and target aspect, sort by natural name, date added, size, hue or seeded random, grid, normalise height, width or area to the mean or a reference, align, distribute, stack), `hit` (hit test, marquee contain or intersect, eight handles plus rotate, rotation-aware resize with keep-aspect and from-centre, rotate with 15° snap, multi-selection scale), `snap` (edges and centres with guides, grid). Gate: clippy `-D warnings` clean; `cargo test --workspace --lib` all passed (`rimelight_canvas` 71); wasm32 `--lib` check clean. Known limit: undo restores the item's rectangle as one field, so undoing a move also resets a size someone else changed in between.
- CV.6 (2026-11-19, owner reviewed and approved 2026-11-19 after one round of fixes): canvas page (`ui/pages/canvas/canvas.slint`, `src/pages/canvas/page.rs`) and lists (`canvas-list.slint`, `list.rs`, `/home/canvases`, `/p/{key}/canvases`, New canvas, open). Interaction logic in `src/pages/canvas/controller.rs` (no Slint types): Select, Text and Hand tools; click, shift-click and marquee selection; move with snapping to other items (Ctrl turns it off) and guides; resize with eight handles (images keep their aspect unless Shift, Alt resizes from the centre); rotate knob (Shift snaps to 15°); wheel zooms at the cursor (PureRef-style), Shift+wheel pans, middle-drag or Hand pans; keys (Delete, Escape, Enter edits text, arrows nudge, Ctrl+Z/Y, Ctrl+A, Ctrl+D, Ctrl+L, Ctrl+[ ] and Ctrl+Shift+[ ] for z order, Shift+H/V flip, V/T/H tools, F fit, Shift+F fit selection, Ctrl+0/=/-); right-click menu; inline text editing (Enter commits, Shift+Enter new line, Escape cancels); undo and redo; add image from a file picker (desktop; uploaded, then placed at the view centre at most 600 units on its longer side). Images load as the 1024 px thumbnail, else the original, decoded on a worker, four at a time. Item colours are palette names drawn per theme (`CanvasPalette`), kept out of the generated design tokens because they are user content. Known limits: one live canvas at a time (the `CanvasState` global is shared; the session stays open until another canvas opens); shapes and pen strokes draw as placeholders until CV.9. Gate: clippy `-D warnings` clean; `cargo test --workspace --lib` all passed (controller 16); `SLINT_EMIT_DEBUG_INFO=1 cargo test --workspace --test ui` 168 passed (canvas 5); wasm32 `--lib` check clean. Owner review round 1 (2026-11-19) fixed: Canvases after Issues in the project sidebar with the board icon; wheel up zooms in; the toolbar shows the active tool; the viewport size reaches the controller (fit and zoom to selection); name on creation and rename in place (field sizes to its text); upload progress chip; several images laid out side by side. CV.7 intake done early: drop files and folders from the OS (desktop; drops gathered for 100 ms and placed together), paste with Ctrl+V (image data, copied files, https links). Gate after the fixes: clippy `-D warnings` clean, lib tests all passed, `--test ui` 169 passed, wasm32 and Android `--lib` checks clean.
- Migration applied 2026-11-19 through Drizzle (`pnpm db:migrate`, ledger 26 rows, last `20261119120000_space_canvases`). Checked in a rolled-back transaction: default title, archived bytes counted, `canvases_one_home` refuses a canvas with no home, delete cascades to items and assets.

**CV.6b: PureRef-style menu and shortcuts (owner, 2026-11-19; built 2026-11-19, owner review pending: clippy `-D warnings` clean, lib tests all passed (rimelight_ui 304), `--test ui` 170 passed, wasm32 and Android `--lib` checks clean).** The canvas right-click menu follows PureRef's menu tree and shortcuts (owner's screenshots of the main menu and the Mode, Window, Images, Selection, Arrange and Normalize submenus). One menu everywhere on the canvas (items that need a selection are disabled without one). Items whose feature is not built yet are shown disabled and switch on as each step lands. Items that only make sense in the pop-out window (Mode, most of Window) are disabled in the main window. The app's shared ContextMenu has one inline submenu level, so the canvas gets its own cascading menu: Rust holds the menu tree and one command table (label, shortcut, action), computes the panels (up to three, flipped left or up at the canvas edges) and handles hover, click and keyboard; the markup draws the panels.

Owner answers: clashing shortcuts are remapped (not focus-dependent); arrows nudge and Shift+arrows change z order; PureRef's keys replace mine where PureRef has the command, and my extras stay where PureRef has none.

| Command | PureRef | Ours | Note |
|---|---|---|---|
| Help, Hierarchy, Settings | Ctrl+H, Ctrl+J, Ctrl+U | same | Help opens the canvas shortcuts list; Hierarchy and Settings later |
| Undo, Redo | Ctrl+Z, Ctrl+Shift+Z | same | Ctrl+Y redo dropped (PureRef uses Ctrl+Y for Overlay selection) |
| Copy, Paste | Ctrl+C, Ctrl+V | same | |
| Note, Draw | Ctrl+N, Ctrl+Shift+D | same | Draw with CV.9 |
| Mode: Transparent to mouse, Overlay selection, Always on top, Always on top of…, Always on bottom | Ctrl+T, Ctrl+Y, Ctrl+Shift+A, Ctrl+Alt+Shift+A, Ctrl+Shift+B | same | pop-out only (CV.12) |
| Window: Maximize, Minimize, Resize to selection, Lock window, New window | Ctrl+F, Ctrl+M, Ctrl+Shift+R, Ctrl+W, Ctrl+Alt+Shift+N | same | pop-out (CV.12); in the main window New window pops the canvas out |
| New scene (New canvas) | Ctrl+K | **Ctrl+Shift+K** | Ctrl+K is the app's command palette |
| Close | Ctrl+Q | same | main window: closes the tab |
| Select all | Ctrl+A | same | |
| Selection: Reset cropping, Reset transform, Open source, Toggle bilinear sampling, Toggle grayscale, Toggle locked, Group, Ungroup, Crop image, Comment, Flip H, Flip V, Send to back, Send to front, Slideshow, Delete | Ctrl+Shift+C, Ctrl+Shift+T, Ctrl+Shift+O, Alt+T, Alt+G, Alt+L, Ctrl+G, Ctrl+Shift+G, Ctrl+Alt+Shift+C, Alt+C, Alt+Shift+H, Alt+Shift+V, Down, Up, Alt+S, Del | same except **Send to back Shift+Down, Send to front Shift+Up** | arrows nudge (owner) |
| Arrange: Optimal, By name, By path, By order, By addition, Randomly | Ctrl+P, Ctrl+Alt+N, Ctrl+Alt+D, Ctrl+Alt+O, Ctrl+Alt+A, Ctrl+Alt+R | same except **By name Ctrl+Alt+L** | Ctrl+Alt+N is the app's quick action |
| Normalize (From first / Average): Height, Width, Size, Area, Scale | Ctrl+Alt+Left, Ctrl+Alt+Right, Ctrl+Alt+Up, none, Ctrl+Alt+Down | **Ctrl+Shift+Left, Right, Up, Down** | Alt+Left/Right are the app's back and forward |
| Manage images, Replace image, Relink missing images | Ctrl+Shift+J, none, Ctrl+Alt+Shift+R | same | later |
| Our extras (PureRef has none) | | Ctrl+D duplicate, F fit, Shift+F zoom to selection, V/T/H tools, + and − canvas zoom, arrows nudge 1, Ctrl+arrows nudge 10, Alt+Shift+Up/Down forward/backward one step, Enter edit text, Escape | Ctrl+=/− stay the app's interface zoom; Ctrl+[ and ] stay the app's back and forward (the earlier canvas use never reached the canvas) |

The Canvas, Align and Copy submenus were not in the screenshots; their items were read from the installed PureRef (the menu paths in its executable, on the owner's say-so, 2026-11-19): Copy ▸ Copy as file; Canvas ▸ Grid (none, lines, dots, snap to grid), Reset camera, Reset camera zoom, Lock canvas, Grayscale, Optimize; Align ▸ Left, Right, Top, Bottom, Stack, Distribute horizontal, Distribute vertical. PureRef stores its default shortcuts as key codes, so those items have no shortcut here. Owner: Save and Open are not needed (canvases are in the cloud); in their place go "Add images…" and Export ▸ (canvas as image, selection as image, all images to a folder, selected images to a folder; CV.13). PureRef's Optimize (memory) is left out.

**CV-V: video.** Planned in decision 0046 (owner asked 2026-11-19).

#### 15.40 Decision 0046: Video on canvases (CV-V)

Status: proposed (owner asked 2026-11-19: about 40 reference videos, 50 to 300 MB each, 2.67 GB in total, dropped onto a canvas beside images, notes, shapes and drawings).

**Owner answers (2026-11-19)**

| Topic | Answer |
|---|---|
| Size | 500 MB per video file (images stay 50 MB) |
| Quota | Stays 10 GB per project and 1 GB per person, fixed in code; raised in code when needed |
| Playback | A video shows its thumbnail until someone presses play; only playing videos decode |
| Decoder | FFmpeg (desktop first) |
| Features | Everything that applies to an image item: move, resize, rotate, flip, z-order, lock, frames, groups, arrange, align, hierarchy, arrows bound to it, save originals to a folder, backup. Not crop, greyscale, eyedropper or opacity-per-pixel features. PNG export draws the thumbnail |

**Design (agent proposal)**

1. **Item.** New kind `video` with the shared props (x, y, w, h, rot, flip, opacity, locked, group, frame) plus `asset`, `volume` (0..1, default 1), `muted`, `loop`. Placed at the video's pixel size scaled to at most the image limit. Whether a video is playing is local to each person (not an item prop).
2. **Assets.** Video MIME types found from the file bytes: MP4 and MOV (ISO BMFF `ftyp`), WebM and MKV (EBML). New asset columns: `media` (`image` or `video`), `duration_ms`, `has_audio` (migration through Drizzle). Thumbnails reuse the 256 and 1024 WebP route: the client makes them from a frame about 10 % into the video.
3. **Upload.** Cloudflare Free caps a request body at 100 MB, so files above 90 MB go up as an R2 multipart upload through the Worker's R2 binding: create, parts of at most 90 MB, complete, abort. The client hashes the file (SHA-256) for dedupe; the Worker cannot hash 500 MB within its 10 ms CPU, so the hash the client sends is trusted for dedupe inside one canvas only (a wrong hash only costs that canvas a duplicate). Size and quota checked at create and again at complete. Upload progress shows in the busy chip.
4. **Download and playback.** The desktop downloads the original once into the disk cache (`{sha256}-orig`) and plays from the file. The asset GET answers `Range` requests (R2 `range` option), so a later step can start playback before the whole file is there.
5. **`rimelight_media` crate** (shared, no Slint types in its API): `probe` (duration, size, codecs, audio), `poster` (one frame as RGBA), and `Player` (a decode thread per playing video, a small frame queue, audio through `cpal` as the master clock, play, pause, seek, volume, loop). The canvas page turns frames into `slint::Image` on the UI thread, as the GIF frames are now. At most 4 videos decode at once; pressing play on a fifth pauses the oldest.
6. **UI.** Thumbnail with a play badge and the duration; a click on the badge plays. While a video is selected or hovered: play and pause, a scrub bar, the time, mute and volume. Space plays or pauses the selected video when no text field has focus. The Manage images dialog lists videos too.
7. **Platforms.** Desktop (Windows first) uses FFmpeg. Android: FFmpeg built with the NDK, or Android's own MediaCodec and MediaExtractor through the `ndk` crate (OS codecs, no extra libraries; picked by spike). Web: the browser's WebCodecs. The `Player` API is the same on each, so the canvas code does not change per platform. Until a platform has a backend, its video items show the thumbnail and "Plays in the desktop app".

**Dependencies**

- `ffmpeg-next` (bindings to FFmpeg 7.x): container reading, decoding of H.264, HEVC, VP9, AV1 and the common audio codecs, poster frames, probing. Linked dynamically against an LGPL build with only decoders (no GPL encoders), shipped beside the desktop app (about 80 to 100 MB). Building needs FFmpeg development files and clang (bindgen) on the machine and in CI.
- `cpal`: audio output (WASAPI on Windows, CoreAudio, AAudio on Android, ALSA), the playback clock. Reusable for any other app sound (voice notes, notification sounds).
- Possibly `rubato` (resampling) if FFmpeg's own resampler (`swresample`, part of the same libraries) is not used; the spike decides, `swresample` is preferred (no extra crate).

These serve later video needs too: issue and chat attachments, player support clips, the image viewer, screen recordings.

**Play for everyone (later step, owner asked how it would work).** An editor presses "Play for everyone" on a video. The room sends a `media` message (not an item op, not saved): item id, playing or paused, position in ms, and the server's time. Each client keeps an estimate of the server clock from its pings and computes where the video should be now; it seeks when it is more than about 300 ms off. Pause, seek and play from anyone in the room apply to everyone (last action wins). Constraints: each person must have the file in their cache first (a 300 MB file takes time on a slow line), so a late or slow client starts when its download finishes and jumps to the shared position; sound plays on every machine (each person has their own mute and can leave the shared playback); the server only relays small messages, so the Durable Object cost stays small; nothing is stored, so a reload ends the shared playback for that person.

**Unverified (tested in CV-V.0)**

- `ffmpeg-next` builds on Windows with the 1.100.0 toolchain against a shared LGPL FFmpeg 7.x, and the app starts with the DLLs beside it.
- A 1080p30 H.264 and a VP9 file decode and show in a Slint window at full rate (frame to `slint::Image` on the UI thread), with `cpal` audio in sync (within about 40 ms) through a 10-minute file, and seeking works.
- R2 multipart upload of a 300 MB file through the Worker binding stays within Workers Free limits (10 ms CPU per request, 100 MB body).
- `Range` GETs of R2 objects through the Worker.

**CV-V.0 result (2026-11-20, desktop part).** Spike crate in the agent scratchpad, outside the repo (`cvv_spike`: slint 1.18.1, ffmpeg-next 9.1.0, cpal 0.18.2), built in release with the 1.100.0 toolchain. Tools, not installed system-wide, kept under a shared target folder (since deleted): FFmpeg n9.0 LGPL shared build for win64 (BtbN/FFmpeg-Builds, 78 MB zip, DLLs and import libraries) in `rimelight-space-target/ffmpeg/`, and `libclang.dll` plus the clang headers from the official LLVM 23.1.6 archive (263 MB download, 102 MB kept) in `rimelight-space-target/libclang/`; the archives were deleted. Test files: three of the owner's local reference videos (all H.264 with AAC stereo 44.1 kHz; 1920x1080 at 25 and 24 fps, 854x480 at 30 fps). Reading online-only files in the Proton Drive folder makes Proton Drive download them (the owner saw a notification per file): later spikes and tools only open files that are already local.

- Build: `ffmpeg-next` builds and links against the shared FFmpeg 9.0 with `FFMPEG_DIR` and `LIBCLANG_PATH` set; the app runs with the FFmpeg DLLs on the path. Spike exe 11.5 MB without the DLLs.
- Decode speed (1080p H.264 to RGBA, one decoder): 74 fps at full size, 129 fps scaled to 960x540. FFmpeg frame threading made it slower (53 and 116 fps): the cost is the colour conversion and copy, not decoding. The player converts at the on-screen size; threads for the converter (or YUV to the GPU) are CV-V.2 tuning, needed before 4 videos play at once.
- Thumbnail: a seek to 10 % lands on the keyframe before it (22.4 s for 27.7 s); the 1080p frame was decoded in 45 ms with correct colours.
- Playback in a Slint window with cpal as the clock (output 44.1 kHz stereo f32, muted for the test): steady 25.0 fps over 30 s, picture against sound mean +2.5 ms, worst 12 ms. First frame after 655 ms with 11 frames dropped while the decoder caught up; a seek to 60 % showed its first frame after 199 ms, then 7 drops in the next 5 s. The player will pre-decode a few frames before it starts the clock.
- No clang for other developers: `ffmpeg-sys-next` 9.1.1 always runs bindgen and a C feature check (MSVC `cl`). Its whole output is one `bindings.rs` (9.3 MB) and 776 `cfg` flags plus 7 link lines. Plan: a small replacement crate in the repo (through `[patch.crates-io]`) with these files checked in per target (Windows x64 first; Android aarch64 when CV-V.6 comes), and `cargo xtask ffmpeg` fetching the pinned FFmpeg build into `ffmpeg/current` at the workspace root. Developers and CI then need neither clang nor FFmpeg development files.
- R2 multipart upload and `Range` reads through the Worker: tested live in CV-V.1 (2026-11-20), see the CV-V row in section 4.

**Steps** (one commit each)

| Step | Content |
|---|---|
| CV-V.0 | Spikes for every unverified item, outside the repo. Results recorded here |
| CV-V.1 | Migration (asset `media`, `duration_ms`, `has_audio`); video MIME sniffing; 500 MB video limit; multipart upload routes; `Range` on the asset GET (website) |
| CV-V.2 | `rimelight_media` crate: probe, poster, Player with audio clock (desktop backend) |
| CV-V.3 | Client: multipart upload with progress, thumbnails from the poster frame, disk cache |
| CV-V.4 | Canvas `video` item: model, thumbnail and play badge, controls, at most 4 decoding, every image feature that applies, export and backup |
| CV-V.5 | Play for everyone |
| CV-V.6 | Android and web backends (by spike: FFmpeg or MediaCodec on Android, WebCodecs on the web) |

**CV-V.6 scope (proposed, 2026-11-20).** Checked before starting: (1) the browser build has no live room (`LiveSession::connect` on wasm32 ends at once with "unsupported"), so a canvas does not load in the browser at all; a web video backend needs the browser live session first (its own step, WebSocket through web-sys). (2) On Android the canvas opens, but the page has no file drop or picker there, so videos can be watched, not added; playback needs either FFmpeg built with the NDK (one code path, about 20 to 40 MB more in the APK, fiddly build) or Android's MediaCodec and MediaExtractor through the `ndk` crate (OS decoders, no extra libraries, a second code path); testing needs the emulator (AVD `Medium_Phone`, API 37, is installed) or a phone. Options for the owner: A) Android playback now (spike MediaCodec against FFmpeg on the emulator), web later; B) the browser live session first, then web playback (WebCodecs and WebAudio); C) close CV-V at desktop and return to it later.

#### 15.41 Decision 0047: Release pipeline and auto-update (REL)

Status: proposed (owner asked 2026-11-20: executable installers for users, built by an automated pipeline, with installed copies updating themselves).

**Owner answers (2026-11-20)**

| Topic | Answer |
|---|---|
| Installer and updater | Velopack: per-user `Setup.exe`, no admin prompt, delta updates, restart into the new version |
| Hosting | The existing Space assets R2 bucket under a `releases/` prefix, public through rimelight.com. Bucket `rimelight-dot-com` (the website Worker binding `BLOB`, read from `cloudflare.config.ts`). Served by a public Worker route `GET /v1/releases/win/<file>` in the website repo (no custom domain needed; the Worker already has R2 bound) |
| First install | A stable link on rimelight.com (`/download`) pointing at the latest `Setup.exe`. The page itself lives in the website repo and is a separate slice |
| Trigger | Pushing a `vX.Y.Z` tag runs a GitHub Actions job on a Windows runner. The job does not run the quick gate again (owner choice: trust CI on the tagged commit). Risk noted: a tag on a commit CI never passed can ship |
| Signing | Unsigned first (SmartScreen warns). The pipeline has a signing step that stays off until a certificate exists |
| Update behaviour | On launch, before the main UI, the app checks the feed. If a newer version exists it downloads, applies and restarts into it; the user cannot skip it. The feed also carries a minimum version, re-checked while the app runs |
| Offline | The check must succeed before the app opens: no check, no launch. An R2 outage blocks every launch (owner choice; the alternative of launching offline on the last known version was offered). This conflicts with the spirit of decision 0035 (online only) only in that it is stricter, not in direction |
| Other platforms | Recorded as later slices, none buildable today |

**Design (agent proposal)**

- Version comes from the tag and must equal `[workspace.package] version`; the job fails if they differ.
- The desktop crate depends on the `velopack` crate. The startup check lives in `apps/desktop` before the Slint window opens; the Velopack hook for install, update and uninstall runs first in `main`. `unsafe_code = "forbid"` still holds for workspace crates (the crate's own unsafe is in the dependency).
- The package contains the desktop exe plus the FFmpeg DLLs that `vendor/ffmpeg-sys-next` copies next to it (LGPL, decision 0046).
- Feed files (`releases.stable.json`, `RELEASES-stable`, `assets.stable.json`, full and delta nupkg, `<PackId>-stable-Setup.exe`, Portable.zip; names as `vpk pack --channel stable` wrote them in REL.0) upload flat to `space/releases/win/` in the bucket `rimelight-dot-com` (the channel is in the file names, so channels can coexist). Names carry the channel, so `RimelightSpace-stable-Setup.exe` is already a stable name for the `/download` link; no extra copy.
- R2 write token goes in GitHub secrets `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` (an R2 API token with Object Read and Write on the bucket), and optional `RIMELIGHT_SIGN_PARAMS`; no secret value is ever written to the repo.
- The `vpk` CLI is installed in CI (version pinned, exact tool and version **unverified**, checked in REL.0).

**Steps**

| Step | Content |
|---|---|
| REL.0 | Spike: build a Velopack package of the desktop app locally, install it, ship a second version to a local folder feed, confirm the update path and that the FFmpeg DLLs and the pop-out window work installed. Settle the `vpk` and crate versions |
| REL.1 | R2 prefix, public domain and token scope settled with the owner; secret names recorded here |
| REL.2 | `xtask release` (package locally) and the Velopack hooks in `apps/desktop` |
| REL.3 | Startup update check, blocking splash with progress, failure screen with retry; minimum-version check while running |
| REL.4 | GitHub Actions release workflow on `v*` tags (version check, build, package, upload, unsigned) |
| REL.5 | End to end: install version N, tag N+1, installed app updates on launch; owner sees it |
| REL.6 | Signing step wired but off; notes on obtaining a certificate (Azure Trusted Signing or OV) |

**Later platform slices (recorded, not started)**

| Slice | Prerequisites |
|---|---|
| REL-M macOS | Apple Developer account, notarisation, a macOS port of `rimelight_media` (FFmpeg) and audio output |
| REL-L Linux | AppImage via Velopack, a Linux port of `rimelight_media` and audio |
| REL-A Android | APK sideload feed (no Play Store, PLAN section 2), in-app update prompt, signing key |
| REL-I iOS | Distribution route to be chosen (ad hoc or enterprise; no public store), Apple account |
| Windows arm64 | An arm64 FFmpeg build and a runner or cross build |

**REL.0 result, part 1 (2026-11-20).** Spike crate in the agent scratchpad, outside the repo (`relspike`: `velopack =1.3.39-g8d2844f`, a console exe that runs `VelopackApp::build().run()`, then checks a `FileSource` feed and calls `download_updates` and `apply_updates_and_restart`). Tooling: .NET SDK 10.0.403 was already installed; `vpk` 1.3.39-g8d2844f installed with `dotnet tool install vpk --tool-path <scratchpad> --prerelease` (not global). The Rust crate exists only as this prerelease on crates.io, and the stable `vpk` on NuGet is 1.2.161: pin both to the same 1.3.39 prerelease.

| Checked | Result |
|---|---|
| `vpk pack` without signing parameters, 1.0.0 | Setup.exe 11.7 MB for a 1.5 MB exe, full nupkg, Portable.zip, feed files; warning "will not be signed", as expected |
| Silent install (`Setup.exe --silent`) | Installs to `%LOCALAPPDATA%\<PackId>` with no admin prompt (folders `current` and `packages`, plus `Update.exe` and a launcher exe) |
| Pack 1.0.1 into the same output folder | Full nupkg plus a delta nupkg of 2.6 KB (the full one is 3.3 MB) |
| Installed 1.0.0 run against a file feed | Found 1.0.1, downloaded, applied and restarted; the new process reported 1.0.1; `sq.version` shows 1.0.1 |
| API facts | `apply_updates_and_restart` takes `&info.TargetFullRelease` (an `AsRef<VelopackAsset>`), not the `UpdateInfo`; `UpdateCheck` is `RemoteIsEmpty`, `NoUpdateAvailable` or `UpdateAvailable(Box<UpdateInfo>)`; sources include `HttpSource`, `FileSource`, `GithubSource`; `UpdateOptions.ExplicitChannel` switches channel |

Not yet checked: `HttpSource` against a real HTTP server (R2); the real desktop app packaged (the newest release exe in the shared target is from 2026-11-18, before FFmpeg, so it says nothing about the DLLs, and a fresh release build is needed); the FFmpeg DLLs and the pop-out window in an installed copy; uninstall; Setup.exe launching the app. The spike install was uninstalled with `Update.exe --uninstall --silent` and its folder is gone (checked). The fresh desktop release build is deferred to the end of the current batch (owner, 2026-11-20).

**REL.1 to REL.4 code written, not compiled or run (2026-11-20, owner: "keep implementing, check it all at the end").** Nothing below has been through cargo, the workflow or the Worker; the end-of-batch gate is the first check.

| Step | Files | Notes |
|---|---|---|
| REL.1 | decision text above; `release/min-version.txt` (`0.0.0`) | Bucket `rimelight-dot-com`, prefix `space/releases/win/`, feed base `https://rimelight.com/v1/releases/win`. Secret names fixed. The owner still has to create the R2 API token and add the three secrets in GitHub |
| REL.2 | `xtask/src/release.rs`, `xtask/src/main.rs` | `cargo xtask release pack [--expect-version x.y.z] [--out dir] [--no-download]` checks the version against `[workspace.package]`, builds with `cargo build --workspace --release --bin rimelight_desktop`, stages the exe and the FFmpeg DLLs, runs `vpk download http ... --allowEmptyChannel` (previous releases, for deltas) and `vpk pack`, then writes `min-version.stable.txt`. `RIMELIGHT_SIGN_PARAMS` becomes `--signParams` (REL.6 hook, off by default). Unit tests: manifest version parse, flag parse |
| REL.2/3 | `apps/desktop/Cargo.toml`, `apps/desktop/src/main.rs`, `apps/desktop/src/update.rs`, new crate `crates/rimelight_update_slint` | `velopack =1.3.39-g8d2844f` (feature `public-utils`) on Windows. `update::init()` first in `main` (install/update/uninstall hooks); `update::gate()` before the shell: not installed (cargo run) passes; installed checks the feed, downloads with progress, applies and restarts; a failed check shows the error with Try again and Quit and the app does not start (owner choice); `update::watch()` re-checks the minimum version every 30 minutes and force-updates a copy below it. The window is its own small Slint crate so the desktop crate keeps `unsafe_code = "forbid"`. Its strings are English literals, not `rimelight_i18n` keys (the window runs before the shell exists): to be decided with the owner. Unit tests: version parse and compare |
| REL.4 | `.github/workflows/release.yml` | On tags `vX.Y.Z`: install `vpk` at the pinned version, `cargo xtask ffmpeg`, `cargo xtask release pack --expect-version`, upload with the AWS CLI to R2 (packages first, `releases.*.json` last), one release at a time. No quick gate (owner choice) |
| Website | `apps/rimelight.com/src/space/releases.ts`, `api.ts` (website repo, **uncommitted**) | Public `GET /v1/releases/:os/:name` streams from `space/releases/<os>/` in `BLOB`; strict file-name check; `.nupkg` cached a year, everything else 30 s to 5 min. No test yet: the helper functions live in a file that imports `cloudflare:workers` |

**To check at the end of the batch (all unverified):** `cargo fmt`, clippy and the `--bins` tests for `rimelight_desktop` and `xtask`; that `Arc<dyn Fn>` calls and the `velopack::download` path compile; that Slint accepts running the update window's event loop and then the shell's (a second `run` in one process); `vpk download http --allowEmptyChannel` on an empty feed; the `std-widgets` `Palette` and `ProgressIndicator` names in Slint 1.18.1; a real release build with the FFmpeg DLLs, installed, with the pop-out window; `HttpSource` against the Worker route; the workflow on a test tag (a dry tag on a branch is not possible: it needs the three secrets).

**REL.5 and REL.6 written as procedure, still unrun (2026-11-20).** The GitHub secrets `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` are set (owner, 2026-11-20). Website: the pure helpers moved to `src/space/releases-files.ts` with `test/space-releases.test.ts` (not run); `releases.ts` imports them. Desktop: the report closure in `update.rs` now names its parameter type. No cargo, vitest or workflow run happened after the fmt check (owner stopped the clippy run: it was too slow for the shared PC).

- **REL.5 (end to end, owner present):** (1) run the gate once, (2) `cargo xtask ffmpeg`, (3) set `[workspace.package] version` to the first release, push the website route (`apps/rimelight.com`, deploys the Worker), (4) push tag `v<version>`; the workflow must go green and put the files under `space/releases/win/`, (5) download `https://rimelight.com/v1/releases/win/RimelightSpace-stable-Setup.exe`, install, check the app opens, the FFmpeg video plays and the pop-out window works, (6) bump the version, tag again, open the installed app: it must show the update window, restart on the new version and the second release must have produced delta packages. Step 4 and 6 need the owner's go-ahead to push (AGENTS: never push unasked).
- **REL.6 (signing):** `RIMELIGHT_SIGN_PARAMS` (a `signtool.exe` argument string) is already passed to `vpk pack --signParams` when set, as a repository secret. Until a certificate exists the exe, DLLs and Setup.exe are unsigned and Windows SmartScreen warns. Routes to a certificate: Azure Trusted Signing (`vpk pack --azureTrustedSignFile`, no hardware token, works on a runner) or an OV certificate from a CA (an EV token needs a self-hosted runner). Nothing is bought or set up.

**REL.1 to REL.4 gate (2026-11-20, owner asked for one run).** `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean (exit 0); `cargo test --workspace --bins -- update::tests release::tests`: desktop 4 passed, xtask 4 passed. Not run: the website vitest file, the wasm32 and Android checks, the workflow, a release build, an installed run (the update window's two-event-loops question and `vpk download --allowEmptyChannel` on an empty feed are still unverified). REL.0 part 2 and REL.5 need the owner present.

**REL.5 start (2026-11-20, owner said go ahead).** Website `f48d0ccf` pushed and deployed (`/v1/releases/win/<file>` answers "No such file." for a missing file). Space `main` and tag `v0.1.0` pushed. The Release run `44758150559` did not start: GitHub reports "recent account payments have failed or your spending limit needs to be increased" (Billing & plans), so no job ran and nothing is in R2. Not a code failure. After billing is fixed: `gh run rerun 44758150559 --repo Rimelight-Entertainment/rimelight-space` (same tag, same commit). First real run of the workflow is still unverified.

**CI minutes (2026-11-20, owner chose to cut usage).** The Release run was refused by GitHub (free plan, private repo, 2000 minutes a month; the owner confirmed the actions had used a lot). `ci.yml` ran four jobs on every push to every branch, one of them macOS (ten times the minutes of Linux) and one Windows (double). Now: push runs only on `main` plus pull requests, a newer run cancels an older one, and the macOS and iOS check runs Mondays 03:00 UTC and on `workflow_dispatch` only. Tag pushes no longer start `ci.yml`. Not yet pushed; unverified until the first run after the push. Usage was not read from GitHub (the `gh` login lacks `admin:org`).

#### 15.42 Decision 0048: Nightly toolchain and the parallel frontend (owner, 2026-11-20)

- **Status:** the pin and the flags are the owner's choice (2026-11-20: "pin 2026-11-19, benchmark"). The thread count is open until the benchmark below is read.
- **Context:** the Rust compiler blog (2026-11-16) enables the parallel frontend on nightly by default, with 2 threads. `--jobs-frontend=N` raises it. Parallel frontend on stable does not exist yet, so the pin moves from `1.100.0` to nightly.
- **Decision:** `rust-toolchain.toml` pins `nightly-2026-11-19` (rustc 1.102.0-nightly, 2026-11-18), not a bare `nightly`, so the lints stop moving. `.cargo/config.toml` sets `[build] rustflags = ["-Zunstable-options", "--jobs-frontend=8"]`. The wasm32 target has its own `rustflags` in Cargo, which replaces the `[build]` ones, so it repeats the same flags with the getrandom cfg.
- **Consequences:** the pre-commit hook and CI use the same nightly (CI reads the same file), so local lints still match CI. Nightly clippy can add lints that fail `-D warnings` without a code change; the dated pin is what limits that. `-Z` flags are rejected on stable, so the pin must stay nightly while the flags are set. Parallel frontend builds may not be bit-reproducible and diagnostic order may vary (per the blog).
- **Rollback:** revert the two files; 1.100.0 stays installed.
- **Benchmark stopped (2026-11-20, owner said "kill it", disk at 7 GB free):** only the 8-thread run finished, in 1572 s. It shared the target folder with another `cargo check --workspace --all-targets` (PID 10884), so its time is not a clean number. The 1-thread and 4-thread runs were not finished. The thread count stays open.
- **Benchmark run 2026-11-20 (clean, nothing else building):** `cargo clippy --workspace --all-targets`, sampled rustc memory. 8 threads: 30 s, peak 1.8 GB. 1 thread: 135 s, peak 1.9 GB. 4 threads: 18 min 9 s, peak 3.5 GB. **Not comparable:** the 8 and 1 thread builds reused artifacts from earlier runs, so only the 4-thread time is close to a cold build. A fair comparison needs each setting built cold, one at a time.
- **Cold benchmark 2026-11-21 (owner's PC clear, `cargo clean` before each run):** `cargo clippy --workspace --all-targets`, Windows, 8 logical cores, 16 GB. 1 thread: 5 min 54 s, sampled rustc peak 2.7 GB. 4 threads: 5 min 42 s, 3.1 GB. 8 threads: 5 min 49 s, 3.2 GB. Each thread count was run once. The spread is about 3%, which is within run-to-run noise, so this run shows **no measurable improvement** from more frontend threads for a cold check. Not measured: a link, the test run, an incremental rebuild after a one-crate edit.
- **Reverted 2026-11-21 (owner's choice, after the benchmark):** `rust-toolchain.toml` is back to `1.100.0` and the `--jobs-frontend` flags are removed. `.cargo/config.toml` matches the last commit. The workspace builds on 1.100.0 (`cargo build --workspace`, exit 0). The nightly toolchain is still installed on this machine; removing it is a separate step.
- **Unverified until the benchmark and the first gate:** thread count (1, 4, 8 on this 8-core, 16 GB machine, wall time and sampled memory); the full gate on nightly (fmt, clippy `-D warnings`, tests, wasm32 and Android checks); whether `RUSTFLAGS` in a shell overrides the config (it does, by Cargo's rules).

#### 15.43 Decision 0049: Content (CMS) moves into Space (CMS)

**Status:** accepted by the owner, 2026-11-23 (14 questions answered in one session). Reverses the "No access to rimelight.com's CMS" part of the "Relation to rimelight.com" row in section 2.

**Context.** rimelight.com has a CMS (`@rimelight/cms`, repo `Rimelight-Entertainment/rimelight`, `packages/cms`) for wiki pages, docs, blog posts and franchise content. It is unfinished and buggy. Space's API already runs on the same website Worker and the same Postgres (decisions 0039, 0041). The CMS's data model fits Space as it is: pages are a JSON block tree (`id`, `type`, `props`, `children`), rich text is a small inline AST (text, link, mention, emoji), and drafts, versions, approvals, comments, locks, templates and per-role block filtering already exist.

**Owner answers.**

| # | Topic | Decision |
|---|---|---|
| 1 | Scope | Everything moves: wiki, docs, blog, franchise collections, screenplays and site pages. Space owns all content; rimelight.com only renders it |
| 2 | Ownership | A page belongs to exactly one scope: a project, a team, a department or the company |
| 3 | `@rimelight/ui` docs | Hybrid. Written guides become Space content. The API reference and live previews generated from component source stay in the rimelight repo; the website stitches the two together |
| 4 | Content types | Keep the generic system: collections and their fields are defined in the database and managed in Space. Wiki, docs and blog ship as built-in presets |
| 5 | Visibility | Internal by default. A page, or a whole space (for example a blog or the franchise wiki), is published on purpose. Only published versions of public content reach the website |
| 6 | URLs | The website keeps today's public URLs and decides where each collection renders. Space's internal routes are its own and need not match |
| 7 | Block visibility | Kept (today's `allowedRoles` / `requiredPermission`, moved onto Space's policy engine). The API strips hidden blocks before content reaches the website or a user without access |
| 8 | Editor scope | Full parity on day one, including the web-only blocks (`LivePreviewBlock`, `ApiDocumentationBlock`). The screenplay blocks (`ScriptBlock`, `SceneBlock`, `DialogueBlock`) become standard built-ins: the package is no longer public, so there is no site-specific block layer |
| 9 | Co-editing | Full CRDT co-editing (character-level, cursors, presence) from day one |
| 10 | Phones | Full editing parity on phones from day one. A13 holds |
| 11 | Transition | Editing is frozen from the data move until the Space editor ships. The Solid dashboard is not kept alive in between |
| 12 | Data move | Content moves as-is (same IDs and block JSON) plus its owner scope. History is dropped: old versions, version comments and approvals are not migrated |
| 13 | Packaging | `@rimelight/cms` stops being a public npm package. The content model, block editor and renderers for Slint go into `rimelight-rs`, so rimelight-personal can use them too. The server side becomes a private shared TypeScript package: rimelight.com mounts it for Space, idantity.me mounts it on its own Worker and database |
| 14 | Priority | Main track now. S1.5, CV, CV-V and REL are paused |

**Architecture.**

- **Server (TypeScript, private package).** The CMS's schema, versions, publish rules, search, media (R2), MCP and scheduled jobs stay TypeScript and become the content service. In rimelight.com it is mounted under the Space API (`/v1/content/...`, behind Space sessions) plus a public read API for the website. `cms_users`, CMS roles and the CMS's own Auth0 / Cloudflare Access identity are removed: Space users and the Space policy engine decide access. Every page gets an owner scope (`project`, `team`, `department`, `company`) and a visibility. The Solid dashboard and editor leave the package.
- **Rust (`rimelight-rs`).** A content crate with the block and inline types (serde shapes identical to the TypeScript JSON), the block registry (built-ins including screenplay), the CRDT document mapping and the API client types. A Slint block editor and renderers in `rimelight_ui` (or its own crate, decided in the design step). Space and rimelight-personal pin it like the other shared crates.
- **Website.** Block renderers move from `packages/cms` into `@rimelight/ui`. The site renders published content from the public read API at its current URLs, with a per-collection route mapping it owns.
- **Co-editing.** A CRDT room per document in a Durable Object on the website Worker. The Worker is TypeScript (decision 0041) and the client is Rust, so the library must have matching Rust and JS implementations with one wire format: Yrs with Yjs, or Loro with `loro-crdt`. Chosen in CMS.0. The block JSON stays the published, rendered form; the CRDT document is the editing form, and publishing snapshots one into the other.

**Steps.** Per the owner's build rule (root `CLAUDE.md`), steps are written in batches with one build and test pass at the end of a batch, not per step.

| Step | Content |
|---|---|
| CMS.0 | Spikes, before any design is locked. (a) Web-only blocks: embed a web view inside a Slint window on Windows, Android and the wasm build, and compile-check iOS and macOS; if this fails, answer 8 goes back to the owner. (b) CRDT: Yrs+Yjs vs Loro+`loro-crdt`, a Rust client talking to a TypeScript Durable Object on the website Worker, one rich-text paragraph edited from two clients. (c) Rich text input in Slint: inline marks, mention chips, IME and the phone keyboard in one editable line |
| CMS.1 | Editor design step (owner review): block list, selection model across blocks, keyboard map, slash menu, drag and drop, mobile behaviour, how the block JSON maps to the CRDT document, the publish snapshot, comments anchored to blocks |
| CMS.2 | Shared content model in `rimelight-rs`: types, registry, serde round-trip tests against real exported CMS JSON |
| CMS.3 | Content service: private package, owner scopes, visibility, per-block filtering on Space policy, Space identity in place of `cms_users`, screenplay blocks as built-ins, dashboard and editor removed from the package, Space API routes and the public read API |
| CMS.4 | Freeze and move: the website dashboard goes read-only (editing frozen from here), content is moved as-is with owner scopes and without history, users mapped to Space users |
| CMS.5 | Slint block editor: every block including the web-only ones, CRDT co-editing with cursors and presence, full phone parity |
| CMS.6 | Space pages: per-scope wiki, docs and blog trees, collection and field admin, templates, publish and visibility, comments and approvals, search |
| CMS.7 | Website: renders from the public read API at today's URLs, per-collection route mapping, renderers from `@rimelight/ui`, the hybrid `@rimelight/ui` docs |
| CMS.8 | Retire: delete the Solid dashboard and editor, unfreeze editing; idantity.me mounts the content service and rimelight-personal uses the shared editor (their own plans schedule this) |

**Open, to settle before the step that needs it.**

- Languages: the CMS stores `en`, `pt`, `es`; Space supports `en`, `pt-BR`. Decide whether Spanish content is kept, and whether `pt` maps to `pt-BR` (before CMS.4).
- History: confirm whether dropping history also drops the drafts of unpublished pages, or only old published versions (before CMS.4).
- Hybrid UI docs: how the website places generated API reference inside a Space-authored page (a block that names a component, as `ApiDocumentationBlock` does today, is the likely answer) (before CMS.7).
- Package name and registry for the private TypeScript package (GitHub Packages or a git dependency) (before CMS.3).
- Decision 0039's open risk (Space tables next to website tables in one database) now also covers the content tables.

**Risks.** CMS.5 is the largest piece of UI in the product: a rich-text block editor with CRDT co-editing and full phone parity, in a toolkit with no rich-text input. Editing is frozen for all of it (answer 11), so the freeze lasts from CMS.4 until CMS.5 and CMS.6 ship. Doing CMS.4 as late as possible shortens the freeze without changing any answer.

**CMS.0 result (2026-11-23, partial: owner's on-screen checks pending).** Spike workspace `rimelight-rs/spikes/cms0` (its own workspace, excluded from rimelight-rs by `spikes/*`; not committed): crates `webview_spike`, `richtext_spike`, `crdt_spike`, plus the Node folder `crdt_js`. One build (debug, incremental off); `debug/incremental` of rimelight-rs and rimelight-personal deleted first (12 GB free before, owner rule). Versions resolved: wry 0.57.3, yrs 0.28.2, loro 1.18.0, yjs 13.6.33, loro-crdt 1.19.0.

- **(a) Web view, Windows: created.** `wry::WebViewBuilder::build_as_child` on Slint 1.18.1's window handle (feature `raw-window-handle-06`) works; the spike logged `handle ok`, `webview created` at the box Slint laid out (x 16, y 157, 600x320). A 16 ms timer moves it when the box moves. **Owner to check:** page visible in the box, stays aligned on resize and scroll, typing into the page, and that Slint popups are hidden behind it (a native child window always draws on top). **Not done:** the wasm iframe path is written but not compiled; Android is a stub only. wry's Android support needs tao's own activity glue, so Android needs a JNI `android.webkit.WebView` on Slint's android-activity backend, still to be written and tried on a device. iOS and macOS not tried.
- **(b) CRDT: both libraries interoperate Rust to JS and back.** Same document (list of blocks, each a map with a rich-text field; bold, italic and a mention mark with an entry id). Rust created it, Node made a concurrent edit, Rust merged and made its own, Node imported that; Rust's and Node's final documents are identical for both (`compare.mjs`: yrs PASS, loro PASS, 4 blocks, marks and mention value kept). Update sizes: Yrs 77 to 101 bytes per edit, Loro 116 to 178. **Worker size:** a Yjs room bundles to 77 KB (23.5 KB gzip); a Loro room needs a 4.1 MB wasm (1.37 MB gzip) on top of the website Worker, against a 3 MB compressed limit on the free plan and 10 MB on paid. Difference seen: Loro's mark expansion is configured once per document (text typed at the end of bold stays bold, Peritext rules); Yrs applies exactly the attributes the client sends, so the editor decides expansion. Loro also has a movable list (concurrent block moves); with Yrs a block move is delete and insert. **Recommendation:** Yrs + Yjs (tiny Worker cost, `y-protocols` awareness gives cursors and presence), with block moves and mark expansion handled in the editor. Loro only if the Worker is on the paid plan and concurrent block moves matter more than 1.4 MB.
- **(c) Rich text: model works; on-screen behaviour pending.** Slint 1.18.1 has no rich-text input, but has a read-only `StyledText` element (bold, italic, underline, strikethrough, inline code, links, colour; set from Rust with `StyledText::from_markdown`, which returns a `Result`). `StyledText` has no `wrap` or `font-size` property (`default-font-size` only). The spike stacks a transparent `TextInput` (focus, keyboard, IME, caret, selection) on a `StyledText` rebuilt from a Rust model (plain text plus mark ranges) on every edit; mentions render as coloured links. Model unit tests: 12 passed (range shifting on insert and delete, typing at a range end continues the mark, toggle, markdown escaping and nesting). **Owner to check** (README in the crate): caret drift where bold or code text is wider (toggle "Show input layer"), a Japanese or Pinyin IME, Portuguese dead keys, wrapping across lines, backspace through a mention. Expected weak point: the caret drifts on wider glyphs; the fallback is hiding the `TextInput` caret and drawing our own from a Rust-side `parley` layout.

**CRDT choice (owner, 2026-11-23): Loro** (`loro` in Rust, `loro-crdt` in the Durable Object), against the Yrs recommendation above. Gains: mark expansion configured once per document and shared by every peer, and concurrent block moves through the movable list. Cost: about 1.4 MB compressed of wasm in the Worker that hosts the document rooms. Before CMS.5, check the website Worker's current compressed size and the Cloudflare plan (3 MB free, 10 MB paid); if it does not fit, the document rooms go in a separate Worker with its own Durable Object binding. Cursors and presence are ours to build (Loro has an ephemeral store for this; no `y-protocols` equivalent is assumed).

**Owner's on-screen checks (2026-11-23): fine.** The owner ran both windows and reported the web view (alignment on resize and scroll, typing into the page) and the rich text checks (caret, IME, dead keys, wrapping, mention) as fine. The overlay approach (transparent `TextInput` over `StyledText`) is the starting point for CMS.5; the `parley` caret fallback stays in reserve.

**Decision 8 status:** confirmed on Windows. Android (JNI `android.webkit.WebView`) and the wasm iframe are still unproven; they are built and tried in the first CMS.5 batch, before the editor depends on them. CMS.0 is closed with that carry-over.

**CMS.1 (2026-11-23): design written, owner's full read pending.** `docs/cms-editor-design.md`: block catalogue (all built-ins plus screenplay; web-only blocks in a native web view loading a website render route), rich text as Loro text with marks (mentions atomic), the Loro layout (movable tree of blocks, node maps, per-locale texts, table cells as texts), import and publish snapshot (mapping in Rust and TypeScript, tested on shared fixtures), keyboard map and markdown shortcuts, phone behaviour, `DocRoom` co-editing with presence, comments in Postgres anchored by stable cursors, locks and visibility, crates `rimelight_content` and `rimelight_editor`. Owner answers: Notion-style selection; locks and visibility enforced by the editor plus the room's filter and a publish check; locale switcher; build the `parley` layout for remote carets; **a draft per author** (each a Loro fork of the page's main document, co-editable by invitation, merged into main on publish or approval). Owner approved the whole design, 2026-11-23.

**Answer 13 changed (owner, 2026-11-23): the content service lives in the website app for now**, in `apps/rimelight.com/src/space/content/`, next to the rest of the Space API, not in a separate package. idantity.me would have to copy it later; a shared package is a later decision. `@rimelight/cms` keeps only what the website still renders with until CMS.7, and is retired in CMS.8.

**Design correction (2026-11-23):** a live Loro room cannot send a partial document, so anyone with edit access to a page sees all its blocks while editing. Per-block visibility is enforced on every read outside the editor (`docs/cms-editor-design.md` section 7).

**CMS.2 done (2026-11-23).** `rimelight-rs/crates/rimelight_content`: `MAPPING.md` (the Rust/TypeScript contract: block JSON, Loro layout, rich text marks, normal form, drafts), `registry/blocks.json` (single source of the block registry; list items may now hold nested lists), serde types, registry, normalize, rich text and document mapping, draft fork, merge and catch-up, four fixture pages. Gate: clippy `-D warnings` clean, `--test mapping` 8 passed (round trip of every fixture, normal form, registry and nesting, draft merge, concurrent drafts, catch-up), wasm32 check clean.

**CMS.3 part 1 done (2026-11-23).** Website repo:
- Screenplay blocks and `ApiDocumentationBlock` are built-ins of `@rimelight/cms`; their renderers moved into the package; the site supplies renderers for the two web-only blocks through a new `renderers` option in `cms.config.ts`.
- `src/space/content/collab/`: the TypeScript side of `MAPPING.md` (`loro-crdt` pinned to `~1.18.0`, matching Rust `loro` 1.18; 1.19.0 was refused by the repo's minimum release age). `registry/blocks.json` and `test/fixtures/content/` are byte copies of the Rust crate's files.
- `src/space/content/`: access rules (`content-access.ts`), routes under `/v1/content` (collections; pages in a scope; drafts per author with members, discard and publish; approve and reject; comments), and the `DocRoom` Durable Object with tickets and a binary socket protocol (`0x00` snapshot, `0x01` Loro update, `0x02` presence, `0x03` JSON event). The room appends each Loro update to its SQLite store and writes a full snapshot only on flush (30 s alarm or last socket close).
- Access rules (confirmed by the owner, 2026-11-23): Project pages follow project roles (admin manage, member edit, viewer view). Team and Department pages: every employee views, members of that team or department edit, global admin manages. Company pages: every employee views, global admin or the new `content_editor` role edits, global admin manages. A draft is open to its author and invited members who also have edit on the page.
- Migrations: CMS package migration `0014_space_scope_and_visibility` (SQLite and Postgres) adds `pages.scope_kind`, `scope_id`, `visibility` and marks every existing page Public; Space migration `0017_content.sql` (mirrored in the website's `drizzle/`) adds `content_docs`, `content_drafts`, `content_draft_members`, `content_comments` and widens `user_roles` to allow `content_editor`. 0014 must run before 0017.
- The app's test config maps `loro-crdt` to its Node build (`test.alias`), because the default bundler build imports the `.wasm` directly and does not start under Node; the Worker keeps the default build.
- Gate: `vp check` clean (1293 files); all 7 test suites pass (the app's 22 files include 69 content tests).
- **Not verified:** no migration applied to any database; the room and routes not run in workerd or against Postgres; the Rust and TypeScript exports of the same Loro bytes not yet compared directly (each side passes the same fixtures); nothing deployed or pushed.
- **Left for CMS.3 part 2:** Space identity in place of `cms_users` and the CMS roles, removal of the Solid dashboard and editor, MCP on Space access, the public read API for the website, and a cross-language byte test.

**CMS.3 part 2 done (2026-11-23).** Owner chose to remove the editing side now (asked first: removal freezes website editing at the next deploy of the website Worker, which also carries the Space API).
- **Removed from `@rimelight/cms`:** the Solid dashboard and block editor, the dashboard route injection, the whole admin HTTP API (CSRF gate, `/me`), the CMS's own identity (`identity/`, Auth0 and Cloudflare Access identifiers), CMS users (`cms_users`, its service, provisioning; migration `0015_drop_cms_users` for SQLite and Postgres), the CMS MCP server, the media upload and library routes and their storage module, and the package exports and dependencies only they used. The package now serves only the anonymous delivery API (`/api/content`, `/api/search`) and the website's block renderers until CMS.7 and CMS.8.
- **Kept on purpose:** `GET /api/cms/media/file/*`, read-only, because the site had no CDN set for CMS media, so published pages link images there (it streams from the `BLOB` bucket, with a test). The media table and services stay until Space's media library (CMS.6).
- **Delivery API:** lists, single pages, docs navigation, markdown and search return only `visibility = 'Public'` pages, and drop blocks marked `visibility: "internal"` (tests added).
- **Website sign-in:** the JWT and session helpers it borrowed from the CMS identity module now live in `apps/rimelight.com/src/auth/jwt.ts` (with tests); `src/auth/identity.ts` is deleted.
- **MCP on Space access:** `POST /v1/content/mcp` (JSON-RPC MCP, Space bearer session, Space access rules): `list_collections`, `list_pages`, `get_page`, `search_pages`, `create_page`, `open_draft`, `get_draft`, `replace_draft_content` (through the draft's live room, so editors see it), `publish_draft` (same code as the REST publish). Tool descriptions and the protocol are in `content-mcp-logic.ts`; the route file holds each tool's code.
- **Public read API:** `/v1/content/public/...` mirrors the CMS delivery routes and shapes (collections, entry by slug, docs nav, taxonomies, bylines, search) over Space data: published, Public pages only; blocks filtered for anonymous readers; mentions of non-Public pages get no link. Differences recorded in `content-public.ts`. **Owner decision (2026-11-23): role-restricted blocks are shown when signed in**, as the CMS did: the website session (cookie or Auth0 bearer) gives the reader its roles, or `member` when it has none; anonymous readers get none; internal blocks are never shown. Responses filtered for a signed-in reader are `private, no-store`, anonymous ones keep shared caching (tests added; the app suite is 25 files, 328 tests).
- `docRoomStub` now returns a written-out `DocRoomRpc` interface (the inferred Durable Object stub type was too deep for the checker).
- **Cross-language test done:** each side writes Loro bytes for every fixture (`fixtures/loro/*.rust.bin`, `*.ts.bin`, committed in both repos) and both sides check that bytes from either language export to the fixture's normal form. Rust 9 mapping tests pass; TypeScript passes on all eight byte files.
- Gate: rimelight-rs clippy `-D warnings` clean; website `vp check` clean (1253 files), all 7 test suites pass (the app: 25 files, 325 tests).
- At the time of this record: migrations not applied, nothing deployed. `rfc/0001` still shows the old `@rimelight/cms/identity` import as a historical example.

**Migrated, pushed and deployed (owner said go, 2026-11-23).**
- **Live database (Neon, through `.dev.vars` `DATABASE_URL`):** the CMS ledger stopped at `0009` in production, so `0010_media_library`, `0011_byline_fields`, `0012_cms_users`, `0013_cms_users_managed_fields` had never run. Applied `0010` to `0015` in order, each in one transaction, with the exact SQL and statement split of `migrateCms` (its CLI and built entry import a Vite-only virtual module and do not start outside Vite). Then Space `0017` through `pnpm db:migrate` (the only pending Drizzle migration; ledger 29 to 30). Checked after: 323 pages, all `Company` and `Public`; `content_docs`, `content_drafts`, `content_draft_members`, `content_comments` and `media_assets` exist; `cms_users` gone; `user_roles` allows `content_editor`.
- **Pushed** rimelight-rs, the website and rimelight-space `main`.
- **Two deploy blockers fixed:** (1) the Worker imported Loro's `.wasm` but the Cloudflare build left it out of the upload. The Worker now uses `loro-crdt/web` with a precompiled `.wasm?module` import behind the app's `#loro` import (`workerd` condition; Node tests get `loro-crdt/nodejs` from the same import, which replaced the test alias). (2) Every website deploy had failed since `e7c56113` (Node 27 alpha, before this work): Workers Builds cannot install Node `27.0.0-alpha.0.1.1`. Owner chose to pin Node `26.13.1` (`.node-version` and the preview workflow); install, check, production build and tests pass on it. Deployed `00573f35` (Workers Builds success).
- **Live checks:** `/v1/health` 200; `/v1/content/public/collections/blog` returns entries; `/v1/content/pages` and `/v1/content/mcp` answer 401 without a session. `/api/content` and `/cms` answer the site's construction-mode page (503), as every non-`/v1` path does while construction mode is on.
- **Watch:** the Worker is about 2.67 MB compressed with Loro's wasm, against 3 MB on the Workers free plan. GitHub Actions jobs still do not start (account billing or spending limit).
- **Still not exercised live:** a signed-in content request, a draft room socket, publish, MCP tool calls.

**CMS.4 (2026-11-23): the move is done; two defaults set** (owner: "keep going until CMS 4, 5 and 6 are done").
- The data move happened with the migrations above: every existing page keeps its id and block JSON, becomes a
  Company-scope page and stays Public. History was not migrated (answer 12): the 0010 to 0015 run kept the CMS tables
  but Space never reads their old versions. Editing is frozen until CMS.5 and CMS.6 ship (answer 11).
- **Languages:** content keeps the locales it is stored in (`en`, `pt`, `es`). They are content locales, separate from
  the app's interface languages (`en`, `pt-BR`), so nothing is converted or dropped; the editor's locale switcher lists
  the locales a page uses (`EditorDoc::locales`).
- **Owners:** existing pages stay at Company scope. CMS.6 adds a "move to project, team or department" action
  (`PATCH /v1/content/pages/:id/scope`, manage access on both scopes), so they can be reassigned from the app.
- Editor API contract for CMS.5: `rimelight-rs/crates/rimelight_content/EDITOR_API.md`.

**CMS.5 and CMS.6 built (2026-11-24); owner's run pending.**
- **rimelight-rs `v0.5.2`** (pinned by every Space rimelight-rs dependency): `rimelight_content::editor` (`EditorDoc`: text with marks, mentions atomic, blocks insert/split/merge/move/indent/outdent/turn-into/duplicate, props, access, tables, undo/redo, locales; registry nesting and template locks enforced; web-only blocks editable) and `::sync` (DocRoom frames, `SyncSession` with outgoing updates and JSON presence). Default content locale `en`: text stored without a locale is the `en` text; the first text typed in another locale makes the field localized and copies `en` (marks included). `rimelight_editor`: the Slint block editor (library markup + `wire_block_editor!` over an `EditorBridge` global; keyboard map and markdown shortcuts of the design, registry-filtered insert menu, mention, emoji and link UI, locale switcher with `en` fallback, comment panel, presence carets and strip, phone layout, virtualised rows, parley caret layout, `WebViewHost`). `rimelight_platform::webview` (`WebViews`: wry child view on Windows, JNI `android.webkit.WebView` on Android, positioned iframe on the web, unsupported elsewhere).
- **Space**: `areas/content` (scope homes for project, team, department and company with collection tabs, search, New page with templates, move to another scope, visibility, delete, Drafts tab; page view and draft editor over the DocRoom socket with presence colours per user, invite, publish/discard, catch-up, comments, share; reviews; collections admin with field editor; media library and picker), `space_client::content` and `content_live`, nav rows (team and department docs, Admin > Content collections and media library, Home > Reviews), shell wiring (macros expanded in `space_ui_slint`), native web views attached through Slint's window handle (`raw-window-handle-06`), hidden while an overlay or dialog is open.
- **Website**: scope move, collections and templates admin, media library routes, drafts list across pages, mention search, templates on page create, `/embed/block` (allowed past construction mode).
- **Gate**: rimelight-rs clippy `-D warnings` clean, every test suite passes (content 23, editor 34, the rest unchanged; FFmpeg from `ffmpeg/current` at the workspace root via `FFMPEG_DIR`), wasm32 check clean. Space clippy `-D warnings` clean, 570 tests pass, `xtask gate targets` passes (host, wasm32, Android arm64). Website check clean, the app's 27 test files (377 tests) and the CMS package's pass; deployed (Workers Builds success).
- **Not verified, owner's run needed:** the app has not been run against the live API: sign-in, opening a scope, creating a page, editing a draft live (two clients), web-only block views, publish and review, media upload, the phone layout on a device. The Android web view and the web iframe are compile-checked only.
- **Known limits:** one live editor per window (the `EditorBridge` global; a second tab showing a draft rebinds it); the interface strings are English literals like the other areas; the reviews page lists Company and project scopes' drafts through the drafts route, which covers every scope.
