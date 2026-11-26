# Rimelight Space

built with Rust and Slint.

## Overview

**Rimelight Space** unifies operational management, HR, corporate accounting, customer support, and CMS authoring into a single native multiplatform application.

## Features

## Local Development

### Prerequisites

- **Rust:** `1.100.0`
- 
### Initial Build & Verification

To run verification gates across the workspace:

```bash
# Run the quick verification gate
cargo xtask gate quick

# Run full workspace unit tests
cargo test --workspace --lib

# Run UI test suite
cargo test --workspace --test ui
```

## Repository Structure

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