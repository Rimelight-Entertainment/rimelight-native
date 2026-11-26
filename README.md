# rimelight-native

Rimelight Rust monorepo — apps and shared packages.

## Structure

```
apps/
  rimelight-media/      ← Media: pictures, video, music, screenshots, screen recording
  rimelight-space/      ← Space: collaborative whiteboard and canvas
  rimelight-personal/   ← Personal: todos, notes, habits, focus, finances, health…
packages/
  rimelight_ui/         ← Slint component library (markup + fonts + icons)
  rimelight_ui_kit/     ← Slint glue: plain-Rust rules + macros
  rimelight_shell/      ← Shared shell markup
  rimelight_media/      ← FFmpeg + cpal media engine
  rimelight_picture/    ← Picture decoding, thumbnails, animation
  rimelight_sound/      ← Sound utilities
  rimelight_platform/   ← OS calls: Windows, Android
  rimelight_update/     ← Self-update (Velopack)
  rimelight_content/    ← Block content / Loro CRDT
  rimelight_editor/     ← Block editor controller + markup
  gallery/              ← Component story gallery runner
  ui_check/             ← Slint compile-check harness
vendor/
  ffmpeg-sys-next/      ← FFmpeg bindings (checked in; no clang/headers needed)
```

## Getting started

```sh
# Build a specific app
cargo build -p rimelight-media

# Run the component gallery
cargo run -p rimelight_gallery
```

## Shared packages

Packages in `packages/` are consumed by all apps as local path dependencies — no git tags or network required after cloning this repo.

## License

MIT License. See [LICENSE.md](LICENSE.md) for more details.