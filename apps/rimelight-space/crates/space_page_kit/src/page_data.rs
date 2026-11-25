//! The page data pattern (SP.3b). A page that shows data from the API follows these steps, so later pages copy them.
//!
//! 1. The page's markup has a load state (`PageLoadState`: loading, loaded, empty, error), an error line, and callbacks
//!    for `opened` (sent from the page's `init`, so the first load runs when its route becomes active), `retry` and any
//!    actions. `components/page-state.slint` (`PageState`) draws the spinner, the empty message and the error with Try again.
//! 2. The window (`AppShell`) holds those as `<page>-*` properties and callbacks, and the page's markup binds to them.
//! 3. The page's plain-Rust side (its area crate) keeps its request state (generation, query, cursor) behind a `Mutex`,
//!    so a worker can read and write it. A request bumps the generation, and a result for an older generation is dropped.
//!    The window glue (`wire_<area>!`) wires the callbacks.
//! 4. A load calls [`DataSource::fetch`]. The session's `ApiClient` and the stored token run on a worker (`task::spawn_task`),
//!    and the result comes back to the UI thread through [`ui_thread`].
//! 5. Tests inject results straight into the page's Rust handle, with no network: a test window has no [`DataSource`].
//!
//! The session driver installs the source. Until then a load does nothing, and the page stays in the loading state.
//! Pictures (avatars) are fetched the same way and cached by their path in [`Pictures`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer, Weak};
use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_client::session_store::SessionStore;

use crate::task::spawn_task;

/// The session's API client and store, for page loads. Set by the session driver; a window with no session has none.
pub struct DataSource {
    api: Arc<ApiClient>,
    store: Arc<dyn SessionStore>,
}

/// The window's slot for the [`DataSource`]. Read on the UI thread only (the pages hold it as an `Rc`).
pub type DataCell = Rc<RefCell<Option<Arc<DataSource>>>>;

impl DataSource {
    pub fn new(api: Arc<ApiClient>, store: Arc<dyn SessionStore>) -> Self {
        Self { api, store }
    }

    /// The session's API client, for the canvas live session, which keeps its own connection.
    pub fn api(&self) -> Arc<ApiClient> {
        self.api.clone()
    }

    /// The stored session token, for the canvas live session, which keeps its own connection. `None` when there is no session
    /// or the store cannot be read.
    pub fn token(&self) -> Option<String> {
        self.store
            .load()
            .ok()
            .flatten()
            .map(|stored| stored.session_token)
    }

    /// Runs `work` with the API and the stored session token on a worker, then `done` on the UI thread. A missing session is
    /// reported as `Unauthorized` without a request.
    pub fn fetch<C, T, F, Fut, D>(self: &Arc<Self>, ui: &Weak<C>, work: F, done: D)
    where
        C: ComponentHandle + 'static,
        F: FnOnce(Arc<ApiClient>, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, ClientError>>,
        T: Send + 'static,
        D: FnOnce(&C, Result<T, ClientError>) + Send + 'static,
    {
        let api = self.api.clone();
        let store = self.store.clone();
        let ui = ui.clone();
        spawn_task(move || async move {
            let result = match store.load() {
                Ok(Some(stored)) => work(api, stored.session_token).await,
                Ok(None) => Err(ClientError::Unauthorized),
                Err(error) => Err(error),
            };
            ui_thread(&ui, move |ui| done(ui, result));
        });
    }
}

/// Runs `f` on the UI thread with the window, if it is still open. Native: posted to the event loop. Web: already on the page's
/// only thread, so it runs now.
#[cfg(not(target_arch = "wasm32"))]
pub fn ui_thread<C, F>(ui: &Weak<C>, f: F)
where
    C: ComponentHandle + 'static,
    F: FnOnce(&C) + Send + 'static,
{
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            f(&ui);
        }
    });
}

/// Runs `f` on the UI thread with the window, if it is still open. Web: already on the page's only thread, so it runs now.
#[cfg(target_arch = "wasm32")]
pub fn ui_thread<C, F>(ui: &Weak<C>, f: F)
where
    C: ComponentHandle + 'static,
    F: FnOnce(&C) + Send + 'static,
{
    if let Some(ui) = ui.upgrade() {
        f(&ui);
    }
}

/// The text a failed load shows. The server's own message is kept when it has one.
pub fn error_text(error: &ClientError) -> String {
    match error {
        ClientError::Unauthorized => "Your session has ended. Sign in again.".to_string(),
        ClientError::Network(_) => {
            "Rimelight Space could not be reached. Check your connection and try again.".to_string()
        }
        ClientError::Api { message, .. } if !message.is_empty() => message.clone(),
        other => other.to_string(),
    }
}

/// Decoded pictures (avatars) by their path, and the paths on their way. Kept by each page that shows pictures.
#[derive(Default)]
pub struct Pictures {
    pub ready: HashMap<String, SharedPixelBuffer<Rgba8Pixel>>,
    pub pending: HashSet<String>,
}

/// Decodes a PNG, JPEG or WebP picture into pixels. Runs on the worker, so the UI thread only wraps the result.
pub fn decode_picture(bytes: &[u8]) -> Result<SharedPixelBuffer<Rgba8Pixel>, ClientError> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|e| ClientError::Protocol(format!("picture: {e}")))?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    buffer.make_mut_bytes().copy_from_slice(rgba.as_raw());
    Ok(buffer)
}

/// A Slint image from decoded pixels. Call it on the UI thread.
pub fn picture(buffer: &SharedPixelBuffer<Rgba8Pixel>) -> slint::Image {
    slint::Image::from_rgba8(buffer.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_to_do_and_keep_the_server_message() {
        assert_eq!(
            error_text(&ClientError::Unauthorized),
            "Your session has ended. Sign in again."
        );
        assert!(
            error_text(&ClientError::Network("refused".into())).contains("could not be reached")
        );
        assert_eq!(
            error_text(&ClientError::Api {
                status: 422,
                error: "invalid_field".into(),
                message: "q: too long".into(),
            }),
            "q: too long"
        );
    }

    #[test]
    fn a_png_decodes_to_its_pixels_and_garbage_is_refused() {
        let mut png = Vec::new();
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([10, 20, 30, 255]));
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let buffer = decode_picture(&png).expect("decodes");
        assert_eq!((buffer.width(), buffer.height()), (2, 3));
        assert!(matches!(
            decode_picture(b"not an image"),
            Err(ClientError::Protocol(_))
        ));
    }
}
