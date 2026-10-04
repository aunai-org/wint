//! JavaScript bindings (compiled with the `wasm` feature).
//!
//! The surface is intentionally thin: strings in, strings out. Series, plans
//! and results use the same JSON formats as the CLI, and every function
//! reports failure by throwing a readable message. All engine behaviour lives
//! in the Rust core; nothing is re-implemented here.

use crate::adapters::{csv, open_meteo};
use crate::{presets, Plan, Series, WindowSearch};
use wasm_bindgen::prelude::*;

fn err(message: impl std::fmt::Display) -> JsError {
    JsError::new(&message.to_string())
}

/// Library version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Version of the JSON shape of results (the `schema_version` field they carry).
#[wasm_bindgen(js_name = schemaVersion)]
pub fn schema_version() -> u32 {
    crate::SCHEMA_VERSION
}

/// Runs a window search. `series_json` and `plan_json` use the CLI formats
/// (including `units` / `unit` conversion); returns the result as JSON.
#[wasm_bindgen]
pub fn search(series_json: &str, plan_json: &str) -> Result<String, JsError> {
    let series: Series =
        serde_json::from_str(series_json).map_err(|e| err(format!("invalid series: {e}")))?;
    let plan: Plan =
        serde_json::from_str(plan_json).map_err(|e| err(format!("invalid plan: {e}")))?;
    let result = WindowSearch::new(&series, &plan)
        .run()
        .map_err(|e| err(format!("invalid plan: {e}")))?;
    serde_json::to_string(&result).map_err(err)
}

/// Parses CSV text (see `adapters::csv`) into series JSON.
#[wasm_bindgen(js_name = parseCsv)]
pub fn parse_csv(text: &str) -> Result<String, JsError> {
    let series = csv::parse(text).map_err(|e| err(format!("cannot load CSV: {e}")))?;
    serde_json::to_string(&series).map_err(err)
}

/// Parses an Open-Meteo forecast response into series JSON.
#[wasm_bindgen(js_name = parseOpenMeteo)]
pub fn parse_open_meteo(response: &str) -> Result<String, JsError> {
    let series =
        open_meteo::parse(response).map_err(|e| err(format!("cannot load forecast: {e}")))?;
    serde_json::to_string(&series).map_err(err)
}

/// The Open-Meteo forecast URL for a point; fetching it is up to the caller.
#[wasm_bindgen(js_name = openMeteoUrl)]
pub fn open_meteo_url(latitude: f64, longitude: f64, forecast_days: u32) -> String {
    open_meteo::request_url(latitude, longitude, forecast_days)
}

/// Built-in presets as JSON: `[{"name": ..., "description": ...}]`.
#[wasm_bindgen(js_name = listPresets)]
pub fn list_presets() -> Result<String, JsError> {
    let list: Vec<_> = presets::ALL
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "description": p.description }))
        .collect();
    serde_json::to_string(&list).map_err(err)
}

/// A preset's plan as JSON, for an operation lasting `hours`.
#[wasm_bindgen(js_name = presetPlan)]
pub fn preset_plan(name: &str, hours: f64) -> Result<String, JsError> {
    if !hours.is_finite() || hours <= 0.0 {
        return Err(err("hours must be a positive number"));
    }
    let preset = presets::by_name(name).ok_or_else(|| err(format!("unknown preset `{name}`")))?;
    let plan = preset.plan((hours * 3_600_000.0).round() as i64);
    serde_json::to_string(&plan).map_err(err)
}

/// The metric vocabulary as JSON: `[{"name", "unit", "description"}]`.
#[wasm_bindgen(js_name = listMetrics)]
pub fn list_metrics() -> Result<String, JsError> {
    let list: Vec<_> = crate::units::VOCABULARY
        .iter()
        .map(|(name, unit, description)| {
            serde_json::json!({ "name": name, "unit": unit.symbol(), "description": description })
        })
        .collect();
    serde_json::to_string(&list).map_err(err)
}

/// The Open-Meteo multi-model forecast URL for a point. `models` is a comma-separated list such as
/// `ecmwf_ifs025,gfs_seamless`; fetching it is up to the caller.
#[wasm_bindgen(js_name = multiModelUrl)]
pub fn multi_model_url(
    latitude: f64,
    longitude: f64,
    forecast_days: u32,
    models: &str,
) -> Result<String, JsError> {
    let names: Vec<&str> = models.split(',').map(str::trim).collect();
    open_meteo::multi_model_url(latitude, longitude, forecast_days, &names)
        .map_err(|e| err(format!("cannot build request: {e}")))
}

/// The Open-Meteo ensemble-API URL for a point. `models` is a comma-separated list such as
/// `icon_seamless`; fetching it is up to the caller.
#[wasm_bindgen(js_name = ensembleUrl)]
pub fn ensemble_url(
    latitude: f64,
    longitude: f64,
    forecast_days: u32,
    models: &str,
) -> Result<String, JsError> {
    let names: Vec<&str> = models.split(',').map(str::trim).collect();
    open_meteo::ensemble_url(latitude, longitude, forecast_days, &names)
        .map_err(|e| err(format!("cannot build request: {e}")))
}

/// Parses an Open-Meteo multi-model or ensemble response into ensemble JSON
/// (`{"members": [{"name", "series"}]}`).
#[wasm_bindgen(js_name = parseOpenMeteoEnsemble)]
pub fn parse_open_meteo_ensemble(response: &str) -> Result<String, JsError> {
    let ensemble = open_meteo::parse_ensemble(response)
        .map_err(|e| err(format!("cannot load forecast: {e}")))?;
    serde_json::to_string(&ensemble).map_err(err)
}

/// Searches every member of an ensemble and returns per-window agreement as JSON. `min_agreement`
/// and `min_coverage` are shares in (0, 1]; agreement is a count of forecast versions, not a
/// probability.
#[wasm_bindgen(js_name = searchEnsemble)]
pub fn search_ensemble(
    ensemble_json: &str,
    plan_json: &str,
    min_agreement: f64,
    min_coverage: f64,
) -> Result<String, JsError> {
    let ensemble: crate::Ensemble =
        serde_json::from_str(ensemble_json).map_err(|e| err(format!("invalid ensemble: {e}")))?;
    let plan: Plan =
        serde_json::from_str(plan_json).map_err(|e| err(format!("invalid plan: {e}")))?;
    let result = crate::EnsembleSearch::new(&ensemble, &plan)
        .min_agreement(min_agreement)
        .min_coverage(min_coverage)
        .run()
        .map_err(|e| err(format!("invalid search: {e}")))?;
    serde_json::to_string(&result).map_err(err)
}
