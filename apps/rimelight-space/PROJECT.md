# Rimelight Space

## 1. Vision & Scope

**Rimelight Space** is a native Enterprise Resource Planning (ERP) and operational workstation for Rimelight Entertainment, built in Rust and Slint. It unifies company management, creative production, game development workflows, customer support and work productivity into a single high-performance native binary.

### 1.1 Key Pillars

1. **Native Multiplatform Performance:** Built in Rust and Slint, compiling to native desktop clients (Windows, macOS, Linux) and mobile (Android, iOS), alongside a WebAssembly target.
2. **Unified Operations:** Replaces fragmented third-party SaaS tools by unifying HR, finance, supply chain, asset tracking, project management, customer relationships and player support in one database and UI.
3. **Enterprise-Grade from the Start:** Built for 250+ users, with a relationship-based policy engine, audit trails and a documented public API.
4. **Shared Rimelight Design Tokens:** Maintains visual consistency with the Rimelight ecosystem.

### 1.2 Technical Architecture

- **Language:** Rust
- **GUI:** Slint
- **Decoder:** FFmpeg
- **Asset Storage:** Cloudflare R2
- **Database:** Neon PostgreSQL (futurely PlanetScale) + Cloudflare Hyperdrive
- **Authentication:** Auth0
- **API:** Hono
- **Installer & Updater:** Velopack

### 1.3 Target Platforms

- **Desktop:**
  - **Windows:**
  - **MacOS:**
  - **Linux:**
- **Mobile:**
  - **Android:**
  - **IOS:**

### 1.4 Target Languages

- English (`en`)
- Brazilian Portuguese (`pt-BR`)

### 1.5 Out of Scope

- Offline Support

## 2. Features

### 2.1 Features

#### 2.1.1 Personal Workplace & Communication
- **Personal Tasks & Todos:** Focus modes, task queues, daily schedules, and personal calendar.
- **Unified Chat:** Real-time team messaging, channels, direct messages, and contextual thread discussions.
- **Personal Notes & Scratchpads:** Quick-capture rich text notes with markdown rendering.

#### 2.1.2 Projects & Issue Tracking
- **Issue Tracking:** Kanban boards, sprint backlogs, task assignments, and priority triage.
- **Milestones & Slices:** Vertical slice tracking with verifiable criteria and gate validation.
- **Game Production Workflows:** Asset pipeline tracking linking Unreal Engine and game assets.

#### 2.1.3 Company, People & HR
- **Directory & Org Chart:** Team rosters, departmental structures, reporting lines, and access roles.
- **Time Off & Leave:** Vacation tracking, public holiday calendars, and approval workflows.
- **Recruitment & Onboarding:** Candidate tracking, onboarding checklists, and role provisioning.

#### 2.1.4 Finance & Operations
- **Corporate Accounting:** Expense tracking, invoice generation, balance ledgers, and transaction audit trails.
- **Hardware & Asset Inventory:** Machine allocation, licenses, devkit assignments, and office procurement.

#### 2.1.5 Player Support & Community Operations
- **Customer CRM:** Unified support ticket inbox, game account lookups, and player feedback.
- **Moderation Tools:** In-game audit log inspection and player dispute resolution.

#### 2.1.6 CMS Block Editor Studio
*Canonical authoring tool for wiki, docs, blog, and screenplay content.*
- **Co-Editing:** Multi-user character-level collaborative editing with cursors and presence via Loro CRDT.
- **Adaptive Layout:** Inline WYSIWYG editing with side inspector panel (or bottom sheet on mobile).
- **Block Catalogue:**
  - *Text Primitives:* Paragraph, Section (nesting 2–6 deep), Callout (with status variants), Quote, Code (with language highlighting and line numbers).
  - *Lists:* Unordered List, Ordered List, List Item (with tab-indent nesting).
  - *Media & Data:* Image (with caption, sizing, alt text), Table (cell editing, border toggle), Divider, File Tree (interactive directory hierarchy).
  - *Interactive Elements:* Tabs & Tab Item, Steps & Step Item, Cards & Card (with badges, icons, links).
  - *Screenplay Blocks:* Scene heading, action, character, dialogue, parenthetical, transition, and shot blocks for franchise narrative writing.

### 2.2 Keyboard Shortcuts

| Context | Shortcut | Action |
|---|---|---|
| **Global** | `` | |

## 3. Status

### 3.1 Slices

| ID | Slice | Status | Summary |
|---|---|---|---|
| CMS | Content (wiki, docs, blog, franchise) moves from the rimelight.com CMS into Space | **next (main track)** | Per-scope content with a Slint block editor in `rimelight-rs`, CRDT co-editing and phone parity; rimelight.com renders published content at today's URLs. Owner review 1 done 2026-11-24. Steps CMS.0–CMS.8 in decision 0049 |
| S0 | Shell baseline | done | Rail, sidebar, tabs, splits, status bar, quick action, palette, phone shell. Milestone gate passed 2026-11-12 |
| SA | Sign-in (Auth0) | in progress | Backend-brokered sign-in via the shared rimelight.com app (decision 0014). Desktop, web and Android built; online only with a full-window "Connection lost" overlay (decision 0035). Not yet run against Auth0 or on a device |
| SP | People: profiles, directory, org chart, user admin | in progress | Schema, 300-person fixture seed, People API, Employees, Profile, Teams / org chart, Departments and Admin pages (decisions 0019–0025). Owner review pending |
| SP.5 | Positions as seats | in progress | Seats backend, team budgets, org chart from seats, Seats and Budgets admin tabs (decisions 0036, 0042). Owner review pending |
| S1 | Projects > Issues | in progress | Schema and seed, projects API, YouTrack-style search, issue list, detail, board, backlog and sprints (decisions 0026–0032). Owner review pending |
| UI1 | Shell: profile menu, Give Feedback, Home > Performance | in progress | Built (decision 0040); owner review in the app pending |
| BT | Build time | in progress | .slint edit in the hook clippy cut from 9m16s to ~20–28 s (decisions 0037, 0043, 0044). BT.3 deferred |
| S1.5 | YouTrack-style issue page and New issue dialog | paused (0049) | Votes, stars, spent time, attachments, issue page and dialog built. Open: thumbnails, opening attachments, Markdown rendering |
| CV | Canvases (personal and project whiteboards) | paused (0049) | CV.0–CV.13 built, three owner review rounds (decision 0045) |
| CV-V | Video on canvases | paused (0049) | Upload, playback and controls verified by the owner; "Play for everyone" built but not deployed or tried with two people (decision 0046) |
| REL | Windows release pipeline and auto-update | paused (0049) | Update path proven with a spike; REL.1–REL.6 in decision 0047 |
| W1 | Rust Worker backend | superseded | Replaced by the TypeScript API on the website Worker (decision 0041) |
| S2 | Home > My Work | planned | Assigned issues, today's calendar, unread count, timer |
| S3 | Calendar | planned | Month and week views, day picker, project and leave overlays |
| S4 | Chat | planned | Channel, chat and room lists, thread view, unread badges |
| S5 | Projects > Overview, sprints, timeline | planned | Project overview, sprint list, timeline view |

Not yet scheduled:

- Company
- Finance
- Customers
- Operations
- Analytics
- Admin
- Mail
- Public API

### 3.2 Product decisions

| Topic | Decision |
|---|---|
| Scale target | 250+ users within two years. Enterprise-grade controls and audit from the start |
| Company context | Game studio. Company formally starts after the owner's move to the UK. Single country, single legal entity (UK) |
| Relation to rimelight.com | Separate database and backend, shared sign-in. rimelight.com renders the public pages (careers, support form, share links) by calling the Rimelight Space API. No access to rimelight.com's store or forum data. Space owns all CMS content and rimelight.com renders the published part |
| AI | None for now. Leave a seam |
| Accessibility | Keyboard navigation and WCAG AA contrast in both themes, reduced motion, scalable text. Screen readers where Slint supports them (decision 0007) |

### 3.3 Open questions

| # | Question | Notes |
|---|---|---|
| Q1 |  |  |