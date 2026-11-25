//! Browser side of the Access session (wasm only). The browser holds the `CF_Authorization` cookie; this module only
//! navigates the page and checks that the cookie gives access.

use crate::api::DEFAULT_API_BASE;

/// The page's address without its query or fragment (origin and path), for the Access login's return.
pub fn page_address() -> Option<String> {
    let location = web_sys::window()?.location();
    let origin = location.origin().ok()?;
    let path = location.pathname().ok()?;
    Some(format!("{origin}{path}"))
}

/// Sends the page to `url` (the Access login or logout). The page leaves, so nothing runs after this.
pub fn navigate(url: &str) -> Result<(), String> {
    let location = web_sys::window().ok_or("no window")?.location();
    location.set_href(url).map_err(|e| format!("{e:?}"))
}

/// Asks the API whether the cookie session is accepted. `on_done(true)` when the answer is a JSON body with a 2xx status.
/// A signed-out browser is redirected to the Access login page, which answers HTML, so that counts as signed out.
///
/// TODO(confirm): `/todos` as the probe endpoint. Any read endpoint that needs the owner works the same way.
pub fn probe_cookie_session(on_done: impl FnOnce(bool) + Send + 'static) {
    let url = format!("{DEFAULT_API_BASE}/todos");
    ehttp::fetch(ehttp::Request::get(url), move |result| {
        let signed_in = result.is_ok_and(|response| {
            response.ok && matches!(response.bytes.first(), Some(b'[') | Some(b'{'))
        });
        on_done(signed_in);
    });
}
