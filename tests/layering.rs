//! Guards the layering rule: the engine returns facts and decisions; wording, rounding, units and
//! colors belong to presenters (see `present` for an optional default).

/// The source of a module up to its test section, without comment lines.
fn production_code(path: &str) -> String {
    let source = std::fs::read_to_string(path).unwrap();
    let code = source.split("#[cfg(test)]").next().unwrap();
    code.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_engine_builds_no_display_text() {
    for file in ["src/engine.rs", "src/ensemble.rs"] {
        let code = production_code(file);
        for banned in [
            "format!(",
            "write!(",
            "writeln!(",
            "println!(",
            "eprintln!(",
        ] {
            assert!(
                !code.contains(banned),
                "{file} contains `{banned}`: results must be data, with wording left to `present` or the application"
            );
        }
    }
    // Display helpers are not part of the core API surface.
    let lib = production_code("src/lib.rs");
    for line in lib.lines().filter(|l| l.contains("pub use")) {
        assert!(
            !line.contains("format_") && !line.contains("describe_"),
            "lib.rs re-exports a display helper: {line}"
        );
    }
}

#[cfg(feature = "json")]
mod results_are_data {
    use serde_json::Value;
    use std::collections::BTreeSet;
    use wint::{
        Comparison, Constraint, Ensemble, EnsembleSearch, Member, Metric, Observation, Plan,
        Preference, Schedule, Series, Stage, WindowSearch,
    };

    const H: i64 = 3_600_000;

    fn strings(value: &Value, into: &mut BTreeSet<String>) {
        match value {
            Value::String(s) => {
                into.insert(s.clone());
            }
            Value::Array(items) => items.iter().for_each(|v| strings(v, into)),
            Value::Object(map) => map.values().for_each(|v| strings(v, into)),
            _ => {}
        }
    }

    fn series(wind: &[Option<f64>]) -> Series {
        let obs = wind
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let o = Observation::at(i as i64 * H).with("temperature", 15.0);
                match w {
                    Some(w) => o.with("wind_speed", *w),
                    None => o,
                }
            })
            .collect();
        Series::new(H, obs).unwrap().with_utc_offset(120).unwrap()
    }
    fn hard(name: &str, metric: &str, op: Comparison, limit: f64) -> Constraint {
        Constraint::hard(name, Metric::new(metric), op, limit)
    }

    /// Runs every kind of evidence through the engine and returns all strings in the JSON output.
    fn all_strings() -> BTreeSet<String> {
        let wind = [Some(3.0), Some(4.0), Some(30.0), None, Some(3.0), Some(3.0)];
        let mut found = BTreeSet::new();
        let mut collect = |v: Value| strings(&v, &mut found);

        // Hard pass, soft preference, and a clock window: passes, a clock failure, a limit violation.
        let plan = Plan::new(
            "flight plan",
            vec![Stage::new(
                "flight",
                H,
                vec![
                    hard("wind limit", "wind_speed", Comparison::LessThan, 10.0),
                    Constraint::soft(
                        "calm",
                        Metric::new("wind_speed"),
                        Preference::Minimize {
                            ideal: 0.0,
                            scale: 10.0,
                        },
                        1.0,
                    ),
                ],
            )
            .with_schedule(Schedule::parse("02:00", "05:00").unwrap())],
        );
        collect(
            serde_json::to_value(WindowSearch::new(&series(&wind), &plan).run().unwrap()).unwrap(),
        );

        // Every comparison and preference shape, on windows that fit (soft evidence only appears there).
        let plan = Plan::single_stage(
            "other",
            H,
            vec![
                Constraint::soft(
                    "a",
                    Metric::new("temperature"),
                    Preference::Maximize {
                        ideal: 20.0,
                        scale: 5.0,
                    },
                    1.0,
                ),
                Constraint::soft(
                    "b",
                    Metric::new("temperature"),
                    Preference::Range {
                        min: 10.0,
                        max: 20.0,
                        scale: 5.0,
                    },
                    1.0,
                ),
                hard("exact", "temperature", Comparison::Equal, 15.0),
                hard("above", "temperature", Comparison::GreaterThan, 0.0),
                hard("at most", "temperature", Comparison::LessThanOrEqual, 99.0),
                hard(
                    "at least",
                    "temperature",
                    Comparison::GreaterThanOrEqual,
                    1.0,
                ),
            ],
        );
        collect(
            serde_json::to_value(WindowSearch::new(&series(&wind), &plan).run().unwrap()).unwrap(),
        );

        // A reading that is missing from the data.
        let plan = Plan::single_stage(
            "other",
            H,
            vec![hard(
                "needs radiation",
                "solar_radiation",
                Comparison::GreaterThanOrEqual,
                5.0,
            )],
        );
        collect(
            serde_json::to_value(WindowSearch::new(&series(&wind), &plan).run().unwrap()).unwrap(),
        );

        // An ensemble, with a member that cannot answer.
        let ensemble = Ensemble::new(vec![
            Member {
                name: "model_a".into(),
                series: series(&[Some(3.0), Some(3.0), Some(3.0)]),
            },
            Member {
                name: "model_b".into(),
                series: series(&[Some(30.0), Some(3.0), Some(3.0)]),
            },
            Member {
                name: "model_c".into(),
                series: series(&[None, None, None]),
            },
        ])
        .unwrap();
        let plan = Plan::single_stage(
            "e",
            H,
            vec![hard("wind limit", "wind_speed", Comparison::LessThan, 10.0)],
        );
        collect(
            serde_json::to_value(EnsembleSearch::new(&ensemble, &plan).run().unwrap()).unwrap(),
        );
        found
    }

    #[test]
    fn every_string_in_a_result_is_an_identifier_or_a_data_tag() {
        // Names that the plan's author or the data's owner chose, and the one rule the engine names itself.
        let identifiers = [
            "operation", // the default stage name that `Plan::single_stage` gives
            "flight",
            "other",
            "e",
            "wind limit",
            "calm",
            "a",
            "b",
            "needs radiation",
            "exact",
            "above",
            "at most",
            "at least",
            "wind_speed",
            "temperature",
            "solar_radiation",
            "model_a",
            "model_b",
            "model_c",
            "time of day",
            "local_time",
        ];
        // Tags that name a kind of thing: never prose.
        let tags = [
            "comparison",
            "preference",
            "clock_window",
            "minimize",
            "maximize",
            "range",
            "<",
            "<=",
            ">",
            ">=",
            "==",
            "feasible",
            "infeasible",
            "unknown",
            "mon",
            "tue",
            "wed",
            "thu",
            "fri",
            "sat",
            "sun",
        ];
        let allowed: BTreeSet<&str> = identifiers.iter().chain(tags.iter()).copied().collect();
        let found = all_strings();
        let unexpected: Vec<&String> = found
            .iter()
            .filter(|s| !allowed.contains(s.as_str()))
            .collect();
        assert!(
            unexpected.is_empty(),
            "results contain text that looks like display wording (move it to `present` or the application): {unexpected:?}"
        );
        // The guard must not pass vacuously: every kind of evidence was really exercised.
        for expected in [
            "comparison",
            "preference",
            "clock_window",
            "minimize",
            "maximize",
            "range",
            "<",
            "==",
            "feasible",
            "infeasible",
            "unknown",
            "time of day",
            "local_time",
        ] {
            assert!(
                found.contains(expected),
                "the scenario never produced `{expected}`"
            );
        }
    }
}
