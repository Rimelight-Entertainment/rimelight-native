//! The load state of a page, as plain Rust. The window glue (`load_glue!`) maps it to the generated `PageLoadState`.

/// Where a page's data stands: loading, loaded, empty (loaded with nothing to show) or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Loaded,
    Empty,
    Error,
}

/// Expands, in the app crate, `load_state`, which maps a [`LoadState`](crate::load::LoadState) to the generated
/// `PageLoadState` of the window.
#[macro_export]
macro_rules! load_glue {
    ($PageLoadState:ident) => {
        /// The generated load state the window shows for a plain one.
        pub fn load_state(state: $crate::load::LoadState) -> $PageLoadState {
            match state {
                $crate::load::LoadState::Loading => $PageLoadState::Loading,
                $crate::load::LoadState::Loaded => $PageLoadState::Loaded,
                $crate::load::LoadState::Empty => $PageLoadState::Empty,
                $crate::load::LoadState::Error => $PageLoadState::Error,
            }
        }
    };
}
