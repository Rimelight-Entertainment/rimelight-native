//! Prints `PROJECT.md section 6.5` to stdout. Run through `cargo xtask docs ia`, which writes the output to the file.

fn main() {
    print!("{}", space_nav::ia_doc::render());
}
