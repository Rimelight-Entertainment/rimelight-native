# Rimelight Personal

## Overview

**Rimelight Personal** is a cross-platform personal productivity and life management suite, built with Rust and Slint.

Its infrastructure lives within your own Cloudflare account. 

## Features

- **Productivity:** Overview dashboard, Tasks & Todos, Focus timer, Notes, Habits, Notebooks, and Projects.
- **Lifestyle & Health:** Intermittent fasting, workout logs, body metrics, and household management.
- **H
- **Finances:** Multi-account ledgers, recurring bills, active subscriptions, and net worth tracking (in BRL).
- **Communication & Media:** Unified inbox (Cloudflare Email Routing), cloud storage, watchlist, and music player.

## Local Development

### Prerequisites

- **Rust:** `1.100.0`

### Build Commands

```bash
# Build desktop native application
cargo build -p desktop

# Run desktop native application
cargo run -p desktop

# Run full workspace unit tests
cargo test --workspace --lib

# Build WebAssembly target
cargo build -p web --target wasm32-unknown-unknown
```

## Repository Structure

```
rimelight-personal/
├── apps/                        # Application entry points (apps/desktop, apps/web)
├── crates/
│   ├── core/                    # API client, offline cache, and domain models
│   ├── rimelight_personal_nav/            # Navigation model and sidebar state
│   ├── rimelight_personal_ui_slint/       # Slint compilation target & shell bindings
│   └── features/                # 14 modular domain crates (todos, notes, habits, etc.)
├── AGENTS.md                    # Agent operating rules & "build once" constraint
├── README.md                    # Project overview & local development guide (this file)
└── PROJECT.md                   # Comprehensive technical manual & feature catalog
```

## License

MIT License. See [LICENSE.md](LICENSE.md) for more details.
