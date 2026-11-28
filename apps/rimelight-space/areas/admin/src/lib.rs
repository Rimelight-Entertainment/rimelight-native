//! Admin area (SP.3e): Admin > Users (`/admin/users`), Organisation (`/admin/organisation`), Roles (`/admin/roles`) and Types and
//! tags (`/admin/types-tags`). Each page's plain logic is a module here, and the window side is a trait per page
//! ([`users::UsersWindow`], [`organisation::OrganisationWindow`], [`roles::RolesWindow`], [`types_tags::TypesTagsWindow`]),
//! implemented for the generated window by [`wire_admin!`]. The markup is in [`UI_DIR`], compiled through `@space_area_admin`.
//!
//! The pages follow the page data pattern (`space_page_kit::page_data`). A caller without the admin role sees "You need the
//! admin role", and the API enforces the same rule on every route. The check reads the caller's roles from `GET /v1/me/profile`.
//!
//! After a change made here, the Company pages that already hold a result ask again ([`AdminCompany::refresh_company`]), so
//! their data is current the next time they are shown.

pub use space_page_kit;
pub use space_proto;

pub mod organisation;
pub mod roles;
pub mod types_tags;
pub mod users;

use space_client::api::ApiClient;
use space_client::error::ClientError;
use space_proto::people::{Department, OrgTreeEntry, Position, Seat, Team, TeamBudget};

/// The admin markup: `@space_area_admin/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// The route of Admin > Users.
pub const USERS_ROUTE: &str = "/admin/users";
/// The route of Admin > Organisation.
pub const ORGANISATION_ROUTE: &str = "/admin/organisation";
/// The route of Admin > Roles.
pub const ROLES_ROUTE: &str = "/admin/roles";

/// The lists the admin pages choose from: departments, teams, positions, and every active person (for heads, leads, managers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub departments: Vec<Department>,
    pub teams: Vec<Team>,
    pub positions: Vec<Position>,
    pub people: Vec<OrgTreeEntry>,
    /// Every seat with its salary range (admin and HR).
    pub seats: Vec<Seat>,
    /// Each team's budget (admin and HR).
    pub budgets: Vec<TeamBudget>,
}

/// Loads the reference lists for the admin pages. Runs on a worker.
pub async fn fetch_reference(api: &ApiClient, token: &str) -> Result<Reference, ClientError> {
    Ok(Reference {
        departments: api.departments(token).await?,
        teams: api.teams(token).await?,
        positions: api.positions(token).await?,
        people: api.org_tree(token).await?,
        seats: api.admin_seats(token).await?,
        budgets: api.team_budgets(token).await?,
    })
}

/// The Company pages that already hold a result ask again, so the next time they are shown their data is current (SP.3e).
/// A page still loading is left alone: its result is already on the way. The window implements it in [`wire_admin!`].
pub trait AdminCompany {
    fn refresh_company(&self);
}

/// Expands, in the app crate, the admin window glue: the trait implementations of the four pages and of [`AdminCompany`] for
/// the generated window, and the `admin_glue` helpers (load state, select options, table rows and columns, models).
///
/// The arguments are the generated types, in this order: the window, `PageLoadState`, `SelectOption`, `TableColumn`,
/// `TableRow`, `CalendarDay`, `AdminRow`, `AdminOrgState`, `AdminUserRow`, `AdminUsersState`, `AdminRolesState`, `RolePerson`,
/// `AdminTypesTagsState`, `TypeRow` and `AreaRow`.
///
/// The Company properties of [`AdminCompany`] (`employees-state`, `profile-state`, `teams-state`, `team-state`, `dept-state`
/// and their `*-retry` callbacks) are the window's own, so the expansion must sit where the window is generated.
#[macro_export]
macro_rules! wire_admin {
    (
        $AppWindow:ident, $PageLoadState:ident, $SelectOption:ident, $TableColumn:ident, $TableRow:ident, $CalendarDay:ident,
        $AdminRow:ident, $AdminOrgState:ident, $AdminUserRow:ident, $AdminUsersState:ident, $AdminRolesState:ident,
        $RolePerson:ident, $AdminTypesTagsState:ident, $TypeRow:ident, $AreaRow:ident $(,)?
    ) => {
        /// The generated-type helpers of the admin glue.
        pub mod admin_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            $crate::space_page_kit::load_glue!($PageLoadState);
            $crate::space_page_kit::picker_glue!($SelectOption);
            $crate::space_page_kit::table_glue!($TableColumn, $TableRow);

            /// A plain list as the window's model.
            pub fn model<T: Clone + 'static>(
                rows: Vec<T>,
            ) -> $crate::space_page_kit::__slint::ModelRc<T> {
                $crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                )
            }

            /// The 42 calendar cells of a picker as the window's model.
            pub fn days(
                cells: Vec<$crate::space_page_kit::dates::CalendarCell>,
            ) -> $crate::space_page_kit::__slint::ModelRc<$CalendarDay> {
                model(
                    cells
                        .into_iter()
                        .map(|cell| $CalendarDay {
                            day: cell.day,
                            in_month: cell.in_month,
                            today: cell.today,
                            selected: cell.selected,
                            in_range: cell.in_range,
                        })
                        .collect(),
                )
            }

            /// The members of a role as the window's rows.
            pub fn role_persons(
                members: Vec<$crate::roles::PersonView>,
            ) -> Vec<$RolePerson> {
                members
                    .into_iter()
                    .map(|m| $RolePerson {
                        id: m.id.into(),
                        name: m.name.into(),
                        title: m.title.into(),
                        initials: m.initials.into(),
                    })
                    .collect()
            }

            /// The areas as the window's rows (the colour is read from its `#RRGGBB` text).
            pub fn area_rows(rows: Vec<$crate::types_tags::AreaView>) -> Vec<$AreaRow> {
                rows.into_iter()
                    .map(|a| $AreaRow {
                        id: a.id.into(),
                        name: a.name.into(),
                        shade: $crate::types_tags::colour_of(&a.shade),
                    })
                    .collect()
            }
        }

        impl $crate::AdminCompany for $AppWindow {
            fn refresh_company(&self) {
                if self.get_employees_state() != $PageLoadState::Loading {
                    self.invoke_employees_retry();
                }
                if self.get_profile_state() != $PageLoadState::Loading {
                    self.invoke_profile_retry();
                }
                if self.get_teams_state() != $PageLoadState::Loading {
                    self.invoke_teams_retry();
                }
                if self.get_team_state() != $PageLoadState::Loading {
                    self.invoke_team_retry();
                }
                if self.get_dept_state() != $PageLoadState::Loading {
                    self.invoke_dept_retry();
                }
            }
        }

        impl $crate::roles::RolesWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_state(admin_glue::load_state(state));
            }
            fn show_denied(&self, denied: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_denied(denied);
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_error(text.into());
            }
            fn show_action_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_action_error(text.into());
            }
            fn show_admins(&self, members: Vec<$crate::roles::PersonView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_admins(admin_glue::model(admin_glue::role_persons(members)));
            }
            fn show_hr(&self, members: Vec<$crate::roles::PersonView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_hr(admin_glue::model(admin_glue::role_persons(members)));
            }
            fn show_person_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.set_person_options(admin_glue::options(options));
            }
            fn picked(&self, role: &str) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                if role == $crate::space_proto::people::ROLE_HR {
                    g.get_pick_hr().to_string()
                } else {
                    g.get_pick_admin().to_string()
                }
            }
            fn show_picked(&self, role: &str, person: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                if role == $crate::space_proto::people::ROLE_HR {
                    g.set_pick_hr(person.into());
                } else {
                    g.set_pick_admin(person.into());
                }
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.on_retry(move || f());
            }
            fn on_add(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.on_add(move |role| f(role.to_string()));
            }
            fn on_remove(&self, f: impl Fn(String, String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminRolesState>(self);
                g.on_remove(move |role, id| f(role.to_string(), id.to_string()));
            }
        }

        impl $crate::users::UsersWindow for $AppWindow {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_state(admin_glue::load_state(state));
            }
            fn show_denied(&self, denied: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_denied(denied);
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_error(text.into());
            }
            fn show_loading_more(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_loading_more(on);
            }
            fn show_has_more(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_has_more(on);
            }
            fn show_weekdays(&self, labels: Vec<String>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_weekday_labels(admin_glue::model(
                    labels
                        .into_iter()
                        .map($crate::space_page_kit::__slint::SharedString::from)
                        .collect(),
                ));
            }
            fn show_calendar(
                &self,
                field: $crate::users::Field,
                cells: Vec<$crate::space_page_kit::dates::CalendarCell>,
                title: String,
            ) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                match field {
                    $crate::users::Field::Add => {
                        g.set_add_days(admin_glue::days(cells));
                        g.set_add_month_title(title.into());
                    }
                    $crate::users::Field::Detail => {
                        g.set_d_days(admin_glue::days(cells));
                        g.set_d_month_title(title.into());
                    }
                }
            }
            fn show_rows(&self, rows: Vec<$crate::users::UserRowView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_rows(admin_glue::model(
                    rows.into_iter()
                        .map(|r| $AdminUserRow {
                            id: r.id.into(),
                            name: r.name.into(),
                            email: r.email.into(),
                            title: r.title.into(),
                            department: r.department.into(),
                            team: r.team.into(),
                            status: r.status.into(),
                            active: r.active,
                            roles: r.roles.into(),
                            initials: r.initials.into(),
                            selected: r.selected,
                        })
                        .collect(),
                ));
            }
            fn show_options(&self, options: $crate::users::Options) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_position_options(admin_glue::options(options.position));
                g.set_seat_options(admin_glue::options(options.seat));
                g.set_department_options(admin_glue::options(options.department));
                g.set_team_options(admin_glue::options(options.team));
                g.set_person_options(admin_glue::options(options.person));
            }
            fn show_detail(&self, d: $crate::users::DetailView) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_detail_open(true);
                g.set_detail_name(d.name.into());
                g.set_detail_email(d.email.into());
                g.set_detail_status(d.status.into());
                g.set_detail_roles(d.roles.into());
                g.set_detail_active(d.active);
                g.set_detail_admin(d.admin);
                g.set_detail_hr(d.hr);
                g.set_d_position(d.position.into());
                g.set_d_department(d.department.into());
                g.set_d_team(d.team.into());
                g.set_d_manager(d.manager.into());
                g.set_d_start(d.start.into());
                g.set_d_seat(d.seat.into());
            }
            fn show_detail_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_detail_error(text.into());
            }
            fn show_add_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_add_error(text.into());
            }
            fn show_add_open(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_add_open(on);
            }
            fn clear_add_form(&self) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.set_add_open(false);
                g.set_add_error("".into());
                g.set_add_email("".into());
                g.set_add_name("".into());
                g.set_add_position("".into());
                g.set_add_department("".into());
                g.set_add_team("".into());
                g.set_add_manager("".into());
                g.set_add_start("".into());
            }
            fn add_form(&self) -> $crate::users::AddForm {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                $crate::users::AddForm {
                    email: g.get_add_email().to_string(),
                    name: g.get_add_name().to_string(),
                    start: g.get_add_start().to_string(),
                    position: g.get_add_position().to_string(),
                    department: g.get_add_department().to_string(),
                    team: g.get_add_team().to_string(),
                    manager: g.get_add_manager().to_string(),
                }
            }
            fn detail_form(&self) -> $crate::users::DetailForm {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                $crate::users::DetailForm {
                    start: g.get_d_start().to_string(),
                    position: g.get_d_position().to_string(),
                    department: g.get_d_department().to_string(),
                    team: g.get_d_team().to_string(),
                    manager: g.get_d_manager().to_string(),
                    seat: g.get_d_seat().to_string(),
                }
            }
            fn start_text(&self, field: $crate::users::Field) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                match field {
                    $crate::users::Field::Add => g.get_add_start().to_string(),
                    $crate::users::Field::Detail => g.get_d_start().to_string(),
                }
            }
            fn show_start(&self, field: $crate::users::Field, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                match field {
                    $crate::users::Field::Add => g.set_add_start(text.into()),
                    $crate::users::Field::Detail => g.set_d_start(text.into()),
                }
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_retry(move || f());
            }
            fn on_search(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_search(move |text| f(text.to_string()));
            }
            fn on_load_more(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_load_more(move || f());
            }
            fn on_select(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_select(move |id| f(id.to_string()));
            }
            fn on_add_submit(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_add_submit(move || f());
            }
            fn on_save(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_save(move || f());
            }
            fn on_set_active(&self, f: impl Fn(bool) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_set_active(move |active| f(active));
            }
            fn on_set_role(&self, f: impl Fn(String, bool) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_set_role(move |role, grant| f(role.to_string(), grant));
            }
            fn on_add_pick(&self, f: impl Fn(i32) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_add_pick(move |index| f(index));
            }
            fn on_add_month_step(&self, f: impl Fn(i32) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_add_month_step(move |step| f(step));
            }
            fn on_d_pick(&self, f: impl Fn(i32) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_d_pick(move |index| f(index));
            }
            fn on_d_month_step(&self, f: impl Fn(i32) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminUsersState>(self);
                g.on_d_month_step(move |step| f(step));
            }
        }

        impl $crate::organisation::OrganisationWindow for $AppWindow {
            fn tab(&self) -> i32 {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.get_tab()
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_state(admin_glue::load_state(state));
            }
            fn show_denied(&self, denied: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_denied(denied);
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_error(text.into());
            }
            fn show_rows(&self, rows: Vec<$crate::organisation::RowView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_rows(admin_glue::model(
                    rows.into_iter()
                        .map(|r| $AdminRow {
                            id: r.id.into(),
                            title: r.title.into(),
                            detail: r.detail.into(),
                            selected: r.selected,
                        })
                        .collect(),
                ));
            }
            fn show_table(&self, table: $crate::organisation::TableView) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                let table = admin_glue::table(&table.columns, table.rows);
                g.set_table_columns(table.columns);
                g.set_table_rows(table.rows);
            }
            fn show_options(&self, options: $crate::organisation::OrgOptions) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_parent_options(admin_glue::options(options.parent));
                g.set_person_options(admin_glue::options(options.person));
                g.set_level_options(admin_glue::options(options.level));
                g.set_team_options(admin_glue::options(options.team));
                g.set_position_options(admin_glue::options(options.position));
                g.set_seat_options(admin_glue::options(options.seat));
                g.set_state_options(admin_glue::options(options.state));
            }
            fn show_level_choice(&self, value: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_level_choice(value.into());
            }
            fn show_form_title(&self, title: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_form_title(title.into());
            }
            fn show_fields(&self, f: &$crate::organisation::Fields) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_name(f.name.as_str().into());
                g.set_parent(f.parent.as_str().into());
                g.set_person(f.person.as_str().into());
                g.set_level(f.level.as_str().into());
                g.set_description(f.description.as_str().into());
                g.set_reports(f.reports.as_str().into());
                g.set_seat_state(f.seat_state.as_str().into());
                g.set_position(f.position.as_str().into());
                g.set_salary_min(f.salary_min.as_str().into());
                g.set_salary_max(f.salary_max.as_str().into());
                g.set_currency(f.currency.as_str().into());
                g.set_cap(f.cap.as_str().into());
            }
            fn read_fields(&self) -> $crate::organisation::Fields {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                $crate::organisation::Fields {
                    name: g.get_name().to_string(),
                    parent: g.get_parent().to_string(),
                    position: g.get_position().to_string(),
                    person: g.get_person().to_string(),
                    level: g.get_level().to_string(),
                    description: g.get_description().to_string(),
                    reports: g.get_reports().to_string(),
                    seat_state: g.get_seat_state().to_string(),
                    salary_min: g.get_salary_min().to_string(),
                    salary_max: g.get_salary_max().to_string(),
                    currency: g.get_currency().to_string(),
                    cap: g.get_cap().to_string(),
                }
            }
            fn show_form_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_form_error(text.into());
            }
            fn show_form_open(&self, on: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.set_form_open(on);
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_retry(move || f());
            }
            fn on_tab_changed(&self, f: impl Fn(i32) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_tab_changed(move |tab| f(tab));
            }
            fn on_pick(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_pick(move |id| f(id.to_string()));
            }
            fn on_new_item(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_new_item(move || f());
            }
            fn on_save(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_save(move || f());
            }
            fn on_delete_item(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminOrgState>(self);
                g.on_delete_item(move || f());
            }
        }

        impl $crate::types_tags::TypesTagsWindow for $AppWindow {
            fn show_new_colour(&self, hex: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_new_colour($crate::types_tags::colour_of(hex));
            }
            fn new_name(&self) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.get_new_name().to_string()
            }
            fn new_colour(&self) -> String {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                $crate::types_tags::hex_of(g.get_new_colour())
            }
            fn show_new_name(&self, name: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_new_name(name.into());
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_state(admin_glue::load_state(state));
            }
            fn show_denied(&self, denied: bool) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_denied(denied);
            }
            fn show_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_error(text.into());
            }
            fn show_action_error(&self, text: &str) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_action_error(text.into());
            }
            fn show_types(&self, rows: Vec<$crate::types_tags::TypeView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_type_rows(admin_glue::model(
                    rows.into_iter()
                        .map(|t| $TypeRow {
                            label: t.name.as_str().into(),
                            name: t.name.into(),
                            shade: $crate::types_tags::colour_of(&t.shade),
                        })
                        .collect(),
                ));
            }
            fn show_areas(&self, rows: Vec<$crate::types_tags::AreaView>) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.set_areas(admin_glue::model(admin_glue::area_rows(rows)));
            }
            fn areas(&self) -> Vec<$crate::types_tags::AreaView> {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                let model = g.get_areas();
                (0..$crate::space_page_kit::__slint::Model::row_count(&model))
                    .filter_map(|row| $crate::space_page_kit::__slint::Model::row_data(&model, row))
                    .map(|r| $crate::types_tags::AreaView {
                        id: r.id.to_string(),
                        name: r.name.to_string(),
                        shade: $crate::types_tags::hex_of(r.shade),
                    })
                    .collect()
            }
            fn set_area_shade(&self, id: &str, hex: &str) -> Option<String> {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                let model = g.get_areas();
                let index = (0..$crate::space_page_kit::__slint::Model::row_count(&model)).find(|&row| {
                    $crate::space_page_kit::__slint::Model::row_data(&model, row)
                        .is_some_and(|data| data.id.as_str() == id)
                })?;
                let mut data = $crate::space_page_kit::__slint::Model::row_data(&model, index)?;
                let previous = $crate::types_tags::hex_of(data.shade);
                data.shade = $crate::types_tags::colour_of(hex);
                $crate::space_page_kit::__slint::Model::set_row_data(&model, index, data);
                Some(previous)
            }
            fn set_type_shade(&self, name: &str, hex: &str) -> Option<String> {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                let model = g.get_type_rows();
                let index = (0..$crate::space_page_kit::__slint::Model::row_count(&model)).find(|&row| {
                    $crate::space_page_kit::__slint::Model::row_data(&model, row)
                        .is_some_and(|data| data.name.as_str() == name)
                })?;
                let mut data = $crate::space_page_kit::__slint::Model::row_data(&model, index)?;
                let previous = $crate::types_tags::hex_of(data.shade);
                data.shade = $crate::types_tags::colour_of(hex);
                $crate::space_page_kit::__slint::Model::set_row_data(&model, index, data);
                Some(previous)
            }
            fn on_opened(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_opened(move || f());
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_retry(move || f());
            }
            fn on_add(&self, f: impl Fn() + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_add(move || f());
            }
            fn on_remove(&self, f: impl Fn(String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_remove(move |id| f(id.to_string()));
            }
            fn on_set_area_colour(&self, f: impl Fn(String, String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_set_area_colour(move |id, hex| f(id.to_string(), hex.to_string()));
            }
            fn on_set_type_colour(&self, f: impl Fn(String, String) + 'static) {
                let g = $crate::space_page_kit::__slint::ComponentHandle::global::<$AdminTypesTagsState>(self);
                g.on_set_type_colour(move |name, hex| f(name.to_string(), hex.to_string()));
            }
        }
    };
}
