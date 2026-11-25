//! Runs a client task off the UI thread (SP.3b; the session driver's `spawn_task`, moved here for the area crates).

use std::future::Future;

/// Runs a client task off the UI thread. Native: a worker thread with its own runtime. Web: a local task on the page.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task<F, Fut>(task: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()>,
{
    std::thread::spawn(move || {
        // A failure to start the runtime leaves the window as it is. The client calls below do not fail that way.
        let _ = space_client::block_on(task());
    });
}

/// Runs a client task off the UI thread. Native: a worker thread with its own runtime. Web: a local task on the page.
#[cfg(target_arch = "wasm32")]
pub fn spawn_task<F, Fut>(task: F)
where
    F: FnOnce() -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(task());
}
