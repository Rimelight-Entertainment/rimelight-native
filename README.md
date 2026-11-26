# rimelight-native

The monorepository for native rust-based apps and packages by Rimelight Entertainment

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

## License

MIT License. See [LICENSE.md](LICENSE.md) for more details.