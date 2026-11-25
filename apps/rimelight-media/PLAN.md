# Rimelight Media — Development Plan & Progress Tracker

> **Current Milestone:** Windows Desktop Shell & Codec Stabilization  
> **Target Alignment:** Mirrors the architecture and features defined in [PROJECT.md](file:///C:/Users/idant/RustRoverProjects/rimelight-media/PROJECT.md).  
> **Status:** Live Feature Maturity Matrix & Verification Tracker.

---

## 1. Feature Completion Matrix

| Module | Feature | Status | Implementation Details & Next Steps |
|---|---|---|---|
| **Pictures** | Basic Image Viewer | ✅ Done | Zoom, pan, multi-format decoders, animated GIF/WebP playback. |
| | Side-by-Side Comparison | ✅ Done | Multi-image selection compare grid. |
| | Vector Markup & Editing | ✅ Done | Pen, highlighter, shapes, crop, rotation, image paste. |
| | Camera RAW Support | 🟨 Partial | Reads common camera RAW formats (DNG, CR2/3, NEF); add thumbnail cache. |
| **Canvas** | Paint Replacement | ✅ Done | Blank canvas, drawing tools, rectangle selection, PNG export. |
| **Video/Audio** | FFmpeg Player Engine | ✅ Done | Hardware-accelerated decode, multi-audio tracks, subtitles. |
| | Waveform Trimming | ✅ Done | Seek bar with waveform visualization and trim handles. |
| | Details Panels | 🟨 In Review | Video, audio, and text metadata panels built; pending smoke verification on Windows. |
| | Playlists | 🟨 In Review | Audio and video queue management built; pending verification. |
| **Text** | Notepad Replacement | ✅ Done | Text editing, encoding preservation, line ending handling. |
| | Markdown Split View | ✅ Done | Live preview pane alongside raw source. |
| | JSON Inspector | ✅ Done | Formatter, minifier, key sorter, syntax validator. |
| **Capture** | Global Screen Snip | ✅ Done | `Win+Shift+S` hook, area selection, direct markup opening. |
| | Screen Recorder | 🟨 In Review | Audio loopback and microphone MP4 recording. |
| **Camera** | Webcam Stills & Video | ✅ Done | Camera roll integration, grid, timer, and resolution selection. |
| **Voice** | Voice Recorder | ✅ Done | Microphone recording to M4A/WAV/FLAC with waveform markers. |
| **Installer** | Velopack Auto-Update | ✅ Done | Per-user setup, background delta updates, file associations. |

---

## 2. Active Verification Checklist (Owner PC)

- [ ] **Details Panels:** Test Audio, Video, and Text inspector panels for layout accuracy and responsiveness.
- [ ] **Playlists (Audio/Video):** Verify queue persistence and shuffle/repeat modes across long sessions.
- [ ] **Image Slideshow:** Verify transition timings and memory cleanup during folder traversal.
- [ ] **Files Tab:** Verify navigation and context menu actions in the integrated folder browser.
- [ ] **Round 3 Fixes:** Verify high-DPI scaling on 4K displays and multi-monitor snip coordinates.

---

## 3. Immediate Next Steps

1. **Benchmarking Suite:** Run `tools/bench.ps1` to confirm memory footprint remains < 60 MB under typical workloads.
2. **Release Tagging:** Push next release tag (`v0.X.X`) to trigger GitHub Actions Velopack installer build.
3. **File Associations:** Validate default Windows application registration for HEIC and AVIF files.
