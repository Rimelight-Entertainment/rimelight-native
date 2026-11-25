// Export helpers for the canvas (CV.13): a CPU render of items to an RGBA image (PNG export), PNG bytes, file names that
// are safe on Windows, and the backup zip (canvas.json plus assets/) with its reader. Light theme colours, white background.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::sync::OnceLock;

use ab_glyph::{FontRef, PxScale};
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, Pixel, Rgba, RgbaImage, imageops};
use imageproc::drawing::{draw_filled_circle_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::geometric_transformations::{Interpolation, rotate_about_center};
use space_canvas::geom::Rect;
use space_canvas::model::{Item, ItemData, ShapeKind, TextAlign};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// A decoded picture as the page keeps it (RGBA8, not premultiplied).
pub type Buffer = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

type Color = Rgba<u8>;

const WHITE: Color = Rgba([255, 255, 255, 255]);
const ELLIPSE_STEPS: u32 = 120;
const MAX_BACKUP_ENTRIES: usize = 2000;
const MAX_BACKUP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

// Noto Sans Regular, the app's UI font, bundled by the shared library (rimelight-rs, `rimelight_ui::fonts`).
const FONT_DATA: &[u8] = rimelight_ui::fonts::NOTO_SANS_REGULAR;
static FONT: OnceLock<Option<FontRef<'static>>> = OnceLock::new();

fn font() -> Option<&'static FontRef<'static>> {
    FONT.get_or_init(|| FontRef::try_from_slice(FONT_DATA).ok())
        .as_ref()
}

/// The light-theme item colour for a palette name (as CanvasPalette.color in canvas.slint). Unknown names are ink.
fn palette(name: &str) -> [u8; 4] {
    let hex = match name {
        "gray" => 0x6b7280,
        "red" => 0xd92d20,
        "orange" => 0xe8590c,
        "yellow" => 0xb08800,
        "green" => 0x2b8a3e,
        "teal" => 0x0c8599,
        "blue" => 0x1c7ed6,
        "purple" => 0x7048e8,
        "pink" => 0xd6336c,
        "white" => 0xffffff,
        _ => 0x1e1e1e,
    };
    rgb(hex)
}

/// The light-theme note background for a palette name (as CanvasPalette.fill). Unknown names are gray.
fn palette_fill(name: &str) -> [u8; 4] {
    let hex = match name {
        "red" => 0xffe3e3,
        "orange" => 0xffe8cc,
        "yellow" => 0xfff3bf,
        "green" => 0xd3f9d8,
        "teal" => 0xc3fae8,
        "blue" => 0xd0ebff,
        "purple" => 0xe5dbff,
        "pink" => 0xfcc2d7,
        "white" => 0xffffff,
        _ => 0xe9ecef,
    };
    rgb(hex)
}

fn rgb(hex: u32) -> [u8; 4] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255]
}

/// Maps item-local world units to screen px of the output image.
#[derive(Clone, Copy)]
struct View {
    origin_x: f64,
    origin_y: f64,
    scale: f64,
}

impl View {
    /// A world point to output px.
    fn px(self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.origin_x) * self.scale,
            (y - self.origin_y) * self.scale,
        )
    }

    /// A point in item-local world units (from the item's top left) to output px, flipped, then rotated about the item centre.
    fn local(self, item: &Item, x: f64, y: f64) -> (f64, f64) {
        let lx = if item.flip_x { item.rect.w - x } else { x };
        let ly = if item.flip_y { item.rect.h - y } else { y };
        let (px, py) = self.px(item.rect.x + lx, item.rect.y + ly);
        let (cx, cy) = self.px(
            item.rect.x + item.rect.w / 2.0,
            item.rect.y + item.rect.h / 2.0,
        );
        rotate_about((px, py), (cx, cy), item.rot)
    }
}

/// Rotates a point clockwise (screen y points down) about a centre, by degrees.
fn rotate_about(p: (f64, f64), centre: (f64, f64), degrees: f64) -> (f64, f64) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p.0 - centre.0, p.1 - centre.1);
    (
        centre.0 + dx * cos - dy * sin,
        centre.1 + dx * sin + dy * cos,
    )
}

fn px_round(v: f64) -> u32 {
    v.round().max(1.0) as u32
}

fn px_ceil(v: f64) -> u32 {
    v.ceil().max(1.0) as u32
}

/// Renders items (draw order, bottom first) inside `bounds` (world) at `scale` px per world unit on a white background.
/// Images: their picture (the original buffer from `pictures` by asset id; skipped when missing), cropped by `crop`, resized
/// to the item's size, flipped, rotated about the item centre, with opacity, greyscale when `gray`. Videos: the same, from
/// the thumbnail the page passes under the asset id, with no crop and no greyscale. Text: wrapped to the item
/// width (simple word wrap), fill drawn when set. Shapes: Rect/Ellipse/Diamond outlines (and fill), Line/Arrow from pts with
/// a head. Strokes: polyline through pts. Frames: a thin grey outline and the title above. Lines are drawn at least 1 px.
pub fn render(
    items: &[&Item],
    pictures: &HashMap<String, Buffer>,
    bounds: Rect,
    scale: f64,
) -> RgbaImage {
    let mut canvas =
        RgbaImage::from_pixel(px_ceil(bounds.w * scale), px_ceil(bounds.h * scale), WHITE);
    let view = View {
        origin_x: bounds.x,
        origin_y: bounds.y,
        scale,
    };
    for &item in items {
        match &item.data {
            ItemData::Image { asset, .. } | ItemData::Video { asset, .. } => {
                if let Some(pic) = pictures.get(asset) {
                    draw_picture(&mut canvas, view, item, pic);
                }
            }
            ItemData::Text { .. } => draw_text(&mut canvas, view, item),
            ItemData::Shape { .. } => draw_shape(&mut canvas, view, item),
            ItemData::Stroke { .. } => draw_stroke(&mut canvas, view, item),
            ItemData::Frame { .. } => draw_frame(&mut canvas, view, item),
        }
    }
    canvas
}

/// The scale that keeps the longer side of `bounds` at most `max_px` (1.0 when it already fits).
pub fn fit_scale(bounds: Rect, max_px: u32) -> f64 {
    let longer = bounds.w.max(bounds.h);
    let limit = f64::from(max_px);
    if longer <= limit || longer <= 0.0 {
        1.0
    } else {
        limit / longer
    }
}

/// PNG bytes of an image.
pub fn png_bytes(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(err)?;
    Ok(out)
}

/// A backup zip: "canvas.json" (the given JSON, pretty) and "assets/<name>" for each (name, bytes).
pub fn backup_zip(
    canvas_json: &serde_json::Value,
    assets: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, String> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let json = serde_json::to_vec_pretty(canvas_json).map_err(err)?;
    zip.start_file(
        "canvas.json",
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .map_err(err)?;
    zip.write_all(&json).map_err(err)?;
    for (name, bytes) in assets {
        if !safe_asset_name(name) {
            return Err(format!("the asset name {name} cannot go into a backup"));
        }
        zip.start_file(
            format!("assets/{name}"),
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(err)?;
        zip.write_all(bytes).map_err(err)?;
    }
    let cursor = zip.finish().map_err(err)?;
    Ok(cursor.into_inner())
}

/// A read backup: the canvas JSON and the asset files as (name, bytes).
pub type Backup = (serde_json::Value, Vec<(String, Vec<u8>)>);

/// Reads a backup zip back. Refuses entries outside assets/ other than canvas.json, names with "..", and more than
/// 2000 entries or 2 GB in total.
pub fn read_backup(bytes: &[u8]) -> Result<Backup, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(err)?;
    if archive.len() > MAX_BACKUP_ENTRIES {
        return Err("the backup has too many entries".to_string());
    }
    let mut canvas: Option<serde_json::Value> = None;
    let mut assets = Vec::new();
    let mut used: u64 = 0;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(err)?;
        let name = file.name().to_string();
        if name.contains("..") {
            return Err(format!("the backup entry {name} is refused"));
        }
        if file.is_dir() {
            continue;
        }
        let limit = MAX_BACKUP_BYTES - used + 1;
        let mut data = Vec::new();
        file.by_ref()
            .take(limit)
            .read_to_end(&mut data)
            .map_err(err)?;
        used += data.len() as u64;
        if used > MAX_BACKUP_BYTES {
            return Err("the backup is too large".to_string());
        }
        if name == "canvas.json" {
            if canvas.is_some() {
                return Err("the backup has two canvas.json entries".to_string());
            }
            canvas = Some(serde_json::from_slice(&data).map_err(err)?);
        } else if let Some(asset) = name.strip_prefix("assets/") {
            if !safe_asset_name(asset) {
                return Err(format!("the backup entry {name} is refused"));
            }
            assets.push((asset.to_string(), data));
        } else {
            return Err(format!("the backup entry {name} is refused"));
        }
    }
    let canvas = canvas.ok_or_else(|| "the backup has no canvas.json".to_string())?;
    Ok((canvas, assets))
}

/// File names made unique ("a.png", "a (2).png", ...) and safe for Windows (no \ / : * ? " < > |, no trailing dots/spaces).
pub fn unique_file_names(names: &[String]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    names
        .iter()
        .map(|name| {
            let base = safe_file_name(name);
            let mut candidate = base.clone();
            let mut n = 2;
            while used.contains(&candidate.to_lowercase()) {
                candidate = numbered(&base, n);
                n += 1;
            }
            used.insert(candidate.to_lowercase());
            candidate
        })
        .collect()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// An asset name inside a backup: one plain file name, no folders and no "..".
fn safe_asset_name(name: &str) -> bool {
    !name.is_empty() && !name.contains("..") && !name.contains(['/', '\\', ':'])
}

/// A file name with the Windows-invalid characters replaced, no trailing dots or spaces, and no reserved device name.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        return "file".to_string();
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        format!("_{trimmed}")
    } else {
        trimmed
    }
}

/// "name (n).ext", inserting the number before the extension.
fn numbered(base: &str, n: u32) -> String {
    match base.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &base[..i], &base[i..]),
        _ => format!("{base} ({n})"),
    }
}

/// Draws a picture item: its (cropped, resized, flipped, greyed, faded) sprite, rotated about the item centre.
fn draw_picture(canvas: &mut RgbaImage, view: View, item: &Item, pic: &Buffer) {
    let Some(sprite) = picture_sprite(item, pic, view.scale) else {
        return;
    };
    if item.rot.abs() < 1e-9 {
        let (x, y) = view.px(item.rect.x, item.rect.y);
        imageops::overlay(canvas, &sprite, x.round() as i64, y.round() as i64);
        return;
    }
    // Rotate on a square pad large enough for the diagonal, so no corner is clipped.
    let (sw, sh) = sprite.dimensions();
    let side = f64::from(sw).hypot(f64::from(sh)).ceil() as u32 + 2;
    let mut padded = RgbaImage::new(side, side);
    imageops::overlay(
        &mut padded,
        &sprite,
        i64::from((side - sw) / 2),
        i64::from((side - sh) / 2),
    );
    let turned = rotate_about_center(
        &padded,
        item.rot.to_radians() as f32,
        Interpolation::Bilinear,
        Rgba([0, 0, 0, 0]),
    );
    let (cx, cy) = view.px(
        item.rect.x + item.rect.w / 2.0,
        item.rect.y + item.rect.h / 2.0,
    );
    let half = f64::from(side) / 2.0;
    imageops::overlay(
        canvas,
        &turned,
        (cx - half).round() as i64,
        (cy - half).round() as i64,
    );
}

/// The item's picture, cut to its crop, resized to the item's size on screen, then flipped, greyed and faded.
fn picture_sprite(item: &Item, pic: &Buffer, scale: f64) -> Option<RgbaImage> {
    // A video is drawn from its thumbnail as it is: no crop, no greyscale, smooth sampling.
    let (crop, gray, pixelated) = match &item.data {
        ItemData::Image {
            crop,
            gray,
            pixelated,
            ..
        } => (*crop, *gray, *pixelated),
        ItemData::Video { .. } => (None, false, false),
        _ => return None,
    };
    let (pw, ph) = (pic.width(), pic.height());
    if pw == 0 || ph == 0 {
        return None;
    }
    let src = RgbaImage::from_raw(pw, ph, pic.as_bytes().to_vec())?;
    let (x, y, w, h) = match crop {
        Some(c) => {
            let x = ((c.x * f64::from(pw)).round().max(0.0) as u32).min(pw - 1);
            let y = ((c.y * f64::from(ph)).round().max(0.0) as u32).min(ph - 1);
            let w = ((c.w * f64::from(pw)).round().max(1.0) as u32).min(pw - x);
            let h = ((c.h * f64::from(ph)).round().max(1.0) as u32).min(ph - y);
            (x, y, w, h)
        }
        None => (0, 0, pw, ph),
    };
    let part = imageops::crop_imm(&src, x, y, w, h).to_image();
    let filter = if pixelated {
        imageops::FilterType::Nearest
    } else {
        imageops::FilterType::Triangle
    };
    let mut sprite = imageops::resize(
        &part,
        px_round(item.rect.w * scale),
        px_round(item.rect.h * scale),
        filter,
    );
    if item.flip_x {
        imageops::flip_horizontal_in_place(&mut sprite);
    }
    if item.flip_y {
        imageops::flip_vertical_in_place(&mut sprite);
    }
    if gray {
        for p in sprite.pixels_mut() {
            let [r, g, b, a] = p.0;
            let l =
                (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)).round() as u8;
            p.0 = [l, l, l, a];
        }
    }
    if item.opacity < 1.0 {
        let opacity = item.opacity.clamp(0.0, 1.0);
        for p in sprite.pixels_mut() {
            p.0[3] = (f64::from(p.0[3]) * opacity).round() as u8;
        }
    }
    Some(sprite)
}

/// Draws a text note: its fill, then the word-wrapped text in the item colour.
fn draw_text(canvas: &mut RgbaImage, view: View, item: &Item) {
    let ItemData::Text {
        text,
        color,
        fill,
        size,
        align,
    } = &item.data
    else {
        return;
    };
    let Some(font) = font() else {
        return;
    };
    let pad = if fill.is_some() {
        6.0 * view.scale
    } else {
        0.0
    };
    let (x0, y0) = view.px(item.rect.x, item.rect.y);
    let box_w = item.rect.w * view.scale;
    let box_h = item.rect.h * view.scale;
    if let Some(name) = fill {
        fill_rect(canvas, x0, y0, box_w, box_h, Rgba(palette_fill(name)));
    }
    let px = (*size * view.scale).max(1.0) as f32;
    let scale = PxScale::from(px);
    let inner = (box_w - 2.0 * pad).max(1.0);
    let line_h = f64::from(px) * 1.3;
    let ink = Rgba(palette(color));
    let mut y = y0 + pad;
    for line in wrap_lines(text, font, scale, inner) {
        if !line.is_empty() {
            let line_w = f64::from(text_size(scale, font, &line).0);
            let dx = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (inner - line_w) / 2.0,
                TextAlign::Right => inner - line_w,
            };
            draw_text_mut(
                canvas,
                ink,
                (x0 + pad + dx).round() as i32,
                y.round() as i32,
                scale,
                font,
                &line,
            );
        }
        y += line_h;
    }
}

/// Greedy word wrap at `max_w` px. Existing line breaks are kept; a word wider than the line stays whole.
fn wrap_lines(text: &str, font: &FontRef<'_>, scale: PxScale, max_w: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
                continue;
            }
            let candidate = format!("{current} {word}");
            if f64::from(text_size(scale, font, &candidate).0) > max_w {
                lines.push(std::mem::replace(&mut current, word.to_string()));
            } else {
                current = candidate;
            }
        }
        lines.push(current);
    }
    lines
}

/// Draws a Rect, Ellipse, Diamond, Line or Arrow shape: outline (and fill for closed shapes), arrow head when `head`.
fn draw_shape(canvas: &mut RgbaImage, view: View, item: &Item) {
    let ItemData::Shape {
        shape,
        stroke,
        fill,
        sw,
        pts,
        head,
        ..
    } = &item.data
    else {
        return;
    };
    let (w, h) = (item.rect.w, item.rect.h);
    let width = (*sw * view.scale).max(1.0);
    let ink = Rgba(palette(stroke));
    let fill_color = fill.as_deref().map(|name| Rgba(palette_fill(name)));
    let closed: Vec<(f64, f64)> = match shape {
        ShapeKind::Rect => vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
        ShapeKind::Diamond => vec![(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)],
        ShapeKind::Ellipse => (0..ELLIPSE_STEPS)
            .map(|i| {
                let t = f64::from(i) / f64::from(ELLIPSE_STEPS) * std::f64::consts::TAU;
                (w / 2.0 + w / 2.0 * t.cos(), h / 2.0 + h / 2.0 * t.sin())
            })
            .collect(),
        ShapeKind::Line | ShapeKind::Arrow => {
            let line: Vec<(f64, f64)> = pts.iter().map(|p| view.local(item, p.x, p.y)).collect();
            polyline(canvas, &line, width, ink, false);
            if *head && line.len() >= 2 {
                draw_arrow_head(canvas, &line, width, ink);
            }
            return;
        }
    };
    let screen: Vec<(f64, f64)> = closed
        .iter()
        .map(|&(x, y)| view.local(item, x, y))
        .collect();
    if let Some(color) = fill_color {
        fill_polygon(canvas, &screen, color);
    }
    polyline(canvas, &screen, width, ink, true);
}

/// Two short barbs at the last point of a line, swept back 25 degrees either side of it.
fn draw_arrow_head(canvas: &mut RgbaImage, line: &[(f64, f64)], width: f64, color: Color) {
    let tip = line[line.len() - 1];
    let prev = line[line.len() - 2];
    let (dx, dy) = (tip.0 - prev.0, tip.1 - prev.1);
    let len = dx.hypot(dy);
    if len < 1e-9 {
        return;
    }
    let (back_x, back_y) = (-dx / len, -dy / len);
    let barb = (width * 4.0).max(10.0);
    for degrees in [25.0_f64, -25.0] {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (bx, by) = (back_x * cos - back_y * sin, back_x * sin + back_y * cos);
        thick_line(
            canvas,
            tip,
            (tip.0 + bx * barb, tip.1 + by * barb),
            width,
            color,
        );
    }
}

/// Draws a stroke (pen) item as a polyline through its points.
fn draw_stroke(canvas: &mut RgbaImage, view: View, item: &Item) {
    let ItemData::Stroke { pts, color, sw } = &item.data else {
        return;
    };
    let line: Vec<(f64, f64)> = pts.iter().map(|p| view.local(item, p.x, p.y)).collect();
    polyline(
        canvas,
        &line,
        (*sw * view.scale).max(1.0),
        Rgba(palette(color)),
        false,
    );
}

/// Draws a frame: a thin grey outline and its title just above it.
fn draw_frame(canvas: &mut RgbaImage, view: View, item: &Item) {
    let ItemData::Frame { title } = &item.data else {
        return;
    };
    let (x0, y0) = view.px(item.rect.x, item.rect.y);
    let (x1, y1) = view.px(item.rect.right(), item.rect.bottom());
    let grey = Rgba(palette("gray"));
    polyline(
        canvas,
        &[(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
        1.0,
        grey,
        true,
    );
    if let Some(font) = font() {
        draw_text_mut(
            canvas,
            grey,
            x0.round() as i32,
            (y0 - 18.0).round() as i32,
            PxScale::from(12.0),
            font,
            title.as_str(),
        );
    }
}

/// A polyline through the points, optionally closed. A single point draws a dot.
fn polyline(canvas: &mut RgbaImage, pts: &[(f64, f64)], width: f64, color: Color, closed: bool) {
    match pts {
        [] => {}
        [only] => thick_line(canvas, *only, *only, width, color),
        _ => {
            for pair in pts.windows(2) {
                thick_line(canvas, pair[0], pair[1], width, color);
            }
            if closed {
                thick_line(canvas, pts[pts.len() - 1], pts[0], width, color);
            }
        }
    }
}

/// A line `width` px thick: parallel 1 px-spaced lines across the width, so an odd count always includes the centre line.
fn thick_line(canvas: &mut RgbaImage, a: (f64, f64), b: (f64, f64), width: f64, color: Color) {
    let width = width.max(1.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len < 1e-9 {
        draw_filled_circle_mut(
            canvas,
            (a.0.round() as i32, a.1.round() as i32),
            (width / 2.0).round().max(1.0) as i32,
            color,
        );
        return;
    }
    let (nx, ny) = (-dy / len, dx / len);
    let count = ((width * 2.0).ceil() as u32).max(1) | 1;
    let spacing = width / f64::from(count - 1);
    for i in 0..count {
        let offset = -width / 2.0 + f64::from(i) * spacing;
        let (ox, oy) = (nx * offset, ny * offset);
        draw_line_segment_mut(
            canvas,
            ((a.0 + ox) as f32, (a.1 + oy) as f32),
            ((b.0 + ox) as f32, (b.1 + oy) as f32),
            color,
        );
    }
}

fn fill_rect(canvas: &mut RgbaImage, x: f64, y: f64, w: f64, h: f64, color: Color) {
    fill_polygon(
        canvas,
        &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
        color,
    );
}

/// Fills a polygon (even-odd scanline fill), blending the colour over the canvas.
fn fill_polygon(canvas: &mut RgbaImage, pts: &[(f64, f64)], color: Color) {
    if pts.len() < 3 {
        return;
    }
    let width = f64::from(canvas.width());
    let height = f64::from(canvas.height());
    let top = pts
        .iter()
        .map(|p| p.1)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0);
    let bottom = pts
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(height - 1.0);
    if bottom < top {
        return;
    }
    let mut crossings: Vec<f64> = Vec::new();
    for row in top as u32..=bottom as u32 {
        let sy = f64::from(row) + 0.5;
        crossings.clear();
        for (i, a) in pts.iter().enumerate() {
            let b = pts[(i + 1) % pts.len()];
            if (a.1 <= sy) != (b.1 <= sy) {
                crossings.push(a.0 + (sy - a.1) * (b.0 - a.0) / (b.1 - a.1));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let lo = pair[0].round().max(0.0);
            let hi = pair[1].round().min(width - 1.0);
            if hi < lo {
                continue;
            }
            for x in lo as u32..=hi as u32 {
                canvas.get_pixel_mut(x, row).blend(&color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::{Rgba8Pixel, SharedPixelBuffer};
    use std::io::{Cursor, Write};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    fn red_picture() -> Buffer {
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(10, 10);
        for p in buf.make_mut_slice() {
            *p = Rgba8Pixel {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            };
        }
        buf
    }

    #[test]
    fn fit_scale_keeps_the_longer_side() {
        assert!((fit_scale(Rect::new(0.0, 0.0, 100.0, 50.0), 50) - 0.5).abs() < 1e-9);
        assert!((fit_scale(Rect::new(0.0, 0.0, 10.0, 20.0), 100) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unique_file_names_are_unique_and_safe() {
        let names = vec![
            "a.png".to_string(),
            "a.png".to_string(),
            "x:y?.png".to_string(),
        ];
        assert_eq!(
            unique_file_names(&names),
            vec!["a.png", "a (2).png", "x_y_.png"]
        );
    }

    #[test]
    fn backup_round_trip() {
        let json = serde_json::json!({"title": "Grand Tale", "items": [1, 2]});
        let assets = vec![
            ("a.png".to_string(), vec![1u8, 2, 3]),
            ("b.jpg".to_string(), vec![9u8; 10]),
        ];
        let zip = backup_zip(&json, &assets).unwrap();
        let (back, files) = read_backup(&zip).unwrap();
        assert_eq!(back, json);
        assert_eq!(files, assets);
    }

    #[test]
    fn read_backup_refuses_escape_names() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("../evil", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(read_backup(&bytes).is_err());
    }

    #[test]
    fn picture_renders_red_at_the_centre() {
        let mut pictures = HashMap::new();
        pictures.insert("a".to_string(), red_picture());
        let item = Item::new(
            "i".to_string(),
            "a0".to_string(),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            ItemData::Image {
                asset: "a".to_string(),
                crop: None,
                gray: false,
                pixelated: false,
                name: "a.png".to_string(),
                added: 0,
                avg: None,
                comment: String::new(),
            },
        );
        let img = render(&[&item], &pictures, Rect::new(0.0, 0.0, 10.0, 10.0), 1.0);
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);
    }

    #[test]
    fn video_renders_its_thumbnail_at_the_centre() {
        let mut pictures = HashMap::new();
        pictures.insert("a".to_string(), red_picture());
        let item = Item::new(
            "v".to_string(),
            "a0".to_string(),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            ItemData::Video {
                asset: "a".to_string(),
                name: "a.mp4".to_string(),
                added: 0,
                volume: 1.0,
                muted: false,
                looping: false,
            },
        );
        let img = render(&[&item], &pictures, Rect::new(0.0, 0.0, 10.0, 10.0), 1.0);
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);
    }

    #[test]
    fn rect_shape_draws_an_ink_edge() {
        let item = Item::new(
            "s".to_string(),
            "a0".to_string(),
            Rect::new(2.0, 2.0, 10.0, 10.0),
            ItemData::Shape {
                shape: ShapeKind::Rect,
                stroke: "ink".to_string(),
                fill: None,
                sw: 2.0,
                pts: Vec::new(),
                start_bind: None,
                end_bind: None,
                head: false,
            },
        );
        let img = render(
            &[&item],
            &HashMap::new(),
            Rect::new(0.0, 0.0, 20.0, 20.0),
            1.0,
        );
        assert_eq!(img.get_pixel(7, 2).0, palette("ink"));
    }
}
