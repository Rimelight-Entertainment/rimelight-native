//! Projects > Create project: the Create project dialog (`ui/create-project.slint`), opened by the project
//! switcher's "Create New Project" row.
//!
//! The dialog's values are checked by [`validate`] and become a `CreateProject` body. [`save`] sends it with the stored
//! session. The shell owns the dialog's properties and callbacks: it calls [`validate`] on `save`, then [`save`], and in
//! `done` it shows the server's message on failure, or closes the dialog and refreshes the project list on success.
//!
//! Key: 2 to 10 characters, uppercase letters and digits, starting with a letter. Name: not blank, at most 100 characters.
//! Description: optional, left out when blank.

use std::cell::RefCell;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer, Weak};
use space_client::error::ClientError;
use space_proto::projects::{CreateProject, Project};

use space_page_kit::page_data::{self, DataCell};

/// A picture chosen for the new project: the file, its media type and its bytes, and a decoded preview for the dialog.
#[derive(Clone)]
pub struct Picked {
    pub file_name: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
    pub preview: SharedPixelBuffer<Rgba8Pixel>,
}

/// The outcome of the native picker, on the worker.
pub enum PickOutcome {
    /// The dialog was closed without a file.
    Cancelled,
    /// The file is not a photo this app accepts, or it is too large. The text says why.
    Rejected(String),
    Chosen(Box<Picked>),
}

/// What a create returned: the new project, and the error of its picture upload when a picture was chosen and it failed.
pub struct Created {
    pub project: Project,
    pub picture_error: Option<String>,
}

thread_local! {
    /// The picture chosen in the dialog, taken by the create or cleared when the dialog closes (UI thread only).
    static PICKED: RefCell<Option<Picked>> = const { RefCell::new(None) };
}

/// Stores the picture chosen in the dialog (or clears it with `None`).
pub fn set_picked(picked: Option<Picked>) {
    PICKED.with(|slot| *slot.borrow_mut() = picked);
}

/// Takes the picture chosen in the dialog, leaving none behind.
pub fn take_picked() -> Option<Picked> {
    PICKED.with(|slot| slot.borrow_mut().take())
}

/// The shortest and longest project key.
pub const KEY_MIN: usize = 2;
pub const KEY_MAX: usize = 10;
/// The longest project name, in characters.
pub const NAME_MAX: usize = 100;

/// Checks the dialog's values and makes the create body. The key is only trimmed (it is not upper-cased for the person).
pub fn validate(key: &str, name: &str, description: &str) -> Result<CreateProject, String> {
    let key = key.trim();
    let len = key.chars().count();
    if !(KEY_MIN..=KEY_MAX).contains(&len) {
        return Err(format!("Enter a key of {KEY_MIN} to {KEY_MAX} characters."));
    }
    let mut chars = key.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    if !starts_with_letter || !chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        return Err(
            "The key starts with a letter and holds only uppercase letters and digits.".to_owned(),
        );
    }

    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name.".to_owned());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("The name is longer than {NAME_MAX} characters."));
    }

    let description = description.trim();
    Ok(CreateProject {
        key: key.to_owned(),
        name: name.to_owned(),
        description: (!description.is_empty()).then(|| description.to_owned()),
        lead_id: None,
    })
}

/// Sends the create with the stored session, then the picture when one was chosen. `done` runs on the UI thread with the new
/// project, or the server's error when the create failed. A failed picture upload does not fail the create: it comes back in
/// `Created::picture_error`. With no session source yet, `done` runs at once with `Unauthorized`.
pub fn save<C, D>(
    data: &DataCell,
    ui: &Weak<C>,
    body: CreateProject,
    picture: Option<Picked>,
    done: D,
) where
    C: ComponentHandle + 'static,
    D: FnOnce(&C, Result<Created, ClientError>) + Send + 'static,
{
    let Some(source) = data.borrow().clone() else {
        if let Some(ui) = ui.upgrade() {
            done(&ui, Err(ClientError::Unauthorized));
        }
        return;
    };
    source.fetch(
        ui,
        move |api, token| async move {
            let project = api.create_project(&token, &body).await?;
            let picture_error = match picture {
                Some(picked) => api
                    .upload_project_image(&token, &project.key, picked.bytes, picked.content_type)
                    .await
                    .err()
                    .map(|error| error_text(&error)),
                None => None,
            };
            Ok(Created {
                project,
                picture_error,
            })
        },
        done,
    );
}

/// Opens the native picker for the project picture (desktop only; the dialog hides the row elsewhere). Accepts PNG, JPEG and
/// WebP up to 5 MB and decodes a preview. `done` runs on the UI thread.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn pick_picture<C, D>(data: &DataCell, ui: &Weak<C>, done: D)
where
    C: ComponentHandle + 'static,
    D: FnOnce(&C, Result<PickOutcome, ClientError>) + Send + 'static,
{
    let Some(source) = data.borrow().clone() else {
        return;
    };
    source.fetch(
        ui,
        move |_api, _token| async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("Choose a project picture")
                .add_filter("Pictures", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
                .await
            else {
                return Ok(PickOutcome::Cancelled);
            };
            let file_name = file.file_name();
            let Some(content_type) = space_page_kit::person::content_type(&file_name) else {
                return Ok(PickOutcome::Rejected(
                    "Choose a PNG, JPEG or WebP picture.".to_owned(),
                ));
            };
            let bytes = file.read().await;
            if bytes.len() > space_proto::people::AVATAR_MAX_BYTES {
                return Ok(PickOutcome::Rejected(
                    "The picture is larger than 5 MB.".to_owned(),
                ));
            }
            let preview = match page_data::decode_picture(&bytes) {
                Ok(preview) => preview,
                Err(_) => {
                    return Ok(PickOutcome::Rejected(
                        "The picture could not be read.".to_owned(),
                    ));
                }
            };
            Ok(PickOutcome::Chosen(Box::new(Picked {
                file_name,
                content_type,
                bytes,
                preview,
            })))
        },
        done,
    );
}

/// No picture picker on this platform: the dialog hides the row, so nothing runs.
#[cfg(not(all(not(target_arch = "wasm32"), not(target_os = "android"))))]
pub fn pick_picture<C, D>(_data: &DataCell, _ui: &Weak<C>, _done: D)
where
    C: ComponentHandle + 'static,
    D: FnOnce(&C, Result<PickOutcome, ClientError>) + Send + 'static,
{
}

/// The words the dialog shows for a failed create.
pub fn error_text(error: &ClientError) -> String {
    page_data::error_text(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_key_name_and_description_make_the_body() {
        let body = validate(" RS2 ", " Rimelight Space ", " Internal ERP. ").unwrap();
        assert_eq!(body.key, "RS2");
        assert_eq!(body.name, "Rimelight Space");
        assert_eq!(body.description.as_deref(), Some("Internal ERP."));
        assert_eq!(body.lead_id, None);
    }

    #[test]
    fn a_blank_description_is_left_out() {
        let body = validate("AB", "Alpha", "   ").unwrap();
        assert_eq!(body.description, None);
    }

    #[test]
    fn the_key_length_bounds_are_two_to_ten() {
        assert!(validate("A", "Name", "").is_err());
        assert!(validate("AB", "Name", "").is_ok());
        assert!(validate("ABCDEFGHIJ", "Name", "").is_ok());
        assert!(validate("ABCDEFGHIJK", "Name", "").is_err());
    }

    #[test]
    fn the_key_starts_with_a_letter_and_holds_uppercase_or_digits() {
        assert!(validate("1AB", "Name", "").is_err());
        assert!(validate("ab", "Name", "").is_err());
        assert!(validate("A-B", "Name", "").is_err());
        assert!(validate("A B", "Name", "").is_err());
        assert!(validate("AB12", "Name", "").is_ok());
    }

    #[test]
    fn the_name_is_required_and_at_most_one_hundred_characters() {
        assert_eq!(validate("AB", "   ", "").unwrap_err(), "Enter a name.");
        assert!(validate("AB", &"x".repeat(NAME_MAX), "").is_ok());
        assert!(validate("AB", &"x".repeat(NAME_MAX + 1), "").is_err());
    }
}
