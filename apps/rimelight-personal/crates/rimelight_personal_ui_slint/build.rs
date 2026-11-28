// Compiles ui/app.slint, the app shell. Library paths let the markup import by name:
// - `rimelight_design`: the design tokens (`@rimelight_design/tokens.slint`).
// - `rimelight_ui`: the component library (`@rimelight_ui/components/...`).
// - `rimelight_shell`: the shared shell components (`@rimelight_shell/components/...`).
// - `rimelight_personal_feature_<page>`: one per feature crate (`@rimelight_personal_feature_<page>/<page>.slint`). Keep this list in step
//   with the feature crates in Cargo.toml. A new page adds its line here.
// Only this crate compiles Slint for the app; feature crates ship markup and plain Rust (crates/features/README.md).
use std::collections::HashMap;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=ui");

    let mut libraries: HashMap<String, PathBuf> = HashMap::from([
        (
            "rimelight_design".to_owned(),
            PathBuf::from(rimelight_design::UI_DIR),
        ),
        (
            "rimelight_ui".to_owned(),
            PathBuf::from(rimelight_ui::UI_DIR),
        ),
        (
            "rimelight_shell".to_owned(),
            PathBuf::from(rimelight_shell::UI_DIR),
        ),
    ]);
    let features: [(&str, &str); 14] = [
        ("overview", rimelight_personal_feature_overview::UI_DIR),
        ("todos", rimelight_personal_feature_todos::UI_DIR),
        ("focus", rimelight_personal_feature_focus::UI_DIR),
        ("habits", rimelight_personal_feature_habits::UI_DIR),
        ("notebooks", rimelight_personal_feature_notebooks::UI_DIR),
        ("notes", rimelight_personal_feature_notes::UI_DIR),
        ("music", rimelight_personal_feature_music::UI_DIR),
        ("watchlist", rimelight_personal_feature_watchlist::UI_DIR),
        ("projects", rimelight_personal_feature_projects::UI_DIR),
        ("finances", rimelight_personal_feature_finances::UI_DIR),
        ("housing", rimelight_personal_feature_housing::UI_DIR),
        ("health", rimelight_personal_feature_health::UI_DIR),
        ("inbox", rimelight_personal_feature_inbox::UI_DIR),
        ("storage", rimelight_personal_feature_storage::UI_DIR),
    ];
    for (page, dir) in features {
        println!("cargo::rerun-if-changed={dir}");
        libraries.insert(
            format!("rimelight_personal_feature_{page}"),
            PathBuf::from(dir),
        );
    }

    // Debug info lets the headless tests find elements by accessible label (i-slint-backend-testing).
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(libraries)
        .with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("slint: ui/app.slint failed to compile");
}
