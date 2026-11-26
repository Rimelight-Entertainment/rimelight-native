//! The viewer's local clock time for an ISO instant. Native builds use jiff with the system zone (as
//! `rimelight_personal_core::local_date` does); the browser build uses `js_sys::Date`, which is also the local zone.

/// Local `(hour, minute)` of an ISO instant or local date-time. `None` when the text does not parse.
#[cfg(not(target_arch = "wasm32"))]
pub fn local_hm(value: &str) -> Option<(u32, u32)> {
    use jiff::tz::TimeZone;
    use jiff::{Timestamp, civil};
    if let Ok(instant) = value.parse::<Timestamp>() {
        let zoned = instant.to_zoned(TimeZone::system());
        return Some((zoned.hour() as u32, zoned.minute() as u32));
    }
    let local = value.parse::<civil::DateTime>().ok()?;
    let zoned = local.to_zoned(TimeZone::system()).ok()?;
    Some((zoned.hour() as u32, zoned.minute() as u32))
}

/// Local `(hour, minute)` of an ISO instant (browser build).
#[cfg(target_arch = "wasm32")]
pub fn local_hm(value: &str) -> Option<(u32, u32)> {
    let date = js_sys::Date::new(&value.into());
    if date.get_time().is_nan() {
        None
    } else {
        Some((date.get_hours(), date.get_minutes()))
    }
}

/// `Nov 22, 10:00 AM`: the en-US form of the web's `toLocaleDateString` with month, day, hour and minute.
pub fn month_day_time(value: &str) -> Option<String> {
    let day = rimelight_personal_core::local_date::local_day_of(value)?;
    let (hour, minute) = local_hm(value)?;
    let (h12, suffix) = match hour % 12 {
        0 => (12, if hour < 12 { "AM" } else { "PM" }),
        h => (h, if hour < 12 { "AM" } else { "PM" }),
    };
    Some(format!(
        "{} {}, {h12:02}:{minute:02} {suffix}",
        MONTH_SHORT[(day.month.clamp(1, 12) - 1) as usize],
        day.day
    ))
}

const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The ISO UTC instant for a local calendar date and time (the picker's value, as the web's datetime input gives).
/// `None` when the date does not exist.
#[cfg(not(target_arch = "wasm32"))]
pub fn local_to_iso(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<String> {
    use jiff::civil;
    use jiff::tz::TimeZone;
    let local = civil::date(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .at(i8::try_from(hour).ok()?, i8::try_from(minute).ok()?, 0, 0);
    let zoned = local.to_zoned(TimeZone::system()).ok()?;
    let instant = zoned.timestamp().to_string();
    // jiff prints `2026-11-22T10:00:00.000Z`-style UTC text; keep the millisecond form the API parses.
    Some(instant.replace("Z", ".000Z").replace(".000.000Z", ".000Z"))
}

/// The ISO UTC instant for a local calendar date and time (browser build).
#[cfg(target_arch = "wasm32")]
pub fn local_to_iso(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<String> {
    let date = js_sys::Date::new_with_year_month_day_hr_min(
        year as u32,
        (month as i32) - 1,
        day as i32,
        hour as i32,
        minute as i32,
    );
    if date.get_time().is_nan() {
        None
    } else {
        Some(String::from(date.to_iso_string()))
    }
}
