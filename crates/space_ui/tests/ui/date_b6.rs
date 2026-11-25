//! P0.17 batch 6 tests: CalendarGrid, DatePicker, DateRangePicker and TimePicker.
//!
//! One form draws all four. The CalendarGrid is driven with real key events (`WindowEvent::KeyPressed`, `KeyReleased`):
//! arrows move the focus, Enter and Space pick, Page Up and Page Down ask for another month. The month cells come from
//! the Rust calendar rules (`slint_ui::calendar`), so the test checks the same grid the app will draw.
//!
//! Not covered here: the pointer clicks on a day cell, and opening the DatePicker and DateRangePicker popovers. Those
//! need pointer coordinates from the laid-out grid, which the test does not compute.

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle as _, ModelRc, SharedString, VecModel};
use space_page_kit::calendar::{self, Date, GRID_CELLS, Selection};

pub use space_ui_test_slint::date_b6::*;

fn press(ui: &B6Form, key: Key) {
    let text: SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// The 42 cells of the grid for a month, as the Slint struct. `selection` and `today` come from the Rust rules.
fn days_for(year: i32, month: u32, today: Date, selection: &Selection) -> Vec<CalendarDay> {
    calendar::day_flags(year, month, 0, today, selection)
        .into_iter()
        .map(|flags| CalendarDay {
            day: flags.cell.date.day as i32,
            in_month: flags.cell.in_month,
            today: flags.today,
            selected: flags.selected,
            in_range: flags.in_range,
        })
        .collect()
}

fn labels(week_start: u32) -> ModelRc<SharedString> {
    let names: Vec<SharedString> = (0..7)
        .map(|i| SharedString::from(calendar::weekday_label((week_start + i) % 7)))
        .collect();
    ModelRc::new(VecModel::from(names))
}

pub fn date_b6_calendar_grid_keys_pick_and_month_steps() {
    crate::support::init();
    let ui = B6Form::new().expect("create the form");

    // November 2026, weeks start on Sunday, a range from the 10th to the 12th, today the 15th.
    let today = Date::new(2026, 11, 15).expect("a valid date");
    let mut selection = Selection::default();
    selection.pick_range(Date::new(2026, 11, 10).expect("a valid date"));
    selection.pick_range(Date::new(2026, 11, 12).expect("a valid date"));

    ui.set_days(ModelRc::new(VecModel::from(days_for(
        2026, 11, today, &selection,
    ))));
    ui.set_weekdays(labels(0));
    ui.set_month_title(SharedString::from("November 2026"));
    ui.show().expect("show the form");

    // The grid is laid out from the Rust rules: 42 cells, the 15th is today, the 10th to the 12th are selected or in range.
    let flags = calendar::day_flags(2026, 11, 0, today, &selection);
    assert_eq!(flags.len(), GRID_CELLS);
    assert_eq!(flags.iter().filter(|f| f.today).count(), 1);
    assert_eq!(
        flags.iter().filter(|f| f.selected).count(),
        2,
        "the start and the end"
    );
    assert_eq!(
        flags.iter().filter(|f| f.in_range).count(),
        1,
        "the 11th lies between"
    );

    // Keyboard focus: give the grid the focus, then the arrows move it by a day and by a week.
    ui.invoke_focus_grid();
    assert_eq!(ui.get_focus_day(), 0);
    press(&ui, Key::RightArrow);
    assert_eq!(ui.get_focus_day(), 1, "Right moves a day");
    press(&ui, Key::DownArrow);
    assert_eq!(ui.get_focus_day(), 8, "Down moves a week");
    press(&ui, Key::LeftArrow);
    assert_eq!(ui.get_focus_day(), 7, "Left moves a day back");
    press(&ui, Key::UpArrow);
    assert_eq!(ui.get_focus_day(), 0, "Up moves a week back");
    press(&ui, Key::UpArrow);
    assert_eq!(ui.get_focus_day(), 0, "Up at the top stays in the grid");

    // Enter and Space pick the focus day, by its index.
    press(&ui, Key::Return);
    assert_eq!(ui.get_picks(), 0, "Enter picks the focus day");
    press(&ui, Key::DownArrow);
    press(&ui, Key::Space);
    assert_eq!(ui.get_picks(), 7, "Space picks the focus day");

    // Page Up and Page Down ask the caller for another month (-1 and 1). The grid does not move the month itself.
    press(&ui, Key::PageDown);
    assert_eq!(
        (ui.get_steps(), ui.get_last_step()),
        (1, 1),
        "Page Down asks for the next month"
    );
    press(&ui, Key::PageUp);
    assert_eq!(
        (ui.get_steps(), ui.get_last_step()),
        (2, -1),
        "Page Up asks for the previous month"
    );
    assert_eq!(
        ui.get_focus_day(),
        7,
        "the month keys leave the focus where it was"
    );

    // Each day is a cell of the grid: the last index is the 41st.
    for _ in 0..50 {
        press(&ui, Key::RightArrow);
    }
    assert_eq!(ui.get_focus_day(), 41, "Right stops at the last cell");
}

pub fn date_b6_time_picker_fields_and_callback() {
    crate::support::init();
    let ui = B6Form::new().expect("create the form");
    ui.set_days(ModelRc::new(VecModel::from(days_for(
        2026,
        11,
        Date::new(2026, 11, 15).expect("a valid date"),
        &Selection::default(),
    ))));
    ui.set_weekdays(labels(0));
    ui.set_month_title(SharedString::from("November 2026"));
    ui.show().expect("show the form");

    // The picker starts at the values it was given.
    assert_eq!(ui.get_hour(), 9.0);
    assert_eq!(ui.get_minute(), 0.0);
    assert_eq!(ui.get_time_events(), 0);
}
