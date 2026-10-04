use wint::{
    Comparison, Constraint, Ensemble, EnsembleSearch, Member, Metric, Observation, Plan,
    Preference, Schedule, Series, ValidationError, Verdict, WindowSearch,
};

const H: i64 = 3_600_000;

/// A member whose wind readings are `winds` (one per hour); `None` leaves the reading out.
fn member(name: &str, winds: &[Option<f64>]) -> Member {
    let obs = winds
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let o = Observation::at(i as i64 * H);
            match w {
                Some(w) => o.with("wind_speed", *w),
                None => o,
            }
        })
        .collect();
    Member {
        name: name.into(),
        series: Series::new(H, obs).unwrap(),
    }
}
fn all(v: &[f64]) -> Vec<Option<f64>> {
    v.iter().map(|x| Some(*x)).collect()
}
fn wind_plan(hours: i64, limit: f64) -> Plan {
    Plan::single_stage(
        "p",
        hours * H,
        vec![Constraint::hard(
            "wind limit",
            Metric::new("wind_speed"),
            Comparison::LessThan,
            limit,
        )],
    )
}
fn window(result: &wint::EnsembleResult, start_h: i64) -> &wint::EnsembleWindow {
    result
        .windows
        .iter()
        .find(|w| w.start_ms == start_h * H)
        .unwrap()
}

#[test]
fn steady_window_outranks_one_that_depends_on_the_model() {
    // Hours 0-1: every model calm (A). Hours 2-3: one model at 9 m/s, another at 13 (B).
    let ens = Ensemble::new(vec![
        member("a", &all(&[3.0, 3.0, 9.0, 9.0])),
        member("b", &all(&[3.0, 3.0, 13.0, 13.0])),
    ])
    .unwrap();
    let result = EnsembleSearch::new(&ens, &wind_plan(2, 10.0))
        .run()
        .unwrap();
    let a = window(&result, 0);
    let b = window(&result, 2);
    assert_eq!((a.feasible, a.infeasible, a.agreement), (2, 0, Some(1.0)));
    assert_eq!((b.feasible, b.infeasible, b.agreement), (1, 1, Some(0.5)));
    assert!(a.meets_requirement && !b.meets_requirement);
    // A comes first, and B is still listed (so the user can see what almost worked).
    assert_eq!(result.windows[0].start_ms, 0);
    assert!(result.windows.iter().any(|w| w.start_ms == 2 * H));
    // The reason B is uncertain is named, with the number of members it stops.
    assert_eq!(b.blockers.len(), 1);
    assert_eq!(
        (b.blockers[0].constraint.as_str(), b.blockers[0].members),
        ("wind limit", 1)
    );
}

#[test]
fn min_agreement_sets_the_bar() {
    let members = ["a", "b", "c", "d", "e"]
        .iter()
        .enumerate()
        .map(|(i, n)| member(n, &all(&[if i == 0 { 20.0 } else { 3.0 }, 3.0])))
        .collect();
    let ens = Ensemble::new(members).unwrap();
    let plan = wind_plan(1, 10.0);
    let strict = EnsembleSearch::new(&ens, &plan).run().unwrap();
    assert_eq!(window(&strict, 0).agreement, Some(0.8)); // 4 of 5
    assert!(!window(&strict, 0).meets_requirement); // default: all members
    let relaxed = EnsembleSearch::new(&ens, &plan)
        .min_agreement(0.8)
        .run()
        .unwrap();
    assert!(
        window(&relaxed, 0).meets_requirement,
        "4 of 5 meets 0.8 despite float rounding"
    );
    assert!(window(&relaxed, 1).meets_requirement);
}

#[test]
fn a_member_without_the_reading_abstains_instead_of_disagreeing() {
    // c has no wind reading in hour 0 (for instance its forecast does not reach that far).
    let ens = Ensemble::new(vec![
        member("a", &all(&[3.0, 3.0])),
        member("b", &all(&[3.0, 3.0])),
        member("c", &[None, Some(3.0)]),
    ])
    .unwrap();
    let result = EnsembleSearch::new(&ens, &wind_plan(1, 10.0))
        .run()
        .unwrap();
    let w = window(&result, 0);
    assert_eq!((w.feasible, w.infeasible, w.unknown), (2, 0, 1));
    assert_eq!(w.agreement, Some(1.0));
    assert!((w.coverage - 2.0 / 3.0).abs() < 1e-9);
    assert!(w.meets_requirement);
    assert_eq!(w.missing[0].metric, "wind_speed");
    assert_eq!(w.missing[0].members, 1);
    let c = w.outcomes.iter().find(|o| o.member == "c").unwrap();
    assert_eq!(c.verdict, Verdict::Unknown);
    assert_eq!(c.failure.as_ref().unwrap().actual, None);
    // Without abstention logic this would have been 2 of 3. With full data in hour 1 it is 3 of 3.
    assert_eq!(window(&result, 1).coverage, 1.0);
}

#[test]
fn low_coverage_cannot_meet_the_requirement() {
    // Only one of four members can answer: a perfect agreement on a single vote is not enough.
    let ens = Ensemble::new(vec![
        member("a", &all(&[3.0])),
        member("b", &[None]),
        member("c", &[None]),
        member("d", &[None]),
    ])
    .unwrap();
    let plan = wind_plan(1, 10.0);
    let result = EnsembleSearch::new(&ens, &plan).run().unwrap();
    let w = window(&result, 0);
    assert_eq!(w.agreement, Some(1.0));
    assert_eq!(w.coverage, 0.25);
    assert!(!w.meets_requirement);
    assert!(
        EnsembleSearch::new(&ens, &plan)
            .min_coverage(0.25)
            .run()
            .unwrap()
            .windows[0]
            .meets_requirement
    );
    // Nobody can answer: no agreement at all, and it is never met.
    let silent = Ensemble::new(vec![member("a", &[None]), member("b", &[None])]).unwrap();
    let w = &EnsembleSearch::new(&silent, &plan).run().unwrap().windows[0];
    assert_eq!(
        (w.agreement, w.coverage, w.meets_requirement),
        (None, 0.0, false)
    );
    assert_eq!(w.unknown, 2);
}

#[test]
fn a_definite_violation_beats_a_missing_reading_in_the_same_window() {
    // Hour 0 is missing in the member, but hour 1 is 20 m/s: the 2 h window cannot fit, whatever hour 0 was.
    let ens = Ensemble::new(vec![
        member("a", &[None, Some(20.0)]),
        member("b", &all(&[3.0, 3.0])),
    ])
    .unwrap();
    let w = &EnsembleSearch::new(&ens, &wind_plan(2, 10.0))
        .run()
        .unwrap()
        .windows[0];
    let a = w.outcomes.iter().find(|o| o.member == "a").unwrap();
    assert_eq!(a.verdict, Verdict::Infeasible);
    assert_eq!(a.failure.as_ref().unwrap().actual, Some(20.0));
    assert_eq!((w.feasible, w.infeasible, w.unknown), (1, 1, 0));
}

#[test]
fn single_series_behaviour_is_unchanged_by_the_tolerant_mode() {
    // A plain WindowSearch still fails on the first missing reading, even if a violation follows.
    let series = Series::new(
        H,
        vec![
            Observation::at(0),
            Observation::at(H).with("wind_speed", 20.0),
        ],
    )
    .unwrap();
    let result = WindowSearch::new(&series, &wind_plan(2, 10.0))
        .run()
        .unwrap();
    assert_eq!(result.rejected[0].failure.actual, None);
}

#[test]
fn one_member_matches_the_plain_search() {
    let series = member("only", &all(&[3.0, 12.0, 3.0, 4.0])).series;
    let plan = Plan::single_stage(
        "p",
        H,
        vec![
            Constraint::hard(
                "wind limit",
                Metric::new("wind_speed"),
                Comparison::LessThan,
                10.0,
            ),
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
    );
    let plain = WindowSearch::new(&series, &plan).run().unwrap();
    let ens = Ensemble::single("only", series);
    let result = EnsembleSearch::new(&ens, &plan).run().unwrap();
    for w in &result.windows {
        let feasible = plain.feasible.iter().find(|p| p.start_ms == w.start_ms);
        match feasible {
            Some(p) => {
                assert_eq!(
                    (w.agreement, w.suitability),
                    (Some(1.0), Some(p.suitability))
                );
            }
            None => assert_eq!(w.agreement, Some(0.0)),
        }
    }
    assert_eq!(result.windows.len(), 4);
}

#[test]
fn suitability_is_the_mean_over_members_where_it_fits() {
    let plan = Plan::single_stage(
        "p",
        H,
        vec![
            Constraint::hard(
                "wind limit",
                Metric::new("wind_speed"),
                Comparison::LessThan,
                10.0,
            ),
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
    );
    // Scores: wind 2 -> 0.8, wind 6 -> 0.4, wind 20 does not fit (excluded from the mean).
    let ens = Ensemble::new(vec![
        member("a", &all(&[2.0])),
        member("b", &all(&[6.0])),
        member("c", &all(&[20.0])),
    ])
    .unwrap();
    let w = &EnsembleSearch::new(&ens, &plan).run().unwrap().windows[0];
    assert!((w.suitability.unwrap() - 0.6).abs() < 1e-9);
}

#[test]
fn ties_and_ordering_are_deterministic() {
    let ens = Ensemble::new(vec![
        member("a", &all(&[3.0, 3.0, 3.0])),
        member("b", &all(&[3.0, 3.0, 3.0])),
    ])
    .unwrap();
    let r = EnsembleSearch::new(&ens, &wind_plan(1, 10.0))
        .run()
        .unwrap();
    let starts: Vec<i64> = r.windows.iter().map(|w| w.start_ms / H).collect();
    assert_eq!(starts, [0, 1, 2]);
    assert_eq!(r.members, ["a", "b"]);
    assert!(r.windows[0]
        .outcomes
        .iter()
        .map(|o| o.member.as_str())
        .eq(["a", "b"]));
}

#[test]
fn schedules_and_offsets_apply_to_every_member() {
    let mut ens = Ensemble::new(vec![
        member("a", &all(&[3.0; 24])),
        member("b", &all(&[3.0; 24])),
    ])
    .unwrap();
    for m in &mut ens.members {
        m.series = m.series.clone().with_utc_offset(120).unwrap();
    }
    let plan = wind_plan(2, 10.0).with_schedule(Schedule::parse("09:00", "12:00").unwrap());
    let r = EnsembleSearch::new(&ens, &plan).run().unwrap();
    let fits: Vec<i64> = r
        .windows
        .iter()
        .filter(|w| w.meets_requirement)
        .map(|w| w.start_ms / H)
        .collect();
    assert_eq!(fits, [7, 8]); // 09:00-11:00 and 10:00-12:00 local at UTC+02:00
    assert!(window(&r, 0)
        .blockers
        .iter()
        .any(|b| b.constraint == "time of day" && b.members == 2));
}

#[test]
fn ensembles_and_requirements_are_validated() {
    assert_eq!(Ensemble::new(vec![]), Err(ValidationError::EmptyEnsemble));
    let a = member("a", &all(&[1.0, 1.0]));
    assert!(matches!(
        Ensemble::new(vec![a.clone(), a.clone()]),
        Err(ValidationError::DuplicateMember { .. })
    ));
    let shorter = member("b", &all(&[1.0]));
    assert!(matches!(
        Ensemble::new(vec![a.clone(), shorter]),
        Err(ValidationError::EnsembleMismatch { .. })
    ));
    let mut shifted = member("c", &all(&[1.0, 1.0]));
    shifted.series.observations[0].timestamp_ms += 1;
    assert!(
        matches!(Ensemble::new(vec![a.clone(), shifted]), Err(ValidationError::EnsembleMismatch { reason, .. }) if reason == "has different timestamps")
    );
    let offset = Member {
        name: "d".into(),
        series: a.series.clone().with_utc_offset(60).unwrap(),
    };
    assert!(
        matches!(Ensemble::new(vec![a.clone(), offset]), Err(ValidationError::EnsembleMismatch { reason, .. }) if reason == "has a different UTC offset")
    );
    let ens = Ensemble::new(vec![a]).unwrap();
    let plan = wind_plan(1, 10.0);
    for bad in [0.0, 1.5, -0.1, f64::NAN] {
        assert!(
            matches!(
                EnsembleSearch::new(&ens, &plan).min_agreement(bad).run(),
                Err(ValidationError::InvalidRequirement {
                    name: "min_agreement"
                })
            ),
            "{bad}"
        );
        assert!(
            matches!(
                EnsembleSearch::new(&ens, &plan).min_coverage(bad).run(),
                Err(ValidationError::InvalidRequirement {
                    name: "min_coverage"
                })
            ),
            "{bad}"
        );
    }
    // An invalid plan is still an error, not an empty result.
    assert!(EnsembleSearch::new(&ens, &Plan::new("e", vec![]))
        .run()
        .is_err());
}

/// A member with a wind and a temperature reading in one hour, either of which may be left out.
fn two_metric_member(name: &str, wind: Option<f64>, temp: Option<f64>) -> Member {
    let mut o = Observation::at(0);
    if let Some(w) = wind {
        o = o.with("wind_speed", w);
    }
    if let Some(t) = temp {
        o = o.with("temperature", t);
    }
    Member {
        name: name.into(),
        series: Series::new(H, vec![o]).unwrap(),
    }
}
fn wind_and_temperature_plan() -> Plan {
    Plan::single_stage(
        "p",
        H,
        vec![
            Constraint::hard(
                "wind limit",
                Metric::new("wind_speed"),
                Comparison::LessThan,
                10.0,
            ),
            Constraint::hard(
                "warm enough",
                Metric::new("temperature"),
                Comparison::GreaterThan,
                0.0,
            ),
            Constraint::hard(
                "radiation",
                Metric::new("solar_radiation"),
                Comparison::LessThan,
                800.0,
            ),
        ],
    )
}

#[test]
fn every_metric_a_member_lacks_is_named_not_only_the_first() {
    // a lacks wind and temperature, b lacks only temperature, c has both; nobody has solar_radiation.
    let ens = Ensemble::new(vec![
        two_metric_member("a", None, None),
        two_metric_member("b", Some(3.0), None),
        two_metric_member("c", Some(3.0), Some(15.0)),
    ])
    .unwrap();
    let r = EnsembleSearch::new(&ens, &wind_and_temperature_plan())
        .run()
        .unwrap();
    let w = &r.windows[0];
    assert_eq!((w.feasible, w.infeasible, w.unknown), (0, 0, 3));
    let count = |metric: &str| {
        w.missing
            .iter()
            .find(|m| m.metric == metric)
            .map(|m| m.members)
    };
    assert_eq!(count("solar_radiation"), Some(3));
    assert_eq!(count("temperature"), Some(2)); // a and b
    assert_eq!(count("wind_speed"), Some(1)); // only a
                                              // The metric nobody provides in any hour is called out once, at the top level.
    assert_eq!(r.unprovided, ["solar_radiation"]);
}

#[test]
fn unprovided_ignores_soft_rules_and_metrics_some_member_has() {
    let plan = Plan::single_stage(
        "p",
        H,
        vec![
            Constraint::hard(
                "wind limit",
                Metric::new("wind_speed"),
                Comparison::LessThan,
                10.0,
            ),
            Constraint::soft(
                "mild",
                Metric::new("temperature"),
                Preference::Minimize {
                    ideal: 20.0,
                    scale: 10.0,
                },
                1.0,
            ),
        ],
    );
    // Wind comes from one member only, temperature (a soft rule) from none: neither is "unprovided".
    let ens = Ensemble::new(vec![
        two_metric_member("a", Some(3.0), None),
        two_metric_member("b", None, None),
    ])
    .unwrap();
    let r = EnsembleSearch::new(&ens, &plan).run().unwrap();
    assert!(r.unprovided.is_empty());
}

#[test]
fn a_member_with_a_definite_violation_is_not_reported_as_missing_data() {
    // b lacks temperature but its wind is far over the limit: it does not fit, whatever the temperature.
    let plan = Plan::single_stage(
        "p",
        H,
        vec![
            Constraint::hard(
                "wind limit",
                Metric::new("wind_speed"),
                Comparison::LessThan,
                10.0,
            ),
            Constraint::hard(
                "warm enough",
                Metric::new("temperature"),
                Comparison::GreaterThan,
                0.0,
            ),
        ],
    );
    let ens = Ensemble::new(vec![
        two_metric_member("a", Some(3.0), Some(15.0)),
        two_metric_member("b", Some(30.0), None),
    ])
    .unwrap();
    let w = &EnsembleSearch::new(&ens, &plan).run().unwrap().windows[0];
    assert_eq!((w.feasible, w.infeasible, w.unknown), (1, 1, 0));
    assert!(w.missing.is_empty());
}

#[cfg(feature = "json")]
mod json {
    use super::*;

    #[test]
    fn ensemble_json_round_trips_and_is_validated() {
        let ens = Ensemble::new(vec![
            member("a", &all(&[1.0, 2.0])),
            member("b", &all(&[1.0, 5.0])),
        ])
        .unwrap();
        let text = serde_json::to_string(&ens).unwrap();
        assert_eq!(serde_json::from_str::<Ensemble>(&text).unwrap(), ens);
        // Mismatched members cannot be smuggled in through JSON.
        let bad = r#"{"members":[
            {"name":"a","series":{"cadence_ms":1,"observations":[{"timestamp_ms":0,"values":{}},{"timestamp_ms":1,"values":{}}]}},
            {"name":"b","series":{"cadence_ms":1,"observations":[{"timestamp_ms":0,"values":{}}]}}]}"#;
        let err = serde_json::from_str::<Ensemble>(bad)
            .unwrap_err()
            .to_string();
        assert!(err.contains("different number of observations"), "{err}");
        assert!(serde_json::from_str::<Ensemble>(r#"{"members":[]}"#)
            .unwrap_err()
            .to_string()
            .contains("no members"));
    }

    #[test]
    fn results_serialize_with_readable_verdicts() {
        let ens = Ensemble::new(vec![
            member("a", &all(&[3.0])),
            member("b", &[None]),
            member("c", &all(&[30.0])),
        ])
        .unwrap();
        let result = EnsembleSearch::new(&ens, &wind_plan(1, 10.0))
            .run()
            .unwrap();
        let value: serde_json::Value = serde_json::to_value(&result).unwrap();
        let w = &value["windows"][0];
        assert_eq!(w["feasible"], 1);
        assert_eq!(w["agreement"], 0.5);
        let verdicts: Vec<&str> = w["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["verdict"].as_str().unwrap())
            .collect();
        assert_eq!(verdicts, ["feasible", "unknown", "infeasible"]);
        assert_eq!(w["missing"][0]["metric"], "wind_speed");
        assert_eq!(value["unprovided"], serde_json::json!([]));
    }
}
