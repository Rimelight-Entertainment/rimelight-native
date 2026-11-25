//! Status bar left side (OQ18): the personal running timer and the background-jobs indicator with its popover. The bar is
//! built on its own (a form that binds its properties) and inside the window (`app_shell::build`), on the headless testing
//! backend. A popover's panel is a separate popup window, so the job list is drawn on its own form (`JobsMenu`) to check
//! the rows. The jobs are mock data: the tests set them.
//!
//! What is drawn is read through the `test-*` state of the bar and the list (`timer-label`, `jobs-label`, `row-count`),
//! and a click runs the callbacks the bar's own button runs (`click-jobs`).

use slint::{Model as _, ModelRc, VecModel};
use space_ui::slint_ui::{JobItem as AppJob, app_shell};
use space_ui::theme::ThemeMode;

pub use space_ui_test_slint::status_bar::*;

fn job(title: &str, state: &str) -> JobItem {
    JobItem {
        title: title.into(),
        state: state.into(),
    }
}

fn model(jobs: Vec<JobItem>) -> ModelRc<JobItem> {
    ModelRc::new(VecModel::from(jobs))
}

/// The title and state of row `i` of a job list.
fn row(jobs: &ModelRc<JobItem>, i: usize) -> (String, String) {
    let item = jobs.row_data(i).unwrap_or_else(|| panic!("no job row {i}"));
    (item.title.to_string(), item.state.to_string())
}

/// The timer and the jobs indicator on the bar, and the popover's open state.
pub fn status_bar_timer_and_jobs_indicator_headless() {
    crate::support::init();

    let form = StatusForm::new().expect("status form builds");

    // Timer: hidden while not running, even with text set; shown with its text once running.
    assert!(!form.get_timer_running());
    assert_eq!(
        form.get_timer_label().as_str(),
        "",
        "the timer is hidden when it is not running"
    );
    form.set_timer_running(true);
    assert_eq!(
        form.get_timer_label().as_str(),
        "1:02:03",
        "the timer shows its text while running"
    );
    form.set_timer_running(false);
    assert_eq!(
        form.get_timer_label().as_str(),
        "",
        "the timer hides again when stopped"
    );

    // Jobs: empty is idle, so no indicator.
    assert!(
        !form.get_jobs_indicator(),
        "the jobs indicator is hidden when there are no jobs"
    );

    // With two jobs the indicator shows, with the count.
    form.set_jobs(model(vec![
        job("Upload report.pdf", "40%"),
        job("Export payroll", "Queued"),
    ]));
    assert!(
        form.get_jobs_indicator(),
        "the jobs indicator shows when jobs are set"
    );
    assert_eq!(
        form.get_jobs_label().as_str(),
        "2 background jobs",
        "the jobs indicator shows the count, labelled as background jobs"
    );
    assert_eq!(form.get_jobs_count_text().as_str(), "2 jobs");
    assert!(!form.get_jobs_open(), "the popover starts closed");

    // A click on the indicator opens the popover; closing is done through the bar's close function (a click outside
    // closes it in the app, which the headless backend does not send).
    form.invoke_click_jobs();
    assert!(form.get_jobs_open(), "a click opens the jobs popover");
    form.invoke_close_jobs();
    assert!(!form.get_jobs_open(), "closing hides the jobs popover");

    // Idle again: the indicator goes away.
    form.set_jobs(model(Vec::new()));
    assert!(
        !form.get_jobs_indicator(),
        "the jobs indicator hides when the jobs are cleared"
    );
    assert_eq!(
        form.get_jobs_count_text().as_str(),
        "",
        "the count hides with the indicator"
    );
}

/// The popover's list: one row per job, with the title and the state.
pub fn status_bar_jobs_popover_lists_each_job_headless() {
    crate::support::init();

    let form = JobsForm::new().expect("jobs form builds");
    form.set_jobs(model(vec![
        job("Upload report.pdf", "40%"),
        job("Export payroll", "Queued"),
    ]));
    assert_eq!(form.get_rows(), 2, "the popover draws one row per job");
    assert_eq!(
        row(&form.get_jobs(), 0),
        ("Upload report.pdf".to_string(), "40%".to_string()),
        "the popover lists the first job with its state"
    );
    assert_eq!(
        row(&form.get_jobs(), 1),
        ("Export payroll".to_string(), "Queued".to_string()),
        "the popover lists the second job with its state"
    );

    form.set_jobs(model(vec![job("Export payroll", "Queued")]));
    assert_eq!(form.get_rows(), 1);
    assert_eq!(
        row(&form.get_jobs(), 0).0,
        "Export payroll",
        "a job that is removed leaves the list"
    );

    // The window: the timer and the indicator sit on the bar, and the shell sets them as properties. The rendered labels
    // of the bar are not reachable from here (no hook on the shell), so this checks the state the shell feeds the bar.
    let app = app_shell::build("/home", ThemeMode::Light).expect("window builds");
    let ui = &app.ui;
    assert!(!ui.get_timer_running(), "the timer is idle by default");
    assert_eq!(ui.get_jobs().row_count(), 0, "no jobs by default");

    ui.set_timer_text("1:02:03".into());
    ui.set_timer_running(true);
    assert!(ui.get_timer_running(), "the window's timer is running");
    assert_eq!(ui.get_timer_text().as_str(), "1:02:03");

    ui.set_jobs(ModelRc::new(VecModel::from(vec![AppJob {
        title: "Upload report.pdf".into(),
        state: "40%".into(),
    }])));
    assert_eq!(
        ui.get_jobs().row_count(),
        1,
        "the window is fed one job, so it shows the jobs indicator"
    );
}
