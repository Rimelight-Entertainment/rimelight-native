//! Company area (SP.3b to SP.3d): Employees, Profile (one person), Teams (list and org chart), Team (one team), Departments and
//! Locations. Each page follows the page data pattern (`space_page_kit::page_data`). Admin pages are in `space_area_admin`.
//!
//! Each page's plain logic is a module here (state, requests, the rows as plain values). The window side is a trait per page
//! (`EmployeesWindow`, `TeamsWindow`, ...), and [`wire_company!`] implements all of them for the generated window. The app
//! expands the macro in its own crate, where the Slint types are in scope (rimelight-rs `rimelight_ui_kit` pattern).
//!
//! The markup is in [`UI_DIR`]; the app compiles it through the `@space_area_company` library path.
//!
//! The pages' unit tests are here (plain logic). The window tests are in space_ui's `tests/ui/`.

pub use rimelight_ui_kit;
pub use space_page_kit;

pub mod departments;
pub mod employees;
pub mod locations;
pub mod org_tree;
pub mod profile;
pub mod team;
pub mod teams;

/// The company markup: `@space_area_company/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Expands, in the app crate, the company window glue: the trait implementations of every company page for the generated
/// window, and the `company_glue` module of table, load-state and picker helpers they use.
///
/// The arguments are the generated types, in this order: the window, `PageLoadState`, `TableColumn`, `TableRow`,
/// `SelectNav`, `SelectOption`, `EmployeesTable`, `EmployeeRow`, `TeamsTable`, `OrgRow`, `TeamRow`, `TeamView`, `DeptsTable`,
/// `DeptView`, `LocationsState`, `ProfileView`, `ProfileDraft` and `PersonLink`.
#[macro_export]
macro_rules! wire_company {
    (
        $AppWindow:ident, $PageLoadState:ident, $TableColumn:ident, $TableRow:ident, $SelectNav:ident,
        $SelectOption:ident, $EmployeesTable:ident, $EmployeeRow:ident, $TeamsTable:ident, $OrgRow:ident,
        $TeamRow:ident, $TeamView:ident, $DeptsTable:ident, $DeptView:ident, $LocationsState:ident,
        $ProfileView:ident, $ProfileDraft:ident, $PersonLink:ident $(,)?
    ) => {
        /// The table, load-state and picker helpers of the company glue (expanded in the app crate).
        pub mod company_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            use $crate::space_page_kit::__slint::{ModelRc, SharedString, VecModel};

            // The kit select rules on the generated `SelectOption` and `SelectNav` (`wire_nav` for the kind picker).
            $crate::rimelight_ui_kit::wire_select!($SelectOption, $SelectNav);

            $crate::space_page_kit::table_glue!($TableColumn, $TableRow);
            $crate::space_page_kit::load_glue!($PageLoadState);
            $crate::space_page_kit::picker_glue!($SelectOption);

            /// A plain person link as the generated `PersonLink`.
            pub fn link(p: $crate::profile::PersonLinkRow) -> $PersonLink {
                $PersonLink {
                    id: p.id.into(),
                    name: p.name.into(),
                    title: p.title.into(),
                }
            }

            /// A plain profile draft as the plain `ProfileDraft`.
            pub fn draft(d: $ProfileDraft) -> $crate::profile::ProfileDraft {
                $crate::profile::ProfileDraft {
                    preferred_name: d.preferred_name.to_string(),
                    pronouns: d.pronouns.to_string(),
                    bio: d.bio.to_string(),
                    work_phone: d.work_phone.to_string(),
                    timezone: d.timezone.to_string(),
                    location: d.location.to_string(),
                    skills: d.skills.to_string(),
                    personal_phone: d.personal_phone.to_string(),
                    personal_email: d.personal_email.to_string(),
                    address: d.address.to_string(),
                    birthday: d.birthday.to_string(),
                    emergency_contact: d.emergency_contact.to_string(),
                }
            }

            pub fn strings(values: Vec<String>) -> ModelRc<SharedString> {
                ModelRc::new(VecModel::from(
                    values
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                ))
            }

            pub fn org_row(n: $crate::teams::OrgNode) -> $OrgRow {
                $OrgRow {
                    id: n.id.into(),
                    name: n.name.into(),
                    title: n.title.into(),
                    initials: n.initials.into(),
                    avatar: n.avatar,
                    placeholder: n.placeholder,
                    person_id: n.person_id.into(),
                    depth: n.depth,
                    reports: n.reports,
                    expandable: n.expandable,
                    expanded: n.expanded,
                    highlighted: n.highlighted,
                }
            }

            pub fn team_row(t: $crate::departments::DeptTeam) -> $TeamRow {
                $TeamRow {
                    id: t.id.into(),
                    name: t.name.into(),
                    department: t.department.into(),
                    lead_id: t.lead_id.into(),
                    lead_name: t.lead_name.into(),
                    lead_initials: t.lead_initials.into(),
                    lead_avatar: t.lead_avatar,
                    members: t.members,
                }
            }

            pub fn member_row(m: $crate::team::MemberRow) -> $EmployeeRow {
                $EmployeeRow {
                    id: m.id.into(),
                    name: m.name.into(),
                    title: m.title.into(),
                    team: m.team.into(),
                    initials: m.initials.into(),
                    avatar: m.avatar,
                }
            }
        }

        impl $crate::employees::EmployeesWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_employees_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_employees_error(text.into());
            }
            fn show_loading_more(&self, on: bool) {
                self.set_employees_loading_more(on);
            }
            fn show_has_more(&self, more: bool) {
                self.set_employees_has_more(more);
            }
            fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>) {
                let table = company_glue::table(columns, rows);
                let state = $crate::space_page_kit::__slint::ComponentHandle::global::<
                    $EmployeesTable,
                >(self);
                state.set_columns(table.columns);
                state.set_rows(table.rows);
                state.set_ids(table.ids);
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                self.on_employees_opened(move || f());
            }
            fn on_search(&self, f: impl Fn(String) + 'static) {
                self.on_employees_search(move |text| f(text.to_string()));
            }
            fn on_load_more(&self, f: impl Fn() + 'static) {
                self.on_employees_load_more(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_employees_retry(move || f());
            }
        }

        impl $crate::teams::TeamsWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_teams_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_teams_error(text.into());
            }
            fn show_find_note(&self, text: &str) {
                self.set_teams_find_note(text.into());
            }
            fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>) {
                let table = company_glue::table(columns, rows);
                let state =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$TeamsTable>(self);
                state.set_columns(table.columns);
                state.set_rows(table.rows);
                state.set_ids(table.ids);
            }
            fn show_org_rows(&self, rows: Vec<$crate::teams::OrgNode>) {
                let rows: Vec<$OrgRow> = rows.into_iter().map(company_glue::org_row).collect();
                self.set_teams_org_rows($crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                ));
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                self.on_teams_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_teams_retry(move || f());
            }
            fn on_toggle(&self, f: impl Fn(String) + 'static) {
                self.on_teams_toggle(move |id| f(id.to_string()));
            }
            fn on_find(&self, f: impl Fn(String) + 'static) {
                self.on_teams_find(move |text| f(text.to_string()));
            }
        }

        impl $crate::team::TeamWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_team_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_team_error(text.into());
            }
            fn show_header(&self, h: $crate::team::TeamHeader) {
                self.set_team_view($TeamView {
                    id: h.id.into(),
                    name: h.name.into(),
                    department: h.department.into(),
                    lead_id: h.lead_id.into(),
                    lead_name: h.lead_name.into(),
                    lead_initials: h.lead_initials.into(),
                    lead_avatar: h.lead_avatar,
                    members: h.members,
                });
            }
            fn show_members(&self, members: Vec<$crate::team::MemberRow>) {
                let members: Vec<$EmployeeRow> =
                    members.into_iter().map(company_glue::member_row).collect();
                self.set_team_members($crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(members),
                ));
            }
            fn on_route_is_team(&self, f: impl Fn(&str) -> bool + 'static) {
                self.on_is_team_route(move |route| f(route.as_str()));
            }
            fn on_opened(&self, f: impl Fn(String) + 'static) {
                self.on_team_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_team_retry(move || f());
            }
        }

        impl $crate::departments::DeptWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_dept_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_dept_error(text.into());
            }
            fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>) {
                let table = company_glue::table(columns, rows);
                let state =
                    $crate::space_page_kit::__slint::ComponentHandle::global::<$DeptsTable>(self);
                state.set_columns(table.columns);
                state.set_rows(table.rows);
                state.set_ids(table.ids);
            }
            fn show_detail(&self, d: $crate::departments::DeptDetail) {
                let teams: Vec<$TeamRow> =
                    d.teams.into_iter().map(company_glue::team_row).collect();
                self.set_dept_view($DeptView {
                    id: d.id.into(),
                    name: d.name.into(),
                    head_id: d.head_id.into(),
                    head_name: d.head_name.into(),
                    head_initials: d.head_initials.into(),
                    head_avatar: d.head_avatar,
                    members: d.members,
                    teams: $crate::space_page_kit::__slint::ModelRc::new(
                        $crate::space_page_kit::__slint::VecModel::from(teams),
                    ),
                    expandable: d.expandable,
                    expanded: d.expanded,
                });
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                self.on_dept_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_dept_retry(move || f());
            }
            fn on_toggle(&self, f: impl Fn(String) + 'static) {
                self.on_dept_toggle(move |id| f(id.to_string()));
            }
            fn on_select(&self, f: impl Fn(String) + 'static) {
                self.on_dept_select(move |id| f(id.to_string()));
            }
        }

        impl $crate::locations::LocationsWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_error(text.into());
            }
            fn show_can_manage(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_can_manage(on);
            }
            fn show_selected(&self, id: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_selected(id.into());
            }
            fn show_table(&self, columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>) {
                let table = company_glue::table(columns, rows);
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_table_columns(table.columns);
                g.set_table_rows(table.rows);
                g.set_ids(table.ids);
            }
            fn show_kind_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_kind_options(company_glue::options(options));
            }
            fn wire_kind_picker(&self) {
                // The kind picker's search callbacks: the kit select rules, expanded in `company_glue` above.
                company_glue::wire_nav(
                    &$crate::space_page_kit::__slint::ComponentHandle::global::<$SelectNav>(self),
                );
            }
            fn show_form(&self, fields: &$crate::locations::Fields, title: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_form_title(title.into());
                g.set_form_error("".into());
                g.set_confirming(false);
                g.set_name(fields.name.as_str().into());
                g.set_kind(fields.kind.as_str().into());
                g.set_address(fields.address.as_str().into());
                g.set_city(fields.city.as_str().into());
                g.set_country(fields.country.as_str().into());
                g.set_timezone(fields.timezone.as_str().into());
                g.set_note(fields.note.as_str().into());
            }
            fn show_form_open(&self, open: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_form_open(open);
            }
            fn show_form_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_form_error(text.into());
            }
            fn show_confirming(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.set_confirming(on);
            }
            fn read_form(&self) -> $crate::locations::Fields {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                $crate::locations::Fields {
                    name: g.get_name().to_string(),
                    kind: g.get_kind().to_string(),
                    address: g.get_address().to_string(),
                    city: g.get_city().to_string(),
                    country: g.get_country().to_string(),
                    timezone: g.get_timezone().to_string(),
                    note: g.get_note().to_string(),
                }
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_retry(move || f());
            }
            fn on_pick(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_pick(move |id| f(id.to_string()));
            }
            fn on_new_item(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_new_item(move || f());
            }
            fn on_save(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_save(move || f());
            }
            fn on_delete_item(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$LocationsState>(
                    self,
                );
                g.on_delete_item(move || f());
            }
        }

        impl $crate::profile::ProfileWindow for $AppWindow {
            fn show_photo_supported(&self, on: bool) {
                self.set_profile_photo_supported(on);
            }
            fn user_id(&self) -> String {
                self.get_user_id().to_string()
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_profile_state(company_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_profile_error(text.into());
            }
            fn show_view(&self, v: $crate::profile::ProfileData) {
                self.set_profile_view($ProfileView {
                    id: v.id.into(),
                    name: v.name.into(),
                    initials: v.initials.into(),
                    avatar: v.avatar,
                    preferred_name: v.preferred_name.into(),
                    pronouns: v.pronouns.into(),
                    title: v.title.into(),
                    team: v.team.into(),
                    department: v.department.into(),
                    email: v.email.into(),
                    work_phone: v.work_phone.into(),
                    location: v.location.into(),
                    timezone: v.timezone.into(),
                    timezone_label: v.timezone_label.into(),
                    start_date: v.start_date.into(),
                    manager_id: v.manager_id.into(),
                    manager_name: v.manager_name.into(),
                    bio: v.bio.into(),
                    skills: company_glue::strings(v.skills),
                    skills_text: v.skills_text.into(),
                    has_personal: v.has_personal,
                    personal_phone: v.personal_phone.into(),
                    personal_email: v.personal_email.into(),
                    address: v.address.into(),
                    birthday: v.birthday.into(),
                    emergency_contact: v.emergency_contact.into(),
                    chain: $crate::space_page_kit::__slint::ModelRc::new(
                        $crate::space_page_kit::__slint::VecModel::from(
                            v.chain
                                .into_iter()
                                .map(company_glue::link)
                                .collect::<Vec<_>>(),
                        ),
                    ),
                    reports: $crate::space_page_kit::__slint::ModelRc::new(
                        $crate::space_page_kit::__slint::VecModel::from(
                            v.reports
                                .into_iter()
                                .map(company_glue::link)
                                .collect::<Vec<_>>(),
                        ),
                    ),
                });
            }
            fn show_own(&self, on: bool) {
                self.set_profile_own(on);
            }
            fn show_editing(&self, on: bool) {
                self.set_profile_editing(on);
            }
            fn show_saving(&self, on: bool) {
                self.set_profile_saving(on);
            }
            fn show_save_error(&self, text: &str) {
                self.set_profile_save_error(text.into());
            }
            fn show_photo_busy(&self, on: bool) {
                self.set_profile_photo_busy(on);
            }
            fn show_photo_error(&self, text: &str) {
                self.set_profile_photo_error(text.into());
            }
            fn on_route_is_profile(&self, f: impl Fn(&str) -> bool + 'static) {
                self.on_is_profile_route(move |route| f(route.as_str()));
            }
            fn on_opened(&self, f: impl Fn(String) + 'static) {
                self.on_profile_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_profile_retry(move || f());
            }
            fn on_start_edit(&self, f: impl Fn() + 'static) {
                self.on_profile_start_edit(move || f());
            }
            fn on_cancel_edit(&self, f: impl Fn() + 'static) {
                self.on_profile_cancel_edit(move || f());
            }
            fn on_save(&self, f: impl Fn($crate::profile::ProfileDraft) + 'static) {
                self.on_profile_save(move |draft| f(company_glue::draft(draft)));
            }
            fn on_change_photo(&self, f: impl Fn() + 'static) {
                self.on_profile_change_photo(move || f());
            }
        }
    };
}
