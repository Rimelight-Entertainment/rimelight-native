//! Mail bodies as structured plain text. HTML is never rendered: the parser keeps the visible text, turns block
//! elements into paragraphs, list items, quote depth and rules, and lists every link after the paragraph that holds
//! it. Scripts, styles, heads and comments are dropped. A body the parser cannot structure falls back to its plain
//! text, so every mail stays readable. The original HTML is still reachable through "View original".

/// One structured piece of a mail body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Running text. `depth` is the blockquote nesting (0 outside quotes).
    Paragraph { text: String, depth: u8 },
    /// A list item with its marker (`•`, or `1.`).
    Item {
        marker: String,
        text: String,
        depth: u8,
    },
    /// A link, listed after the paragraph that contains it.
    Link {
        label: String,
        href: String,
        depth: u8,
    },
    /// A horizontal rule.
    Rule,
    /// An image: `src` is a `cid:`, `data:` or web address; `alt` is its text.
    Image { src: String, alt: String, depth: u8 },
}

/// Deepest quote nesting shown; deeper quotes stay at this depth.
const MAX_DEPTH: u8 = 4;

/// Structured blocks for a mail body, whether it is HTML or plain text.
pub fn blocks_for(body: &str) -> Vec<Block> {
    let blocks = if looks_like_html(body) {
        parse_html(body)
    } else {
        parse_plain(body)
    };
    if blocks.is_empty() && !body.trim().is_empty() {
        return vec![Block::Paragraph {
            text: body.trim().to_owned(),
            depth: 0,
        }];
    }
    blocks
}

/// True when the body has a tag-like `<` (a letter, `/` or `!` after it).
pub fn looks_like_html(body: &str) -> bool {
    body.as_bytes().windows(2).any(|pair| {
        pair[0] == b'<' && (pair[1].is_ascii_alphabetic() || pair[1] == b'/' || pair[1] == b'!')
    })
}

/// Plain text: blank lines separate paragraphs, `>` lines are quotes, `-`/`*` lines are items.
fn parse_plain(body: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut para_depth: u8 = 0;
    let flush = |para: &mut Vec<String>, depth: u8, blocks: &mut Vec<Block>| {
        let text = para.join("\n").trim().to_owned();
        para.clear();
        if !text.is_empty() {
            blocks.push(Block::Paragraph { text, depth });
        }
    };
    for raw in body.lines() {
        let trimmed = raw.trim_end();
        let (depth, line) = quote_depth(trimmed);
        if line.trim().is_empty() {
            flush(&mut para, para_depth, &mut blocks);
            continue;
        }
        if let Some(item) = line
            .trim_start()
            .strip_prefix("- ")
            .or_else(|| line.trim_start().strip_prefix("* "))
        {
            flush(&mut para, para_depth, &mut blocks);
            blocks.push(Block::Item {
                marker: "•".to_owned(),
                text: item.trim().to_owned(),
                depth,
            });
            continue;
        }
        if para.is_empty() {
            para_depth = depth;
        } else if depth != para_depth {
            flush(&mut para, para_depth, &mut blocks);
            para_depth = depth;
        }
        para.push(line.trim().to_owned());
    }
    flush(&mut para, para_depth, &mut blocks);
    blocks
}

/// The quote depth of a plain line (one `>` per level) and the line without its markers.
fn quote_depth(line: &str) -> (u8, &str) {
    let mut rest = line.trim_start();
    let mut depth: u8 = 0;
    while let Some(after) = rest.strip_prefix('>') {
        depth = depth.saturating_add(1).min(MAX_DEPTH);
        rest = after.trim_start();
    }
    (depth, rest)
}

/// Element names whose content is never shown.
const SKIPPED: [&str; 6] = ["script", "style", "head", "title", "noscript", "template"];
/// Elements that end the running paragraph.
const BLOCK_TAGS: [&str; 22] = [
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "tr",
    "table",
    "ul",
    "ol",
    "li",
    "blockquote",
    "section",
    "article",
    "header",
    "footer",
    "pre",
    "dl",
    "dt",
    "dd",
];

struct Parser {
    blocks: Vec<Block>,
    /// Text of the running paragraph.
    buf: String,
    /// Links met in the running paragraph, emitted after it.
    links: Vec<(String, String)>,
    /// Marker of the list item being filled, if the running paragraph is one.
    item_marker: Option<String>,
    /// Blockquote nesting.
    quote: u8,
    /// Open lists: (ordered, next number).
    lists: Vec<(bool, u32)>,
    /// Open anchor: (href, visible text).
    anchor: Option<(String, String)>,
    /// Depth of skipped elements we are inside.
    skip: u32,
}

impl Parser {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            buf: String::new(),
            links: Vec::new(),
            item_marker: None,
            quote: 0,
            lists: Vec::new(),
            anchor: None,
            skip: 0,
        }
    }

    fn text(&mut self, raw: &str) {
        if self.skip > 0 {
            return;
        }
        let decoded = decode_entities(raw);
        for ch in decoded.chars() {
            if ch.is_whitespace() {
                if !self.buf.is_empty() && !self.buf.ends_with([' ', '\n']) {
                    self.buf.push(' ');
                }
            } else {
                self.buf.push(ch);
            }
        }
        if let Some((_, text)) = self.anchor.as_mut() {
            text.push_str(decoded.trim());
            text.push(' ');
        }
    }

    /// Ends the running paragraph: emits it (or the item it fills), then its links.
    fn flush(&mut self) {
        let text = normalise(&self.buf);
        let depth = self.quote.min(MAX_DEPTH);
        if !text.is_empty() {
            match self.item_marker.take() {
                Some(marker) => self.blocks.push(Block::Item {
                    marker,
                    text,
                    depth,
                }),
                None => self.blocks.push(Block::Paragraph { text, depth }),
            }
        }
        self.item_marker = None;
        for (label, href) in std::mem::take(&mut self.links) {
            let label = if label.is_empty() {
                href.clone()
            } else {
                label
            };
            self.blocks.push(Block::Link { label, href, depth });
        }
        self.buf.clear();
    }

    fn open(&mut self, name: &str, attrs: &str) {
        if SKIPPED.contains(&name) {
            self.skip += 1;
            return;
        }
        if self.skip > 0 {
            return;
        }
        match name {
            "br" => self.buf.push('\n'),
            "hr" => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            "img" => {
                // Images are blocks of their own, after the running text: the glue shows the picture when it loads
                // and the alt text when it does not.
                self.flush();
                let src = attribute(attrs, "src")
                    .map(|s| decode_entities(s.trim()))
                    .unwrap_or_default();
                let alt = attribute(attrs, "alt")
                    .map(|a| decode_entities(a.trim()))
                    .unwrap_or_default();
                if !src.is_empty() {
                    self.blocks.push(Block::Image {
                        src,
                        alt,
                        depth: self.quote.min(MAX_DEPTH),
                    });
                } else if !alt.is_empty() {
                    self.buf.push(' ');
                    self.buf.push_str(&format!("[image: {alt}]"));
                }
            }
            "blockquote" => {
                self.flush();
                self.quote = self.quote.saturating_add(1);
            }
            "ul" | "ol" => {
                self.flush();
                self.lists.push((name == "ol", 1));
            }
            "li" => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some((true, next)) => {
                        let m = format!("{next}.");
                        *next += 1;
                        m
                    }
                    _ => "•".to_owned(),
                };
                self.item_marker = Some(marker);
            }
            "a" => {
                let href = attribute(attrs, "href").unwrap_or_default();
                if href.trim().is_empty() {
                    self.anchor = None;
                } else {
                    self.anchor = Some((decode_entities(href.trim()), String::new()));
                }
            }
            _ if BLOCK_TAGS.contains(&name) => self.flush(),
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        if SKIPPED.contains(&name) {
            self.skip = self.skip.saturating_sub(1);
            return;
        }
        if self.skip > 0 {
            return;
        }
        match name {
            "blockquote" => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            "ul" | "ol" => {
                self.flush();
                self.lists.pop();
            }
            "a" => {
                if let Some((href, text)) = self.anchor.take() {
                    let label = normalise(&text);
                    let dup = self.links.iter().any(|(_, h)| *h == href);
                    if !dup {
                        self.links.push((label, href));
                    }
                }
            }
            _ if BLOCK_TAGS.contains(&name) => self.flush(),
            _ => {}
        }
    }
}

/// Parses HTML into blocks. Tags are read by hand: the parser needs only a few element names, and anything
/// unknown is ignored with its text kept.
fn parse_html(html: &str) -> Vec<Block> {
    let mut p = Parser::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        p.text(&rest[..lt]);
        let after = &rest[lt..];
        // A `<` that does not start a tag ("3 < 4") is text.
        let starts_tag = after[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!');
        if !starts_tag {
            p.text("<");
            rest = &after[1..];
            continue;
        }
        if let Some(stripped) = after.strip_prefix("<!--") {
            rest = match stripped.find("-->") {
                Some(end) => &stripped[end + 3..],
                None => "",
            };
            continue;
        }
        let Some(gt) = after.find('>') else {
            // A stray `<` with no end: keep it as text.
            p.text(after);
            rest = "";
            break;
        };
        let inner = &after[1..gt];
        rest = &after[gt + 1..];
        let closing = inner.starts_with('/');
        let body = inner.trim_start_matches('/');
        let name: String = body
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let attrs = &body[name.len()..];
        let self_closing = inner.ends_with('/');
        if SKIPPED.contains(&name.as_str()) && !closing && !self_closing {
            // Skip everything up to the matching close tag.
            let close = format!("</{name}");
            let lower = rest.to_ascii_lowercase();
            rest = match lower.find(&close) {
                Some(idx) => match rest[idx..].find('>') {
                    Some(end) => &rest[idx + end + 1..],
                    None => "",
                },
                None => "",
            };
            continue;
        }
        if closing {
            p.close(&name);
        } else {
            p.open(&name, attrs);
            if self_closing {
                p.close(&name);
            }
        }
    }
    p.text(rest);
    p.flush();
    p.blocks
}

/// The value of attribute `name` in a tag's attribute text (quoted or bare).
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(idx) = lower[from..].find(name) {
        let at = from + idx;
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = &attrs[at + name.len()..];
        from = at + name.len();
        if !before_ok {
            continue;
        }
        let trimmed = after.trim_start();
        let Some(value) = trimmed.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        return Some(match value.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let body = &value[1..];
                body.find(q)
                    .map(|end| body[..end].to_owned())
                    .unwrap_or_default()
            }
            _ => value
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        });
    }
    None
}

/// Decodes the entities a mail body uses: the common named ones and numeric references.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let Some(semi) = after.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "euro" => Some('€'),
            "pound" => Some('£'),
            _ => entity.strip_prefix('#').and_then(|num| {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse::<u32>().ok(),
                }?;
                char::from_u32(code)
            }),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Collapses runs of spaces inside each line and drops blank-line runs.
fn normalise(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() && lines.last().is_some_and(String::is_empty) {
            continue;
        }
        lines.push(collapsed);
    }
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str, depth: u8) -> Block {
        Block::Paragraph {
            text: text.to_owned(),
            depth,
        }
    }

    #[test]
    fn scripts_styles_and_heads_are_dropped_and_text_kept() {
        let html = "<html><head><title>Hi</title><style>p{color:red}</style></head>\
            <body><script>alert(1)</script><p>Hello <b>there</b></p></body></html>";
        assert_eq!(blocks_for(html), vec![para("Hello there", 0)]);
    }

    #[test]
    fn block_elements_make_separate_paragraphs_and_br_keeps_lines() {
        let html = "<div>One<br>two</div><p>Three</p>";
        assert_eq!(
            blocks_for(html),
            vec![para("One\ntwo", 0), para("Three", 0)]
        );
    }

    #[test]
    fn links_follow_their_paragraph_and_repeats_are_listed_once() {
        let html = r#"<p>See <a href="https://example.com/a">the docs</a> or <a href="https://example.com/a">again</a>.</p>"#;
        let blocks = blocks_for(html);
        assert_eq!(blocks[0], para("See the docs or again.", 0));
        assert_eq!(
            blocks[1],
            Block::Link {
                label: "the docs".to_owned(),
                href: "https://example.com/a".to_owned(),
                depth: 0,
            }
        );
        assert_eq!(blocks.len(), 2, "the repeated link is not listed twice");
    }

    #[test]
    fn a_link_without_text_shows_its_address() {
        // No paragraph text, so the link is the only block.
        let blocks = blocks_for(r#"<p><a href="https://example.com/x"></a></p>"#);
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], Block::Link { label, .. } if label == "https://example.com/x")
        );
    }

    #[test]
    fn lists_are_items_with_bullets_or_numbers() {
        let html = "<ul><li>Apples</li><li>Pears</li></ul><ol><li>First</li><li>Second</li></ol>";
        assert_eq!(
            blocks_for(html),
            vec![
                Block::Item {
                    marker: "•".into(),
                    text: "Apples".into(),
                    depth: 0
                },
                Block::Item {
                    marker: "•".into(),
                    text: "Pears".into(),
                    depth: 0
                },
                Block::Item {
                    marker: "1.".into(),
                    text: "First".into(),
                    depth: 0
                },
                Block::Item {
                    marker: "2.".into(),
                    text: "Second".into(),
                    depth: 0
                },
            ]
        );
    }

    #[test]
    fn blockquotes_carry_their_depth() {
        let html =
            "<p>New</p><blockquote>Old reply<blockquote>Older</blockquote></blockquote><p>Done</p>";
        assert_eq!(
            blocks_for(html),
            vec![
                para("New", 0),
                para("Old reply", 1),
                para("Older", 2),
                para("Done", 0),
            ]
        );
    }

    #[test]
    fn entities_are_decoded_and_numeric_references_work() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &quot;d&quot; &#65;&#x42; &nbsp;x"),
            "a & b <c> \"d\" AB  x"
        );
        assert_eq!(decode_entities("fish &chips & more"), "fish &chips & more");
        assert_eq!(
            blocks_for("<p>Tom &amp; Jerry</p>"),
            vec![para("Tom & Jerry", 0)]
        );
    }

    #[test]
    fn images_are_blocks_between_their_paragraphs() {
        let blocks = blocks_for(
            r#"<p>Logo <img src="cid:logo" alt="Company logo"> and <img src="y.png"></p>"#,
        );
        assert_eq!(
            blocks,
            vec![
                para("Logo", 0),
                Block::Image {
                    src: "cid:logo".to_owned(),
                    alt: "Company logo".to_owned(),
                    depth: 0,
                },
                para("and", 0),
                Block::Image {
                    src: "y.png".to_owned(),
                    alt: String::new(),
                    depth: 0,
                },
            ]
        );
    }

    #[test]
    fn rules_and_stray_angle_brackets_survive() {
        let blocks = blocks_for("<p>a</p><hr><p>b 3 < 4</p>");
        assert_eq!(blocks[1], Block::Rule);
        assert_eq!(blocks[2], para("b 3 < 4", 0));
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(
            blocks_for("<!-- hidden --><p>shown</p>"),
            vec![para("shown", 0)]
        );
    }

    #[test]
    fn plain_text_splits_on_blank_lines_and_reads_quotes_and_lists() {
        let body = "Hi there\n\nSecond para\nstill second\n\n> quoted line\n> more\n\n- one\n- two";
        assert_eq!(
            blocks_for(body),
            vec![
                para("Hi there", 0),
                para("Second para\nstill second", 0),
                para("quoted line\nmore", 1),
                Block::Item {
                    marker: "•".into(),
                    text: "one".into(),
                    depth: 0
                },
                Block::Item {
                    marker: "•".into(),
                    text: "two".into(),
                    depth: 0
                },
            ]
        );
    }

    #[test]
    fn a_body_with_only_markup_falls_back_to_its_text() {
        // Nothing visible survives the parse, so the raw body is shown rather than nothing.
        let blocks = blocks_for("<script>x</script>");
        assert_eq!(blocks, vec![para("<script>x</script>", 0)]);
    }

    #[test]
    fn empty_body_has_no_blocks() {
        assert!(blocks_for("   ").is_empty());
    }
}
