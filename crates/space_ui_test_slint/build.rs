// Compiles each test markup file in `../space_ui/tests/ui/markup/` into its own Rust module (decision 0037,
// BT.4). Imports such as "ui/components/banner.slint" resolve against the space_ui crate root, as they did in the
// `slint!` macro. The files compile on parallel threads.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{env, fs, thread};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let ui_crate = manifest
        .join("../space_ui")
        .canonicalize()
        .expect("space_ui crate dir");
    let markup = ui_crate.join("tests/ui/markup");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo::rerun-if-changed={}", markup.display());

    let mut names: Vec<String> = fs::read_dir(&markup)
        .expect("read tests/ui/markup")
        .filter_map(|e| {
            let path = e.ok()?.path();
            (path.extension()? == "slint").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();

    let deps: Vec<Vec<PathBuf>> = thread::scope(|s| {
        let jobs: Vec<_> = names
            .iter()
            .map(|name| {
                let (markup, out, ui_crate) = (&markup, &out, &ui_crate);
                s.spawn(move || {
                    compile(
                        &markup.join(format!("{name}.slint")),
                        &out.join(format!("{name}.rs")),
                        ui_crate,
                    )
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("slint compile thread"))
            .collect()
    });
    for dep in deps.iter().flatten() {
        println!("cargo::rerun-if-changed={}", dep.display());
    }

    let mods: String = names
        .iter()
        .map(|n| format!("#[allow(unsafe_code)]\npub mod {n} {{\n    include!(concat!(env!(\"OUT_DIR\"), \"/{n}.rs\"));\n}}\n"))
        .collect();
    fs::write(out.join("mods.rs"), mods).expect("write mods.rs");
}

fn compile(input: &Path, output: &Path, ui_crate: &Path) -> Vec<PathBuf> {
    // `@rimelight_design/tokens.slint` is the shared design tokens (packages/design in the web monorepo).
    // `@rimelight_ui/...` is the shared component library (rimelight-rs, ST2.3); `@rimelight_shell/...` the shared shell (ST2.5a).
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![ui_crate.to_path_buf()])
        .with_library_paths(HashMap::from([
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
        ]));
    slint_build::compile_with_output_path(input, output, config)
        .unwrap_or_else(|e| panic!("slint: {} failed to compile: {e}", input.display()))
}
