//! [Open-Meteo](https://open-meteo.com) forecast adapter (hourly data).
//!
//! This module is pure: the `*_url` functions build queries and the `parse*` functions turn the JSON
//! responses into a [`Series`] or an [`Ensemble`]. Performing the HTTP request is left to the caller
//! (the CLI does it behind the `net` feature), keeping the library free of any network stack.
//!
//! Values are converted from the units the response declares in `hourly_units` to the canonical
//! vocabulary units, so it does not matter which `wind_speed_unit` was requested. `null` readings
//! become missing values, which makes any hard constraint on that metric fail visibly in a plain
//! series, and makes that model or member "cannot say" in an ensemble.
//!
//! The requests use `timezone=auto`; timestamps stay absolute (Unix seconds) and the response's
//! `utc_offset_seconds` becomes the local-clock offset, so time-of-day schedules refer to the
//! place's own time. The response carries only one offset, so it is used for the whole forecast: across a
//! daylight-saving change the local clock is an hour off afterwards (see [`crate::Schedule`]).
//!
//! # Several forecast versions
//!
//! Two Open-Meteo services return parallel versions of a forecast, and [`parse_ensemble`] reads both:
//!
//! * the **multi-model** forecast (`models=a,b,c`) names columns `<variable>_<model>`; a model may
//!   omit a variable or end before the others (those readings are `null`);
//! * the **ensemble** API names columns `<variable>_member01`, `_member02`, ... plus an unsuffixed
//!   control run. A variable with a single unsuffixed column (such as `is_day`) is shared by all
//!   members. Some variables come back entirely `null` with the unit `undefined` (for example
//!   visibility); such columns carry no readings and are skipped.

use super::AdapterError;
use crate::units::{canonical_unit, Unit};
use crate::{Ensemble, Member, Observation, Series};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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
    ("is_day", "is_day"),
];

const FORECAST_API: &str = "https://api.open-meteo.com/v1/forecast";
const ENSEMBLE_API: &str = "https://ensemble-api.open-meteo.com/v1/ensemble";

fn query(base: &str, latitude: f64, longitude: f64, forecast_days: u32, models: &str) -> String {
    let variables: Vec<&str> = VARIABLES.iter().map(|(v, _)| *v).collect();
    let models = if models.is_empty() {
        String::new()
    } else {
        format!("&models={models}")
    };
    format!(
        "{base}?latitude={latitude}&longitude={longitude}&hourly={}{models}\
         &wind_speed_unit=ms&timeformat=unixtime&timezone=auto&forecast_days={}",
        variables.join(","),
        forecast_days.clamp(1, 16)
    )
}

/// Model names go straight into a URL, so only lowercase letters, digits and `_` are accepted.
fn check_models(models: &[&str]) -> Result<String, AdapterError> {
    if models.is_empty() {
        return Err(AdapterError::format(
            "models",
            "at least one model is required",
        ));
    }
    for model in models {
        let ok = !model.is_empty()
            && model
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !ok {
            return Err(AdapterError::format(
                "models",
                format!("`{model}` is not a model name (lowercase letters, digits and `_` only)"),
            ));
        }
    }
    Ok(models.join(","))
}

/// Builds a forecast request URL for a point (`forecast_days` is clamped to 1..=16).
pub fn request_url(latitude: f64, longitude: f64, forecast_days: u32) -> String {
    query(FORECAST_API, latitude, longitude, forecast_days, "")
}

/// Builds a multi-model forecast URL, e.g. `models = ["ecmwf_ifs025", "gfs_seamless"]`.
pub fn multi_model_url(
    latitude: f64,
    longitude: f64,
    forecast_days: u32,
    models: &[&str],
) -> Result<String, AdapterError> {
    Ok(query(
        FORECAST_API,
        latitude,
        longitude,
        forecast_days,
        &check_models(models)?,
    ))
}

/// Builds an ensemble-API URL, e.g. `models = ["icon_seamless"]` (about 40 members).
pub fn ensemble_url(
    latitude: f64,
    longitude: f64,
    forecast_days: u32,
    models: &[&str],
) -> Result<String, AdapterError> {
    Ok(query(
        ENSEMBLE_API,
        latitude,
        longitude,
        forecast_days,
        &check_models(models)?,
    ))
}

/// The response's time grid and clock, shared by every column.
struct Grid<'a> {
    root: &'a Value,
    hourly: &'a Map<String, Value>,
    times: Vec<i64>,
    offset_minutes: i32,
}

fn read_grid(root: &Value) -> Result<Grid<'_>, AdapterError> {
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
    let offset_seconds = root
        .get("utc_offset_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let offset_minutes = i32::try_from((offset_seconds as f64 / 60.0).round() as i64)
        .map_err(|_| AdapterError::format("utc_offset_seconds", "out of range"))?;
    Ok(Grid {
        root,
        hourly,
        times,
        offset_minutes,
    })
}

/// One response column: a supported variable, optionally tagged with a model or member name.
struct Column {
    variable: &'static str,
    metric: &'static str,
    key: String,
    suffix: Option<String>,
}

/// Finds the supported columns. A key belongs to the *longest* variable name it starts with, so
/// `precipitation_probability_gfs_seamless` is not read as `precipitation` plus a model.
fn columns(hourly: &Map<String, Value>) -> Vec<Column> {
    hourly
        .keys()
        .filter(|key| key.as_str() != "time")
        .filter_map(|key| {
            let (variable, metric) = VARIABLES
                .iter()
                .filter(|(v, _)| {
                    key == v
                        || key
                            .strip_prefix(v)
                            .is_some_and(|rest| rest.starts_with('_') && rest.len() > 1)
                })
                .max_by_key(|(v, _)| v.len())?;
            let suffix =
                (key.len() > variable.len()).then(|| key[variable.len() + 1..].to_string());
            Some(Column {
                variable,
                metric,
                key: key.clone(),
                suffix,
            })
        })
        .collect()
}

/// Builds one series from the given columns. Columns with no readings at all are skipped, so a
/// variable the service does not provide (all `null`, unit `undefined`) causes no error.
fn series_from(grid: &Grid<'_>, cols: &[&Column]) -> Result<Series, AdapterError> {
    let mut observations: Vec<Observation> = grid
        .times
        .iter()
        .map(|t| {
            t.checked_mul(1000)
                .map(Observation::at)
                .ok_or_else(|| AdapterError::format("hourly.time", "timestamp out of range"))
        })
        .collect::<Result<_, _>>()?;
    for col in cols {
        let key = &col.key;
        let values = grid.hourly[key.as_str()]
            .as_array()
            .filter(|v| v.len() == grid.times.len())
            .ok_or_else(|| {
                AdapterError::format(
                    format!("hourly.{key}"),
                    "expected an array as long as `time`",
                )
            })?;
        if values.iter().all(Value::is_null) {
            continue;
        }
        let declared = grid
            .root
            .get("hourly_units")
            .and_then(|u| u.get(key.as_str()))
            .and_then(Value::as_str);
        let from = match (col.variable, declared) {
            // The daylight flag is unitless; the API may report an empty unit or none.
            ("is_day", None | Some("")) => Unit::Flag,
            (_, None) => {
                return Err(AdapterError::format(
                    format!("hourly_units.{key}"),
                    "missing unit",
                ))
            }
            (_, Some(symbol)) => Unit::parse(symbol).ok_or_else(|| {
                AdapterError::format(
                    format!("hourly_units.{key}"),
                    format!("unknown unit `{symbol}`"),
                )
            })?,
        };
        let to = canonical_unit(col.metric).expect("adapter metrics are in the vocabulary");
        for (observation, value) in observations.iter_mut().zip(values) {
            match value {
                Value::Null => {}
                Value::Number(n) => {
                    let raw = n.as_f64().ok_or_else(|| {
                        AdapterError::format(format!("hourly.{key}"), "number out of range")
                    })?;
                    let converted = from.convert(raw, to).map_err(|e| {
                        AdapterError::format(format!("hourly_units.{key}"), e.to_string())
                    })?;
                    observation.values.insert(col.metric.to_string(), converted);
                }
                _ => {
                    return Err(AdapterError::format(
                        format!("hourly.{key}"),
                        "expected numbers or null",
                    ))
                }
            }
        }
    }
    let cadence = (grid.times[1] - grid.times[0]) * 1000;
    Ok(Series::new(cadence, observations)?.with_utc_offset(grid.offset_minutes)?)
}

fn parse_root(json: &str) -> Result<Value, AdapterError> {
    serde_json::from_str(json)
        .map_err(|e| AdapterError::format("response", format!("not valid JSON: {e}")))
}

/// Parses a single-model Open-Meteo forecast response (requested with `timeformat=unixtime`).
pub fn parse(json: &str) -> Result<Series, AdapterError> {
    let root = parse_root(json)?;
    let grid = read_grid(&root)?;
    let cols = columns(grid.hourly);
    let plain: Vec<&Column> = cols.iter().filter(|c| c.suffix.is_none()).collect();
    if plain.is_empty() {
        return Err(AdapterError::format(
            "hourly",
            if cols.is_empty() {
                "none of the supported variables are present"
            } else {
                "the response holds several forecast versions (models or ensemble members); \
                 read it with parse_ensemble"
            },
        ));
    }
    series_from(&grid, &plain)
}

/// Parses a multi-model forecast or an ensemble response into an [`Ensemble`] (see the module
/// docs for the two layouts). A plain single-model response gives a one-member ensemble named
/// `default`. Members are ordered by name, with the ensemble's `control` run first.
pub fn parse_ensemble(json: &str) -> Result<Ensemble, AdapterError> {
    let root = parse_root(json)?;
    let grid = read_grid(&root)?;
    let cols = columns(grid.hourly);
    if cols.is_empty() {
        return Err(AdapterError::format(
            "hourly",
            "none of the supported variables are present",
        ));
    }
    let suffixes: BTreeSet<&str> = cols.iter().filter_map(|c| c.suffix.as_deref()).collect();
    if suffixes.is_empty() {
        let all: Vec<&Column> = cols.iter().collect();
        let series = series_from(&grid, &all)?;
        return Ok(Ensemble::single("default", series));
    }
    // Variables that have suffixed columns elsewhere in the response: an unsuffixed column of
    // such a variable is the control run; an unsuffixed column of any other variable is shared.
    let versioned: BTreeSet<&str> = cols
        .iter()
        .filter(|c| c.suffix.is_some())
        .map(|c| c.variable)
        .collect();
    let shared: Vec<&Column> = cols
        .iter()
        .filter(|c| c.suffix.is_none() && !versioned.contains(c.variable))
        .collect();
    let control: Vec<&Column> = cols
        .iter()
        .filter(|c| c.suffix.is_none() && versioned.contains(c.variable))
        .collect();
    let mut groups: BTreeMap<&str, Vec<&Column>> = BTreeMap::new();
    for c in cols.iter().filter(|c| c.suffix.is_some()) {
        groups
            .entry(c.suffix.as_deref().unwrap())
            .or_default()
            .push(c);
    }
    let mut members = Vec::new();
    let mut add = |name: &str, own: &[&Column]| -> Result<(), AdapterError> {
        let mut all: Vec<&Column> = own.to_vec();
        all.extend(shared.iter().copied());
        members.push(Member {
            name: name.to_string(),
            series: series_from(&grid, &all)?,
        });
        Ok(())
    };
    if !control.is_empty() {
        add("control", &control)?;
    }
    for (name, own) in &groups {
        add(name, own)?;
    }
    Ok(Ensemble::new(members)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{presets, EnsembleSearch};

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
        // The place's local clock and the daylight flag (declared with an empty unit) come through.
        assert_eq!(series.utc_offset_minutes, 120);
        assert_eq!(first["is_day"], 1.0);
        assert_eq!(series.observations[3].values["is_day"], 0.0);
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
        assert!(url.contains("timezone=auto") && url.contains("is_day"));
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

    // ---- several forecast versions: fixtures are real API responses, trimmed (see tests/fixtures) ----
    const MULTI_MODEL: &str = include_str!("../../tests/fixtures/open_meteo_multi_model.json");
    const ENSEMBLE: &str = include_str!("../../tests/fixtures/open_meteo_ensemble.json");

    fn has(series: &Series, metric: &str) -> bool {
        series
            .observations
            .iter()
            .any(|o| o.values.contains_key(metric))
    }
    fn member<'a>(e: &'a Ensemble, name: &str) -> &'a Series {
        &e.members.iter().find(|m| m.name == name).unwrap().series
    }

    #[test]
    fn multi_model_response_becomes_one_member_per_model() {
        let e = parse_ensemble(MULTI_MODEL).unwrap();
        let names: Vec<&str> = e.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "ecmwf_ifs025",
                "gfs_seamless",
                "icon_seamless",
                "meteofrance_seamless"
            ]
        );
        assert_eq!(e.grid().observations.len(), 48);
        assert_eq!(e.grid().utc_offset_minutes, 120); // Berlin
                                                      // The real response: two models provide no visibility, one provides no rain probability.
        assert!(!has(member(&e, "ecmwf_ifs025"), "visibility"));
        assert!(!has(member(&e, "meteofrance_seamless"), "visibility"));
        assert!(has(member(&e, "gfs_seamless"), "visibility"));
        assert!(has(member(&e, "icon_seamless"), "visibility"));
        assert!(has(
            member(&e, "icon_seamless"),
            "precipitation_probability"
        ));
        assert!(!has(
            member(&e, "meteofrance_seamless"),
            "precipitation_probability"
        ));
        // `precipitation_probability_<model>` was not mistaken for `precipitation` plus a model.
        assert!(e.members.iter().all(|m| !m.name.starts_with("probability")));
        for m in &e.members {
            let wind = m.series.observations[0].values["wind_speed"];
            assert!((0.0..40.0).contains(&wind), "{}: {wind}", m.name);
            assert!(has(&m.series, "is_day"));
        }
    }

    #[test]
    fn the_drone_preset_on_real_multi_model_data_shows_abstaining_models() {
        let e = parse_ensemble(MULTI_MODEL).unwrap();
        let plan = presets::by_name("drone").unwrap().plan(2 * 3_600_000);
        let result = EnsembleSearch::new(&e, &plan).run().unwrap();
        // Where nothing else rules it out, ECMWF and Meteo-France cannot judge visibility: half the
        // models answer, which just meets the default coverage, and the gap is reported by name.
        let w = result
            .windows
            .iter()
            .find(|w| w.unknown == 2 && w.feasible == 2)
            .expect("a window where gfs and icon fit and the other two cannot say");
        assert_eq!((w.agreement, w.coverage), (Some(1.0), 0.5));
        assert!(w.meets_requirement);
        assert_eq!(
            (w.missing[0].metric.as_str(), w.missing[0].members),
            ("visibility", 2)
        );
    }

    #[test]
    fn ensemble_response_has_control_members_and_a_shared_daylight_flag() {
        let e = parse_ensemble(ENSEMBLE).unwrap();
        let names: Vec<&str> = e.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["control", "member01", "member02", "member03", "member04", "member05"]
        );
        // is_day is one column in the response and is given to every member.
        let flags: Vec<Vec<f64>> = e
            .members
            .iter()
            .map(|m| {
                m.series
                    .observations
                    .iter()
                    .map(|o| o.values["is_day"])
                    .collect()
            })
            .collect();
        assert!(flags.iter().all(|f| f == &flags[0]));
        // The service returned visibility and rain probability as all-null with unit `undefined`:
        // no error, and no readings.
        assert!(e.members.iter().all(|m| !has(&m.series, "visibility")));
        assert!(e
            .members
            .iter()
            .all(|m| !has(&m.series, "precipitation_probability")));
        // The members genuinely differ (that is the point of an ensemble).
        let temps: std::collections::BTreeSet<String> = e
            .members
            .iter()
            .map(|m| format!("{:.2}", m.series.observations[12].values["temperature"]))
            .collect();
        assert!(temps.len() > 1, "{temps:?}");
    }

    #[test]
    fn real_ensemble_data_answers_plans_it_has_the_variables_for() {
        let e = parse_ensemble(ENSEMBLE).unwrap();
        // field-work needs rain, wind and temperature: every member provides them.
        let field = presets::by_name("field-work").unwrap().plan(2 * 3_600_000);
        let r = EnsembleSearch::new(&e, &field).run().unwrap();
        assert!(r
            .windows
            .iter()
            .all(|w| w.unknown == 0 && w.coverage == 1.0));
        // outdoor-event needs rain probability, which this service does not provide: nobody can say.
        let event = presets::by_name("outdoor-event")
            .unwrap()
            .plan(2 * 3_600_000);
        let r = EnsembleSearch::new(&e, &event).run().unwrap();
        let w = r
            .windows
            .iter()
            .find(|w| w.unknown == 6)
            .expect("a window nobody can judge");
        assert_eq!((w.agreement, w.meets_requirement), (None, false));
        assert_eq!(w.missing[0].metric, "precipitation_probability");
    }

    #[test]
    fn urls_carry_validated_model_names() {
        let url = multi_model_url(52.52, 13.41, 3, &["ecmwf_ifs025", "gfs_seamless"]).unwrap();
        assert!(url.starts_with("https://api.open-meteo.com/v1/forecast?"));
        assert!(url.contains("&models=ecmwf_ifs025,gfs_seamless&"));
        let url = ensemble_url(1.0, 2.0, 99, &["icon_seamless"]).unwrap();
        assert!(url.starts_with("https://ensemble-api.open-meteo.com/v1/ensemble?"));
        assert!(url.contains("models=icon_seamless") && url.ends_with("forecast_days=16"));
        assert!(!request_url(1.0, 2.0, 3).contains("models="));
        for bad in ["", "GFS", "gfs seamless", "gfs&x=1", "a/b", "gfs,icon"] {
            assert!(multi_model_url(0.0, 0.0, 1, &[bad]).is_err(), "{bad:?}");
        }
        assert!(multi_model_url(0.0, 0.0, 1, &[]).is_err());
    }

    #[test]
    fn plain_and_mixed_responses_are_handled_clearly() {
        // A single-model response is a one-member ensemble.
        let e = parse_ensemble(FIXTURE).unwrap();
        assert_eq!(
            (e.members.len(), e.members[0].name.as_str()),
            (1, "default")
        );
        // parse() on a multi-model response says what to do instead of failing obscurely.
        let err = parse(MULTI_MODEL).unwrap_err().to_string();
        assert!(err.contains("parse_ensemble"), "{err}");
        assert!(parse_ensemble(r#"{"hourly":{"time":[0,3600],"unrelated":[1,2]}}"#).is_err());
    }

    #[test]
    fn variable_names_that_prefix_each_other_are_told_apart() {
        let json = r#"{"utc_offset_seconds":0,
          "hourly_units":{"time":"unixtime","precipitation_gfs_seamless":"mm","precipitation_probability_gfs_seamless":"%"},
          "hourly":{"time":[0,3600],"precipitation_gfs_seamless":[0.5,1.5],"precipitation_probability_gfs_seamless":[20,40]}}"#;
        let e = parse_ensemble(json).unwrap();
        assert_eq!(e.members.len(), 1);
        assert_eq!(e.members[0].name, "gfs_seamless");
        let v = &e.members[0].series.observations[1].values;
        assert_eq!(
            (v["precipitation"], v["precipitation_probability"]),
            (1.5, 40.0)
        );
    }

    #[test]
    fn a_column_with_real_values_and_an_unknown_unit_is_an_error_not_a_guess() {
        let mut v: Value = serde_json::from_str(ENSEMBLE).unwrap();
        v["hourly"]["visibility"][0] = Value::from(1000);
        let err = parse_ensemble(&v.to_string()).unwrap_err().to_string();
        assert!(err.contains("unknown unit `undefined`"), "{err}");
    }
}
