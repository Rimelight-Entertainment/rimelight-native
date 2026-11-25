//! Calendar days read the way Solid reads them: `new Date(value)` followed by the local
//! `getDate`/`getMonth`/`getFullYear` getters. A date-only string such as `2026-05-10` is UTC
//! midnight, then viewed in the device's time zone. Solid's "due today" check compares that
//! local day, so the port keeps the same reading.

/// A calendar day. `weekday` counts from Sunday = 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::Day;
    use jiff::{Timestamp, Zoned, civil, tz::TimeZone};

    fn day_of(date: civil::Date) -> Day {
        Day {
            year: i32::from(date.year()),
            month: date.month() as u32,
            day: date.day() as u32,
            weekday: date.weekday().to_sunday_zero_offset() as u32,
        }
    }

    /// Today in the device's time zone (`new Date()` with the local getters).
    pub fn today() -> Day {
        day_of(Zoned::now().date())
    }

    /// The UTC calendar date of now, as `YYYY-MM-DD` (`new Date().toISOString().split("T")[0]`).
    pub fn utc_today_iso() -> String {
        let date = Timestamp::now().to_zoned(TimeZone::UTC).date();
        format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
    }

    /// The local day that `new Date(value)` falls on. `None` when JavaScript would give an
    /// invalid date.
    pub fn local_day_of(value: &str) -> Option<Day> {
        if let Ok(instant) = value.parse::<Timestamp>() {
            return Some(day_of(instant.to_zoned(TimeZone::system()).date()));
        }
        if let Ok(local) = value.parse::<civil::DateTime>() {
            return local
                .to_zoned(TimeZone::system())
                .ok()
                .map(|zoned| day_of(zoned.date()));
        }
        let date = value.parse::<civil::Date>().ok()?;
        let midnight_utc = date.at(0, 0, 0, 0).to_zoned(TimeZone::UTC).ok()?;
        Some(day_of(
            midnight_utc.with_time_zone(TimeZone::system()).date(),
        ))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Day;
    use js_sys::Date;

    fn day_of(date: &Date) -> Day {
        Day {
            year: date.get_full_year() as i32,
            month: date.get_month() + 1,
            day: date.get_date(),
            weekday: date.get_day(),
        }
    }

    /// Today in the browser's time zone.
    pub fn today() -> Day {
        day_of(&Date::new_0())
    }

    /// The UTC calendar date of now, as `YYYY-MM-DD`.
    pub fn utc_today_iso() -> String {
        let iso = String::from(Date::new_0().to_iso_string());
        iso.get(..10).unwrap_or_default().to_owned()
    }

    /// The local day that `new Date(value)` falls on. `None` for an invalid date.
    pub fn local_day_of(value: &str) -> Option<Day> {
        let date = Date::new(&value.into());
        if date.get_time().is_nan() {
            None
        } else {
            Some(day_of(&date))
        }
    }
}

pub use imp::{local_day_of, today, utc_today_iso};

#[cfg(test)]
mod tests {
    use super::{Day, local_day_of};

    #[test]
    fn invalid_text_has_no_day() {
        assert_eq!(local_day_of("not a date"), None);
    }

    #[test]
    fn date_only_text_is_read_as_a_day() {
        // A date-only string is UTC midnight, so the local day is the same date or the day before.
        let day = local_day_of("2026-05-10").expect("valid date");
        assert!(matches!((day.year, day.month), (2026, 5)));
        assert!(matches!(day.day, 9 | 10));
    }

    #[test]
    fn weekday_is_sunday_based() {
        let day: Day = local_day_of("2026-05-10T12:00:00Z").expect("valid timestamp");
        assert!(day.weekday < 7);
    }
}
