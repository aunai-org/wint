//! CSV adapter.
//!
//! ```text
//! timestamp,wind_speed (km/h),temperature (°F),precipitation_probability
//! 2026-09-21T08:00Z,12,75,10
//! 2026-09-21T09:00Z,15,,15
//! ```
//!
//! * The first column is the timestamp: ISO 8601 with a zone (`Z` or
//!   `±HH:MM`), or a bare integer (Unix milliseconds if it has 12+ digits,
//!   otherwise Unix seconds).
//! * Every other header is `metric` or `metric (unit)` / `metric[unit]`. A
//!   unit converts that column to the metric's canonical unit.
//! * A column named `utc_offset_minutes` is not a metric: it gives the local clock offset from UTC
//!   at each row (for example `60`, then `120` after a daylight-saving change), which time-of-day
//!   schedules use instead of one fixed offset. An empty cell uses the series' offset.
//! * An empty cell is a missing reading. Fields are split on commas and
//!   trimmed; quoted fields are not supported.
//! * The cadence is the gap between the first two rows; the series must then
//!   be exactly regular, like every [`Series`].

use super::AdapterError;
use crate::units::Unit;
use crate::{Observation, Series};
use std::collections::BTreeMap;

const OFFSET_COLUMN: &str = "utc_offset_minutes";

/// Splits `name (unit)` / `name[unit]` into its parts.
fn split_header(header: &str) -> Option<(&str, Option<&str>)> {
    let header = header.trim();
    let open = header.find(['(', '[']);
    match open {
        None => Some((header, None)),
        Some(at) => {
            let close = match header.as_bytes()[at] {
                b'(' => ')',
                _ => ']',
            };
            let inner = header[at + 1..].strip_suffix(close)?;
            Some((header[..at].trim(), Some(inner)))
        }
    }
}

fn parse_timestamp(cell: &str) -> Option<i64> {
    if let Ok(n) = cell.parse::<i64>() {
        return Some(if n.abs() >= 100_000_000_000 {
            n
        } else {
            n.checked_mul(1000)?
        });
    }
    crate::time::parse_utc(cell)
}

/// Parses CSV text into a [`Series`].
pub fn parse(text: &str) -> Result<Series, AdapterError> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty());
    let (header_line, header) = lines
        .next()
        .ok_or_else(|| AdapterError::format("line 1", "empty input"))?;
    let mut columns = Vec::new();
    let mut offset_column = None;
    let mut units = BTreeMap::new();
    for (index, cell) in header.split(',').enumerate().skip(1) {
        let location = format!("line {header_line}, column {}", index + 1);
        let (name, unit) = split_header(cell)
            .filter(|(name, _)| !name.is_empty())
            .ok_or_else(|| {
                AdapterError::format(&location, format!("bad header `{}`", cell.trim()))
            })?;
        if columns.contains(&name) {
            return Err(AdapterError::format(
                location,
                format!("duplicate metric `{name}`"),
            ));
        }
        if let Some(symbol) = unit {
            let unit = Unit::parse(symbol).ok_or_else(|| {
                AdapterError::format(&location, format!("unknown unit `{symbol}`"))
            })?;
            units.insert(name.to_string(), unit);
        }
        if name == OFFSET_COLUMN {
            if unit.is_some() {
                return Err(AdapterError::format(
                    location,
                    format!("`{OFFSET_COLUMN}` takes no unit"),
                ));
            }
            offset_column = Some(columns.len());
        }
        columns.push(name);
    }
    if columns.is_empty() {
        return Err(AdapterError::format(
            format!("line {header_line}"),
            "expected a timestamp column followed by at least one metric column",
        ));
    }
    let mut observations = Vec::new();
    for (line, row) in lines {
        let cells: Vec<&str> = row.split(',').map(str::trim).collect();
        if cells.len() != columns.len() + 1 {
            return Err(AdapterError::format(
                format!("line {line}"),
                format!(
                    "expected {} fields, found {}",
                    columns.len() + 1,
                    cells.len()
                ),
            ));
        }
        let timestamp = parse_timestamp(cells[0]).ok_or_else(|| {
            AdapterError::format(
                format!("line {line}, column 1"),
                format!("bad timestamp `{}`", cells[0]),
            )
        })?;
        let mut observation = Observation::at(timestamp);
        for (index, (column, cell)) in columns.iter().zip(&cells[1..]).enumerate() {
            if cell.is_empty() {
                continue;
            }
            if offset_column == Some(index) {
                let minutes: i32 = cell.parse().map_err(|_| {
                    AdapterError::format(
                        format!("line {line}"),
                        format!("bad offset `{cell}` for `{OFFSET_COLUMN}` (whole minutes)"),
                    )
                })?;
                observation = observation.with_utc_offset(minutes);
                continue;
            }
            let value: f64 = cell.parse().map_err(|_| {
                AdapterError::format(
                    format!("line {line}"),
                    format!("bad number `{cell}` for `{column}`"),
                )
            })?;
            observation = observation.with(*column, value);
        }
        observations.push(observation);
    }
    if observations.len() < 2 {
        return Err(AdapterError::format(
            "input",
            "need at least two rows to infer the cadence",
        ));
    }
    let cadence = observations[1].timestamp_ms - observations[0].timestamp_ms;
    Ok(Series::with_units(cadence, observations, &units)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ValidationError;

    const SAMPLE: &str = "timestamp,wind_speed (km/h),temperature[F],custom\n\
        2026-09-21T08:00Z,36,212,1\n\
        2026-09-21T09:00Z,72,,2\n\
        2026-09-21T10:00Z,18,32,\n";

    #[test]
    fn parses_units_missing_cells_and_cadence() {
        let series = parse(SAMPLE).unwrap();
        assert_eq!(series.cadence_ms, 3_600_000);
        assert_eq!(series.observations.len(), 3);
        let first = &series.observations[0].values;
        assert!((first["wind_speed"] - 10.0).abs() < 1e-9);
        assert!((first["temperature"] - 100.0).abs() < 1e-9);
        assert_eq!(first["custom"], 1.0);
        assert!(!series.observations[1].values.contains_key("temperature"));
        assert!(!series.observations[2].values.contains_key("custom"));
    }
    #[test]
    fn offset_column_is_per_row_not_a_metric() {
        let text = "timestamp,wind_speed,utc_offset_minutes\n\
                    2026-03-29T00:00Z,1,60\n2026-03-29T01:00Z,2,120\n2026-03-29T02:00Z,3,\n";
        let series = parse(text).unwrap();
        let offsets: Vec<Option<i32>> = series
            .observations
            .iter()
            .map(|o| o.utc_offset_minutes)
            .collect();
        assert_eq!(offsets, [Some(60), Some(120), None]);
        assert!(!series.observations[0]
            .values
            .contains_key("utc_offset_minutes"));
        assert!(parse("t,wind_speed,utc_offset_minutes (min)\n0,1,60\n3600,2,60\n").is_err());
        assert!(parse("t,wind_speed,utc_offset_minutes\n0,1,abc\n3600,2,60\n").is_err());
        assert!(parse("t,wind_speed,utc_offset_minutes\n0,1,999\n3600,2,60\n").is_err());
    }
    #[test]
    fn accepts_epoch_seconds_and_millis_and_crlf() {
        let text = "t,wind_speed\r\n1790000000,1\r\n1790003600,2\r\n";
        assert_eq!(parse(text).unwrap().cadence_ms, 3_600_000);
        let text = "t,wind_speed\n1790000000000,1\n1790003600000,2\n";
        assert_eq!(
            parse(text).unwrap().observations[0].timestamp_ms,
            1_790_000_000_000
        );
    }
    #[test]
    fn reports_where_things_go_wrong() {
        let cases: &[(&str, &str)] = &[
            ("", "empty input"),
            ("timestamp\n", "at least one metric column"),
            ("t,wind_speed (furlongs)\n", "unknown unit"),
            ("t,a,a\n", "duplicate metric"),
            (
                "t,wind_speed\nnot-a-time,1\n",
                "line 2, column 1: bad timestamp",
            ),
            (
                "t,wind_speed\n2026-09-21T08:00Z,fast\n",
                "bad number `fast`",
            ),
            ("t,wind_speed\n2026-09-21T08:00Z\n", "expected 2 fields"),
            ("t,wind_speed\n2026-09-21T08:00Z,1\n", "two rows"),
        ];
        for (input, expected) in cases {
            let error = parse(input).unwrap_err().to_string();
            assert!(error.contains(expected), "{input:?} -> {error}");
        }
    }
    #[test]
    fn irregular_rows_fail_series_validation() {
        let text = "t,wind_speed\n2026-09-21T08:00Z,1\n2026-09-21T09:00Z,1\n2026-09-21T11:00Z,1\n";
        assert!(matches!(
            parse(text),
            Err(AdapterError::Invalid(
                ValidationError::IrregularTimestamp { .. }
            ))
        ));
    }
}
