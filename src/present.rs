//! Optional wording for results: text, rounding and units.
//!
//! **The engine never calls this module.** It returns facts (numbers, enums, identifiers chosen by the
//! plan's author) and decisions; how they are worded, rounded, colored or translated is up to the
//! application. These helpers are a convenient default for Rust programs such as the command-line
//! tool. A web page, another language or a different rounding can ignore them and format the same
//! data itself, which is what the browser demo does. Nothing in the engine's results depends on
//! anything here.

use crate::units::canonical_unit;
use crate::{ClockSpan, Comparison, Evidence, Expectation, Preference};

/// A number for people: at most 4 decimals, trailing zeros trimmed (`5.5556`, `20`).
pub fn format_number(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Minutes after midnight as `HH:MM` (1440 becomes `24:00`).
pub fn format_clock(minutes: u16) -> String {
    crate::schedule::format_clock(minutes)
}

/// A UTC offset in minutes as `UTC+02:00` / `UTC-05:30`.
pub fn format_offset(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", abs / 60, abs % 60)
}

/// Unix milliseconds as `YYYY-MM-DDTHH:MMZ` (UTC).
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

/// The conventional symbol for a comparison.
pub fn comparison_symbol(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::LessThan => "<",
        Comparison::LessThanOrEqual => "<=",
        Comparison::GreaterThan => ">",
        Comparison::GreaterThanOrEqual => ">=",
        Comparison::Equal => "==",
    }
}

/// What a piece of evidence was checked against, e.g. `<= 10`, `minimize: ideal 2, scale 8` or
/// `within 09:00-12:00 local`.
pub fn describe_expectation(expectation: &Expectation) -> String {
    match expectation {
        Expectation::Comparison {
            comparison,
            threshold,
        } => format!(
            "{} {}",
            comparison_symbol(*comparison),
            format_number(*threshold)
        ),
        Expectation::Preference { preference } => match preference {
            Preference::Minimize { ideal, scale } => format!(
                "minimize: ideal {}, scale {}",
                format_number(*ideal),
                format_number(*scale)
            ),
            Preference::Maximize { ideal, scale } => format!(
                "maximize: ideal {}, scale {}",
                format_number(*ideal),
                format_number(*scale)
            ),
            Preference::Range { min, max, scale } => format!(
                "range: {}..={}, scale {}",
                format_number(*min),
                format_number(*max),
                format_number(*scale)
            ),
        },
        Expectation::ClockWindow {
            from_minute,
            to_minute,
        } => format!(
            "within {}-{} local",
            format_clock(*from_minute),
            format_clock(*to_minute)
        ),
    }
}

/// A clock span, e.g. `09:00 to 11:00 local (UTC+02:00)`.
pub fn describe_clock(span: &ClockSpan) -> String {
    format!(
        "{} to {} local ({})",
        format_clock(span.start_minute),
        format_clock(span.end_minute),
        format_offset(span.utc_offset_minutes)
    )
}

/// A reading with its canonical unit and at most 2 decimals (`10.29 m/s`), or `missing`.
pub fn format_value(metric: &str, value: Option<f64>) -> String {
    match value {
        None => "missing".to_string(),
        Some(v) => {
            let number = format!("{v:.2}");
            let number = number.trim_end_matches('0').trim_end_matches('.');
            match canonical_unit(metric) {
                Some(unit) => format!("{number} {unit}"),
                None => number.to_string(),
            }
        }
    }
}

/// What to show as the reading of a piece of evidence: the clock span for a time-of-day check,
/// otherwise the value with its unit.
pub fn describe_reading(evidence: &Evidence) -> String {
    match &evidence.clock {
        Some(span) => describe_clock(span),
        None => format_value(&evidence.metric, evidence.actual),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wording_is_stable() {
        let cmp = |c, t| Expectation::Comparison {
            comparison: c,
            threshold: t,
        };
        assert_eq!(
            describe_expectation(&cmp(Comparison::LessThanOrEqual, 20.0)),
            "<= 20"
        );
        assert_eq!(
            describe_expectation(&cmp(Comparison::LessThan, 20.0 / 3.6)),
            "< 5.5556"
        );
        assert_eq!(describe_expectation(&cmp(Comparison::Equal, 1.0)), "== 1");
        let pref = |p| Expectation::Preference { preference: p };
        assert_eq!(
            describe_expectation(&pref(Preference::Minimize {
                ideal: 2.0,
                scale: 8.0
            })),
            "minimize: ideal 2, scale 8"
        );
        assert_eq!(
            describe_expectation(&pref(Preference::Range {
                min: 15.0,
                max: 25.0,
                scale: 10.0
            })),
            "range: 15..=25, scale 10"
        );
        assert_eq!(
            describe_expectation(&Expectation::ClockWindow {
                from_minute: 540,
                to_minute: 720
            }),
            "within 09:00-12:00 local"
        );
        let span = ClockSpan {
            start_minute: 540,
            end_minute: 660,
            utc_offset_minutes: 120,
        };
        assert_eq!(describe_clock(&span), "09:00 to 11:00 local (UTC+02:00)");
    }

    #[test]
    fn numbers_values_and_units() {
        assert_eq!(format_number(20.0), "20");
        assert_eq!(format_number(5.55555555), "5.5556");
        assert_eq!(format_value("wind_speed", Some(10.0)), "10 m/s");
        assert_eq!(format_value("wind_speed", Some(10.2889)), "10.29 m/s");
        assert_eq!(format_value("custom", Some(3.5)), "3.5");
        assert_eq!(format_value("wind_speed", None), "missing");
    }

    #[test]
    fn clocks_offsets_and_instants() {
        assert_eq!(format_clock(1440), "24:00");
        assert_eq!(format_offset(120), "UTC+02:00");
        assert_eq!(format_offset(-330), "UTC-05:30");
        assert_eq!(format_offset(0), "UTC+00:00");
        assert_eq!(format_utc(0), "1970-01-01T00:00Z");
        assert_eq!(format_utc(1_790_000_000_000), "2026-09-21T14:13Z");
        assert_eq!(format_utc(-60_000), "1969-12-31T23:59Z");
        assert_eq!(format_utc(951_782_400_000), "2000-02-29T00:00Z");
    }
}
