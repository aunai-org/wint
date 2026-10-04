//! Golden files pin the JSON shapes that applications depend on. If one of these tests fails, a
//! result or plan changed shape: either undo the change, or (for an added field or variant) review
//! the diff and regenerate with `WINT_UPDATE_GOLDEN=1 cargo test --all-features --test golden`.
//! Removing, renaming or reinterpreting a field also means raising `SCHEMA_VERSION`.
#![cfg(feature = "json")]

use serde_json::Value;
use std::path::PathBuf;
use wint::adapters::open_meteo;
use wint::{EnsembleSearch, Plan, Schedule, Series, Weekday, WindowSearch, SCHEMA_VERSION};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
}
fn read(name: &str) -> String {
    std::fs::read_to_string(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}
/// Compares `actual` with `tests/golden/<name>`, or rewrites the file when asked to.
fn check(name: &str, actual: &impl serde::Serialize) {
    let file = format!("tests/golden/{name}");
    let actual: Value = serde_json::to_value(actual).unwrap();
    if std::env::var_os("WINT_UPDATE_GOLDEN").is_some() {
        let text = serde_json::to_string_pretty(&actual).unwrap() + "\n";
        std::fs::write(path(&file), text).unwrap();
        return;
    }
    let expected: Value =
        serde_json::from_str(&read(&file)).unwrap_or_else(|e| panic!("{file}: {e}"));
    assert_eq!(
        actual, expected,
        "{file} differs: the JSON shape changed (see the note at the top of tests/golden.rs)"
    );
}

/// The example plan with a Monday-to-Friday morning window on its first stage, so that every kind
/// of evidence (comparison, preference, clock window) appears.
fn scheduled_plan() -> Plan {
    let mut plan: Plan = serde_json::from_str(&read("examples/plan.json")).unwrap();
    plan.stages[0].schedule = Some(
        Schedule::parse("08:00", "18:00")
            .unwrap()
            .with_days([
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ])
            .unwrap(),
    );
    plan
}

#[test]
fn single_series_result() {
    let series: Series = serde_json::from_str(&read("examples/series.json")).unwrap();
    let result = WindowSearch::new(&series, &scheduled_plan()).run().unwrap();
    assert_eq!(result.schema_version, SCHEMA_VERSION);
    assert!(!result.feasible.is_empty() && !result.rejected.is_empty());
    check("single_result.json", &result);
}

#[test]
fn ensemble_result() {
    let ensemble =
        open_meteo::parse_ensemble(&read("tests/fixtures/open_meteo_ensemble.json")).unwrap();
    let plan = scheduled_plan();
    let result = EnsembleSearch::new(&ensemble, &plan).run().unwrap();
    assert_eq!(result.schema_version, SCHEMA_VERSION);
    assert!(!result.windows.is_empty());
    check("ensemble_result.json", &result);
}

#[test]
fn plan_wire_form() {
    let plan = scheduled_plan();
    check("plan.json", &plan);
    // And it reads back unchanged.
    let again: Plan = serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
    assert_eq!(again, plan);
}
