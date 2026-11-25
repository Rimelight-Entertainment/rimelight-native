// Compiles the update window markup (REL.3). One small file, so this crate builds in a moment.
fn main() {
    slint_build::compile("ui/update.slint").expect("slint: ui/update.slint failed to compile");
}
