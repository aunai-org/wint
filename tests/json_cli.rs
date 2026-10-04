#![cfg(feature = "json")]

use std::process::Command;
use wint::{Plan, SearchResult, Series, WindowSearch};

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
    let c: wint::Constraint = serde_json::from_str(soft).unwrap();
    assert!(matches!(c, wint::Constraint::Soft { weight, .. } if weight == 1.0));
}

fn cli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wint"));
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
    let missing = Command::new(env!("CARGO_BIN_EXE_wint"))
        .args(["--plan", "nope.json", "--series", "examples/series.json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot read plan"));
    let usage = Command::new(env!("CARGO_BIN_EXE_wint")).output().unwrap();
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

    let constraint: wint::Constraint = serde_json::from_str(
        r#"{"type":"hard","name":"w","metric":"wind_speed","comparison":"<","threshold":20,"unit":"kn"}"#,
    )
    .unwrap();
    assert!(matches!(constraint,
        wint::Constraint::Hard { threshold, .. } if (threshold - 10.288888).abs() < 1e-4));
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
    let bad_constraint = serde_json::from_str::<wint::Constraint>(
        r#"{"type":"hard","name":"w","metric":"wind_speed","comparison":"<","threshold":1,"unit":"furlongs"}"#,
    )
    .unwrap_err();
    assert!(
        bad_constraint.to_string().contains("unknown unit"),
        "{bad_constraint}"
    );
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wint"))
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

const MULTI_MODEL: &str = "tests/fixtures/open_meteo_multi_model.json";
const ENSEMBLE: &str = "tests/fixtures/open_meteo_ensemble.json";

fn ensemble_cli(preset: &str, file: &str, extra: &[&str]) -> std::process::Output {
    let mut args = vec![
        "--preset",
        preset,
        "--hours",
        "2",
        "--series",
        file,
        "--format",
        "open-meteo-ensemble",
    ];
    args.extend_from_slice(extra);
    run_cli(&args)
}

#[test]
fn cli_reports_agreement_across_real_multi_model_data() {
    let out = ensemble_cli("drone", MULTI_MODEL, &["--top", "3"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains(
            "4 forecast versions (ecmwf_ifs025, gfs_seamless, icon_seamless, meteofrance_seamless)"
        ),
        "{text}"
    );
    assert!(
        text.contains("it is not a probability"),
        "the wording rule must be printed: {text}"
    );
    // The two models without visibility abstain, and are named.
    assert!(text.contains("fits in 2 of 2 that can answer"), "{text}");
    assert!(
        text.contains("cannot say: 2 (visibility for 2 members)"),
        "{text}"
    );
    // Never presented as a chance.
    assert!(
        !text.contains("chance") && !text.contains("probability of"),
        "{text}"
    );
}

#[test]
fn cli_ensemble_json_output_and_requirements() {
    let out = ensemble_cli(
        "field-work",
        ENSEMBLE,
        &["--json", "--min-agreement", "0.5"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["members"].as_array().unwrap().len(), 6);
    assert_eq!(value["members"][0], "control");
    assert_eq!(value["min_agreement"], 0.5);
    assert_eq!(value["min_coverage"], 0.5);
    let first = &value["windows"][0];
    assert_eq!(first["members_total"], 6);
    assert_eq!(first["outcomes"].as_array().unwrap().len(), 6);
    assert!(first["agreement"].is_number() && first["coverage"] == 1.0);
}

#[test]
fn cli_says_when_no_member_provides_a_metric() {
    // The ensemble service returns no visibility, and the drone preset has a visibility rule.
    let out = ensemble_cli("drone", ENSEMBLE, &["--top", "1"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("Note: no forecast version provides visibility"),
        "{text}"
    );
    assert!(
        text.contains("0 of") && text.contains("windows meet it"),
        "{text}"
    );
}

#[test]
fn cli_reads_native_ensemble_json_and_validates_flags() {
    use wint::{Ensemble, Member, Observation, Series};
    let member = |name: &str, wind: f64| Member {
        name: name.into(),
        series: Series::new(
            3_600_000,
            (0..3)
                .map(|i| Observation::at(i * 3_600_000).with("wind_speed", wind))
                .collect(),
        )
        .unwrap(),
    };
    let ensemble = Ensemble::new(vec![member("calm", 2.0), member("gusty", 20.0)]).unwrap();
    let dir = std::env::temp_dir().join(format!("wint-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ensemble.json");
    std::fs::write(&path, serde_json::to_string(&ensemble).unwrap()).unwrap();
    let plan = r#"{"name":"calm only","stages":[{"name":"s","duration_ms":3600000,"constraints":[
        {"type":"hard","name":"wind","metric":"wind_speed","comparison":"<","threshold":10}]}]}"#;
    let plan_path = dir.join("plan.json");
    std::fs::write(&plan_path, plan).unwrap();
    let out = run_cli(&[
        "--plan",
        plan_path.to_str().unwrap(),
        "--series",
        path.to_str().unwrap(),
        "--format",
        "ensemble-json",
        "--min-agreement",
        "0.5",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("fits in 1 of 2 that can answer")
            && text.contains("blocked by s/wind in 1 member"),
        "{text}"
    );
    assert!(
        text.contains("meets the requirement"),
        "min-agreement 0.5 is met by 1 of 2: {text}"
    );
    // --min-agreement on a plain series, an out-of-range value, and an invalid ensemble are all clear errors.
    let single = run_cli(&[
        "--preset",
        "drone",
        "--series",
        "examples/series.csv",
        "--min-agreement",
        "0.8",
    ]);
    assert_eq!(single.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&single.stderr).contains("apply only to several forecast versions")
    );
    let range = run_cli(&[
        "--preset",
        "drone",
        "--series",
        path.to_str().unwrap(),
        "--format",
        "ensemble-json",
        "--min-agreement",
        "1.5",
    ]);
    assert_eq!(range.status.code(), Some(2));
    std::fs::write(&path, r#"{"members":[]}"#).unwrap();
    let bad = run_cli(&[
        "--preset",
        "drone",
        "--series",
        path.to_str().unwrap(),
        "--format",
        "ensemble-json",
    ]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no members"));
    std::fs::remove_dir_all(&dir).ok();
}
