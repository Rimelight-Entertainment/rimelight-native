//! The app's picture glue: Slint images, the clipboard and the app icon. Decoding, editing, markup and saving are in
//! `rimelight_picture`.

use rimelight_picture::{Picture, render_svg_square};

/// The picture as a Slint image. One copy of the pixels, for a picture shown or a capture (not for video frames, which
/// go through `Rgba::into_slint_image`).
pub fn to_slint(pic: &Picture) -> slint::Image {
    let buf =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&pic.rgba, pic.w, pic.h);
    slint::Image::from_rgba8(buf)
}

/// Puts the picture on the clipboard.
pub fn copy_to_clipboard(pic: &Picture) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: pic.w as usize,
        height: pic.h as usize,
        bytes: std::borrow::Cow::Borrowed(&pic.rgba),
    })
    .map_err(|e| e.to_string())
}

/// The app icon (the logo drawn at `size`), for the places that need pixels: the .ico file at install.
pub fn logo(size: u32) -> Option<Picture> {
    render_svg_square(include_bytes!("../assets/icons/logo-color.svg"), size)
}
