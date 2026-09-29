//! [Open-Meteo](https://open-meteo.com) forecast adapter (hourly data).
//!
//! This module is pure: [`request_url`] builds the query and [`parse`] turns
//! the JSON response into a [`Series`]. Performing the HTTP request is left to
//! the caller (the CLI does it behind the `net` feature), keeping the library
//! free of any network stack.
//!
//! Values are converted from the units the response declares in
//! `hourly_units` to the canonical vocabulary units, so it does not matter
//! which `wind_speed_unit` was requested. `null` readings become missing
//! values, which makes any hard constraint on that metric fail visibly.

use super::AdapterError;
use crate::units::Unit;
use crate::{Observation, Series};
use serde_json::Value;

/// `(Open-Meteo hourly variable, vocabulary metric)`.
pub const VARIABLES: &[(&str, &str)] = &[
    ("temperature_2m", "temperature"),
    ("precipitation", "precipitation"),
    ("precipitation_probability", "precipitation_probability"),
    ("wind_speed_10m", "wind_speed"),
    ("wind_gusts_10m", "wind_gust"),
    ("wind_direction_10m", "wind_direction"),
    ("cloud_cover", "cloud_cover"),
    ("relative_humidity_2m", "relative_humidity"),
    ("visibility", "visibility"),
    ("pressure_msl", "pressure"),
];

/// Builds a forecast request URL for a point (`forecast_days` is clamped to 1..=16).
pub fn request_url(latitude: f64, longitude: f64, forecast_days: u32) -> String {
    let variables: Vec<&str> = VARIABLES.iter().map(|(v, _)| *v).collect();
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}\
         &hourly={}&wind_speed_unit=ms&timeformat=unixtime&timezone=GMT&forecast_days={}",
        variables.join(","),
        forecast_days.clamp(1, 16)
    )
}

/// Parses an Open-Meteo forecast response (requested with `timeformat=unixtime`).
pub fn parse(json: &str) -> Result<Series, AdapterError> {
    let root: Value = serde_json::from_str(json)
        .map_err(|e| AdapterError::format("response", format!("not valid JSON: {e}")))?;
    if root.get("error").and_then(Value::as_bool) == Some(true) {
        let reason = root
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(AdapterError::format(
            "response",
            format!("Open-Meteo reported an error: {reason}"),
        ));
    }
    let hourly = root
        .get("hourly")
        .and_then(Value::as_object)
        .ok_or_else(|| AdapterError::format("response", "missing `hourly` object"))?;
    let times: Vec<i64> = hourly
        .get("time")
        .and_then(Value::as_array)
        .ok_or_else(|| AdapterError::format("hourly.time", "missing time array"))?
        .iter()
        .map(|t| {
            t.as_i64().ok_or_else(|| {
                AdapterError::format(
                    "hourly.time",
                    "expected integer Unix seconds; request with timeformat=unixtime",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    if times.len() < 2 {
        return Err(AdapterError::format(
            "hourly.time",
            "need at least two time steps",
        ));
    }
    let mut observations: Vec<Observation> = times
        .iter()
        .map(|t| {
            t.checked_mul(1000)
                .map(Observation::at)
                .ok_or_else(|| AdapterError::format("hourly.time", "timestamp out of range"))
        })
        .collect::<Result<_, _>>()?;
    let mut found = 0;
    for (variable, metric) in VARIABLES {
        let Some(values) = hourly.get(*variable) else {
            continue;
        };
        let values = values
            .as_array()
            .filter(|v| v.len() == times.len())
            .ok_or_else(|| {
                AdapterError::format(
                    format!("hourly.{variable}"),
                    "expected an array as long as `time`",
                )
            })?;
        let symbol = root
            .get("hourly_units")
            .and_then(|u| u.get(*variable))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AdapterError::format(format!("hourly_units.{variable}"), "missing unit")
            })?;
        let from = Unit::parse(symbol).ok_or_else(|| {
            AdapterError::format(
                format!("hourly_units.{variable}"),
                format!("unknown unit `{symbol}`"),
            )
        })?;
        let to =
            crate::units::canonical_unit(metric).expect("adapter metrics are in the vocabulary");
        for (observation, value) in observations.iter_mut().zip(values) {
            match value {
                Value::Null => {}
                Value::Number(n) => {
                    let raw = n.as_f64().ok_or_else(|| {
                        AdapterError::format(format!("hourly.{variable}"), "number out of range")
                    })?;
                    let converted = from.convert(raw, to).map_err(|e| {
                        AdapterError::format(format!("hourly_units.{variable}"), e.to_string())
                    })?;
                    observation.values.insert((*metric).to_string(), converted);
                }
                _ => {
                    return Err(AdapterError::format(
                        format!("hourly.{variable}"),
                        "expected numbers or null",
                    ))
                }
            }
        }
        found += 1;
    }
    if found == 0 {
        return Err(AdapterError::format(
            "hourly",
            "none of the supported variables are present",
        ));
    }
    let cadence = (times[1] - times[0]) * 1000;
    Ok(Series::new(cadence, observations)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-written in the documented Open-Meteo response shape (not captured live).
    const FIXTURE: &str = include_str!("../../tests/fixtures/open_meteo_hourly.json");

    #[test]
    fn parses_and_converts_the_documented_shape() {
        let series = parse(FIXTURE).unwrap();
        assert_eq!(series.cadence_ms, 3_600_000);
        assert_eq!(series.observations.len(), 4);
        assert_eq!(series.observations[0].timestamp_ms, 1_790_064_000_000);
        let first = &series.observations[0].values;
        // wind_speed_10m is declared in km/h in the fixture: 36 km/h = 10 m/s.
        assert!((first["wind_speed"] - 10.0).abs() < 1e-9);
        assert!((first["wind_gust"] - 15.0).abs() < 1e-9);
        assert_eq!(first["temperature"], 18.5);
        assert_eq!(first["visibility"], 24140.0);
        // null becomes a missing reading, not zero.
        assert!(!series.observations[2]
            .values
            .contains_key("precipitation_probability"));
    }
    #[test]
    fn request_url_lists_variables_and_clamps_days() {
        let url = request_url(52.52, 13.41, 99);
        assert!(url.contains("latitude=52.52&longitude=13.41"));
        assert!(url.contains("wind_speed_10m") && url.contains("timeformat=unixtime"));
        assert!(url.ends_with("forecast_days=16"));
    }
    #[test]
    fn reports_api_errors_and_bad_shapes() {
        let error = parse(r#"{"error":true,"reason":"Latitude must be in range of -90 to 90"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Latitude must be in range"), "{error}");
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"hourly":{"time":["2026-09-21T08:00"],"temperature_2m":[1]}}"#).is_err());
        let iso = r#"{"hourly_units":{"temperature_2m":"°C"},"hourly":{"time":["a","b"],"temperature_2m":[1,2]}}"#;
        assert!(parse(iso)
            .unwrap_err()
            .to_string()
            .contains("timeformat=unixtime"));
        let none = r#"{"hourly":{"time":[0,3600],"unrelated":[1,2]}}"#;
        assert!(parse(none)
            .unwrap_err()
            .to_string()
            .contains("supported variables"));
    }
}
