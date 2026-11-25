//! Locking a page's request state.

use std::sync::{Mutex, MutexGuard};

/// Locks a page's request state. A panic in another holder does not stop the page.
pub fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
