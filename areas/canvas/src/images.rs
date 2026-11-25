//! Canvas pictures (CV.7): decoding with animation frames, greyscale copies, WebP thumbnails, level of detail, the
//! eyedropper sample and the desktop disk cache. No Slint UI; buffers are Send so the work runs on a worker.

use std::path::PathBuf;

use rimelight_picture::{AnimationLimits, Picture};

/// A decoded RGBA picture, shareable across threads.
pub type Buffer = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

/// Most frames kept from one animated picture.
const MAX_FRAMES: usize = 600;

/// A decoded picture: one frame for still images, every frame with its delay for animated GIF and WebP.
#[derive(Clone)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Buffer>,
    pub delays_ms: Vec<u32>,
}

impl Decoded {
    /// True when the picture has more than one frame.
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// The first frame (the still picture, or the first frame of an animation).
    pub fn first(&self) -> &Buffer {
        &self.frames[0]
    }

    /// A still picture from RGBA bytes (4 bytes per pixel, rows top to bottom). Bytes of the wrong length give a blank
    /// picture of that size.
    pub fn from_rgba(width: u32, height: u32, pixels: &[u8]) -> Decoded {
        let mut buffer = Buffer::new(width, height);
        // Bytes of the wrong length leave the picture blank, as before.
        if pixels.len() == buffer.as_bytes().len() {
            buffer.make_mut_bytes().copy_from_slice(pixels);
        }
        Decoded {
            width,
            height,
            frames: vec![buffer],
            delays_ms: vec![100],
        }
    }
}

/// A shared picture as a Slint buffer (one copy of the pixels).
fn buffer_from_picture(pic: &Picture) -> Buffer {
    let mut buffer = Buffer::new(pic.w, pic.h);
    buffer.make_mut_bytes().copy_from_slice(&pic.rgba);
    buffer
}

/// A Slint buffer as a shared picture (one copy of the pixels).
fn picture_from_buffer(buf: &Buffer) -> Picture {
    Picture {
        w: buf.width(),
        h: buf.height(),
        rgba: buf.as_bytes().to_vec(),
    }
}

/// Decodes PNG, JPEG, WebP, GIF, BMP, ICO or SVG bytes (`rimelight_picture`, with EXIF orientation applied). GIF and
/// animated WebP give every frame: delays are at least 20 ms (a frame that shows 10 ms or less is 100 ms, as the
/// shared decoder gives it), and at most 600 frames are kept.
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    let limits = AnimationLimits {
        max_frames: MAX_FRAMES,
        max_bytes: usize::MAX,
    };
    if let Some(animation) = rimelight_picture::load_animation_bytes(bytes, &limits) {
        let frames: Vec<Buffer> = animation
            .frames
            .iter()
            .map(|(pic, _)| buffer_from_picture(pic))
            .collect();
        let delays_ms: Vec<u32> = animation
            .frames
            .iter()
            .map(|(_, ms)| (*ms).max(20))
            .collect();
        let first = frames
            .first()
            .ok_or_else(|| "picture has no frames".to_string())?;
        return Ok(Decoded {
            width: first.width(),
            height: first.height(),
            frames,
            delays_ms,
        });
    }
    let pic = rimelight_picture::load_bytes(bytes).map_err(|e| format!("picture: {e}"))?;
    let buffer = buffer_from_picture(&pic);
    Ok(Decoded {
        width: buffer.width(),
        height: buffer.height(),
        frames: vec![buffer],
        delays_ms: vec![100],
    })
}

/// Greyscale of one buffer (Rec. 709 luma, alpha kept).
fn grayscale_buffer(buf: &Buffer) -> Buffer {
    let mut out = Buffer::new(buf.width(), buf.height());
    for (dst, src) in out.make_mut_slice().iter_mut().zip(buf.as_slice()) {
        let luma =
            0.2126 * f64::from(src.r) + 0.7152 * f64::from(src.g) + 0.0722 * f64::from(src.b);
        let y = luma.round().clamp(0.0, 255.0) as u8;
        *dst = slint::Rgba8Pixel {
            r: y,
            g: y,
            b: y,
            a: src.a,
        };
    }
    out
}

/// A greyscale copy (Rec. 709 luma, alpha kept).
pub fn grayscale(buf: &Buffer) -> Buffer {
    grayscale_buffer(buf)
}

/// Every frame in greyscale.
pub fn grayscale_all(d: &Decoded) -> Decoded {
    Decoded {
        width: d.width,
        height: d.height,
        frames: d.frames.iter().map(grayscale_buffer).collect(),
        delays_ms: d.delays_ms.clone(),
    }
}

/// A lossless WebP thumbnail of the first frame with the longer side at most `max_side` (no upscaling), Triangle filter.
pub fn thumbnail_webp(d: &Decoded, max_side: u32) -> Result<Vec<u8>, String> {
    let picture = picture_from_buffer(d.first());
    rimelight_picture::encode_webp_lossless(&picture.thumbnail(max_side))
        .map_err(|e| format!("thumbnail: {e}"))
}

/// The picture to request for an item drawn `screen_long_side` px on its longer side: 256, 1024 or 0 (= the original).
/// `current` is what is shown now: keep it unless the size moved more than 10 % past a boundary (hysteresis).
pub fn level_for(screen_long_side: f64, current: u32) -> u32 {
    let raw = if screen_long_side <= 256.0 {
        256
    } else if screen_long_side <= 1024.0 {
        1024
    } else {
        0
    };
    if raw == current {
        return raw;
    }
    let keep = match current {
        256 => screen_long_side <= 256.0 * 1.1,
        1024 => (256.0 * 0.9..=1024.0 * 1.1).contains(&screen_long_side),
        0 => screen_long_side >= 1024.0 * 0.9,
        _ => false,
    };
    if keep { current } else { raw }
}

/// The RGBA pixel at fractions (0..1) of the picture (clamped).
pub fn sample(buf: &Buffer, x_frac: f64, y_frac: f64) -> [u8; 4] {
    let width = buf.width().max(1);
    let height = buf.height().max(1);
    let x = (x_frac.clamp(0.0, 1.0) * f64::from(width - 1)).round() as u32;
    let y = (y_frac.clamp(0.0, 1.0) * f64::from(height - 1)).round() as u32;
    let index = (y * width + x) as usize;
    match buf.as_slice().get(index) {
        Some(p) => [p.r, p.g, p.b, p.a],
        None => [0, 0, 0, 0],
    }
}

/// "#rrggbb".
pub fn hex(rgba: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2])
}

/// True for a 64-character hexadecimal sha256.
fn valid_sha(sha256: &str) -> bool {
    sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for the three stored variants.
fn valid_variant(variant: &str) -> bool {
    matches!(variant, "orig" | "256" | "1024")
}

/// Disk cache of picture bytes, keyed by sha256 and variant ("orig", "256", "1024"). Desktop only; elsewhere every call is a no-op.
pub struct DiskCache {
    dir: Option<PathBuf>,
}

impl DiskCache {
    /// `<OS cache dir>/rimelight-space/canvas`, created on first put. None-dir when unavailable.
    pub fn open() -> DiskCache {
        DiskCache { dir: cache_root() }
    }

    /// The file for a sha256 and variant, or None when either is invalid or there is no cache folder.
    fn path(&self, sha256: &str, variant: &str) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        if !valid_sha(sha256) || !valid_variant(variant) {
            return None;
        }
        Some(dir.join(format!("{sha256}-{variant}")))
    }

    /// The cached bytes, or None when absent or unreadable.
    pub fn get(&self, sha256: &str, variant: &str) -> Option<Vec<u8>> {
        let path = self.path(sha256, variant)?;
        std::fs::read(path).ok()
    }

    /// The cached file's path for a sha256 and variant, when the file exists.
    pub fn file(&self, sha256: &str, variant: &str) -> Option<PathBuf> {
        self.path(sha256, variant).filter(|path| path.is_file())
    }

    /// Stores bytes atomically (written to "<name>.tmp", then renamed). I/O errors are ignored.
    pub fn put(&self, sha256: &str, variant: &str, bytes: &[u8]) {
        let Some(path) = self.path(sha256, variant) else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let mut tmp_name = path.clone().into_os_string();
        tmp_name.push(".tmp");
        let tmp = PathBuf::from(tmp_name);
        if std::fs::write(&tmp, bytes).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Copies a file into the cache atomically (copied to "<name>.tmp", then renamed), without reading it all into
    /// memory. I/O errors are ignored.
    pub fn put_file(&self, sha256: &str, variant: &str, source: &std::path::Path) {
        let Some(path) = self.path(sha256, variant) else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let mut tmp_name = path.clone().into_os_string();
        tmp_name.push(".tmp");
        let tmp = PathBuf::from(tmp_name);
        if std::fs::copy(source, &tmp).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// A cache in the given folder (tests only).
    #[cfg(test)]
    pub fn at(dir: PathBuf) -> DiskCache {
        DiskCache { dir: Some(dir) }
    }
}

/// The cache folder on desktop: `<OS cache dir>/rimelight-space/canvas`.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
fn cache_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("rimelight-space").join("canvas"))
}

/// No disk cache outside desktop platforms.
#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
fn cache_root() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::new();
        let image = RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        png
    }

    #[test]
    fn a_png_decodes_to_one_frame_of_its_size() {
        let decoded = decode(&png_bytes(3, 2)).expect("decodes");
        assert_eq!(decoded.frames.len(), 1);
        assert!(!decoded.is_animated());
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!((decoded.first().width(), decoded.first().height()), (3, 2));
    }

    #[test]
    fn a_two_frame_gif_keeps_both_frames_and_delays() {
        let mut gif = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut gif);
            let frames = vec![
                image::Frame::from_parts(
                    RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(100, 1),
                ),
                image::Frame::from_parts(
                    RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(200, 1),
                ),
            ];
            encoder.encode_frames(frames).expect("encode gif");
        }
        let decoded = decode(&gif).expect("decodes");
        assert_eq!(decoded.frames.len(), 2);
        assert!(decoded.is_animated());
        assert_eq!(decoded.delays_ms, vec![100, 200]);
    }

    #[test]
    fn grayscale_of_pure_red_is_equal_channels_near_54() {
        let mut buf = Buffer::new(1, 1);
        buf.make_mut_slice()[0] = slint::Rgba8Pixel {
            r: 255,
            g: 0,
            b: 0,
            a: 200,
        };
        let gray = grayscale(&buf);
        let p = gray.as_slice()[0];
        assert_eq!(p.r, p.g);
        assert_eq!(p.g, p.b);
        assert!((53..=55).contains(&p.r), "luma was {}", p.r);
        assert_eq!(p.a, 200);
    }

    #[test]
    fn a_thumbnail_is_resized_to_the_longer_side_and_decodes_back() {
        let decoded = decode(&png_bytes(2000, 1000)).expect("decodes");
        let webp = thumbnail_webp(&decoded, 256).expect("encodes");
        let back = decode(&webp).expect("decodes webp");
        assert_eq!((back.width, back.height), (256, 128));
    }

    #[test]
    fn a_small_picture_is_not_upscaled_in_its_thumbnail() {
        let decoded = decode(&png_bytes(10, 5)).expect("decodes");
        let webp = thumbnail_webp(&decoded, 256).expect("encodes");
        let back = decode(&webp).expect("decodes webp");
        assert_eq!((back.width, back.height), (10, 5));
    }

    #[test]
    fn levels_follow_the_on_screen_size_with_hysteresis() {
        assert_eq!(level_for(100.0, 0), 256);
        assert_eq!(level_for(600.0, 256), 1024);
        assert_eq!(level_for(2000.0, 1024), 0);
        assert_eq!(level_for(270.0, 256), 256);
        assert_eq!(level_for(1000.0, 0), 0);
        assert_eq!(level_for(900.0, 0), 1024);
    }

    #[test]
    fn sample_reads_the_corners() {
        let mut buf = Buffer::new(2, 2);
        {
            let px = buf.make_mut_slice();
            px[0] = slint::Rgba8Pixel {
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            };
            px[1] = slint::Rgba8Pixel {
                r: 5,
                g: 6,
                b: 7,
                a: 8,
            };
            px[2] = slint::Rgba8Pixel {
                r: 9,
                g: 10,
                b: 11,
                a: 12,
            };
            px[3] = slint::Rgba8Pixel {
                r: 13,
                g: 14,
                b: 15,
                a: 16,
            };
        }
        assert_eq!(sample(&buf, 0.0, 0.0), [1, 2, 3, 4]);
        assert_eq!(sample(&buf, 1.0, 0.0), [5, 6, 7, 8]);
        assert_eq!(sample(&buf, 0.0, 1.0), [9, 10, 11, 12]);
        assert_eq!(sample(&buf, 1.0, 1.0), [13, 14, 15, 16]);
        assert_eq!(sample(&buf, 5.0, -3.0), [5, 6, 7, 8]);
    }

    #[test]
    fn hex_is_lowercase_rrggbb() {
        assert_eq!(hex([255, 0, 16, 255]), "#ff0010");
    }

    #[test]
    fn the_disk_cache_refuses_a_bad_sha_and_a_bad_variant() {
        let dir =
            std::env::temp_dir().join(format!("rimelight-canvas-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let cache = DiskCache {
            dir: Some(dir.clone()),
        };
        cache.put("not-a-sha", "orig", b"data");
        cache.put(&"a".repeat(64), "999", b"data");
        assert!(cache.get("not-a-sha", "orig").is_none());
        assert!(cache.get(&"a".repeat(64), "999").is_none());
        assert_eq!(std::fs::read_dir(&dir).expect("read dir").count(), 0);
        let good = "a".repeat(64);
        cache.put(&good, "256", b"data");
        assert_eq!(cache.get(&good, "256"), Some(b"data".to_vec()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
