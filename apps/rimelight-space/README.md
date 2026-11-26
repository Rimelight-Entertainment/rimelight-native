# Rimelight Space

> **Internal Enterprise Resource Planning (ERP) & Workstation for Rimelight Entertainment.**  
> Built in Rust with Slint 1.18.1, powered by Loro CRDT, and backed by the Space API on the rimelight.com Worker.

---

## 1. Overview

**Rimelight Space** unifies operational management, software and game production workflows, HR, corporate accounting, player support, and CMS authoring into a single native multiplatform application.

- **Frontend:** Rust 2024 + Slint 1.18.1, featuring accessible themes, native performance, and real-time co-editing via Loro CRDT.
- **Backend:** TypeScript Space API on the rimelight.com Worker (repo `Rimelight-Entertainment/rimelight`) querying Neon PostgreSQL via Cloudflare Hyperdrive.
- **Design System:** Pinned shared tokens from `rimelight_design` and shared UI components from `rimelight_ui`.

---

## 2. Standard Documentation Suite

Our repository adheres to the standardized 4-file documentation architecture:

| Document | Purpose |
|---|---|
| [**README.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-space/README.md) | Simple overview of Rimelight Space with local development and build instructions (this file). |
| [**PROJECT.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-space/PROJECT.md) | Comprehensive description: architecture, product modules, CMS block editor specifications, and conventions. |
| [**AGENTS.md**](file:///C:/Users/idant/RustRoverProjects/rimelight-space/AGENTS.md) | Specific agent instructions: strict "build once" rule, gate checks, cargo workspace scoping, and verification rules. |

---

## 3. Local Development Quickstart

### Prerequisites

- **Rust:** Pinned via `rust-toolchain.toml` (1.100.0+ / 2024 edition).
- **Target Folder:** Cargo uses the default `target/` folder in the workspace. No `CARGO_TARGET_DIR` override is set.
- **Git Dependencies:** Access to `Rimelight-Entertainment/rimelight` and `Rimelight-Entertainment/rimelight-rs`.

### Initial Build & Verification

> [!IMPORTANT]
> **Owner Rule:** Write features first in batches. **Build ONCE at the very end** of a task batch to avoid disk thrashing and system lag.

To run verification gates across the workspace:
```bash
# Run the quick verification gate
cargo xtask gate quick

# Run full workspace unit tests
cargo test --workspace --lib

# Run UI test suite
cargo test --workspace --test ui
```

---

## 4. Repository Layout

```
rimelight-space/
├── apps/                        # Application binaries (apps/desktop)
├── areas/                       # Functional areas (company, management, personal, projects)
├── backend/                     # Rust helper crates (Postgres migration SQL, jobs placeholder); the API is on rimelight.com
├── crates/                      # Core and UI support crates (space_ui, space_page_kit)
├── release/                     # Velopack release scripts and manifests
├── xtask/                       # Monorepo build and gate automation
├── AGENTS.md                    # Agent operating manual & "build once" constraint
├── README.md                    # Project overview & local development guide (this file)
└── PROJECT.md                   # Comprehensive technical manual & architecture
```

## License

MIT License. See [LICENSE.md](LICENSE.md) for more details.