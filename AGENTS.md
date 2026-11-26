# AI Agent Guidelines

## Tools, Environment & CLI Commands

Run all Cargo commands from the monorepo root (`rimelight-native/`):

```powershell
cargo run -p media_desktop       # Rimelight Media
cargo run -p space_desktop       # Rimelight Space
cargo run -p personal_desktop    # Rimelight Personal
cargo run -p ui_gallery          # Design system component gallery
```

## Layout

- `apps/` — Desktop/web/mobile applications (`rimelight-media`, `rimelight-space`, `rimelight-personal`)
- `packages/` — Shared Rust/Slint libraries (`rimelight_ui`, `rimelight_media`, `rimelight_platform`, etc.)
- `vendor/` — Vendored third-party crates (`ffmpeg-sys-next` with checked-in bindings)

## References

- **README.md**: Overview of the repository with local development and build instructions.
- App-specific `PROJECT.md` files live under each directory in `apps/`; each ends with a Status section that tracks progress.
