//! space_ui: the design system. Tokens, themes, Slint components and plain-Rust rules (PLAN 7.2, 8).
//!
//! Slint is the only UI stack. Feature code
//! imports this crate only: its Rust API and its `.slint` components (decision 0007, facade rule).
//!
//! - Design tokens come from the shared `rimelight_design` crate (web monorepo, packages/design). Rust reads its
//!   constants as `rimelight_design::tokens`; Slint reads its `Tokens` global. Space has no token generator.
//! - [`theme`]: [`theme::ThemeMode`], the user's choice of light, dark or system.
//! - [`icon`]: our icon names mapped to the bundled Lucide files.
//! - [`motion`]: motion durations, all zero when reduced motion is on.
//! - [`slint_ui`]: the Slint markup's Rust side (app shell, gallery and plain-Rust rules).

pub mod icon;
pub mod motion;
pub mod slint_ui;
pub mod theme;

pub use icon::{IconName, IconSize};
pub use theme::{Theme, ThemeMode};
