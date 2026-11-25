//! Person rules shared by the area pages: avatar initials, the photo media type and the admin role check.

use space_proto::people::{PersonProfile, ROLE_ADMIN};

/// The initials the avatar draws: the first letter of the first two words, upper case. A blank name gives "?".
pub fn initials(name: &str) -> String {
    let mut out = String::new();
    for word in name.split_whitespace().take(2) {
        if let Some(first) = word.chars().next() {
            out.extend(first.to_uppercase());
        }
    }
    if out.is_empty() { "?".to_string() } else { out }
}

/// Whether the caller holds the admin role.
pub fn is_admin(me: &PersonProfile) -> bool {
    me.profile.roles.iter().any(|role| role == ROLE_ADMIN)
}

/// The media type of a photo by its file name, or `None` for a type this app does not upload.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn content_type(name: &str) -> Option<&'static str> {
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_letter_of_the_first_two_words() {
        assert_eq!(initials("Ana Souza"), "AS");
        assert_eq!(initials("  daniel  "), "D");
        assert_eq!(initials("ana maria souza"), "AM");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("   "), "?");
    }

    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    #[test]
    fn photos_are_png_jpeg_or_webp_only() {
        assert_eq!(content_type("me.PNG"), Some("image/png"));
        assert_eq!(content_type("me.jpeg"), Some("image/jpeg"));
        assert_eq!(content_type("me.webp"), Some("image/webp"));
        assert_eq!(content_type("me.gif"), None);
        assert_eq!(content_type("me"), None);
    }
}
