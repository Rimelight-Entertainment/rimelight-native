// Compiles the start-up update window (ui/update.slint) on Windows, the only Slint markup this app ships. The app shell and
// every page are compiled by crates/rimelight_personal_ui_slint.
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=ui/update.slint");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        slint_build::compile("ui/update.slint").expect("slint: ui/update.slint failed to compile");
    }
}
