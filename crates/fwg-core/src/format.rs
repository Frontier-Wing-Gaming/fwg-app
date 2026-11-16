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

/// An RFC 3339 time ("2026-11-16T21:22:21Z") → "Nov 16, 2026, 4:22 PM" in local time.
/// Anything else falls back to [`date_label`].
pub fn datetime_label(value: &str) -> String {
    match DateTime::parse_from_rfc3339(value.trim()) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%b %-d, %Y, %-I:%M %p")
            .to_string(),
        Err(_) => date_label(value),
    }
}

/// Milliseconds since the epoch → "Nov 6, 2026, 7:05 PM" (for "Offline copy · updated …").
pub fn timestamp_label(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%b %-d, %Y, %-I:%M %p").to_string(),
        None => String::new(),
    }
}

/// 5800000000 → "5,800,000,000" (XP and levels on the RuneScape leaderboard).
pub fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(5_800_000_000), "5,800,000,000");
        assert_eq!(thousands(-1_234_567), "-1,234,567");
    }

    #[test]
    fn dates() {
        assert_eq!(date_label("2026-11-01 18:00:00"), "Nov 1, 2026");
        assert_eq!(date_label("2026-03-09"), "Mar 9, 2026");
        assert_eq!(date_label("soon"), "soon");
        // RFC 3339 is converted to local time, so only check the year.
        assert!(date_label("2026-07-04T12:00:00Z").ends_with("2026"));
        assert!(timestamp_label(1_762_400_000_000).contains("2025"));
        assert!(datetime_label("2026-11-16T21:22:21Z").contains(", 2026, "));
        assert_eq!(datetime_label("2026-03-09"), "Mar 9, 2026");
    }
}
