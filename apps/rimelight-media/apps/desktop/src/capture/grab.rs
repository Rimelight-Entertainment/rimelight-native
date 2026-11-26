//! Screen pictures with Windows' own GDI calls (no FFmpeg device library): the whole virtual desktop.

use rimelight_picture::Picture;
use rimelight_platform::{ScreenGrabber, ScreenRect};

/// BGRA rows to RGBA with full alpha (GDI leaves alpha undefined).
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    out
}

/// A picture of every monitor, and the rectangle it covers.
pub fn screenshot() -> Result<(Picture, ScreenRect), String> {
    let vs = rimelight_platform::virtual_screen();
    let mut grabber = ScreenGrabber::new(vs).ok_or("Could not reach the screen")?;
    let bgra = grabber.grab(false).ok_or("Could not copy the screen")?;
    Ok((
        Picture {
            w: vs.w as u32,
            h: vs.h as u32,
            rgba: bgra_to_rgba(bgra),
        },
        vs,
    ))
}
