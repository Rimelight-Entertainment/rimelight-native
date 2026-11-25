// Compiles ui/main.slint, the app shell. Library paths let the markup import by name:
// - `rimelight_design`: the design tokens (`@rimelight_design/tokens.slint`).
// - `rimelight_ui`: the component library (`@rimelight_ui/components/...`).
// - `rimelight_shell`: the shared shell components (`@rimelight_shell/components/...`).
// - `idantity_feature_<page>`: one per feature crate (`@idantity_feature_<page>/<page>.slint`). Keep this list in step
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
        ("overview", idantity_feature_overview::UI_DIR),
        ("todos", idantity_feature_todos::UI_DIR),
        ("focus", idantity_feature_focus::UI_DIR),
        ("habits", idantity_feature_habits::UI_DIR),
        ("notebooks", idantity_feature_notebooks::UI_DIR),
        ("notes", idantity_feature_notes::UI_DIR),
        ("music", idantity_feature_music::UI_DIR),
        ("watchlist", idantity_feature_watchlist::UI_DIR),
        ("projects", idantity_feature_projects::UI_DIR),
        ("finances", idantity_feature_finances::UI_DIR),
        ("housing", idantity_feature_housing::UI_DIR),
        ("health", idantity_feature_health::UI_DIR),
        ("inbox", idantity_feature_inbox::UI_DIR),
        ("storage", idantity_feature_storage::UI_DIR),
    ];
    for (page, dir) in features {
        println!("cargo::rerun-if-changed={dir}");
        libraries.insert(format!("idantity_feature_{page}"), PathBuf::from(dir));
    }

    // Debug info lets the headless tests find elements by accessible label (i-slint-backend-testing).
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(libraries)
        .with_debug_info(true);
    slint_build::compile_with_config("ui/main.slint", config)
        .expect("slint: ui/main.slint failed to compile");
}
