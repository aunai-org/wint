//! Minimal UTC date/time helpers (Unix milliseconds <-> ISO 8601), so the
//! library needs no date-time dependency.

/// Days since 1970-01-01 for a proleptic Gregorian civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// Formats Unix milliseconds as `YYYY-MM-DDTHH:MMZ` (UTC).
pub fn format_utc(ms: i64) -> String {
    let minutes = ms.div_euclid(60_000);
    let (days, minute_of_day) = (minutes.div_euclid(1440), minutes.rem_euclid(1440));
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}Z",
        minute_of_day / 60,
        minute_of_day % 60
    )
}

/// Parses `YYYY-MM-DDTHH:MM[:SS[.fff]]` followed by `Z` or a `±HH:MM` offset
/// (a space may replace `T`) into Unix milliseconds. A timestamp with no
/// zone designator is rejected rather than guessed.
pub fn parse_utc(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (year, month, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    if d.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let offset_minutes = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (h, m) = zone[1..].split_once(':')?;
            let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
            if h > 23 || m > 59 {
                return None;
            }
            sign * (h * 60 + m)
        }
    };
    let mut c = clock.split(':');
    let hour: i64 = c.next()?.parse().ok()?;
    let minute: i64 = c.next()?.parse().ok()?;
    let seconds: f64 = match c.next() {
        Some(s) => s.parse().ok()?,
        None => 0.0,
    };
    if c.next().is_some() || hour > 23 || minute > 59 || !(0.0..60.0).contains(&seconds) {
        return None;
    }
    let day_ms = days_from_civil(year, month, day) * 86_400_000;
    let clock_ms = (hour * 3600 + minute * 60) * 1000 + (seconds * 1000.0).round() as i64;
    Some(day_ms + clock_ms - offset_minutes * 60_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_known_instants() {
        assert_eq!(format_utc(0), "1970-01-01T00:00Z");
        assert_eq!(format_utc(1_790_000_000_000), "2026-09-21T14:13Z");
        assert_eq!(format_utc(-60_000), "1969-12-31T23:59Z");
        assert_eq!(format_utc(951_782_400_000), "2000-02-29T00:00Z");
    }
    #[test]
    fn parses_zones_and_round_trips() {
        assert_eq!(parse_utc("1970-01-01T00:00Z"), Some(0));
        assert_eq!(parse_utc("2000-02-29T00:00:00Z"), Some(951_782_400_000));
        assert_eq!(
            parse_utc("2026-09-21T10:00:00+02:00"),
            parse_utc("2026-09-21T08:00Z")
        );
        assert_eq!(
            parse_utc("2026-09-21 08:00:30.5Z"),
            Some(parse_utc("2026-09-21T08:00Z").unwrap() + 30_500)
        );
        for ms in [0, 1_790_000_040_000, -86_400_000, 4_102_444_800_000] {
            assert_eq!(parse_utc(&format_utc(ms)), Some(ms));
        }
    }
    #[test]
    fn rejects_bad_timestamps() {
        for bad in [
            "2026-09-21T08:00",
            "2026-13-01T00:00Z",
            "2026-02-30T00:00Z",
            "2026-09-21T25:00Z",
            "yesterday",
            "2026-09-21T08:00+99:00",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
    }
}
