# Space gallery

`space_gallery` is Space's component gallery. It is the shared `rimelight_gallery` (rimelight-rs `apps/gallery`) with
Space's own stories added, plus one capture that Space owns: the whole app shell.

The shared gallery and its flags are documented in rimelight-rs: `apps/gallery/README.md` in the
`Rimelight-Entertainment/rimelight-rs` repository.

## Commands

```
cargo run -p space_gallery
cargo run -p space_gallery -- --story components/board-page --theme dark
cargo run -p space_gallery -- --list-stories
cargo run -p space_gallery -- --story shell/desktop-default --screenshot target/shell.png --theme light --size 1280x800
cargo xtask screenshot shell/desktop-default --out target/shell.png --theme light --size 1280x800
```

The last command builds `space_gallery`, captures the shell state, and checks the PNG (see `xtask/src/screenshot.rs`).

## Shell captures

`--story shell/<state> --screenshot <png>` builds Space's real shell (`space_ui::slint_ui::shell_capture`) in the state
named after `shell/`, and writes one frame. `--theme light|dark|system` (default system) and `--size WxH` (default
1280x800) apply. The list of states is `STATES` in `crates/space_ui/src/slint_ui/shell_capture.rs`.

## Stories

Space's stories are in `crates/space_ui/ui/stories/`, the default `--stories` folder:

- `stories.txt`: one story per line, `section/name|Title|variants`. The line order is the story index.
- `stories.slint`: exports `AppStories`, which shows story N for `story == N`. Each story is a component that uses
  `@rimelight_design`, `@rimelight_ui` and Space's own components.

To add a story: add a line to `stories.txt`, add its component and an `if root.story == N` branch to `stories.slint`.
The gallery test (`space_stories_compile` in `src/main.rs`) checks that the markup compiles and the list parses.
