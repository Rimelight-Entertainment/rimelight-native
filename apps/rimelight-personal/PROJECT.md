# Rimelight Personal

## 1. Vision & Scope

**Rimelight Personal** is a native multiplatform personal productivity suite and life operating system built in Rust and Slint, bringing every private-life domain into a single high-performance native binary.

### 1.1 Key Pillars

1. **Native Multiplatform Performance:** Built in Rust and Slint, compiling to high-performance native desktop clients (Windows, macOS, Linux) and mobile (Android, iOS), alongside a WebAssembly target.
2. **Unified Life Dashboard:** Aggregates 13 distinct life domains (tasks, calendar, finances, notes, health, housing, habits, and media) into a unified personal workstation.
3. **User-owned Data:** Connects to a user's Cloudflare account and infrastructure, authenticated securely through Cloudflare Access.
4. **Shared Rimelight Design Tokens:** Maintains visual consistentency with the Rimelight ecosystem.

### 1.2 Technical Architecture

- **Language:** Rust 2024
- **GUI:** Slint
- **Decoder:** FFmpeg
- **Asset Storage:** Cloudflare R2
- **Database:** Cloudflare D1
- **Authentication:** Cloudflare Access
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

### 1.4 Out of Scope

- Offline Support

## 2. Features

### 2.1 Features

#### 2.1.1 Overview (`/`)
* **Unified Command Center:** Dynamic hero dashboard bringing all active modules together.
* **Quick Task Input:** Instant task capture submitting to the daily todo queue.
* **Daily Agenda:** Filtered list of tasks due today with one-click completion toggles.
* **Financial Summary:** Liquid balances, monthly fixed bills total, active subscription counts, and debt ledger ("I owe" / "Owed to me").
* **Module Shortcuts:** Fast-action widgets for active fasting timers, workout logs, habits, and pinned notes.

#### 2.1.2 Todos (`/todos`)
* **Task Management:** Full task creation with title, priority (low, medium, high, urgent), categories, and scheduled due dates.
* **Filter Views:** Dedicated views for Today, Upcoming, Completed, and Categorized tasks.
* **Batch Operations:** Multi-select toolbar supporting batch completion and deletion.

#### 2.1.3 Focus (`/focus`)
* **Productivity Timer:** Pomodoro and interval focus timer with configurable work/break cycles.
* **Ambient Audio:** Integrated background ambient noise player for deep work sessions.

#### 2.1.4 Notes (`/notes`)
* **Organization:** Tabbed categorization (Active, Archived, Trash) with real-time text and tag search.
* **Rich Editor:** Debounced autosave, color coding, tag chip management, and quick note modals.
* **Pinned Notes:** Dedicated top tier for frequently referenced documentation.

#### 2.1.5 Habits (`/habits`)
* **Calendar Mode:** 12-month visual grid tracking completion days across custom categories.
* **Streaks Mode:** 52-week activity heatmap (365 days) displaying current streaks, historical best streaks, and annual completion percentage.

#### 2.1.6 Watchlist (`/watchlist`)
* **Media Catalog:** Tracking for Movies, TV Shows, Anime, and Books.
* **Statuses:** Categorized into Want to Watch/Read, In Progress, Completed, and Dropped.

#### 2.1.7 Music (`/music`)
* **Audio Player:** Album drop zone, playlist queue, local audio playback, and metadata tags.

#### 2.1.8 Notebooks (`/notebooks`)
* **Hierarchical Documentation:** Structured multi-page notebooks divided into volumes and chapters for deep writing.

#### 2.1.9 Projects (`/projects`)
* **Kanban Boards:** Customizable columns, drag-and-drop card movements, task checklists, and sprint tracking.

#### 2.1.10 Finances (`/finances`)
* **Accounting:** Multi-account balances (checking, savings, investments), transaction logs, income vs expenses breakdown.
* **Currency Support:** Primary tracking in BRL with automatic exchange rate conversion via Frankfurter API.
* **Recurring Obligations:** Scheduled bill reminders and active subscription renewals.

#### 2.1.11 Housing (`/housing`)
* **Household Inventory:** Grocery lists, out-of-stock alerts, household supplies tracking.
* **Home Maintenance:** Recurring chore schedules, appliance warranties, and home repair logs.

#### 2.1.12 Health (`/health`)
* **Biometrics:** Weight, body fat, and physical measurement logging over time with progress charts.
* **Intermittent Fasting:** Live fasting timer tracking hours elapsed and target fasting windows.
* **Workouts:** Routine templates and exercise set/rep/weight logging.

#### 2.1.13 Inbox & Storage (`/inbox`, `/storage`)
* **Unified Communications:** Cloudflare Email Routing integration directing incoming emails (`daniel@`, `hello@`, `admin@`) into a triage inbox.
* **Cloud Storage Explorer:** File and document management with category tagging and file upload pipelines.

### 2.2 Keyboard Shortcuts

| Context | Shortcut | Action |
|---|---|---|
| **Global** | `` | |

## 3. Status

> **Current Milestone:** Open-source, self-hosted restructure (decided 2026-11-24), alongside the remaining feature porting.  
> **Target Alignment:** Mirrors the modules defined in PROJECT.md. PROJECT.md, README.md and AGENTS.md still describe the idantity.me backend and get rewritten in step R1.6.  
> **Status:** Plan drafted, nothing started. Build rule applies: every step below is written without building; one build and test pass runs at the end of each batch (AGENTS.md).

### 3.5 Open questions

| # | Question | Notes |
|---|---|
| Q1 |  |  |
