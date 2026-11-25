# Rimelight Personal

> **Native Multiplatform Personal Productivity Suite & Life OS**  
> Built in Rust 2024 with Slint, powered by Cloudflare D1 + Access. Formerly known as `idantity-native`.

---

## 1. Overview

**Rimelight Personal** is the official native client replacing the legacy web application `personal.idantity.me`. It unifies 13 core life management domains into a fast, fluid desktop and web workspace:

- **Productivity:** Overview dashboard, Tasks & Todos, Focus timer, Notes, Habits, Notebooks, and Projects.
- **Lifestyle & Health:** Intermittent fasting, workout logs, body metrics, and household management.
- **Finances:** Multi-account ledgers, recurring bills, active subscriptions, and net worth tracking (in BRL).
- **Communication & Media:** Unified inbox (Cloudflare Email Routing), cloud storage, watchlist, and music player.

---

## 2. Standard Documentation Suite

This repository adheres to the standardized 4-file documentation architecture:

| Document | Purpose |
|---|---|
| [**README.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-personal/README.md) | Simple overview of Rimelight Personal with local development and build instructions (this file). |
| [**PROJECT.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-personal/PROJECT.md) | Comprehensive description: technical architecture, 13-module feature catalog, and design tokens. |
| [**PLAN.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-personal/PLAN.md) | Progress tracker: page-by-page porting matrix, decision log, and web retirement checklist. |
| [**AGENTS.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-personal/AGENTS.md) | Specific agent instructions: strict "build once" rule, gate checks, and Cloudflare Access auth rules. |

---

## 3. Local Development Quickstart

### Prerequisites

- **Rust:** Pinned to `1.100.0` via `rust-toolchain.toml` (2024 edition).
- **Network Access:** Required on initial build to fetch Git dependencies (`rimelight-rs` and `rimelight` design tokens).

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

> [!IMPORTANT]
> **Owner Rule:** Write features first in batches. **Build ONCE at the very end** of a task batch to prevent disk bloat and compiler lock contention.

---

## 4. Repository Layout

```
rimelight-personal/
├── apps/                        # Application entry points (apps/desktop, apps/web)
├── crates/
│   ├── core/                    # API client, offline cache, and domain models
│   ├── idantity_nav/            # Navigation model and sidebar state
│   ├── idantity_ui_slint/       # Slint compilation target & shell bindings
│   └── features/                # 14 modular domain crates (todos, notes, habits, etc.)
├── AGENTS.md                    # Agent operating rules & "build once" constraint
├── README.md                    # Project overview & local development guide (this file)
├── PROJECT.md                   # Comprehensive technical manual & feature catalog
└── PLAN.md                      # Feature porting progress matrix & decision log
```

---

## 5. License

Proprietary. Copyright (c) 2026 Rimelight Entertainment / Daniel Marchi. All rights reserved.
