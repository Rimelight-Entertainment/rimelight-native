//! Plain view mapping for the content pages: localized text, slugs from titles, status and visibility labels, dates, colour
//! chips, and the rows the lists draw. No Slint types here; `wire_content!` converts these to the generated structs.

use serde_json::Value;
use space_client::content::{ContentCollection, PageSummary, Visibility};

use crate::scope::{Access, parse_access};

/// The locale the interface falls back to when a localized value has no text in the wanted one.
pub const FALLBACK_LOCALE: &str = "en";

/// The text of a localized value (`{"en": "..", "pt": ".."}`) in `locale`, else in English, else the first text there is.
/// A plain string is returned as it is.
pub fn localized(value: &Value, locale: &str) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(map) => {
            let pick = |key: &str| {
                map.get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            pick(locale)
                .or_else(|| pick(FALLBACK_LOCALE))
                .or_else(|| map.values().find_map(|v| v.as_str().map(str::to_string)))
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// A URL slug from a title: lowercase ASCII letters and digits, every other run becomes one hyphen, no leading or trailing
/// hyphen. A title with no ASCII letters or digits gives "page" (the user can type their own slug).
pub fn slug_from_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "page".to_string()
    } else {
        out
    }
}

/// Whether a slug is valid as typed: lowercase ASCII letters, digits and single hyphens, 1 to 80 characters.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 80
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The status label for a page or a draft. Server values: Open, InReview, Published, Discarded (drafts) and the page's own
/// words. Anything unknown reads as Draft.
pub fn status_label(status: &str) -> &'static str {
    match status {
        "InReview" | "in_review" | "In review" => "In review",
        "Published" | "published" => "Published",
        "Discarded" | "discarded" => "Discarded",
        _ => "Draft",
    }
}

/// The visibility label (answer 5: Internal by default).
pub fn visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Internal => "Internal",
        Visibility::Public => "Public",
    }
}

/// `2026-11-23T14:05:09Z` to `2026-11-23 14:05`. Text that is too short is returned as it is.
pub fn short_time(stamp: &str) -> String {
    if stamp.len() < 16 {
        return stamp.to_string();
    }
    stamp[..16].replace('T', " ")
}

/// A `#rrggbb` colour to its channels. `None` for anything else.
pub fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// The colour chip of a collection; grey when it has none.
pub fn chip_colour(collection: Option<&ContentCollection>) -> (u8, u8, u8) {
    collection
        .and_then(|c| c.color.as_deref())
        .and_then(parse_hex)
        .unwrap_or((0x80, 0x80, 0x80))
}

/// One row of a page list (the scope home's tree and search results).
#[derive(Debug, Clone, PartialEq)]
pub struct PageRow {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub collection: String,
    pub chip: (u8, u8, u8),
    pub status: &'static str,
    pub updated: String,
    pub visibility: &'static str,
    pub public: bool,
    pub access: Access,
}

/// Maps a page summary to its row. `locale` picks the title text; `collections` gives the colour chip.
pub fn page_row(page: &PageSummary, locale: &str, collections: &[ContentCollection]) -> PageRow {
    let collection = collections.iter().find(|c| c.slug == page.page_type);
    PageRow {
        id: page.id.clone(),
        title: localized(&page.title, locale),
        slug: page.slug.clone(),
        collection: collection
            .map(|c| c.name.clone())
            .unwrap_or_else(|| page.page_type.clone()),
        chip: chip_colour(collection),
        status: status_label(&page.status),
        updated: short_time(&page.updated_at),
        visibility: visibility_label(page.visibility),
        public: page.visibility == Visibility::Public,
        access: parse_access(&page.my_access),
    }
}

/// A search over rows: the title, slug and collection name contain the text (case-insensitive). An empty query keeps all.
pub fn matches_query(row: &PageRow, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || row.title.to_lowercase().contains(&q)
        || row.slug.to_lowercase().contains(&q)
        || row.collection.to_lowercase().contains(&q)
}

/// One collection tab. `slug` is empty for "All".
#[derive(Debug, Clone, PartialEq)]
pub struct TabView {
    pub slug: String,
    pub label: String,
    pub chip: (u8, u8, u8),
    pub count: i32,
}

/// The tabs of a scope home: "All" first, then one tab per collection that has pages, with its page count.
pub fn tabs(collections: &[ContentCollection], pages: &[PageSummary]) -> Vec<TabView> {
    let mut out = vec![TabView {
        slug: String::new(),
        label: "All".to_string(),
        chip: (0x80, 0x80, 0x80),
        count: pages.len() as i32,
    }];
    for c in collections {
        let count = pages.iter().filter(|p| p.page_type == c.slug).count() as i32;
        out.push(TabView {
            slug: c.slug.clone(),
            label: c.name.clone(),
            chip: chip_colour(Some(c)),
            count,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn localized_falls_back_to_english_then_anything() {
        let v = json!({"en": "Hello", "pt": "Olá"});
        assert_eq!(localized(&v, "pt"), "Olá");
        assert_eq!(localized(&v, "es"), "Hello");
        assert_eq!(localized(&json!({"pt": "Olá"}), "en"), "Olá");
        assert_eq!(localized(&json!("plain"), "en"), "plain");
        assert_eq!(localized(&json!(null), "en"), "");
    }

    #[test]
    fn slugs_come_from_titles() {
        assert_eq!(slug_from_title("Hello, World!"), "hello-world");
        assert_eq!(
            slug_from_title("  Release   notes 2026 "),
            "release-notes-2026"
        );
        assert_eq!(slug_from_title("---"), "page");
        assert_eq!(slug_from_title("Olá Mundo"), "ol-mundo");
    }

    #[test]
    fn typed_slugs_are_checked() {
        assert!(valid_slug("release-notes-2026"));
        assert!(!valid_slug(""));
        assert!(!valid_slug("-lead"));
        assert!(!valid_slug("double--hyphen"));
        assert!(!valid_slug("Upper"));
        assert!(!valid_slug("with space"));
    }

    #[test]
    fn labels_and_times() {
        assert_eq!(status_label("InReview"), "In review");
        assert_eq!(status_label("Open"), "Draft");
        assert_eq!(visibility_label(Visibility::Public), "Public");
        assert_eq!(short_time("2026-11-23T14:05:09Z"), "2026-11-23 14:05");
        assert_eq!(short_time("soon"), "soon");
    }

    #[test]
    fn hex_colours_parse() {
        assert_eq!(parse_hex("#ff8000"), Some((255, 128, 0)));
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(parse_hex("#ff80"), None);
        assert_eq!(parse_hex("#gg8000"), None);
        assert_eq!(chip_colour(None), (0x80, 0x80, 0x80));
    }

    #[test]
    fn search_matches_title_slug_or_collection() {
        let row = PageRow {
            id: "1".into(),
            title: "Onboarding".into(),
            slug: "onboarding".into(),
            collection: "Wiki".into(),
            chip: (0, 0, 0),
            status: "Draft",
            updated: String::new(),
            visibility: "Internal",
            public: false,
            access: Access::Edit,
        };
        assert!(matches_query(&row, ""));
        assert!(matches_query(&row, "  ONBOARD "));
        assert!(matches_query(&row, "wiki"));
        assert!(!matches_query(&row, "blog"));
    }
}
