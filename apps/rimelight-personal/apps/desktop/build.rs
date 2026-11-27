// Compiles the start-up update window (ui/update.slint) on Windows, the only Slint markup this app ships. The app shell and
// every page are compiled by crates/rimelight_personal_ui_slint.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=ui/update.slint");
    println!("cargo::rerun-if-changed=ui/setup.slint");
    println!("cargo::rerun-if-changed=ui/app.slint");
    println!("cargo::rerun-if-changed=app.rc");
    println!("cargo::rerun-if-changed=../../assets/logo/logo_color.ico");
    // Both windows are compiled from ui/app.slint, so one generated module holds them.
    slint_build::compile("ui/app.slint").expect("slint: ui/app.slint failed to compile");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The app icon and version information (app.rc) inside personal_desktop.exe. The version is taken from the
        // package version, as rimelight-media does.
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut parts: Vec<String> = version
            .split(['.', '-'])
            .take(3)
            .map(str::to_string)
            .collect();
        parts.resize(3, "0".into());
        let commas = format!("{},{},{},0", parts[0], parts[1], parts[2]);
        // app.rc is a template: fill in the version and the icon's full path, then compile the copy.
        let manifest = std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_default()
            .replace('\\', "/");
        let rc = std::fs::read_to_string("app.rc")
            .expect("app.rc")
            .replace("APP_VERSION_COMMAS", &commas)
            .replace("APP_VERSION_TEXT", &format!("\"{version}\""))
            .replace("\"../../assets/", &format!("\"{manifest}/../../assets/"));
        let out =
            std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("app.rc");
        std::fs::write(&out, rc).expect("write app.rc");
        embed_resource::compile(&out, embed_resource::NONE)
            .manifest_optional()
            .expect("the resources compile");
    }
}
