//! Phone More drawer (OQ24, OQ26, OQ29; IA section 8). The drawer's top holds the running timer, the background jobs and the
//! phone status tools (Record, Language, Theme; not Fullscreen or Shortcuts). Its middle lists the six areas that have no
//! phone tab. Its footer holds About, Report Issue and Settings, and each opens its shell screen. The connection state is
//! not in the drawer.
//!
//! The drawer is drawn on its own (a form that binds its properties), and then inside the window at phone width
//! (`app_shell::build`). The headless backend sends the clicks; no window opens.

use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize, Model as _, ModelRc, VecModel};
use space_nav::Area;
use space_ui::slint_ui::{JobItem as AppJob, app_shell};
use space_ui::theme::ThemeMode;

pub use space_ui_test_slint::more_drawer::*;

// Slint applies property changes to the element tree on the next event-loop pass, so each read runs one pass first (the
// quick action test does the same).
fn settle() {
    slint::platform::update_timers_and_animations();
}

fn job(title: &str, state: &str) -> JobItem {
    JobItem {
        title: title.into(),
        state: state.into(),
    }
}

/// The six areas with no phone tab, by title, in nav order.
const MORE_TITLES: [&str; 6] = [
    "People",
    "Finance",
    "Customers",
    "Operations",
    "Analytics",
    "Admin",
];

/// The drawer on its own: the timer, the jobs, the tools, the areas and the footer.
pub fn more_drawer_top_areas_and_footer_on_the_form() {
    crate::support::init();

    let form = MoreForm::new().expect("more drawer form builds");

    // Timer: the drawer shows its text while running, and no text while stopped.
    settle();
    assert!(
        form.get_test_timer_text().is_empty(),
        "the timer is hidden when it is not running"
    );
    form.set_timer_running(true);
    settle();
    assert_eq!(
        form.get_test_timer_text().as_str(),
        "1:02:03",
        "the timer shows its text while running"
    );
    form.set_timer_running(false);
    settle();
    assert!(
        form.get_test_timer_text().is_empty(),
        "the timer hides again when stopped"
    );

    // Jobs: shown only while there are jobs, with the count.
    settle();
    assert!(!form.get_test_jobs_shown(), "no jobs, no jobs row");
    form.set_jobs(ModelRc::new(VecModel::from(vec![
        job("Upload report.pdf", "40%"),
        job("Export payroll", "Queued"),
    ])));
    settle();
    assert!(form.get_test_jobs_shown(), "the jobs row shows");
    assert_eq!(
        form.get_test_jobs_count(),
        2,
        "the jobs row shows the count"
    );
    form.set_jobs(ModelRc::new(VecModel::<JobItem>::default()));
    settle();
    assert!(!form.get_test_jobs_shown(), "the jobs row hides when idle");

    // Top tools: Record, Language and Theme, and no Fullscreen or Shortcuts (not on a phone).
    settle();
    assert!(form.get_test_has_record(), "the drawer top has Record");
    assert!(form.get_test_has_language(), "the drawer top has Language");
    assert_eq!(
        form.get_theme_label().as_str(),
        "Theme: light",
        "the drawer top has Theme"
    );
    assert!(
        !form.get_test_has_fullscreen(),
        "Fullscreen is not on a phone"
    );
    assert!(
        !form.get_test_has_shortcuts(),
        "Shortcuts is not on a phone"
    );

    // Language records a choice: the list opens, and a pick sets the index and closes the list.
    settle();
    assert!(
        !form.get_test_language_open(),
        "the language list starts closed"
    );
    form.invoke_test_click_language();
    settle();
    assert!(form.get_test_language_open(), "the language list opens");
    assert!(
        form.get_test_language_choices()
            .iter()
            .any(|name| name.as_str() == "Português"),
        "the language list offers Português"
    );
    form.invoke_test_pick_language(1);
    settle();
    assert_eq!(
        form.get_language_index(),
        1,
        "the pick records the language"
    );
    assert!(
        !form.get_test_language_open(),
        "the language list closes after a pick"
    );

    // Footer: About, Report Issue and Settings.
    assert!(form.get_test_has_about(), "the footer has About");
    assert!(
        form.get_test_has_report_issue(),
        "the footer has Report Issue"
    );
    assert!(form.get_test_has_settings(), "the footer has Settings");

    // Areas: one row per area with no tab, read from the model the drawer's rows are drawn from.
    let areas: Vec<RailItem> = app_shell::more_areas()
        .iter()
        .map(|area| RailItem {
            label: area.title().into(),
            icon: Default::default(),
        })
        .collect();
    form.set_areas(ModelRc::new(VecModel::from(areas)));
    settle();
    let rows = form.get_areas();
    let labels: Vec<String> = (0..rows.row_count())
        .map(|i| rows.row_data(i).expect("area row").label.to_string())
        .collect();
    for title in MORE_TITLES {
        assert!(
            labels.iter().any(|label| label.as_str() == title),
            "the areas list has {title}"
        );
    }
}

/// The window at phone width: the More tab opens the drawer, the drawer lists the six areas, and the footer opens each
/// shell screen. Picking an area shows its tree in the drawer.
pub fn more_drawer_window_routes_areas_and_footer_on_phone() {
    crate::support::init();

    // The six non-tab areas: nav order, Home, Chat, Projects and Company left out.
    let more = app_shell::more_areas();
    let titles: Vec<&str> = more.iter().map(|area| area.title()).collect();
    assert_eq!(titles, MORE_TITLES, "the More drawer lists the six areas");
    assert!(
        !more.contains(&Area::Home) && !more.contains(&Area::Chat),
        "the tabbed areas are not listed again"
    );

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");

    let rows = ui.get_more_areas();
    let row_titles: Vec<String> = (0..rows.row_count())
        .map(|i| rows.row_data(i).expect("area row").label.to_string())
        .collect();
    assert_eq!(
        row_titles, MORE_TITLES,
        "the window's More model has six rows"
    );

    // Opening the More drawer: the More panel is shown, not the area tree. The drawer is parked off the left edge until
    // it opens, so the timer checks come after the open.
    assert!(!ui.get_drawer_open());
    ui.invoke_tab_activated(4);
    assert!(ui.get_drawer_open(), "the More tab opens the drawer");
    assert!(ui.get_more_view(), "the More tab shows the More panel");
    // The slide-in is an animation on the mock clock: run it to the end so the panel is on screen.
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));

    // The timer is in the drawer and shows only while it runs (the drawer shows the timer text exactly when running).
    ui.set_timer_text("1:02:03".into());
    assert!(
        !ui.get_timer_running(),
        "the timer is hidden when not running"
    );
    ui.set_timer_running(true);
    assert!(ui.get_timer_running(), "the timer shows while running");
    ui.set_timer_running(false);

    // The three phone checks below read the app window's presence hooks (real drawn state).
    assert!(ui.get_test_has_record(), "the More panel has Record");
    assert_eq!(
        ui.get_theme_label().as_str(),
        "Theme: light",
        "the More panel has Theme"
    );
    assert!(!ui.get_test_has_fullscreen(), "no Fullscreen on a phone");
    assert!(!ui.get_test_has_shortcuts(), "no Shortcuts on a phone");

    // Theme cycles light, dark, system, from the drawer as from the status bar.
    ui.invoke_theme_clicked();
    assert_eq!(ui.get_theme_label().as_str(), "Theme: dark");

    // Record and Language use the same callbacks as the status bar. The Language row only opens its list, so the pick
    // is sent to the language callback directly.
    ui.invoke_record_clicked();
    ui.invoke_language_picked(1);
    assert_eq!(ui.get_language_index(), 1, "a language pick is recorded");

    // Choosing an area shows its tree in the drawer, and the drawer stays open. People is the first More area.
    ui.invoke_more_area_activated(0);
    assert!(!ui.get_more_view(), "a pick leaves the More panel");
    assert!(
        ui.get_drawer_open(),
        "the drawer stays open on the area tree"
    );
    assert_eq!(ui.get_sidebar_title().as_str(), "People");

    // The footer: each opens its shell screen and closes the drawer. Reopen the drawer between picks.
    ui.invoke_tab_activated(4);
    ui.invoke_test_pick_more_footer(0);
    assert!(ui.get_about_open(), "About opens its screen");
    assert!(!ui.get_drawer_open(), "a footer pick closes the drawer");
    assert!(!ui.get_report_open() && !ui.get_settings_open());

    ui.invoke_tab_activated(4);
    ui.invoke_test_pick_more_footer(1);
    assert!(ui.get_report_open(), "Report Issue opens its screen");
    assert!(!ui.get_about_open(), "only one shell screen is open");

    ui.invoke_tab_activated(4);
    ui.invoke_test_pick_more_footer(2);
    assert!(ui.get_settings_open(), "Settings opens its screen");
    assert!(!ui.get_report_open(), "only one shell screen is open");

    // Background jobs show in the drawer while there are jobs (the jobs row is shown exactly when the list is not empty).
    ui.invoke_tab_activated(4);
    assert_eq!(ui.get_jobs().row_count(), 0, "no jobs row while idle");
    ui.set_jobs(ModelRc::new(VecModel::from(vec![AppJob {
        title: "Export payroll".into(),
        state: "Queued".into(),
    }])));
    assert!(
        ui.get_jobs().row_count() > 0,
        "the jobs row shows in the drawer"
    );
}

/// The phone drawer's back row (tail C2): an area's tree opened from More has a back row ("Back to More") that returns to
/// the More list. The drawer stays open on the More list.
pub fn more_drawer_back_row_returns_from_an_area_to_the_more_list() {
    crate::support::init();

    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    let phone_size = LogicalSize::new(390.0, 844.0);
    ui.window().set_size(phone_size);
    ui.window()
        .dispatch_event(WindowEvent::Resized { size: phone_size });
    slint::platform::update_timers_and_animations();
    assert!(ui.get_phone(), "390 px is below the phone breakpoint");

    // More, then People: the area's tree replaces the More list, and the drawer stays open.
    ui.invoke_tab_activated(4);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    assert!(ui.get_more_view(), "the More list shows first");
    ui.invoke_more_area_activated(0);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    assert!(!ui.get_more_view(), "the area tree replaces the More list");
    assert_eq!(ui.get_sidebar_title().as_str(), "People");

    // The back row returns to the More list.
    ui.invoke_test_back_to_more();
    assert!(ui.get_more_view(), "the back row shows the More list again");
    assert!(ui.get_drawer_open(), "the drawer stays open");
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
    assert!(
        ui.get_more_view(),
        "the More list stays after the slide ends"
    );
}
