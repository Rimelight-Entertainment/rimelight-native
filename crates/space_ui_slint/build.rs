use std::collections::HashMap;
use std::path::PathBuf;

// Compiles the Slint markup that lives in space_ui (decision 0013). The markup stays in `crates/space_ui/ui/`,
// so the UI tests and asset checks keep their paths.
// slint-build reports every file main.slint imports to cargo, so a gallery-only file (the stories in ui/stories/, which
// the shared gallery in rimelight-rs loads) does not rebuild this crate.
fn main() {
    // `@rimelight_design/tokens.slint` is the shared design tokens (packages/design in the web monorepo).
    // `@rimelight_ui/...` is the shared component library (rimelight-rs, ST2.3); `@rimelight_shell/...` the shared shell (ST2.5a).
    let config = slint_build::CompilerConfiguration::new().with_library_paths(HashMap::from([
        (
            "rimelight_design".to_string(),
            PathBuf::from(rimelight_design::UI_DIR),
        ),
        (
            "rimelight_ui".to_string(),
            PathBuf::from(rimelight_ui::UI_DIR),
        ),
        (
            "rimelight_shell".to_string(),
            PathBuf::from(rimelight_shell::UI_DIR),
        ),
        (
            "space_area_company".to_string(),
            PathBuf::from(space_area_company::UI_DIR),
        ),
        (
            "space_area_management".to_string(),
            PathBuf::from(space_area_management::UI_DIR),
        ),
        (
            "space_area_admin".to_string(),
            PathBuf::from(space_area_admin::UI_DIR),
        ),
        (
            "space_area_projects".to_string(),
            PathBuf::from(space_area_projects::UI_DIR),
        ),
        (
            "space_area_home".to_string(),
            PathBuf::from(space_area_home::UI_DIR),
        ),
        (
            "space_area_content".to_string(),
            PathBuf::from(space_area_content::UI_DIR),
        ),
        (
            "rimelight_editor".to_string(),
            PathBuf::from(rimelight_editor::UI_DIR),
        ),
        (
            "space_area_canvas".to_string(),
            // By path, not through the crate: the canvas crate links FFmpeg, whose DLLs a build script cannot load.
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../areas/canvas/ui"),
        ),
    ]));
    slint_build::compile_with_config("../space_ui/ui/main.slint", config)
        .expect("slint: ui/main.slint failed to compile");
}
