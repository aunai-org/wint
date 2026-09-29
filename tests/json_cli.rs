#![cfg(feature = "json")]

use env_operability::{Plan, SearchResult, Series, WindowSearch};
use std::process::Command;

const PLAN: &str = include_str!("../examples/plan.json");
const SERIES: &str = include_str!("../examples/series.json");

#[test]
fn example_files_parse_and_search() {
    let series: Series = serde_json::from_str(SERIES).unwrap();
    let plan: Plan = serde_json::from_str(PLAN).unwrap();
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    assert_eq!(result.feasible.len(), 2);
    assert_eq!(result.rejected.len(), 2);
    let best = &result.feasible[0];
    assert_eq!(best.stages.len(), 2);
    assert_eq!(best.stages[0].name, "flight");
    assert_eq!(best.stages[1].start_ms, best.stages[0].end_ms);
}

#[test]
fn json_series_cannot_bypass_validation() {
    let irregular = r#"{"cadence_ms":10,"observations":[
        {"timestamp_ms":0,"values":{}},{"timestamp_ms":25,"values":{}}]}"#;
    let err = serde_json::from_str::<Series>(irregular).unwrap_err();
    assert!(err.to_string().contains("expected timestamp 10"), "{err}");
}

#[test]
fn soft_weight_defaults_to_one_and_plan_round_trips() {
    let plan: Plan = serde_json::from_str(PLAN).unwrap();
    let text = serde_json::to_string(&plan).unwrap();
    assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
    let soft = r#"{"type":"soft","name":"n","metric":"m",
        "preference":{"kind":"maximize","ideal":1,"scale":2}}"#;
    let c: env_operability::Constraint = serde_json::from_str(soft).unwrap();
    assert!(matches!(c, env_operability::Constraint::Soft { weight, .. } if weight == 1.0));
}

fn cli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_env-operability"));
    cmd.args([
        "--plan",
        "examples/plan.json",
        "--series",
        "examples/series.json",
    ]);
    cmd
}

#[test]
fn cli_json_output_matches_library() {
    let out = cli().arg("--json").output().unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["feasible"].as_array().unwrap().len(), 2);
    assert_eq!(value["feasible"][0]["stages"][0]["name"], "flight");
    let _ = SearchResult::default();
}

#[test]
fn cli_text_output_lists_windows_and_rejections() {
    let out = cli().arg("--rejected").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("2 feasible, 2 rejected"), "{text}");
    assert!(text.contains("2026-09-21T09:00Z"), "{text}");
    assert!(
        text.contains("rejected 2026-09-21T11:00Z: flight/safe wind"),
        "{text}"
    );
}

#[test]
fn cli_reports_errors_with_nonzero_exit() {
    let missing = Command::new(env!("CARGO_BIN_EXE_env-operability"))
        .args(["--plan", "nope.json", "--series", "examples/series.json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot read plan"));
    let usage = Command::new(env!("CARGO_BIN_EXE_env-operability"))
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

#[test]
fn json_series_and_plan_units_are_converted() {
    let series: Series = serde_json::from_str(
        r#"{"cadence_ms":3600000,
            "units":{"wind_speed":"km/h","temperature":"F"},
            "observations":[{"timestamp_ms":0,"values":{"wind_speed":36,"temperature":212}}]}"#,
    )
    .unwrap();
    assert!((series.observations[0].values["wind_speed"] - 10.0).abs() < 1e-9);
    assert!((series.observations[0].values["temperature"] - 100.0).abs() < 1e-9);

    let constraint: env_operability::Constraint = serde_json::from_str(
        r#"{"type":"hard","name":"w","metric":"wind_speed","comparison":"<","threshold":20,"unit":"kn"}"#,
    )
    .unwrap();
    assert!(matches!(constraint,
        env_operability::Constraint::Hard { threshold, .. } if (threshold - 10.288888).abs() < 1e-4));
}

#[test]
fn json_unknown_units_are_errors() {
    let bad_series = serde_json::from_str::<Series>(
        r#"{"cadence_ms":1,"units":{"wind_speed":"furlongs"},"observations":[{"timestamp_ms":0,"values":{}}]}"#,
    )
    .unwrap_err();
    assert!(
        bad_series.to_string().contains("unknown unit `furlongs`"),
        "{bad_series}"
    );
    let bad_constraint = serde_json::from_str::<env_operability::Constraint>(
        r#"{"type":"hard","name":"w","metric":"wind_speed","comparison":"<","threshold":1,"unit":"furlongs"}"#,
    )
    .unwrap_err();
    assert!(
        bad_constraint.to_string().contains("unknown unit"),
        "{bad_constraint}"
    );
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_env-operability"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_reads_csv_and_open_meteo_files_and_presets() {
    let csv = run_cli(&[
        "--plan",
        "examples/plan.json",
        "--series",
        "examples/series.csv",
        "--top",
        "1",
    ]);
    assert!(
        csv.status.success(),
        "{}",
        String::from_utf8_lossy(&csv.stderr)
    );
    let text = String::from_utf8(csv.stdout).unwrap();
    assert!(text.contains("2 feasible, 2 rejected"), "{text}");
    assert!(
        text.contains("m/s"),
        "values should show canonical units: {text}"
    );

    let om = run_cli(&[
        "--preset",
        "drone",
        "--hours",
        "1",
        "--series",
        "tests/fixtures/open_meteo_hourly.json",
        "--format",
        "open-meteo",
        "--json",
    ]);
    assert!(
        om.status.success(),
        "{}",
        String::from_utf8_lossy(&om.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&om.stdout).unwrap();
    // Hours 0 (gust 15 m/s) and 2 (0.2 mm rain) breach the drone limits; 1 and 3 pass.
    let starts = |key: &str| -> Vec<i64> {
        let mut v: Vec<i64> = value[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["start_ms"].as_i64().unwrap())
            .collect();
        v.sort();
        v
    };
    assert_eq!(starts("feasible"), [1_790_067_600_000, 1_790_074_800_000]);
    assert_eq!(starts("rejected"), [1_790_064_000_000, 1_790_071_200_000]);
}

#[test]
fn cli_lists_presets_and_rejects_unknown_ones() {
    let list = run_cli(&["--list-presets"]);
    assert!(list.status.success());
    assert!(String::from_utf8(list.stdout)
        .unwrap()
        .contains("outdoor-event"));
    let unknown = run_cli(&["--preset", "nope", "--series", "examples/series.csv"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown preset `nope`"));
}

#[test]
fn cli_reports_adapter_errors_with_location() {
    let bad = run_cli(&[
        "--preset",
        "drone",
        "--series",
        "tests/fixtures/open_meteo_hourly.json",
    ]);
    // Parsed as the native JSON format, an Open-Meteo response is not a series.
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("invalid series"));
}
