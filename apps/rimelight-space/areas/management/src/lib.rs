//! Management area: Project > Settings (`/p/:key/settings`) and Project > Members (`/p/:key/members`). Each page's plain logic is
//! a module here; the window side is a trait per page ([`settings::SettingsWindow`], [`members::MembersWindow`]), implemented for
//! the generated window by [`wire_management!`]. The markup is in [`UI_DIR`], compiled through `@space_area_management`.

pub use space_page_kit;

pub mod members;
pub mod settings;

/// The management markup: `@space_area_management/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Expands, in the app crate, the management window glue: the trait implementations of both pages for the generated window,
/// and the `management_glue` helpers (load state, select options).
///
/// The arguments are the generated types, in this order: the window, `PageLoadState`, `ProjectSettingsState`,
/// `ProjectMembersState`, `MemberRow` and `SelectOption`.
#[macro_export]
macro_rules! wire_management {
    (
        $AppShell:ident, $PageLoadState:ident, $ProjectSettingsState:ident, $ProjectMembersState:ident,
        $MemberRow:ident, $SelectOption:ident $(,)?
    ) => {
        /// The load-state and select-option helpers of the management glue.
        pub mod management_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            $crate::space_page_kit::load_glue!($PageLoadState);
            $crate::space_page_kit::picker_glue!($SelectOption);
        }

        impl $crate::settings::SettingsWindow for $AppShell {
            fn route(&self) -> String {
                self.get_page_route().to_string()
            }
            fn open_issues(&self, route: String) {
                self.invoke_issues_open(route.into());
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.set_state(management_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.set_error(text.into());
            }
            fn show_action_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.set_action_error(text.into());
            }
            fn show_saving(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.set_saving(on);
            }
            fn saving(&self) -> bool {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.get_saving()
            }
            fn can_edit(&self) -> bool {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.get_can_edit()
            }
            fn form(&self) -> $crate::settings::Form {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                $crate::settings::Form {
                    name: g.get_name().to_string(),
                    key: g.get_key().to_string(),
                    description: g.get_description().to_string(),
                }
            }
            fn show_project(&self, v: $crate::settings::SettingsView) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.set_name(v.name.as_str().into());
                g.set_key(v.key.as_str().into());
                g.set_description(v.description.as_str().into());
                g.set_can_edit(v.can_edit);
                g.set_initials(v.initials.as_str().into());
                g.set_has_image(v.has_image);
                g.set_image(v.image);
            }
            fn retry_self(&self) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.invoke_retry();
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.on_retry(move || f());
            }
            fn on_save(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.on_save(move || f());
            }
            fn on_pick_icon(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectSettingsState,
                >(self);
                g.on_pick_icon(move || f());
            }
        }

        impl $crate::members::MembersWindow for $AppShell {
            fn route(&self) -> String {
                self.get_page_route().to_string()
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_state(management_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_error(text.into());
            }
            fn show_action_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_action_error(text.into());
            }
            fn show_count(&self, count: i32) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_count(count);
            }
            fn show_members(&self, members: Vec<$crate::members::MemberView>) {
                let rows: Vec<$MemberRow> = members
                    .into_iter()
                    .map(|m| $MemberRow {
                        id: m.id.into(),
                        name: m.name.into(),
                        initials: m.initials.into(),
                        role: m.role.into(),
                        role_label: m.role_label.into(),
                        is_me: m.is_me,
                    })
                    .collect();
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_members($crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                ));
            }
            fn show_person_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_person_options(management_glue::options(options));
            }
            fn show_role_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_role_options(management_glue::options(options));
            }
            fn show_can_edit(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_can_edit(on);
            }
            fn pick_role(&self) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.get_pick_role().to_string()
            }
            fn show_pick_role(&self, role: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_pick_role(role.into());
            }
            fn pick_person(&self) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.get_pick_person().to_string()
            }
            fn show_pick_person(&self, person: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_pick_person(person.into());
            }
            fn show_add_open(&self, open: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.set_add_open(open);
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_retry(move || f());
            }
            fn on_open_add(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_open_add(move || f());
            }
            fn on_close_add(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_close_add(move || f());
            }
            fn on_add(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_add(move || f());
            }
            fn on_set_role(&self, f: impl Fn(String, String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_set_role(move |id, role| f(id.to_string(), role.to_string()));
            }
            fn on_remove(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $ProjectMembersState,
                >(self);
                g.on_remove(move |id| f(id.to_string()));
            }
        }
    };
}
