//! Calendar rules (P0.17 batch 6): date arithmetic, the month grid, the day flags and the date range selection.
//!
//! Pure functions with no Slint types, so the CalendarGrid, DatePicker, TimePicker and DateRangePicker components draw
//! what these return. Locale parts are data: the week start is passed in (see [`week_start`]), and the month and weekday
//! names are English until the i18n layer exists (P0.22).

/// A calendar date in the proleptic Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// The date, or `None` when the month or the day is out of range for that year.
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        if (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day) {
            Some(Date { year, month, day })
        } else {
            None
        }
    }

    /// The day `n` days after this one (negative goes back).
    pub fn add_days(self, n: i64) -> Date {
        civil_from_days(days_from_civil(self) + n)
    }

    /// The same day `n` months later, with the day clamped to the length of the new month (31 Jan + 1 month = 28 or 29 Feb).
    pub fn add_months(self, n: i32) -> Date {
        let index = self.year * 12 + (self.month as i32 - 1) + n;
        let year = index.div_euclid(12);
        let month = index.rem_euclid(12) as u32 + 1;
        let day = self.day.min(days_in_month(year, month));
        Date { year, month, day }
    }

    /// The weekday, 0 for Sunday through 6 for Saturday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (4).
        (days_from_civil(self) + 4).rem_euclid(7) as u32
    }
}

/// True for leap years in the Gregorian calendar.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days in `month` (1 to 12) of `year`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        _ => 28,
    }
}

/// English month names, January first.
pub const MONTH_NAMES: [&str; 12] = [
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

/// The two-letter weekday label for `weekday` (0 = Sunday).
pub fn weekday_label(weekday: u32) -> &'static str {
    ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"][(weekday % 7) as usize]
}

/// The first day of the week for a locale tag: 0 (Sunday) for the United States and Brazil, 1 (Monday) elsewhere.
/// This is a small table, not a full locale database; add a locale here when a screen needs it.
pub fn week_start(locale: &str) -> u32 {
    match locale {
        "en-US" | "pt-BR" => 0,
        _ => 1,
    }
}

/// One cell of the month grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub date: Date,
    /// False for the days from the neighbouring months that fill the first and last weeks.
    pub in_month: bool,
}

/// The number of cells in the grid: six weeks, so every month fits.
pub const GRID_CELLS: usize = 42;

/// The 42 cells of the month grid for `year` and `month`, starting on `week_start` (0 Sunday or 1 Monday).
pub fn month_grid(year: i32, month: u32, week_start: u32) -> Vec<Cell> {
    let first = Date {
        year,
        month,
        day: 1,
    };
    let offset = (first.weekday() + 7 - week_start % 7) % 7;
    let start = first.add_days(-i64::from(offset));
    (0..GRID_CELLS)
        .map(|i| {
            let date = start.add_days(i as i64);
            Cell {
                date,
                in_month: date.year == year && date.month == month,
            }
        })
        .collect()
}

/// The date selection of a calendar: one date for a DatePicker, or a start and an end for a DateRangePicker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub start: Option<Date>,
    pub end: Option<Date>,
}

impl Selection {
    /// A single date: picking a day sets the start and clears the end.
    pub fn pick_single(&mut self, date: Date) {
        self.start = Some(date);
        self.end = None;
    }

    /// A range: the first pick sets the start, the second sets the end (swapped when it comes before the start).
    /// A pick after a complete range starts a new one.
    pub fn pick_range(&mut self, date: Date) {
        match (self.start, self.end) {
            (Some(start), None) => {
                if date < start {
                    self.start = Some(date);
                    self.end = Some(start);
                } else {
                    self.end = Some(date);
                }
            }
            _ => {
                self.start = Some(date);
                self.end = None;
            }
        }
    }

    /// True when `date` is the start or the end.
    pub fn is_endpoint(&self, date: Date) -> bool {
        self.start == Some(date) || self.end == Some(date)
    }

    /// True when `date` lies strictly between the start and the end (both must be set).
    pub fn is_between(&self, date: Date) -> bool {
        match (self.start, self.end) {
            (Some(start), Some(end)) => date > start && date < end,
            _ => false,
        }
    }
}

/// What a cell of the grid shows: the cell, whether it is today, and how it is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayFlags {
    pub cell: Cell,
    pub today: bool,
    pub selected: bool,
    pub in_range: bool,
}

/// The flags of every cell of the month grid, for the given month, today and selection.
pub fn day_flags(
    year: i32,
    month: u32,
    week_start: u32,
    today: Date,
    selection: &Selection,
) -> Vec<DayFlags> {
    month_grid(year, month, week_start)
        .into_iter()
        .map(|cell| DayFlags {
            cell,
            today: cell.date == today,
            selected: selection.is_endpoint(cell.date),
            in_range: selection.is_between(cell.date),
        })
        .collect()
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(date: Date) -> i64 {
    let y = i64::from(date.year) - i64::from(date.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(date.month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(date.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> Date {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Date { year, month, day }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: i32, month: u32, day: u32) -> Date {
        Date::new(year, month, day).expect("a valid test date")
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2026));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2000));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 11), 30);
    }

    #[test]
    fn new_rejects_impossible_dates() {
        assert_eq!(Date::new(2026, 2, 29), None);
        assert_eq!(Date::new(2026, 13, 1), None);
        assert_eq!(Date::new(2026, 4, 31), None);
        assert!(Date::new(2024, 2, 29).is_some());
    }

    #[test]
    fn weekdays_are_correct() {
        assert_eq!(d(1970, 1, 1).weekday(), 4, "1970-01-01 was a Thursday");
        assert_eq!(d(2026, 11, 15).weekday(), 0, "2026-11-15 is a Sunday");
        assert_eq!(d(2026, 11, 16).weekday(), 1);
        assert_eq!(d(2000, 2, 29).weekday(), 2, "2000-02-29 was a Tuesday");
    }

    #[test]
    fn add_days_crosses_month_and_year_boundaries() {
        assert_eq!(d(2026, 12, 31).add_days(1), d(2027, 1, 1));
        assert_eq!(d(2024, 2, 28).add_days(1), d(2024, 2, 29));
        assert_eq!(d(2026, 3, 1).add_days(-1), d(2026, 2, 28));
        assert_eq!(d(2026, 11, 15).add_days(0), d(2026, 11, 15));
    }

    #[test]
    fn add_months_clamps_the_day() {
        assert_eq!(d(2026, 1, 31).add_months(1), d(2026, 2, 28));
        assert_eq!(d(2024, 1, 31).add_months(1), d(2024, 2, 29));
        assert_eq!(d(2026, 11, 15).add_months(-12), d(2025, 11, 15));
        assert_eq!(d(2026, 1, 15).add_months(-1), d(2025, 12, 15));
    }

    #[test]
    fn week_start_by_locale() {
        assert_eq!(week_start("en-US"), 0);
        assert_eq!(week_start("pt-BR"), 0);
        assert_eq!(week_start("en-GB"), 1);
    }

    #[test]
    fn month_grid_has_42_cells_and_starts_on_the_week_start() {
        // November 2026 starts on a Sunday.
        let sunday = month_grid(2026, 11, 0);
        assert_eq!(sunday.len(), GRID_CELLS);
        assert_eq!(sunday[0].date, d(2026, 11, 1));
        assert!(sunday[0].in_month);

        // Monday start: the Sunday 1 November moves to the end of the previous week.
        let monday = month_grid(2026, 11, 1);
        assert_eq!(monday[0].date, d(2026, 10, 26));
        assert!(!monday[0].in_month);
        assert_eq!(monday[6].date, d(2026, 11, 1));
        assert!(monday[6].in_month);
    }

    #[test]
    fn month_grid_marks_only_the_month_days() {
        for year_month in [(2026, 2), (2024, 2), (2026, 11), (2026, 3)] {
            let cells = month_grid(year_month.0, year_month.1, 0);
            let in_month = cells.iter().filter(|c| c.in_month).count();
            assert_eq!(in_month as u32, days_in_month(year_month.0, year_month.1));
            // The grid is contiguous: each cell is the day after the previous one.
            for pair in cells.windows(2) {
                assert_eq!(pair[1].date, pair[0].date.add_days(1));
            }
        }
    }

    #[test]
    fn single_pick_replaces_the_date() {
        let mut sel = Selection::default();
        sel.pick_single(d(2026, 11, 15));
        assert_eq!(sel.start, Some(d(2026, 11, 15)));
        assert_eq!(sel.end, None);
        sel.pick_single(d(2026, 12, 1));
        assert_eq!(sel.start, Some(d(2026, 12, 1)));
    }

    #[test]
    fn range_pick_sets_start_then_end_and_swaps_when_earlier() {
        let mut sel = Selection::default();
        sel.pick_range(d(2026, 11, 10));
        assert_eq!((sel.start, sel.end), (Some(d(2026, 11, 10)), None));
        sel.pick_range(d(2026, 11, 20));
        assert_eq!(
            (sel.start, sel.end),
            (Some(d(2026, 11, 10)), Some(d(2026, 11, 20)))
        );

        // A pick before the start makes it the start, and the old start the end.
        let mut back = Selection::default();
        back.pick_range(d(2026, 11, 20));
        back.pick_range(d(2026, 11, 10));
        assert_eq!(
            (back.start, back.end),
            (Some(d(2026, 11, 10)), Some(d(2026, 11, 20)))
        );

        // A pick after a complete range starts a new one.
        sel.pick_range(d(2026, 12, 2));
        assert_eq!((sel.start, sel.end), (Some(d(2026, 12, 2)), None));
    }

    #[test]
    fn range_flags_mark_the_ends_and_the_days_between() {
        let mut sel = Selection::default();
        sel.pick_range(d(2026, 11, 10));
        sel.pick_range(d(2026, 11, 12));
        assert!(sel.is_endpoint(d(2026, 11, 10)));
        assert!(sel.is_endpoint(d(2026, 11, 12)));
        assert!(sel.is_between(d(2026, 11, 11)));
        assert!(!sel.is_between(d(2026, 11, 10)));
        assert!(!sel.is_between(d(2026, 11, 13)));
    }

    #[test]
    fn day_flags_mark_today_and_the_selection() {
        let today = d(2026, 11, 15);
        let mut sel = Selection::default();
        sel.pick_single(d(2026, 11, 3));
        let flags = day_flags(2026, 11, 0, today, &sel);
        assert_eq!(flags.len(), GRID_CELLS);
        let todays: Vec<_> = flags.iter().filter(|f| f.today).collect();
        assert_eq!(todays.len(), 1);
        assert_eq!(todays[0].cell.date, today);
        assert_eq!(flags.iter().filter(|f| f.selected).count(), 1);
    }
}
