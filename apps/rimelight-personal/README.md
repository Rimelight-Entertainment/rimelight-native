# Rimelight Personal

## Overview

**Rimelight Personal** is a cross-platform personal productivity and life management suite, built with Rust and Slint.

Its data lives in the backend instance you point it at. The app does not ship with a deployment: it connects to the
instance named by the environment variables below.

## Features

- **Productivity:** Overview dashboard, Tasks & Todos, Focus timer, Notes, Habits, Notebooks, and Projects.
- **Lifestyle & Health:** Intermittent fasting, workout logs, body metrics, and household management.
- **Finances:** Multi-account ledgers, recurring bills, active subscriptions, and net worth tracking (in BRL).
- **Communication & Media:** Unified inbox (Cloudflare Email Routing), cloud storage, watchlist, and music player.

## Connecting to an instance

Three variables name the instance. None has a default; the desktop app stops with a message if one is missing.

| Variable | Meaning | Example |
|---|---|---|
| `RIMELIGHT_API_BASE` | API root (no trailing slash). Cloudflare Access protects this URL. | `https://example.com/api/personal` |
| `RIMELIGHT_ACCESS_AUD` | The Access application's AUD tag (not a secret). | the 64-character tag from the Access app page |
| `RIMELIGHT_ACCESS_TEAM_ORIGIN` | Your Access team's login host. | `https://your-team.cloudflareaccess.com` |

The web build reads the same three names at build time (`trunk build` in `apps/web`).

```powershell
$env:RIMELIGHT_API_BASE = "https://example.com/api/personal"
$env:RIMELIGHT_ACCESS_AUD = "<aud tag>"
$env:RIMELIGHT_ACCESS_TEAM_ORIGIN = "https://your-team.cloudflareaccess.com"
cargo run -p personal_desktop
```

## Local Development

### Prerequisites

- **Rust:** `1.100.0` (pinned in the monorepo's `rust-toolchain.toml`)

### Build Commands

Run from the monorepo root (`rimelight-native/`):

```powershell
# Run the desktop app (needs the RIMELIGHT_* variables above)
cargo run -p personal_desktop

# Run the unit tests of the personal crates
cargo test -p rimelight_personal_core -p rimelight_personal_ui_slint --lib

# Build the WebAssembly target (from apps/rimelight-personal/apps/web, with trunk installed)
trunk build --release
```

## Repository Structure

```
apps/rimelight-personal/
├── apps/
│   ├── desktop/                 # Native Windows/macOS/Linux entry point (personal_desktop)
│   └── web/                     # WebAssembly entry point (rimelight-personal-web)
├── crates/
│   ├── core/                    # API client, instance config, sign-in and domain models
│   ├── rimelight_personal_nav/  # Navigation model and sidebar state
│   ├── rimelight_personal_ui_slint/  # Slint shell and page glue
│   └── features/                # One crate per page (todos, notes, habits, ...)
└── PROJECT.md                   # Technical manual, feature catalog and migration plan
```

## License

MIT. See the `LICENSE` file at the monorepo root.
