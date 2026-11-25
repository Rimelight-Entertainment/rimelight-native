//! Issue search query language (PLAN section 4, S1; decision 0027). Pure: no I/O, no store.
//!
//! The parser turns a query such as `project: EMB state: #Unresolved sort by: priority desc` into a
//! `Query` (an AST with spans), or a `ParseError` that names the column span of the problem. The
//! backend resolves names (projects, states, people, sprints) against the caller's data; this module
//! only checks syntax and the fixed value sets (types, priorities, has-kinds, dates, numbers).
//!
//! Grammar (informal):
//!
//! ```text
//! query   := (clause | sort)*
//! clause  := ['-' | 'not'] (field ':' values | '#' tag | phrase | word)
//! values  := item (',' item)*          -- comma is OR within one field
//! item    := value ['..' value]        -- a range, for dates and estimates
//! value   := phrase | word+            -- unquoted values may hold spaces: `In Progress`
//! sort    := 'sort' 'by' ':' key (',' key)*
//! key     := sortfield ['asc' | 'desc']
//! ```
//!
//! Fields: `project`, `state`, `assignee`, `reporter`, `type`, `priority`, `sprint`, `tag`, `parent`,
//! `created`, `updated`, `resolved`, `estimate`, `has`. A bare word or phrase is a full-text term.
//! A negated clause excludes its matches. Clauses are joined with AND.

use std::fmt;

use crate::projects::{ISSUE_TYPES, PRIORITIES};

/// Character offsets into the query text, 0-based, end exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// The span from the start of `self` to the end of `other`.
    fn to(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end.max(self.start),
        }
    }
}

/// A parse or resolution error. `span` is what the message is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for ParseError {
    /// `message (column 8)` or `message (columns 8-14)`, with 1-based inclusive columns.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first = self.span.start + 1;
        let last = self.span.end.max(first);
        if last <= first {
            write!(f, "{} (column {first})", self.message)
        } else {
            write!(f, "{} (columns {first}-{last})", self.message)
        }
    }
}

/// A value as written. `text` is the value without quotes; a multi-word value is joined by one space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub quoted: bool,
    pub span: Span,
}

/// A value with its span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

/// The fields a `field:` clause can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Project,
    State,
    Assignee,
    Reporter,
    Type,
    Priority,
    Sprint,
    Tag,
    Parent,
    Created,
    Updated,
    Resolved,
    Estimate,
    Has,
}

impl Field {
    /// Every field name, for error messages.
    pub const NAMES: &'static [&'static str] = &[
        "project", "state", "assignee", "reporter", "type", "priority", "sprint", "tag", "parent",
        "created", "updated", "resolved", "estimate", "has",
    ];

    fn from_name(name: &str) -> Option<Field> {
        Some(match name.to_ascii_lowercase().as_str() {
            "project" => Field::Project,
            "state" => Field::State,
            "assignee" => Field::Assignee,
            "reporter" => Field::Reporter,
            "type" => Field::Type,
            "priority" => Field::Priority,
            "sprint" => Field::Sprint,
            "tag" => Field::Tag,
            "parent" => Field::Parent,
            "created" => Field::Created,
            "updated" => Field::Updated,
            "resolved" => Field::Resolved,
            "estimate" => Field::Estimate,
            "has" => Field::Has,
            _ => return None,
        })
    }
}

/// A state reference: a state name, or a category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateRef {
    Name(String),
    Category(Category),
}

/// The two state groups: `#Unresolved` (unstarted or started) and `#Resolved` (done or cancelled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Unresolved,
    Resolved,
}

/// The date fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateField {
    Created,
    Updated,
    Resolved,
}

/// A date as written: `2026-11-01`, `today`, or `-7d` (seven days before today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateExpr {
    Day(Date),
    Today,
    Ago(u32),
}

/// One date item: a single day, or an inclusive range of days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateItem {
    Day(DateExpr),
    Range(DateExpr, DateExpr),
}

/// One estimate condition, in story points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumItem {
    Exact(u32),
    Less(u32),
    LessEq(u32),
    More(u32),
    MoreEq(u32),
    Between(u32, u32),
}

/// `has:` kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HasKind {
    Assignee,
    Estimate,
    Sprint,
    Subtasks,
    Links,
}

/// What one clause tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Full-text: summary or description contains the term.
    Text(Value),
    /// Tags (`tag: x, y` or `#x`), without the `#`. Any of them matches.
    Tag(Vec<Value>),
    Project(Vec<Value>),
    State(Vec<Spanned<StateRef>>),
    /// `me`, `Unassigned`, an email or a display name. Resolved by the backend.
    Assignee(Vec<Value>),
    Reporter(Vec<Value>),
    /// Lower-case issue type.
    Type(Vec<Value>),
    /// Lower-case priority.
    Priority(Vec<Value>),
    /// A sprint name, or `current`.
    Sprint(Vec<Value>),
    /// An issue key, upper-case, such as `EMB-12`.
    Parent(Vec<Value>),
    Date(DateField, Vec<Spanned<DateItem>>),
    Estimate(Vec<Spanned<NumItem>>),
    Has(Vec<Spanned<HasKind>>),
}

/// One clause. Clauses are joined with AND.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub negated: bool,
    pub span: Span,
    pub body: Body,
}

/// Fields `sort by:` can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Priority,
    Updated,
    Created,
    Resolved,
    Estimate,
    State,
    Key,
}

impl SortField {
    fn from_name(name: &str) -> Option<SortField> {
        Some(match name.to_ascii_lowercase().as_str() {
            "priority" => SortField::Priority,
            "updated" => SortField::Updated,
            "created" => SortField::Created,
            "resolved" => SortField::Resolved,
            "estimate" => SortField::Estimate,
            "state" => SortField::State,
            "key" | "number" => SortField::Key,
            _ => return None,
        })
    }
}

/// One sort key. Without `desc` the order is ascending. For `priority`, `desc` puts critical first.
/// Missing dates and estimates sort last in either direction. Ties break on project key and number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub field: SortField,
    pub descending: bool,
    pub span: Span,
}

/// A parsed query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub clauses: Vec<Clause>,
    pub sort: Vec<SortKey>,
}

/// A calendar date (proleptic Gregorian, UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i64,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Parses `YYYY-MM-DD`, checking the month and the day of the month.
    pub fn parse(text: &str) -> Option<Date> {
        let mut parts = text.split('-');
        let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return None;
        }
        if !(y.bytes().chain(m.bytes()).chain(d.bytes())).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let date = Date {
            year: y.parse().ok()?,
            month: m.parse().ok()?,
            day: d.parse().ok()?,
        };
        (date.month >= 1
            && date.month <= 12
            && date.day >= 1
            && date.day <= days_in_month(date.year, date.month))
        .then_some(date)
    }

    /// Days since 1970-01-01.
    pub fn to_unix_day(self) -> i64 {
        let y = self.year - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date of a day count since 1970-01-01.
    pub fn from_unix_day(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
        let year = yoe + era * 400 + i64::from(month <= 2);
        Date { year, month, day }
    }

    /// The UTC date of a unix timestamp in seconds.
    pub fn from_unix(now: i64) -> Date {
        Date::from_unix_day(now.div_euclid(86_400))
    }

    /// This date `days` earlier.
    pub fn minus_days(self, days: u32) -> Date {
        Date::from_unix_day(self.to_unix_day() - i64::from(days))
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Parses a query. An empty or blank query is valid and matches everything.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let tokens = lex(input)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        end: input.chars().count(),
    };
    let mut query = Query::default();
    while !parser.done() {
        if parser.at_sort() {
            query.sort.extend(parser.sort()?);
        } else {
            query.clauses.push(parser.clause()?);
        }
    }
    Ok(query)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    Quoted(String),
    Colon,
    Comma,
    Range,
}

/// Splits the input into tokens with their spans. Quotes group a phrase, `..` is the range marker.
fn lex(input: &str) -> Result<Vec<(Tok, Span)>, ParseError> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        match c {
            '"' => {
                let close = chars[i + 1..].iter().position(|&ch| ch == '"');
                let Some(offset) = close else {
                    return Err(ParseError::new(
                        "A quoted phrase is missing its closing quote",
                        Span {
                            start,
                            end: chars.len(),
                        },
                    ));
                };
                let text: String = chars[i + 1..i + 1 + offset].iter().collect();
                i = i + 1 + offset + 1;
                out.push((Tok::Quoted(text), Span { start, end: i }));
            }
            ':' => {
                i += 1;
                out.push((Tok::Colon, Span { start, end: i }));
            }
            ',' => {
                i += 1;
                out.push((Tok::Comma, Span { start, end: i }));
            }
            '.' if chars.get(i + 1) == Some(&'.') => {
                i += 2;
                out.push((Tok::Range, Span { start, end: i }));
            }
            _ => {
                while i < chars.len() {
                    let ch = chars[i];
                    let range_here = ch == '.' && chars.get(i + 1) == Some(&'.') && i > start;
                    if ch.is_whitespace() || matches!(ch, ':' | ',' | '"') || range_here {
                        break;
                    }
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                out.push((Tok::Word(text), Span { start, end: i }));
            }
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<(Tok, Span)>,
    pos: usize,
    /// Length of the input in characters, for errors at the end.
    end: usize,
}

impl Parser {
    fn done(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&(Tok, Span)> {
        self.tokens.get(self.pos)
    }

    fn peek_at(&self, offset: usize) -> Option<&(Tok, Span)> {
        self.tokens.get(self.pos + offset)
    }

    /// Where the next token is, or the end of the input.
    fn here(&self) -> Span {
        self.peek().map_or(
            Span {
                start: self.end,
                end: self.end,
            },
            |(_, s)| *s,
        )
    }

    fn is_word(tok: Option<&(Tok, Span)>, word: &str) -> bool {
        matches!(tok, Some((Tok::Word(w), _)) if w.eq_ignore_ascii_case(word))
    }

    fn at_sort(&self) -> bool {
        Self::is_word(self.peek(), "sort")
            && Self::is_word(self.peek_at(1), "by")
            && matches!(self.peek_at(2), Some((Tok::Colon, _)))
    }

    fn sort(&mut self) -> Result<Vec<SortKey>, ParseError> {
        let keyword = self.here();
        self.pos += 3;
        let mut keys = Vec::new();
        loop {
            let Some((Tok::Word(word), span)) = self.peek().cloned() else {
                return Err(ParseError::new(
                    "Expected a sort field after `sort by:`",
                    self.here_or(keyword),
                ));
            };
            let field = SortField::from_name(&word).ok_or_else(|| {
                ParseError::new(
                    format!(
                        "Unknown sort field `{word}`. Use priority, updated, created, resolved, estimate, state or key"
                    ),
                    span,
                )
            })?;
            self.pos += 1;
            let mut descending = false;
            let mut last = span;
            if let Some((Tok::Word(dir), dspan)) = self.peek().cloned()
                && (dir.eq_ignore_ascii_case("desc") || dir.eq_ignore_ascii_case("asc"))
            {
                descending = dir.eq_ignore_ascii_case("desc");
                last = dspan;
                self.pos += 1;
            }
            keys.push(SortKey {
                field,
                descending,
                span: span.to(last),
            });
            if matches!(self.peek(), Some((Tok::Comma, _))) {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(keys)
    }

    /// The span of the next token, or `fallback` at the end.
    fn here_or(&self, fallback: Span) -> Span {
        self.peek().map_or(fallback, |(_, s)| *s)
    }

    fn clause(&mut self) -> Result<Clause, ParseError> {
        let mut negated = false;
        let mut first = self.here();
        if Self::is_word(self.peek(), "not") && !matches!(self.peek_at(1), Some((Tok::Colon, _))) {
            negated = true;
            self.pos += 1;
            if self.done() {
                return Err(ParseError::new("`not` needs a term after it", first));
            }
        }
        // A leading `-` on a word negates the clause, so `-state: Done` and `-crash` work.
        if let Some((Tok::Word(word), span)) = self.peek().cloned()
            && let Some(rest) = word.strip_prefix('-')
        {
            if rest.is_empty() {
                return Err(ParseError::new("`-` needs a term after it", span));
            }
            negated = !negated;
            first = span;
            self.tokens[self.pos] = (
                Tok::Word(rest.to_string()),
                Span {
                    start: span.start + 1,
                    end: span.end,
                },
            );
        }
        let (body, last) = self.body()?;
        Ok(Clause {
            negated,
            span: first.to(last),
            body,
        })
    }

    fn body(&mut self) -> Result<(Body, Span), ParseError> {
        let Some((tok, span)) = self.peek().cloned() else {
            return Err(ParseError::new("Expected a search term", self.here()));
        };
        match tok {
            Tok::Quoted(text) => {
                self.pos += 1;
                let value = Value {
                    text,
                    quoted: true,
                    span,
                };
                Ok((Body::Text(value), span))
            }
            Tok::Word(word) => {
                if matches!(self.peek_at(1), Some((Tok::Colon, _))) {
                    let field = Field::from_name(&word)
                        .ok_or_else(|| ParseError::new(unknown_field_message(&word), span))?;
                    self.pos += 2;
                    self.field_body(field, span)
                } else if let Some(tag) = word.strip_prefix('#') {
                    if tag.is_empty() {
                        return Err(ParseError::new("`#` needs a tag name after it", span));
                    }
                    self.pos += 1;
                    let value = Value {
                        text: tag.to_string(),
                        quoted: false,
                        span: Span {
                            start: span.start + 1,
                            end: span.end,
                        },
                    };
                    Ok((Body::Tag(vec![value]), span))
                } else {
                    self.pos += 1;
                    let value = Value {
                        text: word,
                        quoted: false,
                        span,
                    };
                    Ok((Body::Text(value), span))
                }
            }
            Tok::Colon => Err(ParseError::new(
                "Unexpected `:` with no field name before it",
                span,
            )),
            Tok::Comma => Err(ParseError::new("Unexpected `,`", span)),
            Tok::Range => Err(ParseError::new("Unexpected `..`", span)),
        }
    }

    /// The values of one `field:` clause, checked for that field.
    fn field_body(&mut self, field: Field, field_span: Span) -> Result<(Body, Span), ParseError> {
        let colon_span = Span {
            start: field_span.end,
            end: field_span.end + 1,
        };
        let mut items: Vec<(Item, Span)> = Vec::new();
        loop {
            let first = self.value().ok_or_else(|| {
                ParseError::new(
                    format!("Expected a value after `{}:`", field_name(field)),
                    self.missing_span(colon_span),
                )
            })?;
            let item_span;
            let item = if matches!(self.peek(), Some((Tok::Range, _))) {
                self.pos += 1;
                let second = self.value().ok_or_else(|| {
                    ParseError::new("Expected a value after `..`", self.missing_span(first.span))
                })?;
                item_span = first.span.to(second.span);
                Item::Range(first, second)
            } else {
                item_span = first.span;
                Item::Single(first)
            };
            items.push((item, item_span));
            if matches!(self.peek(), Some((Tok::Comma, _))) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let last = items.last().map_or(field_span, |(_, s)| *s);
        let span = field_span.to(last);
        let body = match field {
            Field::Project => Body::Project(singles(field, items, |v| check_project(&v))?),
            Field::State => Body::State(states(items)?),
            Field::Assignee => Body::Assignee(singles(field, items, Ok::<Value, ParseError>)?),
            Field::Reporter => Body::Reporter(singles(field, items, Ok::<Value, ParseError>)?),
            Field::Type => Body::Type(singles(field, items, |v| {
                check_choice(v, "type", ISSUE_TYPES)
            })?),
            Field::Priority => Body::Priority(singles(field, items, |v| {
                check_choice(v, "priority", PRIORITIES)
            })?),
            Field::Sprint => Body::Sprint(singles(field, items, Ok::<Value, ParseError>)?),
            Field::Parent => Body::Parent(singles(field, items, check_issue_key)?),
            Field::Tag => Body::Tag(singles(field, items, |v| {
                let text = v.text.strip_prefix('#').unwrap_or(&v.text).to_string();
                if text.is_empty() {
                    return Err(ParseError::new("A tag needs a name", v.span));
                }
                Ok(Value { text, ..v })
            })?),
            Field::Created => Body::Date(DateField::Created, dates(items)?),
            Field::Updated => Body::Date(DateField::Updated, dates(items)?),
            Field::Resolved => Body::Date(DateField::Resolved, dates(items)?),
            Field::Estimate => Body::Estimate(estimates(items)?),
            Field::Has => Body::Has(has_kinds(items)?),
        };
        Ok((body, span))
    }

    /// Span to report a missing value at: just after the colon or range marker, or the end of input.
    fn missing_span(&self, after: Span) -> Span {
        match self.peek() {
            Some((_, s)) => *s,
            None => Span {
                start: after.end,
                end: after.end,
            },
        }
    }

    /// One value: a phrase, or unquoted words joined by spaces. Stops before the next field, the
    /// next `-` term, `not`, `sort by:`, a comma or a range marker.
    fn value(&mut self) -> Option<Value> {
        match self.peek()?.clone() {
            (Tok::Quoted(text), span) => {
                self.pos += 1;
                Some(Value {
                    text,
                    quoted: true,
                    span,
                })
            }
            (Tok::Word(first), first_span) => {
                self.pos += 1;
                let mut text = first;
                let mut last = first_span;
                while let Some((Tok::Word(next), next_span)) = self.peek().cloned() {
                    let followed_by_colon = matches!(self.peek_at(1), Some((Tok::Colon, _)));
                    let sort_follows = next.eq_ignore_ascii_case("sort")
                        && Self::is_word(self.peek_at(1), "by")
                        && matches!(self.peek_at(2), Some((Tok::Colon, _)));
                    if followed_by_colon
                        || sort_follows
                        || next.starts_with('-')
                        || next.eq_ignore_ascii_case("not")
                    {
                        break;
                    }
                    text.push(' ');
                    text.push_str(&next);
                    last = next_span;
                    self.pos += 1;
                }
                Some(Value {
                    text,
                    quoted: false,
                    span: first_span.to(last),
                })
            }
            _ => None,
        }
    }
}

/// A value, or the two ends of a range.
enum Item {
    Single(Value),
    Range(Value, Value),
}

fn field_name(field: Field) -> &'static str {
    Field::NAMES[match field {
        Field::Project => 0,
        Field::State => 1,
        Field::Assignee => 2,
        Field::Reporter => 3,
        Field::Type => 4,
        Field::Priority => 5,
        Field::Sprint => 6,
        Field::Tag => 7,
        Field::Parent => 8,
        Field::Created => 9,
        Field::Updated => 10,
        Field::Resolved => 11,
        Field::Estimate => 12,
        Field::Has => 13,
    }]
}

fn unknown_field_message(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let suggestion = Field::NAMES
        .iter()
        .map(|name| (edit_distance(&lower, name), *name))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| format!(". Did you mean `{name}`?"))
        .unwrap_or_default();
    format!(
        "Unknown field `{word}`{suggestion} Fields: {}",
        Field::NAMES.join(", ")
    )
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Single values only: a range is refused for this field.
fn singles(
    field: Field,
    items: Vec<(Item, Span)>,
    check: impl Fn(Value) -> Result<Value, ParseError>,
) -> Result<Vec<Value>, ParseError> {
    items
        .into_iter()
        .map(|(item, _)| match item {
            Item::Single(v) => check(v),
            Item::Range(_, second) => Err(ParseError::new(
                format!("`{}:` does not take a range", field_name(field)),
                second.span,
            )),
        })
        .collect()
}

fn check_project(v: &Value) -> Result<Value, ParseError> {
    let key = v.text.to_ascii_uppercase();
    let valid = (2..=10).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_uppercase())
        && key.chars().all(|c| c.is_ascii_alphanumeric());
    if !valid {
        return Err(ParseError::new(
            format!(
                "`{}` is not a project key. Use letters and digits, such as EMB",
                v.text
            ),
            v.span,
        ));
    }
    Ok(Value {
        text: key,
        ..v.clone()
    })
}

fn check_choice(v: Value, what: &str, allowed: &[&str]) -> Result<Value, ParseError> {
    // Matched case-insensitively; the stored (capitalised) value is what the query carries.
    let Some(stored) = allowed.iter().find(|a| a.eq_ignore_ascii_case(&v.text)) else {
        return Err(ParseError::new(
            format!("`{}` is not a {what}. Use {}", v.text, allowed.join(", ")),
            v.span,
        ));
    };
    Ok(Value {
        text: (*stored).to_string(),
        ..v
    })
}

fn check_issue_key(v: Value) -> Result<Value, ParseError> {
    let bad = || ParseError::new("Expected an issue key such as EMB-12", v.span);
    let (project, number) = v.text.rsplit_once('-').ok_or_else(bad)?;
    let project = check_project(&Value {
        text: project.to_string(),
        ..v.clone()
    })
    .map_err(|_| bad())?;
    let number_ok = !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && number.parse::<u32>().is_ok_and(|n| n >= 1);
    if !number_ok {
        return Err(bad());
    }
    Ok(Value {
        text: format!("{}-{number}", project.text),
        ..v
    })
}

fn states(items: Vec<(Item, Span)>) -> Result<Vec<Spanned<StateRef>>, ParseError> {
    singles(Field::State, items, Ok::<Value, ParseError>)?
        .into_iter()
        .map(|v| -> Result<Spanned<StateRef>, ParseError> {
            let span = v.span;
            let value = if let Some(name) = v.text.strip_prefix('#') {
                let category = if name.eq_ignore_ascii_case("unresolved") {
                    Category::Unresolved
                } else if name.eq_ignore_ascii_case("resolved") {
                    Category::Resolved
                } else {
                    return Err(ParseError::new(
                        format!("`{}` is not a state category. Use #Unresolved or #Resolved, or a state name", v.text),
                        span,
                    ));
                };
                StateRef::Category(category)
            } else {
                StateRef::Name(v.text)
            };
            Ok(Spanned { value, span })
        })
        .collect()
}

fn parse_date_expr(v: &Value) -> Result<DateExpr, ParseError> {
    let text = v.text.as_str();
    if text.eq_ignore_ascii_case("today") {
        return Ok(DateExpr::Today);
    }
    if let Some(days) = text.strip_prefix('-').and_then(|t| t.strip_suffix('d'))
        && let Ok(n) = days.parse::<u32>()
    {
        return Ok(DateExpr::Ago(n));
    }
    Date::parse(text).map(DateExpr::Day).ok_or_else(|| {
        ParseError::new(
            format!("`{text}` is not a date. Use YYYY-MM-DD, today, or -Nd such as -7d"),
            v.span,
        )
    })
}

fn dates(items: Vec<(Item, Span)>) -> Result<Vec<Spanned<DateItem>>, ParseError> {
    items
        .into_iter()
        .map(|(item, span)| -> Result<Spanned<DateItem>, ParseError> {
            let value = match item {
                Item::Single(v) => DateItem::Day(parse_date_expr(&v)?),
                Item::Range(a, b) => DateItem::Range(parse_date_expr(&a)?, parse_date_expr(&b)?),
            };
            Ok(Spanned { value, span })
        })
        .collect()
}

fn parse_number(text: &str, span: Span) -> Result<u32, ParseError> {
    text.trim().parse::<u32>().map_err(|_| {
        ParseError::new(
            format!(
                "`{}` is not a number. Use 5, >3, <=2 or 2 .. 5",
                text.trim()
            ),
            span,
        )
    })
}

fn estimates(items: Vec<(Item, Span)>) -> Result<Vec<Spanned<NumItem>>, ParseError> {
    items
        .into_iter()
        .map(|(item, span)| -> Result<Spanned<NumItem>, ParseError> {
            let value = match item {
                Item::Range(a, b) => {
                    let lo = parse_number(&a.text, a.span)?;
                    let hi = parse_number(&b.text, b.span)?;
                    if lo > hi {
                        return Err(ParseError::new(
                            "The range starts above where it ends",
                            span,
                        ));
                    }
                    NumItem::Between(lo, hi)
                }
                Item::Single(v) => {
                    let text = v.text.trim();
                    let (op, rest) = [">=", "<=", ">", "<", "="]
                        .iter()
                        .find(|op| text.starts_with(*op))
                        .map_or(("", text), |op| (*op, &text[op.len()..]));
                    let n = parse_number(rest, v.span)?;
                    match op {
                        ">=" => NumItem::MoreEq(n),
                        "<=" => NumItem::LessEq(n),
                        ">" => NumItem::More(n),
                        "<" => NumItem::Less(n),
                        _ => NumItem::Exact(n),
                    }
                }
            };
            Ok(Spanned { value, span })
        })
        .collect()
}

fn has_kinds(items: Vec<(Item, Span)>) -> Result<Vec<Spanned<HasKind>>, ParseError> {
    singles(Field::Has, items, Ok::<Value, ParseError>)?
        .into_iter()
        .map(|v| -> Result<Spanned<HasKind>, ParseError> {
            let span = v.span;
            let value = match v.text.to_ascii_lowercase().as_str() {
                "assignee" => HasKind::Assignee,
                "estimate" => HasKind::Estimate,
                "sprint" => HasKind::Sprint,
                "subtasks" => HasKind::Subtasks,
                "links" => HasKind::Links,
                _ => {
                    return Err(ParseError::new(
                        format!(
                            "`{}` is not a `has:` kind. Use assignee, estimate, sprint, subtasks or links",
                            v.text
                        ),
                        span,
                    ));
                }
            };
            Ok(Spanned { value, span })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> Query {
        parse(input).unwrap_or_else(|e| panic!("{input:?} should parse: {e}"))
    }

    fn err(input: &str) -> ParseError {
        match parse(input) {
            Ok(q) => panic!("{input:?} should fail, got {q:?}"),
            Err(e) => e,
        }
    }

    fn only(input: &str) -> Body {
        let mut q = ok(input);
        assert_eq!(q.clauses.len(), 1, "{input:?}");
        q.clauses.remove(0).body
    }

    fn texts(values: &[Value]) -> Vec<&str> {
        values.iter().map(|v| v.text.as_str()).collect()
    }

    #[test]
    fn empty_and_blank_queries_are_valid() {
        assert_eq!(ok(""), Query::default());
        assert_eq!(ok("   \t "), Query::default());
    }

    #[test]
    fn bare_words_and_phrases_are_full_text() {
        let q = ok("login crash");
        assert_eq!(q.clauses.len(), 2);
        assert!(matches!(&q.clauses[0].body, Body::Text(v) if v.text == "login"));
        let Body::Text(phrase) = only("\"save as draft\"") else {
            panic!("phrase")
        };
        assert_eq!(phrase.text, "save as draft");
        assert!(phrase.quoted);
    }

    #[test]
    fn a_quote_without_its_partner_is_an_error_at_the_quote() {
        let e = err("login \"save");
        assert_eq!(e.span, Span { start: 6, end: 11 });
        assert!(e.message.contains("closing quote"), "{e}");
    }

    #[test]
    fn hash_words_are_tags_and_tag_field_takes_both_forms() {
        assert!(matches!(only("#ui"), Body::Tag(v) if v[0].text == "ui"));
        assert!(
            matches!(only("tag: #ui, bug"), Body::Tag(v) if v[0].text == "ui" && v[1].text == "bug")
        );
        let e = err("#");
        assert!(e.message.contains("tag name"), "{e}");
    }

    #[test]
    fn project_keys_are_upper_cased_and_checked() {
        let Body::Project(v) = only("project: emb, hld") else {
            panic!("project")
        };
        assert_eq!(texts(&v), ["EMB", "HLD"]);
        let e = err("project: 1A");
        assert_eq!(e.span, Span { start: 9, end: 11 });
    }

    #[test]
    fn state_names_and_categories() {
        let Body::State(v) = only("state: Open, In Progress") else {
            panic!("state")
        };
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].value, StateRef::Name("Open".into()));
        assert_eq!(v[1].value, StateRef::Name("In Progress".into()));
        let Body::State(c) = only("state: #Unresolved, #resolved") else {
            panic!("state")
        };
        assert_eq!(c[0].value, StateRef::Category(Category::Unresolved));
        assert_eq!(c[1].value, StateRef::Category(Category::Resolved));
        let e = err("state: #Open");
        assert!(e.message.contains("#Unresolved or #Resolved"), "{e}");
    }

    #[test]
    fn people_fields_keep_the_text_for_the_backend() {
        let Body::Assignee(v) = only("assignee: me, Unassigned, \"Alice Smith\"") else {
            panic!("assignee")
        };
        assert_eq!(texts(&v), ["me", "Unassigned", "Alice Smith"]);
        let Body::Reporter(r) = only("reporter: alice@example.com") else {
            panic!("reporter")
        };
        assert_eq!(texts(&r), ["alice@example.com"]);
    }

    #[test]
    fn type_and_priority_are_checked_and_canonicalised() {
        let Body::Type(t) = only("type: Bug, feature") else {
            panic!("type")
        };
        assert_eq!(texts(&t), ["Bug", "Feature"]);
        let Body::Priority(p) = only("priority: CRITICAL") else {
            panic!("priority")
        };
        assert_eq!(texts(&p), ["Critical"]);
        let e = err("priority: urgent");
        assert_eq!(e.span, Span { start: 10, end: 16 });
        assert!(e.message.contains("Critical, High, Normal, Low"), "{e}");
        let e = err("type: story");
        assert!(e.message.contains("Task, Bug, Feature, Epic"), "{e}");
    }

    #[test]
    fn sprint_values_are_names_or_current() {
        let Body::Sprint(v) = only("sprint: current") else {
            panic!("sprint")
        };
        assert_eq!(texts(&v), ["current"]);
        let Body::Sprint(n) = only("sprint: Sprint 3") else {
            panic!("sprint")
        };
        assert_eq!(texts(&n), ["Sprint 3"]);
    }

    #[test]
    fn parent_takes_an_issue_key() {
        let Body::Parent(v) = only("parent: emb-12") else {
            panic!("parent")
        };
        assert_eq!(texts(&v), ["EMB-12"]);
        let e = err("parent: EMB-0");
        assert!(e.message.contains("issue key"), "{e}");
        assert!(err("parent: 12").message.contains("issue key"));
    }

    #[test]
    fn date_fields_accept_days_today_relative_and_ranges() {
        let Body::Date(DateField::Created, v) = only("created: 2026-11-01") else {
            panic!("created")
        };
        assert_eq!(
            v[0].value,
            DateItem::Day(DateExpr::Day(Date {
                year: 2026,
                month: 11,
                day: 1
            }))
        );
        let Body::Date(DateField::Updated, v) = only("updated: today, -7d") else {
            panic!("updated")
        };
        assert_eq!(v[0].value, DateItem::Day(DateExpr::Today));
        assert_eq!(v[1].value, DateItem::Day(DateExpr::Ago(7)));
        let Body::Date(DateField::Resolved, v) = only("resolved: 2026-11-01 .. 2026-11-15") else {
            panic!("resolved")
        };
        assert!(matches!(
            v[0].value,
            DateItem::Range(DateExpr::Day(_), DateExpr::Day(_))
        ));
        let Body::Date(_, v) = only("created: -7d..today") else {
            panic!("range glued")
        };
        assert_eq!(
            v[0].value,
            DateItem::Range(DateExpr::Ago(7), DateExpr::Today)
        );
    }

    #[test]
    fn bad_dates_are_refused_with_their_span() {
        let e = err("created: 2026-02-30");
        assert_eq!(e.span, Span { start: 9, end: 19 });
        assert!(e.message.contains("YYYY-MM-DD"), "{e}");
        assert!(err("created: yesterday").message.contains("not a date"));
    }

    #[test]
    fn estimates_take_comparisons_and_ranges() {
        let Body::Estimate(v) = only("estimate: >3, <=2, 5, >= 1, 2 .. 8") else {
            panic!("estimate")
        };
        let values: Vec<NumItem> = v.iter().map(|s| s.value).collect();
        assert_eq!(
            values,
            [
                NumItem::More(3),
                NumItem::LessEq(2),
                NumItem::Exact(5),
                NumItem::MoreEq(1),
                NumItem::Between(2, 8),
            ]
        );
        assert!(err("estimate: many").message.contains("not a number"));
        assert!(err("estimate: 9 .. 2").message.contains("starts above"));
    }

    #[test]
    fn has_takes_the_five_kinds() {
        let Body::Has(v) = only("has: assignee, sprint, subtasks, links, estimate") else {
            panic!("has")
        };
        assert_eq!(v.len(), 5);
        assert_eq!(v[1].value, HasKind::Sprint);
        assert_eq!(v[2].value, HasKind::Subtasks);
        let e = err("has: owner");
        assert!(e.message.contains("assignee, estimate, sprint"), "{e}");
    }

    #[test]
    fn negation_with_a_minus_or_not() {
        let q = ok("-state: Done not has: links -crash");
        assert_eq!(q.clauses.len(), 3);
        assert!(q.clauses[0].negated);
        assert!(q.clauses[1].negated);
        assert!(q.clauses[2].negated);
        assert!(matches!(&q.clauses[2].body, Body::Text(v) if v.text == "crash"));
        let r = ok("not state: Done");
        assert!(r.clauses[0].negated);
        assert!(matches!(r.clauses[0].body, Body::State(_)));
        // The span of a negated clause starts at the minus sign.
        assert_eq!(q.clauses[0].span.start, 0);
        assert!(err("-").message.contains("needs a term"));
        assert!(err("not").message.contains("needs a term"));
    }

    #[test]
    fn a_negated_minus_and_not_flip_back() {
        let q = ok("not -state: Done");
        assert!(!q.clauses[0].negated);
    }

    #[test]
    fn sort_keys_with_directions() {
        let q = ok("project: EMB sort by: priority desc, updated");
        assert_eq!(q.clauses.len(), 1);
        assert_eq!(q.sort.len(), 2);
        assert_eq!(q.sort[0].field, SortField::Priority);
        assert!(q.sort[0].descending);
        assert_eq!(q.sort[1].field, SortField::Updated);
        assert!(!q.sort[1].descending);
        let r = ok("sort by: Estimate ASC, number desc");
        assert_eq!(r.sort[0].field, SortField::Estimate);
        assert_eq!(r.sort[1].field, SortField::Key);
        assert!(r.sort[1].descending);
    }

    #[test]
    fn unknown_sort_field_is_refused_with_span() {
        let e = err("sort by: colour");
        assert_eq!(e.span, Span { start: 9, end: 15 });
        assert!(e.message.contains("Unknown sort field"), "{e}");
    }

    #[test]
    fn unknown_field_gets_a_span_and_a_suggestion() {
        let e = err("prority: high");
        assert_eq!(e.span, Span { start: 0, end: 7 });
        assert!(e.message.contains("Did you mean `priority`"), "{e}");
        let far = err("colour: red");
        assert!(!far.message.contains("Did you mean"), "{far}");
        assert_eq!(far.to_string(), format!("{} (columns 1-6)", far.message));
    }

    #[test]
    fn missing_values_are_reported_after_the_colon() {
        let e = err("state:");
        assert!(e.message.contains("Expected a value after `state:`"), "{e}");
        assert_eq!(e.span, Span { start: 6, end: 6 });
        let f = err("state: Open,");
        assert!(f.message.contains("Expected a value"), "{f}");
    }

    #[test]
    fn stray_punctuation_is_an_error() {
        assert!(err(": Open").message.contains("no field name"));
        assert!(err(", open").message.contains("Unexpected `,`"));
    }

    #[test]
    fn a_value_stops_before_the_next_field() {
        let q = ok("state: In Progress assignee: me");
        assert_eq!(q.clauses.len(), 2);
        let Body::State(v) = &q.clauses[0].body else {
            panic!("state")
        };
        assert_eq!(v[0].value, StateRef::Name("In Progress".into()));
    }

    #[test]
    fn spans_point_at_the_value_text() {
        let q = ok("priority: high");
        let Body::Priority(v) = &q.clauses[0].body else {
            panic!("priority")
        };
        assert_eq!(v[0].span, Span { start: 10, end: 14 });
        assert_eq!(q.clauses[0].span, Span { start: 0, end: 14 });
    }

    #[test]
    fn date_arithmetic_matches_the_calendar() {
        assert_eq!(
            Date {
                year: 1970,
                month: 1,
                day: 1
            }
            .to_unix_day(),
            0
        );
        assert_eq!(
            Date::from_unix_day(0),
            Date {
                year: 1970,
                month: 1,
                day: 1
            }
        );
        let d = Date::parse("2026-11-15").expect("date");
        assert_eq!(Date::from_unix_day(d.to_unix_day()), d);
        assert_eq!(
            d.minus_days(15),
            Date {
                year: 2026,
                month: 10,
                day: 31
            }
        );
        assert_eq!(Date::from_unix(1_800_000_000).to_string(), "2027-01-15");
        assert_eq!(Date::parse("2024-02-29").map(|d| d.day), Some(29));
        assert!(Date::parse("2023-02-29").is_none());
        assert!(Date::parse("2026-1-01").is_none());
        assert!(Date::parse("2026-11-01-02").is_none());
    }
}
