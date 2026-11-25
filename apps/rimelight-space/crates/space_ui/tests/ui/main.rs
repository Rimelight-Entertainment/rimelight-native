//! The one UI test binary for `space_ui`. Each test file is a module here, so the crate links Slint once instead of
//! once per file. Add new UI tests as `tests/ui/<name>.rs` (never a new top-level `tests/*.rs`), then list them in `TESTS`.
//!
//! No libtest harness (`harness = false` in Cargo.toml). Slint's platform is bound to the thread that first creates it,
//! and libtest runs every test on a new thread, so the tests run one after another on the main thread. An optional first
//! argument runs only the tests whose name contains it.

use std::io::Write as _;
use std::process::ExitCode;

mod admin;
mod admin_users;
mod app_shell;
mod back_forward;
mod back_overlays;
mod canvas;
mod company;
mod connection_lost;
mod copy_link;
mod date_b6;
mod departments;
mod employees;
mod home;
mod issue;
mod issues;
mod locations;
mod more_drawer;
mod new_issue;
mod page_header;
mod page_tabs;
mod pane_headers;
mod persistence;
mod phone_workspaces;
mod pointer_drag;
mod profile;
mod project_tree;
mod quick_action;
mod rail_badge;
mod shell;
mod shell_chrome;
mod shortcuts;
mod sign_in;
mod status_bar;
mod strip_keyboard;
mod support;
mod tab_drag;
mod team;
mod teams;
mod tiles_ui;
mod workspaces;

/// Every UI test, in run order.
const TESTS: &[(&str, fn())] = &[
    (
        "app_shell::app_shell_palette_about_and_theme_work_headless",
        app_shell::app_shell_palette_about_and_theme_work_headless,
    ),
    (
        "back_forward::back_forward_buttons_and_shortcuts",
        back_forward::back_forward_buttons_and_shortcuts,
    ),
    (
        "back_forward::back_forward_sidebar_click_replaces_and_ctrl_click_opens_a_tab",
        back_forward::back_forward_sidebar_click_replaces_and_ctrl_click_opens_a_tab,
    ),
    (
        "back_overlays::back_closes_page_tab_menu",
        back_overlays::back_closes_page_tab_menu,
    ),
    (
        "back_overlays::back_closes_share_menu",
        back_overlays::back_closes_share_menu,
    ),
    (
        "back_overlays::back_closes_page_overflow_menu",
        back_overlays::back_closes_page_overflow_menu,
    ),
    (
        "back_overlays::back_closes_workspace_menu_on_the_phone_sheet",
        back_overlays::back_closes_workspace_menu_on_the_phone_sheet,
    ),
    (
        "back_overlays::back_closes_profile_popover",
        back_overlays::back_closes_profile_popover,
    ),
    (
        "back_overlays::back_closes_quick_action_menu",
        back_overlays::back_closes_quick_action_menu,
    ),
    (
        "back_overlays::back_closes_project_switcher",
        back_overlays::back_closes_project_switcher,
    ),
    (
        "back_overlays::back_closes_status_bar_jobs_popover",
        back_overlays::back_closes_status_bar_jobs_popover,
    ),
    (
        "back_overlays::back_closes_status_bar_language_popover",
        back_overlays::back_closes_status_bar_language_popover,
    ),
    (
        "back_overlays::back_closes_rename_dialog",
        back_overlays::back_closes_rename_dialog,
    ),
    (
        "back_overlays::back_closes_shortcuts_dialog",
        back_overlays::back_closes_shortcuts_dialog,
    ),
    (
        "back_overlays::back_closes_command_palette",
        back_overlays::back_closes_command_palette,
    ),
    (
        "back_overlays::back_closes_workspace_sheet",
        back_overlays::back_closes_workspace_sheet,
    ),
    (
        "back_overlays::back_on_the_phone_drawer_returns_to_more_then_closes_the_drawer",
        back_overlays::back_on_the_phone_drawer_returns_to_more_then_closes_the_drawer,
    ),
    (
        "back_overlays::back_closes_about_report_and_settings_screens",
        back_overlays::back_closes_about_report_and_settings_screens,
    ),
    (
        "back_overlays::back_with_nothing_open_steps_history_then_is_not_accepted",
        back_overlays::back_with_nothing_open_steps_history_then_is_not_accepted,
    ),
    (
        "connection_lost::disconnect_shows_the_overlay_and_blocks_clicks_underneath",
        connection_lost::disconnect_shows_the_overlay_and_blocks_clicks_underneath,
    ),
    (
        "connection_lost::reconnect_hides_the_overlay_and_restores_clicks",
        connection_lost::reconnect_hides_the_overlay_and_restores_clicks,
    ),
    (
        "copy_link::copy_link_enabled_in_each_menu_and_writes_the_page_link",
        copy_link::copy_link_enabled_in_each_menu_and_writes_the_page_link,
    ),
    (
        "more_drawer::more_drawer_top_areas_and_footer_on_the_form",
        more_drawer::more_drawer_top_areas_and_footer_on_the_form,
    ),
    (
        "more_drawer::more_drawer_window_routes_areas_and_footer_on_phone",
        more_drawer::more_drawer_window_routes_areas_and_footer_on_phone,
    ),
    (
        "more_drawer::more_drawer_back_row_returns_from_an_area_to_the_more_list",
        more_drawer::more_drawer_back_row_returns_from_an_area_to_the_more_list,
    ),
    (
        "page_header::page_header_share_menu_rows_by_page_kind",
        page_header::page_header_share_menu_rows_by_page_kind,
    ),
    (
        "page_header::page_header_overflow_menu_reuses_the_tab_operations",
        page_header::page_header_overflow_menu_reuses_the_tab_operations,
    ),
    (
        "page_header::page_header_primary_action_slot_is_empty_until_a_page_has_one",
        page_header::page_header_primary_action_slot_is_empty_until_a_page_has_one,
    ),
    (
        "page_header::page_header_inspector_toggle_is_enabled_on_detail_split_and_board_only",
        page_header::page_header_inspector_toggle_is_enabled_on_detail_split_and_board_only,
    ),
    (
        "page_header::page_header_phone_back_arrow_steps_back_and_hides_without_history",
        page_header::page_header_phone_back_arrow_steps_back_and_hides_without_history,
    ),
    (
        "page_header::page_header_project_crumb_shows_the_project_name",
        page_header::page_header_project_crumb_shows_the_project_name,
    ),
    (
        "page_tabs::page_tab_menu_right_click_and_actions",
        page_tabs::page_tab_menu_right_click_and_actions,
    ),
    (
        "pane_headers::panes_show_their_own_title_under_their_strip_below_the_top_bar",
        pane_headers::panes_show_their_own_title_under_their_strip_below_the_top_bar,
    ),
    (
        "pane_headers::breadcrumb_in_pane_b_navigates_pane_b_and_focuses_it",
        pane_headers::breadcrumb_in_pane_b_navigates_pane_b_and_focuses_it,
    ),
    (
        "pane_headers::share_copy_link_in_pane_b_copies_pane_bs_link",
        pane_headers::share_copy_link_in_pane_b_copies_pane_bs_link,
    ),
    (
        "quick_action::quick_action_button_menu_shortcut_and_phone_hiding",
        quick_action::quick_action_button_menu_shortcut_and_phone_hiding,
    ),
    (
        "quick_action::quick_action_shortcut_accepts_each_layout_text",
        quick_action::quick_action_shortcut_accepts_each_layout_text,
    ),
    (
        "project_tree::project_tree_lists_each_project_page_for_the_current_key",
        project_tree::project_tree_lists_each_project_page_for_the_current_key,
    ),
    (
        "project_tree::project_switch_moves_every_project_row_and_the_active_page",
        project_tree::project_switch_moves_every_project_row_and_the_active_page,
    ),
    (
        "project_tree::project_switcher_filter_narrows_by_name_or_key",
        project_tree::project_switcher_filter_narrows_by_name_or_key,
    ),
    (
        "rail_badge::rail_badges_show_counts_and_only_for_queue_areas",
        rail_badge::rail_badges_show_counts_and_only_for_queue_areas,
    ),
    (
        "shell::rail_and_sidebar_selection_scrolling_and_elision",
        shell::rail_and_sidebar_selection_scrolling_and_elision,
    ),
    (
        "shell_chrome::sf4_top_bar_status_bar_and_sidebar_sections_headless",
        shell_chrome::sf4_top_bar_status_bar_and_sidebar_sections_headless,
    ),
    (
        "shortcuts::shortcuts_dialog_opens_lists_and_closes",
        shortcuts::shortcuts_dialog_opens_lists_and_closes,
    ),
    (
        "status_bar::status_bar_timer_and_jobs_indicator_headless",
        status_bar::status_bar_timer_and_jobs_indicator_headless,
    ),
    (
        "status_bar::status_bar_jobs_popover_lists_each_job_headless",
        status_bar::status_bar_jobs_popover_lists_each_job_headless,
    ),
    (
        "date_b6::date_b6_calendar_grid_keys_pick_and_month_steps",
        date_b6::date_b6_calendar_grid_keys_pick_and_month_steps,
    ),
    (
        "date_b6::date_b6_time_picker_fields_and_callback",
        date_b6::date_b6_time_picker_fields_and_callback,
    ),
    (
        "workspaces::workspace_strip_shortcuts_menu_and_empty_state",
        workspaces::workspace_strip_shortcuts_menu_and_empty_state,
    ),
    (
        "tiles_ui::tile_groups_render_focus_split_resize_and_switch",
        tiles_ui::tile_groups_render_focus_split_resize_and_switch,
    ),
    (
        "pointer_drag::press_drag_to_the_edge_splits_with_pointer_events",
        pointer_drag::press_drag_to_the_edge_splits_with_pointer_events,
    ),
    (
        "pointer_drag::press_drag_within_a_strip_reorders_with_pointer_events",
        pointer_drag::press_drag_within_a_strip_reorders_with_pointer_events,
    ),
    (
        "pointer_drag::press_drag_onto_another_groups_strip_with_pointer_events",
        pointer_drag::press_drag_onto_another_groups_strip_with_pointer_events,
    ),
    (
        "pointer_drag::escape_during_a_pointer_drag_cancels_it",
        pointer_drag::escape_during_a_pointer_drag_cancels_it,
    ),
    (
        "pointer_drag::release_outside_every_group_cancels_a_pointer_drag",
        pointer_drag::release_outside_every_group_cancels_a_pointer_drag,
    ),
    (
        "pointer_drag::press_drag_reorders_the_workspace_strip_with_pointer_events",
        pointer_drag::press_drag_reorders_the_workspace_strip_with_pointer_events,
    ),
    (
        "pointer_drag::pinned_tab_tooltip_goes_on_press_and_drag_and_stays_gone",
        pointer_drag::pinned_tab_tooltip_goes_on_press_and_drag_and_stays_gone,
    ),
    (
        "pointer_drag::quick_action_tooltip_does_not_return_after_a_menu_pick",
        pointer_drag::quick_action_tooltip_does_not_return_after_a_menu_pick,
    ),
    (
        "pointer_drag::workspace_plus_button_follows_the_last_tab_and_pins_right_on_overflow",
        pointer_drag::workspace_plus_button_follows_the_last_tab_and_pins_right_on_overflow,
    ),
    (
        "pointer_drag::split_handle_drag_resizes_the_groups_live_with_pointer_events",
        pointer_drag::split_handle_drag_resizes_the_groups_live_with_pointer_events,
    ),
    (
        "tab_drag::tab_drag_reorders_within_a_strip",
        tab_drag::tab_drag_reorders_within_a_strip,
    ),
    (
        "tab_drag::tab_drag_to_an_edge_splits_the_group",
        tab_drag::tab_drag_to_an_edge_splits_the_group,
    ),
    (
        "tab_drag::tab_drag_onto_another_groups_strip_moves_the_tab",
        tab_drag::tab_drag_onto_another_groups_strip_moves_the_tab,
    ),
    (
        "tab_drag::tab_drag_refuses_the_only_tab_on_its_own_edge_and_the_cap",
        tab_drag::tab_drag_refuses_the_only_tab_on_its_own_edge_and_the_cap,
    ),
    (
        "tab_drag::tab_drag_escape_and_a_release_outside_cancel",
        tab_drag::tab_drag_escape_and_a_release_outside_cancel,
    ),
    (
        "tab_drag::workspace_strip_drag_reorders_the_workspaces",
        tab_drag::workspace_strip_drag_reorders_the_workspaces,
    ),
    (
        "persistence::saved_workspaces_load_at_start_and_the_exit_save_writes_them_back",
        persistence::saved_workspaces_load_at_start_and_the_exit_save_writes_them_back,
    ),
    (
        "persistence::a_corrupt_save_starts_with_one_workspace_and_does_not_crash",
        persistence::a_corrupt_save_starts_with_one_workspace_and_does_not_crash,
    ),
    (
        "persistence::a_change_waits_for_the_debounce_and_flush_saves_it_at_once",
        persistence::a_change_waits_for_the_debounce_and_flush_saves_it_at_once,
    ),
    (
        "persistence::closing_every_workspace_saves_the_empty_state",
        persistence::closing_every_workspace_saves_the_empty_state,
    ),
    (
        "phone_workspaces::phone_workspace_switcher_sheet_switches_adds_and_closes",
        phone_workspaces::phone_workspace_switcher_sheet_switches_adds_and_closes,
    ),
    (
        "phone_workspaces::web_hides_the_strip_and_reaches_the_switcher_with_two_workspaces",
        phone_workspaces::web_hides_the_strip_and_reaches_the_switcher_with_two_workspaces,
    ),
    (
        "phone_workspaces::phone_workspace_sheet_row_menu_pins_renames_duplicates_and_closes",
        phone_workspaces::phone_workspace_sheet_row_menu_pins_renames_duplicates_and_closes,
    ),
    (
        "strip_keyboard::strip_keyboard_roving_focus_menus_and_tooltip",
        strip_keyboard::strip_keyboard_roving_focus_menus_and_tooltip,
    ),
    (
        "sign_in::sign_in_screen_states_render",
        sign_in::sign_in_screen_states_render,
    ),
    (
        "sign_in::sign_in_button_invokes_callback_only_when_enabled",
        sign_in::sign_in_button_invokes_callback_only_when_enabled,
    ),
    (
        "sign_in::avatar_menu_shows_the_injected_user",
        sign_in::avatar_menu_shows_the_injected_user,
    ),
    (
        "sign_in::sign_out_calls_back_and_returns_to_sign_in",
        sign_in::sign_out_calls_back_and_returns_to_sign_in,
    ),
    (
        "sign_in::shell_shortcuts_are_off_while_signed_out",
        sign_in::shell_shortcuts_are_off_while_signed_out,
    ),
    (
        "sign_in::start_up_unreachable_shows_the_screen_and_retry_recovers",
        sign_in::start_up_unreachable_shows_the_screen_and_retry_recovers,
    ),
    (
        "employees::employees_page_states_rows_and_retry",
        employees::employees_page_states_rows_and_retry,
    ),
    (
        "employees::employees_search_waits_then_asks_for_a_new_list",
        employees::employees_search_waits_then_asks_for_a_new_list,
    ),
    (
        "employees::employees_row_opens_the_person_profile_route",
        employees::employees_row_opens_the_person_profile_route,
    ),
    (
        "home::home_cards_start_loading_and_show_their_loaded_rows",
        home::home_cards_start_loading_and_show_their_loaded_rows,
    ),
    (
        "home::home_cards_show_their_empty_states",
        home::home_cards_show_their_empty_states,
    ),
    (
        "home::home_cards_fail_and_retry_on_their_own",
        home::home_cards_fail_and_retry_on_their_own,
    ),
    (
        "home::home_rows_and_view_all_open_their_routes",
        home::home_rows_and_view_all_open_their_routes,
    ),
    (
        "company::company_lists_are_tables_with_columns_and_ids",
        company::company_lists_are_tables_with_columns_and_ids,
    ),
    (
        "issues::issues_list_shows_rows_empty_and_error_states",
        issues::issues_list_shows_rows_empty_and_error_states,
    ),
    (
        "issues::issues_refused_query_marks_its_span_under_the_bar",
        issues::issues_refused_query_marks_its_span_under_the_bar,
    ),
    (
        "issues::issues_chips_toggle_their_clause_in_the_query",
        issues::issues_chips_toggle_their_clause_in_the_query,
    ),
    (
        "issues::issues_header_click_writes_the_sort_and_toggles_it",
        issues::issues_header_click_writes_the_sort_and_toggles_it,
    ),
    (
        "issues::new_issue_is_hidden_for_a_viewer_and_shown_for_a_member",
        issues::new_issue_is_hidden_for_a_viewer_and_shown_for_a_member,
    ),
    (
        "issues::a_row_opens_its_issue_route",
        issues::a_row_opens_its_issue_route,
    ),
    (
        "new_issue::dialog_opens_on_the_route_project_with_the_defaults",
        new_issue::dialog_opens_on_the_route_project_with_the_defaults,
    ),
    (
        "new_issue::dialog_takes_the_first_editable_project_when_the_route_has_none",
        new_issue::dialog_takes_the_first_editable_project_when_the_route_has_none,
    ),
    (
        "new_issue::a_blank_summary_blocks_create",
        new_issue::a_blank_summary_blocks_create,
    ),
    (
        "new_issue::create_sends_the_body_then_opens_the_issue",
        new_issue::create_sends_the_body_then_opens_the_issue,
    ),
    (
        "new_issue::priority_choices_show_the_stored_values",
        new_issue::priority_choices_show_the_stored_values,
    ),
    (
        "new_issue::areas_are_picked_from_the_admin_catalogue_and_the_due_date_from_the_grid",
        new_issue::areas_are_picked_from_the_admin_catalogue_and_the_due_date_from_the_grid,
    ),
    (
        "new_issue::quick_action_opens_the_dialog_with_the_project_preselected",
        new_issue::quick_action_opens_the_dialog_with_the_project_preselected,
    ),
    (
        "new_issue::quick_action_new_issue_is_disabled_without_an_editable_project",
        new_issue::quick_action_new_issue_is_disabled_without_an_editable_project,
    ),
    (
        "issue::issue_page_shows_loading_error_then_loaded",
        issue::issue_page_shows_loading_error_then_loaded,
    ),
    (
        "issue::issue_edit_modal_prefills_and_saves_only_the_changed_fields",
        issue::issue_edit_modal_prefills_and_saves_only_the_changed_fields,
    ),
    (
        "issue::issue_add_subtask_sends_it_under_this_issue",
        issue::issue_add_subtask_sends_it_under_this_issue,
    ),
    (
        "issue::issue_links_add_and_remove",
        issue::issue_links_add_and_remove,
    ),
    (
        "issue::issue_comments_add_and_edit_own_only",
        issue::issue_comments_add_and_edit_own_only,
    ),
    (
        "issue::issue_activity_lines_read_as_sentences",
        issue::issue_activity_lines_read_as_sentences,
    ),
    (
        "issue::issue_viewer_and_non_member_see_it_read_only",
        issue::issue_viewer_and_non_member_see_it_read_only,
    ),
    (
        "issue::issue_delete_needs_a_confirm_step_and_an_admin",
        issue::issue_delete_needs_a_confirm_step_and_an_admin,
    ),
    (
        "issue::issue_vote_and_star_send_their_requests_for_any_role",
        issue::issue_vote_and_star_send_their_requests_for_any_role,
    ),
    (
        "issue::issue_add_work_item_sends_the_entry_and_refuses_bad_minutes",
        issue::issue_add_work_item_sends_the_entry_and_refuses_bad_minutes,
    ),
    (
        "issue::issue_prev_and_next_follow_the_list_it_was_opened_from",
        issue::issue_prev_and_next_follow_the_list_it_was_opened_from,
    ),
    (
        "profile::profile_other_person_is_read_only_and_hides_personal_when_absent",
        profile::profile_other_person_is_read_only_and_hides_personal_when_absent,
    ),
    (
        "profile::profile_own_profile_shows_edit_and_only_self_fields_in_the_form",
        profile::profile_own_profile_shows_edit_and_only_self_fields_in_the_form,
    ),
    (
        "profile::profile_save_success_and_refused_save_show_the_result",
        profile::profile_save_success_and_refused_save_show_the_result,
    ),
    (
        "profile::profile_org_links_and_the_avatar_menu_open_profiles",
        profile::profile_org_links_and_the_avatar_menu_open_profiles,
    ),
    (
        "profile::profile_error_then_try_again",
        profile::profile_error_then_try_again,
    ),
    (
        "teams::teams_list_states_and_team_rows",
        teams::teams_list_states_and_team_rows,
    ),
    (
        "teams::org_chart_arrows_and_find_person",
        teams::org_chart_arrows_and_find_person,
    ),
    (
        "teams::teams_rows_open_the_team_route",
        teams::teams_rows_open_the_team_route,
    ),
    (
        "team::team_page_states_and_retry",
        team::team_page_states_and_retry,
    ),
    (
        "team::team_without_members_is_loaded_and_empty_of_people",
        team::team_without_members_is_loaded_and_empty_of_people,
    ),
    (
        "team::team_lead_and_member_rows_open_profiles",
        team::team_lead_and_member_rows_open_profiles,
    ),
    (
        "departments::departments_states_and_retry",
        departments::departments_states_and_retry,
    ),
    (
        "departments::departments_arrows_and_selection_show_head_and_teams",
        departments::departments_arrows_and_selection_show_head_and_teams,
    ),
    (
        "departments::departments_team_and_head_rows_open_their_pages",
        departments::departments_team_and_head_rows_open_their_pages,
    ),
    (
        "departments::departments_page_body_fills_the_group",
        departments::departments_page_body_fills_the_group,
    ),
    (
        "locations::locations_table_rows_and_states",
        locations::locations_table_rows_and_states,
    ),
    (
        "locations::locations_write_is_admin_only",
        locations::locations_write_is_admin_only,
    ),
    (
        "canvas::canvas_page_shows_items_from_the_room",
        canvas::canvas_page_shows_items_from_the_room,
    ),
    (
        "canvas::dragging_an_item_sends_one_op",
        canvas::dragging_an_item_sends_one_op,
    ),
    (
        "canvas::read_only_room_sends_nothing",
        canvas::read_only_room_sends_nothing,
    ),
    (
        "canvas::deleted_canvas_shows_an_error",
        canvas::deleted_canvas_shows_an_error,
    ),
    (
        "canvas::canvas_list_shows_rows_and_empty",
        canvas::canvas_list_shows_rows_and_empty,
    ),
    (
        "canvas::list_row_actions_open_the_dialogs",
        canvas::list_row_actions_open_the_dialogs,
    ),
    (
        "canvas::canvas_list_sorts_by_name",
        canvas::canvas_list_sorts_by_name,
    ),
    (
        "canvas::canvas_list_double_click_survives_pick",
        canvas::canvas_list_double_click_survives_pick,
    ),
    (
        "canvas::canvas_list_share_dialog_opens",
        canvas::canvas_list_share_dialog_opens,
    ),
    (
        "canvas::several_images_are_spread_not_stacked",
        canvas::several_images_are_spread_not_stacked,
    ),
    (
        "canvas::a_video_item_shows_its_duration_and_a_stopped_control",
        canvas::a_video_item_shows_its_duration_and_a_stopped_control,
    ),
    (
        "canvas::right_click_opens_the_cascading_menu",
        canvas::right_click_opens_the_cascading_menu,
    ),
    (
        "admin_users::users_start_date_picks_an_iso_date",
        admin_users::users_start_date_picks_an_iso_date,
    ),
    (
        "admin_users::users_table_shows_one_row_per_person_with_seven_cells",
        admin_users::users_table_shows_one_row_per_person_with_seven_cells,
    ),
    (
        "admin_users::users_edit_opens_a_modal_for_the_picked_person",
        admin_users::users_edit_opens_a_modal_for_the_picked_person,
    ),
    (
        "admin::admin_users_states_and_the_admin_gate",
        admin::admin_users_states_and_the_admin_gate,
    ),
    (
        "admin::admin_users_add_validates_then_saves",
        admin::admin_users_add_validates_then_saves,
    ),
    (
        "admin::admin_users_edit_shows_the_server_errors",
        admin::admin_users_edit_shows_the_server_errors,
    ),
    (
        "admin::admin_org_tabs_create_and_the_delete_conflict",
        admin::admin_org_tabs_create_and_the_delete_conflict,
    ),
    (
        "admin::admin_org_seats_and_budgets",
        admin::admin_org_seats_and_budgets,
    ),
    (
        "admin::admin_org_position_form",
        admin::admin_org_position_form,
    ),
    (
        "admin::admin_roles_add_and_remove",
        admin::admin_roles_add_and_remove,
    ),
];

fn main() -> ExitCode {
    // The first argument that is not a flag. Cargo or the caller may add flags, which must not filter out every test.
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    let mut passed = 0;
    let mut failed = Vec::new();
    for (name, test) in TESTS {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        print!("test {name} ... ");
        let _ = std::io::stdout().flush();
        match std::panic::catch_unwind(*test) {
            Ok(()) => {
                println!("ok");
                passed += 1;
            }
            Err(_) => {
                println!("FAILED");
                failed.push(*name);
            }
        }
    }
    println!("\nui: {passed} passed; {} failed", failed.len());
    for name in &failed {
        println!("  failed: {name}");
    }
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
