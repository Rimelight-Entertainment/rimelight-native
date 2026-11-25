//! Home area (S2): Home > Dashboard (`/home`). The cards that have a backend today: Assigned to me and Recent canvases. The
//! calendar, unread, timer and approvals cards come with their own slices.
//!
//! The page's plain logic is [`dashboard`] (the card states, the row mapping, the requests). The window side is the
//! [`dashboard::HomeWindow`] trait, which [`wire_home!`] implements for the generated window. The app expands the macro in its
//! own crate, where the Slint types are in scope (the same pattern as `wire_admin!`).
//!
//! The markup is in [`UI_DIR`]; the app compiles it through the `@space_area_home` library path. The page's unit tests are in
//! `dashboard.rs` (plain logic). The window tests are in space_ui's `tests/ui/home.rs`.

pub use space_page_kit;

pub mod dashboard;

/// The home markup: `@space_area_home/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Expands, in the app crate, the home window glue: the [`dashboard::HomeWindow`] implementation for the generated window, and
/// the `home_glue` module of load-state helpers it uses.
///
/// The arguments are the generated types, in this order: the window, `PageLoadState`, `HomeState`, `HomeIssueRow` and
/// `HomeCanvasRow`.
#[macro_export]
macro_rules! wire_home {
    (
        $AppShell:ident, $PageLoadState:ident, $HomeState:ident, $HomeIssueRow:ident, $HomeCanvasRow:ident $(,)?
    ) => {
        /// The load-state helper of the home glue (expanded in the app crate).
        pub mod home_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            $crate::space_page_kit::load_glue!($PageLoadState);
        }

        impl $crate::dashboard::HomeWindow for $AppShell {
            fn show_assigned(
                &self,
                state: $crate::space_page_kit::load::LoadState,
                error: &str,
                count: &str,
                rows: &[$crate::dashboard::IssueLine],
            ) {
                let g =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$HomeState>(self);
                let rows: Vec<$HomeIssueRow> = rows
                    .iter()
                    .map(|r| $HomeIssueRow {
                        route: r.route.as_str().into(),
                        key: r.key.as_str().into(),
                        summary: r.summary.as_str().into(),
                        state: r.state.as_str().into(),
                        priority: r.priority.as_str().into(),
                        project: r.project.as_str().into(),
                    })
                    .collect();
                g.set_assigned_state(home_glue::load_state(state));
                g.set_assigned_error(error.into());
                g.set_assigned_count(count.into());
                g.set_assigned_rows($crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                ));
            }
            fn show_canvases(
                &self,
                state: $crate::space_page_kit::load::LoadState,
                error: &str,
                rows: &[$crate::dashboard::CanvasLine],
            ) {
                let g =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$HomeState>(self);
                let rows: Vec<$HomeCanvasRow> = rows
                    .iter()
                    .map(|r| $HomeCanvasRow {
                        route: r.route.as_str().into(),
                        title: r.title.as_str().into(),
                        updated: r.updated.as_str().into(),
                    })
                    .collect();
                g.set_canvases_state(home_glue::load_state(state));
                g.set_canvases_error(error.into());
                g.set_canvases_rows($crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                ));
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$HomeState>(self);
                g.on_opened(move || f());
            }
            fn on_retry_assigned(&self, f: impl Fn() + 'static) {
                let g =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$HomeState>(self);
                g.on_retry_assigned(move || f());
            }
            fn on_retry_canvases(&self, f: impl Fn() + 'static) {
                let g =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$HomeState>(self);
                g.on_retry_canvases(move || f());
            }
        }
    };
}
