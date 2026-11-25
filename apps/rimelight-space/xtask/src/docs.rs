//! `cargo xtask docs ia`: regenerates the route table in `PLAN.md` section 6.5 from the navigation manifest.
//!
//! Each runs an example that prints Markdown. The output replaces only the text between the marker lines
//! `<!-- BEGIN GENERATED: <name> -->` and `<!-- END GENERATED: <name> -->` in `PLAN.md`. Everything else in the
//! file is left alone. Headings in the output are shifted down so they sit under the plan's section headings.
//! Run from the repo root. std only.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

const PLAN: &str = "PLAN.md";

/// One generated region: its marker name, the example that prints it, and how far its headings move down.
struct Target {
    name: &'static str,
    package: &'static str,
    example: &'static str,
    heading_shift: usize,
}

const IA: Target = Target {
    name: "ia",
    package: "space_nav",
    example: "ia_doc",
    heading_shift: 3,
};

/// Entry point for `cargo xtask docs <args...>`.
pub fn run(args: &[&str]) -> ExitCode {
    match args {
        ["ia"] => generate(&IA),
        _ => {
            eprintln!("usage: cargo xtask docs ia");
            ExitCode::from(2)
        }
    }
}

/// Runs the example and writes its Markdown into the marked region of `PLAN.md`.
fn generate(t: &Target) -> ExitCode {
    let output = match Command::new("cargo")
        .args(["run", "--quiet", "-p", t.package, "--example", t.example])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!("FAILED: xtask docs {}: cannot run cargo: {err}", t.name);
            return ExitCode::FAILURE;
        }
    };
    if !output.status.success() {
        eprintln!(
            "FAILED: xtask docs {}: {} failed ({})\n{}",
            t.name,
            t.example,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return ExitCode::FAILURE;
    }
    let body = shift_headings(&String::from_utf8_lossy(&output.stdout), t.heading_shift);
    let plan = match fs::read_to_string(Path::new(PLAN)) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("FAILED: xtask docs {}: cannot read {PLAN}: {err}", t.name);
            return ExitCode::FAILURE;
        }
    };
    let updated = match replace_region(&plan, t.name, &body) {
        Ok(text) => text,
        Err(msg) => {
            eprintln!("FAILED: xtask docs {}: {msg}", t.name);
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = fs::write(Path::new(PLAN), updated) {
        eprintln!("FAILED: xtask docs {}: cannot write {PLAN}: {err}", t.name);
        return ExitCode::FAILURE;
    }
    println!(
        "xtask docs {}: ok, wrote the {} region of {PLAN} ({} bytes)",
        t.name,
        t.name,
        body.len()
    );
    ExitCode::SUCCESS
}

/// Moves each Markdown heading down by `by` levels (capped at h6). Other lines are unchanged.
fn shift_headings(text: &str, by: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        let is_heading = hashes > 0 && line[hashes..].starts_with(' ');
        if is_heading {
            let new = (hashes + by).min(6);
            out.push_str(&"#".repeat(new));
            out.push_str(&line[hashes..]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Replaces the lines between the BEGIN and END markers for `name`. Fails unless each marker appears exactly once.
fn replace_region(plan: &str, name: &str, body: &str) -> Result<String, String> {
    let begin = format!("<!-- BEGIN GENERATED: {name} -->");
    let end = format!("<!-- END GENERATED: {name} -->");
    let b = match plan.matches(&begin).count() {
        1 => plan.find(&begin).expect("counted"),
        n => {
            return Err(format!(
                "expected one `{begin}` marker in {PLAN}, found {n}"
            ));
        }
    };
    let e = match plan.matches(&end).count() {
        1 => plan.find(&end).expect("counted"),
        n => return Err(format!("expected one `{end}` marker in {PLAN}, found {n}")),
    };
    if e < b {
        return Err(format!("the `{end}` marker is above the `{begin}` marker"));
    }
    let head = &plan[..b + begin.len()];
    let tail = &plan[e..];
    Ok(format!("{head}\n\n{body}\n{tail}"))
}
