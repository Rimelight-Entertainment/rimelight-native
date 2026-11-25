//! Date rules of the date pickers (plain Rust): today, the ISO form of a date, the month titles, and the cells of one month
//! with the chosen date selected. The window glue (`calendar_glue!`) turns the cells into the generated `CalendarDay` model.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::calendar::{Date, Selection, day_flags};
use crate::picker::valid_date;

/// The English month names, for the picker's title.
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Today's date (UTC), from the system clock.
pub fn today() -> Date {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / 86_400);
    Date::new(1970, 1, 1)
        .expect("the epoch is a date")
        .add_days(i64::try_from(days).unwrap_or_default())
}

/// A `YYYY-MM-DD` value as a date, or `None` when it is empty or not a real date.
pub fn parse_iso(value: &str) -> Option<Date> {
    if value.is_empty() || !valid_date(value) {
        return None;
    }
    let mut parts = value.split('-').map(|part| part.parse::<i64>().ok());
    let year = parts.next()??;
    let month = parts.next()??;
    let day = parts.next()??;
    Date::new(
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    )
}

/// A date as `YYYY-MM-DD`.
pub fn iso(date: Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

/// The title of a month, as the picker shows it: `November 2026`.
pub fn month_title(shown: (i32, u32)) -> String {
    format!(
        "{} {}",
        MONTHS[(shown.1 as usize).saturating_sub(1) % 12],
        shown.0
    )
}

/// One cell of a picker's month, as plain values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarCell {
    pub day: i32,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
    pub in_range: bool,
}

/// The 42 cells of the month `shown`, with `chosen` selected (today is marked).
pub fn cells(shown: (i32, u32), chosen: Option<Date>) -> Vec<CalendarCell> {
    let mut selection = Selection::default();
    if let Some(date) = chosen {
        selection.pick_single(date);
    }
    day_flags(shown.0, shown.1, 0, today(), &selection)
        .into_iter()
        .map(|flags| CalendarCell {
            day: flags.cell.date.day as i32,
            in_month: flags.cell.in_month,
            today: flags.today,
            selected: flags.selected,
            in_range: flags.in_range,
        })
        .collect()
}

/// Expands, in the app crate, `calendar_cells`: the cells of a month as the generated `CalendarDay` model and its title.
#[macro_export]
macro_rules! calendar_glue {
    ($CalendarDay:ident) => {
        /// The 42 cells of the month `shown` as the window's model, and the month's title.
        pub fn calendar_cells(
            shown: (i32, u32),
            chosen: Option<$crate::calendar::Date>,
        ) -> (
            $crate::__slint::ModelRc<$CalendarDay>,
            $crate::__slint::SharedString,
        ) {
            let days: Vec<$CalendarDay> = $crate::dates::cells(shown, chosen)
                .into_iter()
                .map(|cell| $CalendarDay {
                    day: cell.day,
                    in_month: cell.in_month,
                    today: cell.today,
                    selected: cell.selected,
                    in_range: cell.in_range,
                })
                .collect();
            (
                $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(days)),
                $crate::dates::month_title(shown).into(),
            )
        }
    };
}
