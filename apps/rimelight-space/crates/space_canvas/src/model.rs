//! The typed item model over the live room's `props`, and the document that holds the live items.
//!
//! The room stores each item as a kind, a z key, a version and a JSON `props` map. This module reads that map into an
//! [`Item`] and writes it back. Reading is forgiving: a wrong type falls back to the default, and unknown keys are kept
//! in [`Item::extra`] so a write never drops another client's fields.
//!
//! Props schema (the contract between clients; key names are short and fixed). `z`, the id and the version are not
//! props: they travel on the item itself.
//!
//! | Key | Kinds | Type | Default | Meaning |
//! |-----|-------|------|---------|---------|
//! | `x`, `y` | all | number | 0 | Top-left corner in canvas units |
//! | `w`, `h` | all | number | 1 | Size; a missing or non-positive value reads as 1 |
//! | `rot` | all | number | 0 | Rotation in degrees |
//! | `flip_x`, `flip_y` | all | bool | false | Mirror flags |
//! | `opacity` | all | number 0..1 | 1 | Clamped to 0..1 |
//! | `locked` | all | bool | false | Item cannot be edited |
//! | `group` | all | string or null | null | Group id |
//! | `frame` | all | string or null | null | Id of the Frame item this item belongs to |
//! | `asset` | Image | string | "" | Asset id |
//! | `crop` | Image | {x,y,w,h} or null | null | Fractions 0..1 of the image; null is the whole image |
//! | `gray` | Image | bool | false | Draw in greyscale |
//! | `pixel` | Image | bool | false | Nearest-neighbour sampling (pixelated) instead of smooth |
//! | `name` | Image | string | "" | File name |
//! | `added` | Image | integer (epoch ms) | 0 | When it was added |
//! | `avg` | Image | "#rrggbb" or null | null | Average colour, for sorting by colour |
//! | `comment` | Image | string | "" | Caption shown under the image |
//! | `asset` | Video | string | "" | Asset id of the video file |
//! | `name` | Video | string | "" | File name |
//! | `added` | Video | integer (epoch ms) | 0 | When it was added |
//! | `volume` | Video | number 0..1 | 1 | Playback volume; clamped to 0..1 |
//! | `muted` | Video | bool | false | Sound off for everyone |
//! | `loop` | Video | bool | false | Play again from the start when it ends |
//! | `text` | Text | string | "" | The text |
//! | `color` | Text | palette name | "ink" | Text colour |
//! | `fill` | Text, Shape | palette name or null | null | Background; null is none |
//! | `size` | Text | number | 24 | Font size in world units |
//! | `align` | Text | "left", "center", "right" | "left" | Text alignment |
//! | `shape` | Shape | "rect", "ellipse", "diamond", "line", "arrow" | "rect" | Outline kind |
//! | `stroke` | Shape | palette name | "ink" | Outline colour |
//! | `sw` | Shape, Stroke | number | 2 (Shape), 3 (Stroke) | Stroke width |
//! | `pts` | Shape, Stroke | [[x,y], ...] | Shape: [[0,0],[w,h]]; Stroke: [] | Points relative to x,y |
//! | `start_bind`, `end_bind` | Shape | item id or null | null | Items a line or arrow is attached to |
//! | `head` | Shape | "arrow" or "none" | "arrow" for arrows, else "none" | Arrow head |
//! | `color` | Stroke | palette name | "ink" | Pen colour |
//! | `title` | Frame | string | "Frame" | Frame title |
//!
//! Palette names are in [`PALETTE`]. The UI maps each to a colour per theme; images are never recoloured.

use crate::geom::{Rect, RotRect, Vec2};
use serde_json::{Map, Value};
use space_proto::canvases::ItemState;
use std::collections::HashMap;

/// Palette names that colour text, shapes and strokes.
pub const PALETTE: &[&str] = &[
    "ink", "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "white",
];

/// Props every kind has.
const COMMON_KEYS: &[&str] = &[
    "x", "y", "w", "h", "rot", "flip_x", "flip_y", "opacity", "locked", "group", "frame",
];
const IMAGE_KEYS: &[&str] = &[
    "asset", "crop", "gray", "pixel", "name", "added", "avg", "comment",
];
const TEXT_KEYS: &[&str] = &["text", "color", "fill", "size", "align"];
const SHAPE_KEYS: &[&str] = &[
    "shape",
    "stroke",
    "fill",
    "sw",
    "pts",
    "start_bind",
    "end_bind",
    "head",
];
const STROKE_KEYS: &[&str] = &["pts", "color", "sw"];
const FRAME_KEYS: &[&str] = &["title"];
const VIDEO_KEYS: &[&str] = &["asset", "name", "added", "volume", "muted", "loop"];

/// The kind of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Image,
    Text,
    Shape,
    Stroke,
    Frame,
    Video,
}

impl ItemKind {
    /// The kind name the room stores (`"Image"`, `"Text"`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Text => "Text",
            Self::Shape => "Shape",
            Self::Stroke => "Stroke",
            Self::Frame => "Frame",
            Self::Video => "Video",
        }
    }

    /// Reads a kind name; None when it is not one this client knows.
    pub fn parse(s: &str) -> Option<ItemKind> {
        match s {
            "Image" => Some(Self::Image),
            "Text" => Some(Self::Text),
            "Shape" => Some(Self::Shape),
            "Stroke" => Some(Self::Stroke),
            "Frame" => Some(Self::Frame),
            "Video" => Some(Self::Video),
            _ => None,
        }
    }
}

/// The outline of a shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Diamond,
    Line,
    Arrow,
}

impl ShapeKind {
    /// The lowercase name stored in the `shape` prop.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rect => "rect",
            Self::Ellipse => "ellipse",
            Self::Diamond => "diamond",
            Self::Line => "line",
            Self::Arrow => "arrow",
        }
    }

    /// Reads a shape name; None when unknown.
    pub fn parse(s: &str) -> Option<ShapeKind> {
        match s {
            "rect" => Some(Self::Rect),
            "ellipse" => Some(Self::Ellipse),
            "diamond" => Some(Self::Diamond),
            "line" => Some(Self::Line),
            "arrow" => Some(Self::Arrow),
            _ => None,
        }
    }
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    /// The lowercase name stored in the `align` prop.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    /// Reads an alignment name; None when unknown.
    pub fn parse(s: &str) -> Option<TextAlign> {
        match s {
            "left" => Some(Self::Left),
            "center" => Some(Self::Center),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// The kind-specific data of an item.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemData {
    Image {
        asset: String,
        crop: Option<Rect>,
        gray: bool,
        pixelated: bool,
        name: String,
        added: i64,
        avg: Option<String>,
        comment: String,
    },
    Text {
        text: String,
        color: String,
        fill: Option<String>,
        size: f64,
        align: TextAlign,
    },
    Shape {
        shape: ShapeKind,
        stroke: String,
        fill: Option<String>,
        sw: f64,
        pts: Vec<Vec2>,
        start_bind: Option<String>,
        end_bind: Option<String>,
        head: bool,
    },
    Stroke {
        pts: Vec<Vec2>,
        color: String,
        sw: f64,
    },
    Frame {
        title: String,
    },
    /// A video file on the canvas. Playing state is local to each client and is not stored here.
    Video {
        asset: String,
        name: String,
        added: i64,
        volume: f64,
        muted: bool,
        looping: bool,
    },
}

impl ItemData {
    /// The kind this data belongs to.
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Image { .. } => ItemKind::Image,
            Self::Text { .. } => ItemKind::Text,
            Self::Shape { .. } => ItemKind::Shape,
            Self::Stroke { .. } => ItemKind::Stroke,
            Self::Frame { .. } => ItemKind::Frame,
            Self::Video { .. } => ItemKind::Video,
        }
    }
}

/// One item on a canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub z: String,
    pub version: u64,
    pub rect: Rect,
    pub rot: f64,
    pub flip_x: bool,
    pub flip_y: bool,
    pub opacity: f64,
    pub locked: bool,
    pub group: Option<String>,
    pub frame: Option<String>,
    pub data: ItemData,
    /// Props this client does not know, kept so a write never drops another client's fields.
    pub extra: Map<String, Value>,
}

impl Item {
    /// The kind of this item.
    pub fn kind(&self) -> ItemKind {
        self.data.kind()
    }

    /// The item's rectangle with its rotation.
    pub fn rot_rect(&self) -> RotRect {
        RotRect::new(self.rect, self.rot)
    }

    /// Reads an item from the room's state. None when the kind is unknown or the item is deleted.
    pub fn from_state(state: &ItemState) -> Option<Item> {
        if state.deleted {
            return None;
        }
        let kind = ItemKind::parse(&state.kind)?;
        let p = &state.props;
        let rect = Rect::new(
            get_f64(p, "x", 0.0),
            get_f64(p, "y", 0.0),
            positive_or_one(p, "w"),
            positive_or_one(p, "h"),
        );
        let data = parse_data(kind, p, rect);
        let extra = p
            .iter()
            .filter(|(key, _)| !is_known(kind, key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Some(Item {
            id: state.id.clone(),
            z: state.z.clone(),
            version: state.version,
            rect,
            rot: get_f64(p, "rot", 0.0),
            flip_x: get_bool(p, "flip_x", false),
            flip_y: get_bool(p, "flip_y", false),
            opacity: get_f64(p, "opacity", 1.0).clamp(0.0, 1.0),
            locked: get_bool(p, "locked", false),
            group: get_opt_str(p, "group"),
            frame: get_opt_str(p, "frame"),
            data,
            extra,
        })
    }

    /// The props map for this item: every known key written (null for absent optionals), plus `extra`.
    pub fn to_props(&self) -> Map<String, Value> {
        let mut m = self.extra.clone();
        put(&mut m, "x", self.rect.x);
        put(&mut m, "y", self.rect.y);
        put(&mut m, "w", self.rect.w);
        put(&mut m, "h", self.rect.h);
        put(&mut m, "rot", self.rot);
        put(&mut m, "flip_x", self.flip_x);
        put(&mut m, "flip_y", self.flip_y);
        put(&mut m, "opacity", self.opacity);
        put(&mut m, "locked", self.locked);
        put(&mut m, "group", self.group.clone());
        put(&mut m, "frame", self.frame.clone());
        match &self.data {
            ItemData::Image {
                asset,
                crop,
                gray,
                pixelated,
                name,
                added,
                avg,
                comment,
            } => {
                put(&mut m, "asset", asset.as_str());
                put(&mut m, "crop", crop.map_or(Value::Null, crop_value));
                put(&mut m, "gray", *gray);
                put(&mut m, "pixel", *pixelated);
                put(&mut m, "name", name.as_str());
                put(&mut m, "added", *added);
                put(&mut m, "avg", avg.clone());
                put(&mut m, "comment", comment.as_str());
            }
            ItemData::Text {
                text,
                color,
                fill,
                size,
                align,
            } => {
                put(&mut m, "text", text.as_str());
                put(&mut m, "color", color.as_str());
                put(&mut m, "fill", fill.clone());
                put(&mut m, "size", *size);
                put(&mut m, "align", align.as_str());
            }
            ItemData::Shape {
                shape,
                stroke,
                fill,
                sw,
                pts,
                start_bind,
                end_bind,
                head,
            } => {
                put(&mut m, "shape", shape.as_str());
                put(&mut m, "stroke", stroke.as_str());
                put(&mut m, "fill", fill.clone());
                put(&mut m, "sw", *sw);
                put(&mut m, "pts", points_value(pts));
                put(&mut m, "start_bind", start_bind.clone());
                put(&mut m, "end_bind", end_bind.clone());
                put(&mut m, "head", if *head { "arrow" } else { "none" });
            }
            ItemData::Stroke { pts, color, sw } => {
                put(&mut m, "pts", points_value(pts));
                put(&mut m, "color", color.as_str());
                put(&mut m, "sw", *sw);
            }
            ItemData::Frame { title } => {
                put(&mut m, "title", title.as_str());
            }
            ItemData::Video {
                asset,
                name,
                added,
                volume,
                muted,
                looping,
            } => {
                put(&mut m, "asset", asset.as_str());
                put(&mut m, "name", name.as_str());
                put(&mut m, "added", *added);
                put(&mut m, "volume", *volume);
                put(&mut m, "muted", *muted);
                put(&mut m, "loop", *looping);
            }
        }
        m
    }

    /// A new item of a kind with default data at `rect`. The id and z are supplied; version starts at 0.
    pub fn new(id: String, z: String, rect: Rect, data: ItemData) -> Item {
        Item {
            id,
            z,
            version: 0,
            rect,
            rot: 0.0,
            flip_x: false,
            flip_y: false,
            opacity: 1.0,
            locked: false,
            group: None,
            frame: None,
            data,
            extra: Map::new(),
        }
    }
}

/// Everything the client knows about a canvas: the live items by id.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    items: HashMap<String, Item>,
}

impl Doc {
    /// A document holding the given room states (deleted ones leave nothing behind).
    pub fn from_states<'a, I: IntoIterator<Item = &'a ItemState>>(states: I) -> Doc {
        let mut doc = Doc::default();
        for state in states {
            doc.apply_state(state);
        }
        doc
    }

    /// Applies a state from the room (hello, ack or applied). A state older than the known version is ignored.
    /// Returns true when something changed.
    pub fn apply_state(&mut self, state: &ItemState) -> bool {
        if self
            .items
            .get(&state.id)
            .is_some_and(|known| state.version < known.version)
        {
            return false;
        }
        if state.deleted {
            return self.items.remove(&state.id).is_some();
        }
        match Item::from_state(state) {
            // A kind this client cannot read is not shown or edited, so it is treated as absent.
            None => self.items.remove(&state.id).is_some(),
            Some(item) => {
                if self.items.get(&state.id) == Some(&item) {
                    return false;
                }
                self.items.insert(state.id.clone(), item);
                true
            }
        }
    }

    /// Sets an item locally before the room confirms it (optimistic edit).
    pub fn put(&mut self, item: Item) {
        self.items.insert(item.id.clone(), item);
    }

    /// Removes an item locally; returns it when it was present.
    pub fn remove(&mut self, id: &str) -> Option<Item> {
        self.items.remove(id)
    }

    /// The item with this id.
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.get(id)
    }

    /// Number of live items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when there are no live items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Items in draw order: ascending (z, id).
    pub fn ordered(&self) -> Vec<&Item> {
        let mut items: Vec<&Item> = self.items.values().collect();
        items.sort_by(|a, b| a.z.cmp(&b.z).then_with(|| a.id.cmp(&b.id)));
        items
    }

    /// Items whose `frame` is this frame id, in draw order.
    pub fn children_of_frame(&self, frame_id: &str) -> Vec<&Item> {
        self.ordered()
            .into_iter()
            .filter(|item| item.frame.as_deref() == Some(frame_id))
            .collect()
    }

    /// Items sharing this group id, in draw order.
    pub fn group_members(&self, group_id: &str) -> Vec<&Item> {
        self.ordered()
            .into_iter()
            .filter(|item| item.group.as_deref() == Some(group_id))
            .collect()
    }

    /// The bounding box of the rotated rectangles of these ids. Unknown ids are skipped; None if none exist.
    pub fn bounds(&self, ids: &[String]) -> Option<Rect> {
        let mut acc: Option<(f64, f64, f64, f64)> = None;
        for id in ids {
            let Some(item) = self.get(id) else {
                continue;
            };
            let b = item.rot_rect().aabb();
            let (x1, y1) = (b.x + b.w, b.y + b.h);
            acc = Some(match acc {
                None => (b.x, b.y, x1, y1),
                Some((x0, y0, ax1, ay1)) => (x0.min(b.x), y0.min(b.y), ax1.max(x1), ay1.max(y1)),
            });
        }
        acc.map(|(x0, y0, x1, y1)| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// The props a kind reads on top of the common ones.
fn kind_keys(kind: ItemKind) -> &'static [&'static str] {
    match kind {
        ItemKind::Image => IMAGE_KEYS,
        ItemKind::Text => TEXT_KEYS,
        ItemKind::Shape => SHAPE_KEYS,
        ItemKind::Stroke => STROKE_KEYS,
        ItemKind::Frame => FRAME_KEYS,
        ItemKind::Video => VIDEO_KEYS,
    }
}

fn is_known(kind: ItemKind, key: &str) -> bool {
    COMMON_KEYS.contains(&key) || kind_keys(kind).contains(&key)
}

fn get_f64(p: &Map<String, Value>, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn get_i64(p: &Map<String, Value>, key: &str, default: i64) -> i64 {
    p.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn get_bool(p: &Map<String, Value>, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn get_str(p: &Map<String, Value>, key: &str, default: &str) -> String {
    p.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

/// A string prop that may be null; any other type reads as None.
fn get_opt_str(p: &Map<String, Value>, key: &str) -> Option<String> {
    p.get(key).and_then(Value::as_str).map(str::to_string)
}

fn get_parsed<T>(p: &Map<String, Value>, key: &str, parse: fn(&str) -> Option<T>, default: T) -> T {
    p.get(key)
        .and_then(Value::as_str)
        .and_then(parse)
        .unwrap_or(default)
}

/// A size prop: a missing, non-numeric or non-positive value reads as 1.
fn positive_or_one(p: &Map<String, Value>, key: &str) -> f64 {
    let v = get_f64(p, key, 1.0);
    if v > 0.0 { v } else { 1.0 }
}

fn in_unit(v: f64) -> bool {
    (0.0..=1.0).contains(&v)
}

/// A crop object. Any field outside 0..=1, or a non-positive size, makes the crop None.
fn parse_crop(v: Option<&Value>) -> Option<Rect> {
    let m = v?.as_object()?;
    let x = m.get("x")?.as_f64()?;
    let y = m.get("y")?.as_f64()?;
    let w = m.get("w")?.as_f64()?;
    let h = m.get("h")?.as_f64()?;
    let ok = [x, y, w, h].into_iter().all(in_unit) && w > 0.0 && h > 0.0;
    ok.then(|| Rect::new(x, y, w, h))
}

fn crop_value(r: Rect) -> Value {
    serde_json::json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h})
}

/// A `#rrggbb` colour; anything else reads as None.
fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A point list. Entries that are not two numbers are skipped. None when the prop is not an array.
fn parse_points(v: Option<&Value>) -> Option<Vec<Vec2>> {
    let arr = v?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|entry| match entry.as_array()?.as_slice() {
                [x, y] => Some(Vec2::new(x.as_f64()?, y.as_f64()?)),
                _ => None,
            })
            .collect(),
    )
}

fn points_value(pts: &[Vec2]) -> Value {
    Value::Array(
        pts.iter()
            .map(|p| Value::Array(vec![Value::from(p.x), Value::from(p.y)]))
            .collect(),
    )
}

fn parse_data(kind: ItemKind, p: &Map<String, Value>, rect: Rect) -> ItemData {
    match kind {
        ItemKind::Image => ItemData::Image {
            asset: get_str(p, "asset", ""),
            crop: parse_crop(p.get("crop")),
            gray: get_bool(p, "gray", false),
            pixelated: get_bool(p, "pixel", false),
            name: get_str(p, "name", ""),
            added: get_i64(p, "added", 0),
            avg: get_opt_str(p, "avg").filter(|s| is_hex_colour(s)),
            comment: get_str(p, "comment", ""),
        },
        ItemKind::Text => ItemData::Text {
            text: get_str(p, "text", ""),
            color: get_str(p, "color", "ink"),
            fill: get_opt_str(p, "fill"),
            size: get_f64(p, "size", 24.0),
            align: get_parsed(p, "align", TextAlign::parse, TextAlign::Left),
        },
        ItemKind::Shape => {
            let shape = get_parsed(p, "shape", ShapeKind::parse, ShapeKind::Rect);
            let default_pts = vec![Vec2::new(0.0, 0.0), Vec2::new(rect.w, rect.h)];
            ItemData::Shape {
                shape,
                stroke: get_str(p, "stroke", "ink"),
                fill: get_opt_str(p, "fill"),
                sw: get_f64(p, "sw", 2.0),
                pts: parse_points(p.get("pts")).unwrap_or(default_pts),
                start_bind: get_opt_str(p, "start_bind"),
                end_bind: get_opt_str(p, "end_bind"),
                head: match p.get("head").and_then(Value::as_str) {
                    Some("arrow") => true,
                    Some("none") => false,
                    _ => shape == ShapeKind::Arrow,
                },
            }
        }
        ItemKind::Stroke => ItemData::Stroke {
            pts: parse_points(p.get("pts")).unwrap_or_default(),
            color: get_str(p, "color", "ink"),
            sw: get_f64(p, "sw", 3.0),
        },
        ItemKind::Frame => ItemData::Frame {
            title: get_str(p, "title", "Frame"),
        },
        ItemKind::Video => ItemData::Video {
            asset: get_str(p, "asset", ""),
            name: get_str(p, "name", ""),
            added: get_i64(p, "added", 0),
            volume: get_f64(p, "volume", 1.0).clamp(0.0, 1.0),
            muted: get_bool(p, "muted", false),
            looping: get_bool(p, "loop", false),
        },
    }
}

fn put(m: &mut Map<String, Value>, key: &str, value: impl Into<Value>) {
    m.insert(key.to_string(), value.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state_of(item: &Item) -> ItemState {
        ItemState {
            id: item.id.clone(),
            kind: item.kind().as_str().to_string(),
            z: item.z.clone(),
            version: item.version,
            props: item.to_props(),
            deleted: false,
            created_by: "u1".to_string(),
            updated_by: "u1".to_string(),
            updated_at: 0,
        }
    }

    fn with_data(id: &str, z: &str, data: ItemData) -> Item {
        let mut item = Item::new(
            id.to_string(),
            z.to_string(),
            Rect::new(1.0, 2.0, 30.0, 40.0),
            data,
        );
        item.version = 3;
        item.rot = 15.0;
        item.flip_x = true;
        item.opacity = 0.5;
        item.locked = true;
        item.group = Some("g1".to_string());
        item.frame = Some("f1".to_string());
        item
    }

    fn all_kinds() -> Vec<Item> {
        vec![
            with_data(
                "img",
                "a0",
                ItemData::Image {
                    asset: "asset1".to_string(),
                    crop: Some(Rect::new(0.1, 0.2, 0.5, 0.25)),
                    gray: true,
                    pixelated: true,
                    name: "pic.png".to_string(),
                    added: 1_795_000_000_000,
                    avg: Some("#ff8000".to_string()),
                    comment: "A caption".to_string(),
                },
            ),
            with_data(
                "txt",
                "a1",
                ItemData::Text {
                    text: "hello".to_string(),
                    color: "red".to_string(),
                    fill: Some("white".to_string()),
                    size: 18.5,
                    align: TextAlign::Center,
                },
            ),
            with_data(
                "shp",
                "a2",
                ItemData::Shape {
                    shape: ShapeKind::Arrow,
                    stroke: "blue".to_string(),
                    fill: None,
                    sw: 4.0,
                    pts: vec![Vec2::new(0.0, 0.0), Vec2::new(30.0, 40.0)],
                    start_bind: Some("txt".to_string()),
                    end_bind: None,
                    head: true,
                },
            ),
            with_data(
                "stk",
                "a3",
                ItemData::Stroke {
                    pts: vec![Vec2::new(1.0, 1.0), Vec2::new(2.0, 3.0)],
                    color: "green".to_string(),
                    sw: 5.0,
                },
            ),
            with_data(
                "frm",
                "a4",
                ItemData::Frame {
                    title: "Board".to_string(),
                },
            ),
            with_data(
                "vid",
                "a5",
                ItemData::Video {
                    asset: "asset2".to_string(),
                    name: "clip.mp4".to_string(),
                    added: 1_795_000_000_500,
                    volume: 0.25,
                    muted: true,
                    looping: true,
                },
            ),
        ]
    }

    #[test]
    fn round_trip_keeps_every_field_for_every_kind() {
        for item in all_kinds() {
            let back = Item::from_state(&state_of(&item)).expect("known kind");
            assert_eq!(back, item);
        }
    }

    #[test]
    fn unknown_props_are_kept_and_written_back() {
        let mut item = all_kinds().remove(4);
        item.extra.insert("future".to_string(), json!({"k": 1}));
        let back = Item::from_state(&state_of(&item)).expect("known kind");
        assert_eq!(back.extra.get("future"), Some(&json!({"k": 1})));
        assert_eq!(back.to_props().get("future"), Some(&json!({"k": 1})));
    }

    #[test]
    fn bad_crop_becomes_none() {
        let mut state = state_of(&all_kinds()[0]);
        state.props.insert(
            "crop".to_string(),
            json!({"x": 0.5, "y": 0.0, "w": 1.5, "h": 1.0}),
        );
        let item = Item::from_state(&state).expect("known kind");
        assert!(matches!(item.data, ItemData::Image { crop: None, .. }));

        state.props.insert(
            "crop".to_string(),
            json!({"x": 0.0, "y": 0.0, "w": 0.0, "h": 1.0}),
        );
        let item = Item::from_state(&state).expect("known kind");
        assert!(matches!(item.data, ItemData::Image { crop: None, .. }));
    }

    #[test]
    fn wrong_types_fall_back_to_defaults() {
        let state = ItemState {
            id: "t".to_string(),
            kind: "Text".to_string(),
            z: "a0".to_string(),
            version: 1,
            props: json!({"x": "left", "w": -4, "opacity": 9, "text": 7, "align": "up"})
                .as_object()
                .cloned()
                .unwrap_or_default(),
            deleted: false,
            created_by: "u".to_string(),
            updated_by: "u".to_string(),
            updated_at: 0,
        };
        let item = Item::from_state(&state).expect("known kind");
        assert_eq!(item.rect.x, 0.0);
        assert_eq!(item.rect.w, 1.0);
        assert_eq!(item.opacity, 1.0);
        assert!(matches!(
            item.data,
            ItemData::Text { ref text, align: TextAlign::Left, .. } if text.is_empty()
        ));
    }

    #[test]
    fn unknown_kind_and_deleted_read_as_none() {
        let mut state = state_of(&all_kinds()[0]);
        state.kind = "Sticker".to_string();
        assert!(Item::from_state(&state).is_none());
        let mut state = state_of(&all_kinds()[0]);
        state.deleted = true;
        assert!(Item::from_state(&state).is_none());
    }

    #[test]
    fn video_props_default_and_volume_clamps() {
        let state = ItemState {
            id: "v".to_string(),
            kind: "Video".to_string(),
            z: "a0".to_string(),
            version: 1,
            props: json!({"asset": "a1", "volume": 3.0})
                .as_object()
                .cloned()
                .unwrap_or_default(),
            deleted: false,
            created_by: "u".to_string(),
            updated_by: "u".to_string(),
            updated_at: 0,
        };
        let item = Item::from_state(&state).expect("known kind");
        assert!(matches!(
            item.data,
            ItemData::Video { ref asset, volume, muted: false, looping: false, .. }
                if asset == "a1" && volume == 1.0
        ));
    }

    #[test]
    fn apply_state_ignores_older_and_removes_deleted() {
        let mut doc = Doc::default();
        let mut item = all_kinds().remove(4);
        item.version = 2;
        assert!(doc.apply_state(&state_of(&item)));
        assert_eq!(doc.len(), 1);

        let mut older = state_of(&item);
        older.version = 1;
        older.props.insert("title".to_string(), json!("old"));
        assert!(!doc.apply_state(&older));
        assert_eq!(doc.get("frm").map(|i| i.version), Some(2));

        let mut deleted = state_of(&item);
        deleted.version = 3;
        deleted.deleted = true;
        assert!(doc.apply_state(&deleted));
        assert!(doc.is_empty());
    }

    #[test]
    fn ordered_sorts_by_z_then_id() {
        let mut doc = Doc::default();
        doc.put(Item::new(
            "x".into(),
            "b".into(),
            Rect::default(),
            ItemData::Frame {
                title: String::new(),
            },
        ));
        doc.put(Item::new(
            "y".into(),
            "a".into(),
            Rect::default(),
            ItemData::Frame {
                title: String::new(),
            },
        ));
        doc.put(Item::new(
            "w".into(),
            "a".into(),
            Rect::default(),
            ItemData::Frame {
                title: String::new(),
            },
        ));
        let order: Vec<(&str, &str)> = doc
            .ordered()
            .into_iter()
            .map(|i| (i.z.as_str(), i.id.as_str()))
            .collect();
        // Equal z ("a") falls back to the id: w before y; then z "b".
        assert_eq!(order, vec![("a", "w"), ("a", "y"), ("b", "x")]);
    }
}
