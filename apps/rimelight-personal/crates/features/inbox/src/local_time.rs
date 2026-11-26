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

/// `Nov 22`: the list's date (the web shows month and day, no time), in the local zone.
pub fn month_day(value: &str) -> Option<String> {
    let day = rimelight_personal_core::local_date::local_day_of(value)?;
    Some(format!(
        "{} {}",
        MONTH_SHORT[(day.month.clamp(1, 12) - 1) as usize],
        day.day
    ))
}

/// Decodes standard base64 (as used in `data:` image URLs). Whitespace is skipped. `None` for bad input.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
