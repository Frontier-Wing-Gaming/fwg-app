//! Date formatting for the UI, in the device's local time zone.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};

/// `published_at` from the API ("2026-11-01 18:00:00", RFC 3339, or a bare date) → "Nov 1, 2026".
/// Unparseable input is returned unchanged.
pub fn date_label(value: &str) -> String {
    let value = value.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Local).format("%b %-d, %Y").to_string();
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(value, fmt) {
            return dt.format("%b %-d, %Y").to_string();
        }
    }
    match value
        .get(..10)
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(date) => date.format("%b %-d, %Y").to_string(),
        None => value.to_owned(),
    }
}

/// Milliseconds since the epoch → "Nov 6, 2026, 7:05 PM" (for "Offline copy · updated …").
pub fn timestamp_label(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%b %-d, %Y, %-I:%M %p").to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_label("2026-11-01 18:00:00"), "Nov 1, 2026");
        assert_eq!(date_label("2026-03-09"), "Mar 9, 2026");
        assert_eq!(date_label("soon"), "soon");
        // RFC 3339 is converted to local time, so only check the year.
        assert!(date_label("2026-07-04T12:00:00Z").ends_with("2026"));
        assert!(timestamp_label(1_762_400_000_000).contains("2025"));
    }
}
