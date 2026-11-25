//! Projects area: Project overview (`/p/:key`), Issues (`/p/:key/issues`), Issue (`/p/:key/issues/:key`), and the New issue,
//! New sprint and Create project dialogs. Each page's plain logic is a module here; the window side is a trait per page
//! ([`overview::OverviewWindow`], [`issues::IssuesWindow`], [`issue::IssueWindow`], [`new_issue::NewIssueWindow`],
//! [`new_sprint::NewSprintWindow`]), implemented for the generated window by [`wire_projects!`]. The markup is in [`UI_DIR`],
//! compiled through `@space_area_projects`.
//!
pub use space_page_kit;

pub mod create_project;
pub mod issue;
pub mod issues;
pub mod new_issue;
pub mod new_sprint;
pub mod overview;

/// The projects markup: `@space_area_projects/<file>.slint` (the app compiles it).
pub const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Expands, in the app crate, the projects window glue: the trait implementations of the converted pages for the generated
/// window, and the `projects_glue` helpers (load state, select options, calendar days).
///
/// The arguments are the generated types, in this order: the window, `PageLoadState`, `SelectOption`, `MultiOption`,
/// `CalendarDay`, `IssueRow`, `BoardChip`, `BoardItem`, `BoardLane`, `OverviewCountRow`, `OverviewAreaRow`, then the issue
/// page's `AreaChip`, `IssueDetailView`, `IssueRef`, `LinkRow`, `CommentRow`, `WorkRow`, `AttachmentRow`, `AttachmentItem`
/// and `ImageViewerItem`, and last the path of the app's clipboard function `fn(&AppShell, &str, impl FnOnce(bool) + 'static)`
/// (the issue page's copy link).
#[macro_export]
macro_rules! wire_projects {
    (
        $AppShell:ident, $PageLoadState:ident, $SelectOption:ident, $MultiOption:ident, $CalendarDay:ident,
        $IssueRow:ident, $BoardChip:ident, $BoardItem:ident, $BoardLane:ident, $OverviewCountRow:ident,
        $OverviewAreaRow:ident, $AreaChip:ident, $IssueDetailView:ident, $IssueRef:ident, $LinkRow:ident,
        $CommentRow:ident, $WorkRow:ident, $AttachmentRow:ident, $AttachmentItem:ident,
        $ImageViewerItem:ident, $write_link:path $(,)?
    ) => {
        /// The helpers of the projects glue: load state, select options and the calendar's day model.
        pub mod projects_glue {
            // The generated types named by the macro arguments live in the parent module.
            #[allow(unused_imports)]
            use super::*;
            $crate::space_page_kit::load_glue!($PageLoadState);
            $crate::space_page_kit::picker_glue!($SelectOption);

            /// The month's cells as the window's day model.
            pub fn days(
                cells: Vec<$crate::space_page_kit::dates::CalendarCell>,
            ) -> $crate::space_page_kit::__slint::ModelRc<$CalendarDay> {
                let days: Vec<$CalendarDay> = cells
                    .into_iter()
                    .map(|cell| $CalendarDay {
                        day: cell.day,
                        in_month: cell.in_month,
                        today: cell.today,
                        selected: cell.selected,
                        in_range: cell.in_range,
                    })
                    .collect();
                $crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(days),
                )
            }

            /// A list as a window model.
            pub fn model<T: Clone + 'static>(
                rows: Vec<T>,
            ) -> $crate::space_page_kit::__slint::ModelRc<T> {
                $crate::space_page_kit::__slint::ModelRc::new(
                    $crate::space_page_kit::__slint::VecModel::from(rows),
                )
            }

            /// A list of strings as a window model.
            pub fn strings(
                rows: Vec<String>,
            ) -> $crate::space_page_kit::__slint::ModelRc<
                $crate::space_page_kit::__slint::SharedString,
            > {
                model(rows.into_iter().map(Into::into).collect())
            }

            /// An area's colour as a window colour.
            pub fn colour(rgb: (u8, u8, u8)) -> $crate::space_page_kit::__slint::Color {
                $crate::space_page_kit::__slint::Color::from_rgb_u8(rgb.0, rgb.1, rgb.2)
            }
        }

        impl $crate::new_sprint::NewSprintWindow for $AppShell {
            fn route(&self) -> String {
                self.get_page_route().to_string()
            }
            fn form(&self) -> $crate::new_sprint::Form {
                $crate::new_sprint::Form {
                    name: self.get_new_sprint_name().to_string(),
                    goal: self.get_new_sprint_goal().to_string(),
                    start: self.get_new_sprint_start_date().to_string(),
                    end: self.get_new_sprint_end_date().to_string(),
                }
            }
            fn date(&self, field: $crate::new_sprint::Field) -> String {
                match field {
                    $crate::new_sprint::Field::Start => {
                        self.get_new_sprint_start_date().to_string()
                    }
                    $crate::new_sprint::Field::End => self.get_new_sprint_end_date().to_string(),
                }
            }
            fn show_date(&self, field: $crate::new_sprint::Field, text: &str) {
                match field {
                    $crate::new_sprint::Field::Start => self.set_new_sprint_start_date(text.into()),
                    $crate::new_sprint::Field::End => self.set_new_sprint_end_date(text.into()),
                }
            }
            fn creating(&self) -> bool {
                self.get_new_sprint_creating()
            }
            fn show_creating(&self, on: bool) {
                self.set_new_sprint_creating(on);
            }
            fn show_error(&self, text: &str) {
                self.set_new_sprint_error(text.into());
            }
            fn clear_form(&self) {
                self.set_new_sprint_name("".into());
                self.set_new_sprint_goal("".into());
                self.set_new_sprint_start_date("".into());
                self.set_new_sprint_end_date("".into());
                self.set_new_sprint_error("".into());
                self.set_new_sprint_creating(false);
            }
            fn show_weekday_labels(&self, labels: Vec<String>) {
                self.set_new_sprint_weekday_labels(projects_glue::strings(labels));
            }
            fn show_days(
                &self,
                field: $crate::new_sprint::Field,
                cells: Vec<$crate::space_page_kit::dates::CalendarCell>,
                title: String,
            ) {
                let days = projects_glue::days(cells);
                match field {
                    $crate::new_sprint::Field::Start => {
                        self.set_new_sprint_start_days(days);
                        self.set_new_sprint_start_title(title.into());
                    }
                    $crate::new_sprint::Field::End => {
                        self.set_new_sprint_end_days(days);
                        self.set_new_sprint_end_title(title.into());
                    }
                }
            }
            fn show_dialog(&self) {
                self.invoke_new_sprint_show();
            }
            fn close_dialog(&self) {
                self.invoke_new_sprint_close();
            }
            fn on_requested(&self, f: impl Fn() + 'static) {
                self.on_new_sprint_requested(move || f());
            }
            fn on_create(&self, f: impl Fn() + 'static) {
                self.on_new_sprint_create(move || f());
            }
            fn on_picked(&self, field: $crate::new_sprint::Field, f: impl Fn(i32) + 'static) {
                match field {
                    $crate::new_sprint::Field::Start => {
                        self.on_new_sprint_start_picked(move |index| f(index))
                    }
                    $crate::new_sprint::Field::End => {
                        self.on_new_sprint_end_picked(move |index| f(index))
                    }
                }
            }
            fn on_step(&self, field: $crate::new_sprint::Field, f: impl Fn(i32) + 'static) {
                match field {
                    $crate::new_sprint::Field::Start => {
                        self.on_new_sprint_start_step(move |step| f(step))
                    }
                    $crate::new_sprint::Field::End => {
                        self.on_new_sprint_end_step(move |step| f(step))
                    }
                }
            }
        }

        impl $crate::overview::OverviewWindow for $AppShell {
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_overview_state(projects_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_overview_error(text.into());
            }
            fn show_summary(&self, v: $crate::overview::SummaryView) {
                self.set_overview_project_name(v.project_name.as_str().into());
                self.set_overview_project_key(v.project_key.as_str().into());
                self.set_overview_description(v.description.as_str().into());
                self.set_overview_owner(v.owner.as_str().into());
                self.set_overview_members(v.members.as_str().into());
                self.set_overview_total(v.total.as_str().into());
                self.set_overview_open(v.open.as_str().into());
                self.set_overview_closed(v.closed.as_str().into());
                self.set_overview_truncated(v.truncated);
                let count_rows = |rows: Vec<$crate::overview::CountView>| {
                    projects_glue::model(
                        rows.into_iter()
                            .map(|row| $OverviewCountRow {
                                label: row.label.as_str().into(),
                                count: row.count.as_str().into(),
                                fraction: row.fraction,
                            })
                            .collect(),
                    )
                };
                self.set_overview_states(count_rows(v.states));
                self.set_overview_priorities(count_rows(v.priorities));
                let recent: Vec<$IssueRow> =
                    v.recent.into_iter().map(|row| issue_row(row)).collect();
                self.set_overview_recent(projects_glue::model(recent));
                match v.sprint {
                    Some(sprint) => {
                        self.set_overview_sprint_present(true);
                        self.set_overview_sprint_name(sprint.name.as_str().into());
                        self.set_overview_sprint_dates(sprint.dates.as_str().into());
                        self.set_overview_sprint_goal(sprint.goal.as_str().into());
                        self.set_overview_sprint_counts(sprint.counts.as_str().into());
                    }
                    None => self.set_overview_sprint_present(false),
                }
                let areas: Vec<$OverviewAreaRow> = v
                    .areas
                    .into_iter()
                    .map(|area| $OverviewAreaRow {
                        name: area.name.as_str().into(),
                        colour: projects_glue::colour(area.colour),
                        count: area.count.as_str().into(),
                    })
                    .collect();
                self.set_overview_areas(projects_glue::model(areas));
            }
            fn open_issues(&self, route: String) {
                self.invoke_issues_open(route.into());
            }
            fn on_is_route(&self, f: impl Fn(String) -> bool + 'static) {
                self.on_is_overview_route(move |route| f(route.to_string()));
            }
            fn on_opened(&self, f: impl Fn(String) + 'static) {
                self.on_overview_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_overview_retry(move || f());
            }
            fn on_open_issue(&self, f: impl Fn(String) + 'static) {
                self.on_overview_open_issue(move |key| f(key.to_string()));
            }
        }

        /// One issue row as the window draws it (the table and the overview's recent list share it).
        fn issue_row(row: $crate::issues::IssueRowView) -> $IssueRow {
            $IssueRow {
                key: row.key.as_str().into(),
                issue_type: row.issue_type.as_str().into(),
                summary: row.summary.as_str().into(),
                state: row.state.as_str().into(),
                priority: row.priority.as_str().into(),
                assignee: row.assignee.as_str().into(),
                updated: row.updated.as_str().into(),
            }
        }

        impl $crate::issues::IssuesWindow for $AppShell {
            fn query(&self) -> String {
                self.get_issues_query().to_string()
            }
            fn show_query(&self, query: &str) {
                self.set_issues_query(query.into());
            }
            fn show_view_mode(&self, view: i32) {
                self.set_issues_view_mode(view);
            }
            fn show_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_issues_state(projects_glue::load_state(state));
            }
            fn show_error(&self, text: &str) {
                self.set_issues_error(text.into());
            }
            fn show_loading_more(&self, on: bool) {
                self.set_issues_loading_more(on);
            }
            fn show_has_more(&self, on: bool) {
                self.set_issues_has_more(on);
            }
            fn show_query_note(&self, note: $crate::issues::QueryNote) {
                self.set_issues_query_message(note.message.as_str().into());
                self.set_issues_query_before(note.before.as_str().into());
                self.set_issues_query_span(note.span.as_str().into());
                self.set_issues_query_after(note.after.as_str().into());
            }
            fn show_chips(&self, on: Vec<bool>) {
                self.set_issues_chip_on(projects_glue::model(on));
            }
            fn show_sort(&self, field: &str, desc: bool) {
                self.set_issues_sort_field(field.into());
                self.set_issues_sort_desc(desc);
            }
            fn show_rows(&self, rows: Vec<$crate::issues::IssueRowView>) {
                let rows: Vec<$IssueRow> = rows.into_iter().map(|row| issue_row(row)).collect();
                self.set_issues_rows(projects_glue::model(rows));
            }
            fn show_board(&self, columns: Vec<$crate::issues::BoardColumnView>) {
                let columns: Vec<$BoardLane> = columns
                    .into_iter()
                    .map(|column| $BoardLane {
                        name: column.name.as_str().into(),
                        cards: projects_glue::model(
                            column
                                .cards
                                .into_iter()
                                .map(|card| $BoardItem {
                                    key: card.key.as_str().into(),
                                    title: card.summary.as_str().into(),
                                    detail: "".into(),
                                    badge: card.key.as_str().into(),
                                    kind: card.issue_type.as_str().into(),
                                    priority: card.priority.as_str().into(),
                                    meta: card.meta.as_str().into(),
                                    assignee: card.assignee.as_str().into(),
                                    chips: projects_glue::model(
                                        card.areas
                                            .into_iter()
                                            .map(|area| $BoardChip {
                                                text: area.name.as_str().into(),
                                                color: projects_glue::colour(area.colour),
                                            })
                                            .collect(),
                                    ),
                                })
                                .collect(),
                        ),
                    })
                    .collect();
                self.set_issues_board(projects_glue::model(columns));
            }
            fn show_board_error(&self, text: &str) {
                self.set_issues_board_error(text.into());
            }
            fn show_can_create(&self, on: bool) {
                self.set_issues_can_create(on);
            }
            fn open_issues(&self, route: String) {
                self.invoke_issues_open(route.into());
            }
            fn on_is_route(&self, f: impl Fn(String) -> bool + 'static) {
                self.on_is_issues_route(move |route| f(route.to_string()));
            }
            fn on_opened(&self, f: impl Fn(String) + 'static) {
                self.on_issues_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_issues_retry(move || f());
            }
            fn on_search(&self, f: impl Fn(String) + 'static) {
                self.on_issues_search(move |text| f(text.to_string()));
            }
            fn on_submit(&self, f: impl Fn(String) + 'static) {
                self.on_issues_submit(move |text| f(text.to_string()));
            }
            fn on_chip(&self, f: impl Fn(i32) + 'static) {
                self.on_issues_chip(move |index| f(index));
            }
            fn on_sort(&self, f: impl Fn(String) + 'static) {
                self.on_issues_sort(move |field| f(field.to_string()));
            }
            fn on_load_more(&self, f: impl Fn() + 'static) {
                self.on_issues_load_more(move || f());
            }
            fn on_board_move(&self, f: impl Fn(String, i32) + 'static) {
                self.on_issues_board_move(move |key, target| f(key.to_string(), target));
            }
            fn on_column_at(&self, f: impl Fn(f32, f32, f32, i32) -> i32 + 'static) {
                self.on_issues_column_at(move |x, width, gap, count| f(x, width, gap, count));
            }
            fn on_view_changed(&self, f: impl Fn(i32) + 'static) {
                self.on_issues_view_changed(move |view| f(view));
            }
            fn on_open_issue(&self, f: impl Fn(String) + 'static) {
                self.on_issues_open_issue(move |key| f(key.to_string()));
            }
        }

        impl $crate::new_issue::NewIssueWindow for $AppShell {
            fn route(&self) -> String {
                self.get_page_route().to_string()
            }
            fn draft(&self) -> $crate::new_issue::Draft {
                $crate::new_issue::Draft {
                    summary: self.get_new_issue_summary().to_string(),
                    description: self.get_new_issue_description().to_string(),
                    issue_type: self.get_new_issue_type().to_string(),
                    state: self.get_new_issue_state().to_string(),
                    priority: self.get_new_issue_priority().to_string(),
                    assignee: self.get_new_issue_assignee().to_string(),
                    sprint: self.get_new_issue_sprint().to_string(),
                    due_date: self.get_new_issue_due_date().to_string(),
                    estimate: self.get_new_issue_estimate().to_string(),
                    tags: self.get_new_issue_tags().to_string(),
                }
            }
            fn project(&self) -> String {
                self.get_new_issue_project().to_string()
            }
            fn creating(&self) -> bool {
                self.get_new_issue_creating()
            }
            fn show_creating(&self, on: bool) {
                self.set_new_issue_creating(on);
            }
            fn show_can_create(&self, on: bool) {
                self.set_new_issue_can_create(on);
            }
            fn show_error(&self, text: &str) {
                self.set_new_issue_error(text.into());
            }
            fn show_edit_mode(&self, on: bool) {
                self.set_new_issue_edit_mode(on);
            }
            fn show_project(&self, key: &str) {
                self.set_new_issue_project(key.into());
            }
            fn show_project_options(&self, options: Vec<$crate::new_issue::ProjectOption>) {
                let rows: Vec<$SelectOption> = options
                    .into_iter()
                    .map(|option| $SelectOption {
                        value: option.choice.value.as_str().into(),
                        label: option.choice.label.as_str().into(),
                        icon: option.icon.unwrap_or_default(),
                        badge: option.badge.as_str().into(),
                        disabled: false,
                        group: Default::default(),
                    })
                    .collect();
                self.set_new_issue_project_options(projects_glue::model(rows));
            }
            fn show_type_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                self.set_new_issue_type_options(projects_glue::options(options));
            }
            fn show_priority_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                self.set_new_issue_priority_options(projects_glue::options(options));
            }
            fn show_state_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                self.set_new_issue_state_options(projects_glue::options(options));
            }
            fn show_assignee_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                self.set_new_issue_assignee_options(projects_glue::options(options));
            }
            fn show_sprint_options(&self, options: Vec<$crate::space_page_kit::picker::Choice>) {
                self.set_new_issue_sprint_options(projects_glue::options(options));
            }
            fn show_area_options(&self, options: Vec<$crate::new_issue::AreaOption>) {
                let rows: Vec<$MultiOption> = options
                    .into_iter()
                    .map(|area| $MultiOption {
                        value: area.value.as_str().into(),
                        label: area.label.as_str().into(),
                        badge: area.badge.as_str().into(),
                        checked: area.checked,
                    })
                    .collect();
                self.set_new_issue_area_options(projects_glue::model(rows));
            }
            fn show_tags(&self, text: &str) {
                self.set_new_issue_tags(text.into());
                self.set_new_issue_area_summary(text.into());
            }
            fn show_summary(&self, text: &str) {
                self.set_new_issue_summary(text.into());
            }
            fn show_description(&self, text: &str) {
                self.set_new_issue_description(text.into());
            }
            fn show_type(&self, text: &str) {
                self.set_new_issue_type(text.into());
            }
            fn show_priority(&self, text: &str) {
                self.set_new_issue_priority(text.into());
            }
            fn show_state(&self, text: &str) {
                self.set_new_issue_state(text.into());
            }
            fn show_assignee(&self, text: &str) {
                self.set_new_issue_assignee(text.into());
            }
            fn show_sprint(&self, text: &str) {
                self.set_new_issue_sprint(text.into());
            }
            fn show_due_date(&self, text: &str) {
                self.set_new_issue_due_date(text.into());
            }
            fn show_estimate(&self, text: &str) {
                self.set_new_issue_estimate(text.into());
            }
            fn show_files(&self, names: Vec<String>) {
                self.set_new_issue_files(projects_glue::strings(names));
            }
            fn show_weekday_labels(&self, labels: Vec<String>) {
                self.set_new_issue_weekday_labels(projects_glue::strings(labels));
            }
            fn show_due_days(
                &self,
                cells: Vec<$crate::space_page_kit::dates::CalendarCell>,
                title: String,
            ) {
                self.set_new_issue_due_days(projects_glue::days(cells));
                self.set_new_issue_due_title(title.into());
            }
            fn show_dialog(&self) {
                self.invoke_new_issue_show();
            }
            fn close_dialog(&self) {
                self.invoke_new_issue_close();
            }
            fn retry_issue(&self) {
                self.invoke_issue_retry();
            }
            fn open_issues(&self, route: String) {
                self.invoke_issues_open(route.into());
            }
            fn on_requested(&self, f: impl Fn() + 'static) {
                self.on_new_issue_requested(move || f());
            }
            fn on_project_picked(&self, f: impl Fn(String) + 'static) {
                self.on_new_issue_project_picked(move |key| f(key.to_string()));
            }
            fn on_summary_edited(&self, f: impl Fn() + 'static) {
                self.on_new_issue_summary_edited(move || f());
            }
            fn on_attach(&self, f: impl Fn() + 'static) {
                self.on_new_issue_attach(move || f());
            }
            fn on_remove_file(&self, f: impl Fn(i32) + 'static) {
                self.on_new_issue_remove_file(move |index| f(index));
            }
            fn on_create(&self, f: impl Fn() + 'static) {
                self.on_new_issue_create(move || f());
            }
            fn on_due_picked(&self, f: impl Fn(i32) + 'static) {
                self.on_new_issue_due_picked(move |index| f(index));
            }
            fn on_due_month_step(&self, f: impl Fn(i32) + 'static) {
                self.on_new_issue_due_month_step(move |step| f(step));
            }
            fn on_area_toggled(&self, f: impl Fn(String) + 'static) {
                self.on_new_issue_area_toggled(move |value| f(value.to_string()));
            }
        }

        impl $crate::issue::IssueWindow for $AppShell {
            fn issue_user_id(&self) -> String {
                self.get_user_id().to_string()
            }
            fn show_issue_state(&self, state: $crate::space_page_kit::load::LoadState) {
                self.set_issue_state(projects_glue::load_state(state));
            }
            fn show_issue_error(&self, text: &str) {
                self.set_issue_error(text.into());
            }
            fn show_saving(&self, on: bool) {
                self.set_issue_saving(on);
            }
            fn show_action_error(&self, text: &str) {
                self.set_issue_action_error(text.into());
            }
            fn show_confirm_delete(&self, on: bool) {
                self.set_issue_confirm_delete(on);
            }
            fn show_access(&self, can_edit: bool, can_moderate: bool, can_delete: bool) {
                self.set_issue_can_edit(can_edit);
                self.set_issue_can_moderate(can_moderate);
                self.set_issue_can_delete(can_delete);
            }
            fn show_body(&self, body: $crate::issue::Body) {
                let v = body.view;
                let tiles: Vec<$AttachmentItem> = v
                    .attachment_tiles
                    .into_iter()
                    .map(|tile| $AttachmentItem {
                        name: tile.name.as_str().into(),
                        size: tile.size.as_str().into(),
                        is_image: tile.is_image,
                        thumbnail: tile.thumbnail.unwrap_or_default(),
                        changeable: tile.changeable,
                    })
                    .collect();
                let images: Vec<$ImageViewerItem> = v
                    .attachment_images
                    .into_iter()
                    .map(|picture| $ImageViewerItem {
                        image: picture.image,
                        title: picture.title.as_str().into(),
                    })
                    .collect();
                let chips: Vec<$AreaChip> = v
                    .area_chips
                    .into_iter()
                    .map(|chip| $AreaChip {
                        name: chip.name.as_str().into(),
                        color: projects_glue::colour(chip.color),
                        ink: projects_glue::colour(chip.ink),
                    })
                    .collect();
                self.set_issue_view($IssueDetailView {
                    key: v.key.as_str().into(),
                    project_name: v.project_name.as_str().into(),
                    position: v.position.as_str().into(),
                    has_prev: v.has_prev,
                    has_next: v.has_next,
                    votes: v.votes,
                    voted: v.voted,
                    starred: v.starred,
                    spent: v.spent.as_str().into(),
                    summary: v.summary.as_str().into(),
                    issue_type: v.issue_type.as_str().into(),
                    state_id: v.state_id.as_str().into(),
                    state_name: v.state_name.as_str().into(),
                    priority: v.priority.as_str().into(),
                    assignee_id: v.assignee_id.as_str().into(),
                    assignee_name: v.assignee_name.as_str().into(),
                    sprint_id: v.sprint_id.as_str().into(),
                    sprint_name: v.sprint_name.as_str().into(),
                    estimate: v.estimate.as_str().into(),
                    due_date: v.due_date.as_str().into(),
                    tags: projects_glue::strings(v.tags),
                    area_chips: projects_glue::model(chips),
                    reporter: v.reporter.as_str().into(),
                    created: v.created.as_str().into(),
                    updated: v.updated.as_str().into(),
                    updated_by: v.updated_by.as_str().into(),
                    description: v.description.as_str().into(),
                    has_parent: v.has_parent,
                    parent_key: v.parent_key.as_str().into(),
                    parent_summary: v.parent_summary.as_str().into(),
                    attachment_tiles: projects_glue::model(tiles),
                    attachment_images: projects_glue::model(images),
                    attachment_image_index: projects_glue::model(v.attachment_image_index),
                    state_options: projects_glue::options(v.state_options),
                    type_options: projects_glue::options(v.type_options),
                    priority_options: projects_glue::options(v.priority_options),
                    assignee_options: projects_glue::options(v.assignee_options),
                    sprint_options: projects_glue::options(v.sprint_options),
                });
                let work: Vec<$WorkRow> = body
                    .work
                    .into_iter()
                    .map(|entry| $WorkRow {
                        id: entry.id.as_str().into(),
                        author: entry.author.as_str().into(),
                        minutes: entry.minutes.as_str().into(),
                        spent_on: entry.spent_on.as_str().into(),
                        note: entry.note.as_str().into(),
                        changeable: entry.changeable,
                    })
                    .collect();
                self.set_issue_work(projects_glue::model(work));
                let files: Vec<$AttachmentRow> = body
                    .files
                    .into_iter()
                    .map(|file| $AttachmentRow {
                        id: file.id.as_str().into(),
                        name: file.name.as_str().into(),
                        size: file.size.as_str().into(),
                        changeable: file.changeable,
                    })
                    .collect();
                self.set_issue_attachments(projects_glue::model(files));
                let subtasks: Vec<$IssueRef> = body
                    .subtasks
                    .into_iter()
                    .map(|sub| $IssueRef {
                        key: sub.key.as_str().into(),
                        summary: sub.summary.as_str().into(),
                        state: sub.state.as_str().into(),
                    })
                    .collect();
                self.set_issue_subtasks(projects_glue::model(subtasks));
                let links: Vec<$LinkRow> = body
                    .links
                    .into_iter()
                    .map(|link| $LinkRow {
                        id: link.id.as_str().into(),
                        heading: link.heading.as_str().into(),
                        key: link.key.as_str().into(),
                        summary: link.summary.as_str().into(),
                        state: link.state.as_str().into(),
                    })
                    .collect();
                self.set_issue_links(projects_glue::model(links));
                let comments: Vec<$CommentRow> = body
                    .comments
                    .into_iter()
                    .map(|comment| $CommentRow {
                        id: comment.id.as_str().into(),
                        initials: comment.initials.as_str().into(),
                        author: comment.author.as_str().into(),
                        when: comment.when.as_str().into(),
                        body: comment.body.as_str().into(),
                        changeable: comment.changeable,
                        editing: comment.editing,
                    })
                    .collect();
                self.set_issue_comments(projects_glue::model(comments));
                self.set_issue_activity(projects_glue::strings(body.activity));
            }
            fn clear_body(&self) {
                self.set_issue_view($IssueDetailView::default());
                self.set_issue_subtasks(projects_glue::model(Vec::<$IssueRef>::new()));
                self.set_issue_links(projects_glue::model(Vec::<$LinkRow>::new()));
                self.set_issue_comments(projects_glue::model(Vec::<$CommentRow>::new()));
                self.set_issue_activity(projects_glue::strings(Vec::new()));
                self.set_issue_work(projects_glue::model(Vec::<$WorkRow>::new()));
                self.set_issue_attachments(projects_glue::model(Vec::<$AttachmentRow>::new()));
            }
            fn show_comment_draft(&self, text: &str) {
                self.set_issue_comment_edit_draft(text.into());
            }
            fn open_route(&self, route: String) {
                self.invoke_issues_open(route.into());
            }
            fn copy_link(&self, link: &str) {
                $write_link(self, link, |_| {});
            }
            fn on_is_route(&self, f: impl Fn(String) -> bool + 'static) {
                self.on_is_issue_route(move |route| f(route.to_string()));
            }
            fn on_vote(&self, f: impl Fn(bool) + 'static) {
                self.on_issue_vote(move |on| f(on));
            }
            fn on_star(&self, f: impl Fn(bool) + 'static) {
                self.on_issue_star(move |on| f(on));
            }
            fn on_prev(&self, f: impl Fn() + 'static) {
                self.on_issue_prev(move || f());
            }
            fn on_next(&self, f: impl Fn() + 'static) {
                self.on_issue_next(move || f());
            }
            fn on_copy_key(&self, f: impl Fn(String) + 'static) {
                self.on_issue_copy_key(move |key| f(key.to_string()));
            }
            fn on_work_add(&self, f: impl Fn(String, String, String) + 'static) {
                self.on_issue_work_add(move |minutes, date, note| {
                    f(minutes.to_string(), date.to_string(), note.to_string())
                });
            }
            fn on_work_delete(&self, f: impl Fn(String) + 'static) {
                self.on_issue_work_delete(move |id| f(id.to_string()));
            }
            fn on_attach_files(&self, f: impl Fn() + 'static) {
                self.on_issue_attach(move || f());
            }
            fn on_attachment_delete(&self, f: impl Fn(String) + 'static) {
                self.on_issue_attachment_delete(move |id| f(id.to_string()));
            }
            fn on_attachment_download(&self, f: impl Fn(i32) + 'static) {
                self.on_issue_attachment_download(move |index| f(index));
            }
            fn on_opened(&self, f: impl Fn(String) + 'static) {
                self.on_issue_opened(move |route| f(route.to_string()));
            }
            fn on_retry(&self, f: impl Fn() + 'static) {
                self.on_issue_retry(move || f());
            }
            fn on_back(&self, f: impl Fn() + 'static) {
                self.on_issue_back(move || f());
            }
            fn on_open_issue(&self, f: impl Fn(String) + 'static) {
                self.on_issue_open_issue(move |key| f(key.to_string()));
            }
            fn on_edit_open(&self, f: impl Fn() + 'static) {
                self.on_issue_edit_open(move || f());
            }
            fn on_subtask_add(&self, f: impl Fn(String) + 'static) {
                self.on_issue_subtask_add(move |summary| f(summary.to_string()));
            }
            fn on_link_add(&self, f: impl Fn(String, String) + 'static) {
                self.on_issue_link_add(move |target, kind| f(target.to_string(), kind.to_string()));
            }
            fn on_link_remove(&self, f: impl Fn(String) + 'static) {
                self.on_issue_link_remove(move |id| f(id.to_string()));
            }
            fn on_comment_add(&self, f: impl Fn(String) + 'static) {
                self.on_issue_comment_add(move |body| f(body.to_string()));
            }
            fn on_comment_edit_start(&self, f: impl Fn(String) + 'static) {
                self.on_issue_comment_edit_start(move |id| f(id.to_string()));
            }
            fn on_comment_edit_cancel(&self, f: impl Fn() + 'static) {
                self.on_issue_comment_edit_cancel(move || f());
            }
            fn on_comment_edit_save(&self, f: impl Fn(String, String) + 'static) {
                self.on_issue_comment_edit_save(move |id, body| {
                    f(id.to_string(), body.to_string())
                });
            }
            fn on_comment_delete(&self, f: impl Fn(String) + 'static) {
                self.on_issue_comment_delete(move |id| f(id.to_string()));
            }
            fn on_delete_ask(&self, f: impl Fn() + 'static) {
                self.on_issue_delete_ask(move || f());
            }
            fn on_delete_cancel(&self, f: impl Fn() + 'static) {
                self.on_issue_delete_cancel(move || f());
            }
            fn on_delete_confirm(&self, f: impl Fn() + 'static) {
                self.on_issue_delete_confirm(move || f());
            }
        }
    };
}
