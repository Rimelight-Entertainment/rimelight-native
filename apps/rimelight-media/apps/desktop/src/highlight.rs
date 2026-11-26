//! Syntax highlighting for the Text tab's editor, with syntect (its pure-Rust `fancy-regex` engine, so no C build) and the
//! extra syntaxes and themes of two-face (TOML, TypeScript, PowerShell, INI and more). The text is coloured with the
//! syntect theme that matches the app (dark or light), and the editor draws one layer per colour: that colour's characters
//! in a monospace copy of the text, the other characters as spaces, so the editor's own text lines up with them (the
//! editor is not wrapped while the layers show). Nothing here knows about the editor's look: each layer is keyed by its
//! colour packed as 0xRRGGBB.
//!
//! The highlighting runs on a background thread (textctl::highlight_now); the syntax set and the themes load on first
//! use and are dropped again after IDLE_RELEASE without a highlight (see `engine`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The syntect and two-face data the highlighter uses.
struct Engine {
    syntaxes: SyntaxSet,
    dark: Theme,
    light: Theme,
}

/// The engine is dropped after this long without a highlight. Loaded, it is about 2 MB, and a syntax that has been used
/// keeps its compiled regexes and their match caches (about 6 MB more for Rust's), measured as private memory. Loading it
/// again takes a moment on the highlight thread, never on the UI thread, and typing highlights far more often than this.
const IDLE_RELEASE: Duration = Duration::from_secs(30);

struct Cache {
    engine: Option<Arc<Engine>>,
    used: Option<Instant>,
    /// A thread is watching for the idle time (one at most).
    reaper: bool,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    engine: None,
    used: None,
    reaper: false,
});

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn load() -> Engine {
    // Newline-keeping syntaxes, because the lines are read with their line ends (LinesWithEndings).
    let syntaxes = two_face::syntax::extra_newlines();
    // Two-face's One Half themes: the same look in both app themes.
    let themes = two_face::theme::extra();
    let dark = themes
        .get(two_face::theme::EmbeddedThemeName::OneHalfDark)
        .clone();
    let light = themes
        .get(two_face::theme::EmbeddedThemeName::OneHalfLight)
        .clone();
    Engine {
        syntaxes,
        dark,
        light,
    }
}

/// The engine, loaded if needed. A caller keeps its Arc for the whole highlight, so a release meanwhile is harmless.
fn engine() -> Arc<Engine> {
    let mut c = cache();
    c.used = Some(Instant::now());
    let e = Arc::clone(c.engine.get_or_insert_with(|| Arc::new(load())));
    if !c.reaper {
        c.reaper = true;
        std::thread::spawn(reap);
    }
    e
}

/// Drops the engine once it has been idle for IDLE_RELEASE, then ends (the next `engine` starts a new watcher).
fn reap() {
    loop {
        std::thread::sleep(IDLE_RELEASE / 3);
        let mut c = cache();
        if c.used.is_some_and(|t| t.elapsed() >= IDLE_RELEASE) {
            let old = c.engine.take();
            c.reaper = false;
            drop(c);
            drop(old);
            return;
        }
    }
}

/// The syntax name (its extension) for a file: from its extension, else from the file type the toolbar's combo shows
/// (0 plain, 1 Markdown, 2 JSON, 3 TOML, 4 YAML, 5 XML, 6 CSV, 7 INI, 8 Log). None means no colour: plain text, and
/// Markdown, which keeps its word wrap (the layers can't follow a wrapped line, so it stays one colour).
pub fn syntax_for(ext: &str, kind: i32) -> Option<&'static str> {
    let by_ext = match ext.to_ascii_lowercase().as_str() {
        "md" | "markdown" => return None,
        "rs" => "rs",
        "js" | "mjs" | "cjs" | "jsx" => "js",
        "ts" | "tsx" => "ts",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "ini" | "cfg" | "conf" | "env" => "ini",
        "xml" | "html" | "htm" | "svg" | "xhtml" => "xml",
        "css" => "css",
        "sh" | "bash" => "sh",
        "ps1" => "ps1",
        "bat" | "cmd" => "bat",
        "csv" => "csv",
        "log" => "log",
        _ => "",
    };
    if !by_ext.is_empty() {
        return Some(by_ext);
    }
    match kind {
        1 => None,
        2 => Some("json"),
        3 => Some("toml"),
        4 => Some("yaml"),
        5 => Some("xml"),
        6 => Some("csv"),
        7 => Some("ini"),
        8 => Some("log"),
        _ => None,
    }
}

/// The colour layers of a text in one syntax (`syntax` from `syntax_for`): for each colour that has characters, its key
/// (0xRRGGBB) and a string of the text's length in which only that colour's characters show (the rest are spaces; line
/// breaks and tabs stay, so the lines line up). None when syntect has no such syntax or the highlighting fails.
pub fn layers(text: &str, syntax: &str, dark: bool) -> Option<Vec<(i32, String)>> {
    let held = engine();
    let e = &*held;
    let syn = e.syntaxes.find_syntax_by_extension(syntax)?;
    let theme = if dark { &e.dark } else { &e.light };
    let mut hl = HighlightLines::new(syn, theme);

    // The colour of each byte of the text (the pieces of each line come back in order and cover the line).
    let mut colour: Vec<i32> = Vec::with_capacity(text.len());
    for line in LinesWithEndings::from(text) {
        for (style, piece) in hl.highlight_line(line, &e.syntaxes).ok()? {
            let c = style.foreground;
            let key = (u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)) as i32;
            colour.extend(std::iter::repeat_n(key, piece.len()));
        }
    }
    if colour.len() != text.len() {
        return None;
    }

    // One layer per colour, in the order the colours first show.
    let mut index: HashMap<i32, usize> = HashMap::new();
    let mut out: Vec<(i32, String)> = Vec::new();
    for i in 0..text.len() {
        if text.is_char_boundary(i) && !index.contains_key(&colour[i]) {
            index.insert(colour[i], out.len());
            out.push((colour[i], String::with_capacity(text.len())));
        }
    }
    for (key, layer) in out.iter_mut() {
        for (i, c) in text.char_indices() {
            if c == '\n' || c == '\t' {
                layer.push(c);
            } else if colour[i] == *key {
                layer.push(c);
            } else {
                layer.push(' ');
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_keep_the_text_length_and_lines() {
        let t = "fn main() {\n    let s = \"hi\";\n}\n";
        let layers = layers(t, "rs", true).expect("Rust has a syntax");
        assert!(!layers.is_empty());
        for (_, layer) in &layers {
            assert_eq!(layer.len(), t.len());
            assert_eq!(layer.matches('\n').count(), 3);
        }
    }

    #[test]
    fn markdown_is_not_coloured() {
        assert_eq!(syntax_for("md", 0), None);
        assert_eq!(syntax_for("", 1), None);
        assert_eq!(syntax_for("toml", 0), Some("toml"));
    }
}
