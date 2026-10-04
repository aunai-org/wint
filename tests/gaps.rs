use wint::{
    Comparison, Constraint, Ensemble, EnsembleSearch, Gap, Member, Metric, Observation, Plan,
    Preference, Series, Stage, ValidationError, Verdict, WindowSearch, MAX_ARRANGEMENTS,
};

const H: i64 = 3_600_000;

fn series(winds: &[Option<f64>]) -> Series {
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
    Series::new(H, obs).unwrap()
}
fn winds(v: &[f64]) -> Series {
    series(&v.iter().map(|x| Some(*x)).collect::<Vec<_>>())
}
fn calm(limit: f64) -> Vec<Constraint> {
    vec![Constraint::hard(
        "wind",
        Metric::new("wind_speed"),
        Comparison::LessThan,
        limit,
    )]
}
/// Two one-hour stages: the first needs wind < 5, the second wind < 5 too, starting `min..=max`
/// hours after the first ends.
fn two_stage(min: i64, max: i64) -> Plan {
    Plan::new(
        "p",
        vec![
            Stage::new("first", H, calm(5.0)),
            Stage::new("second", H, calm(5.0)).with_gap(min * H, max * H),
        ],
    )
}
fn feasible_starts(series: &Series, plan: &Plan) -> Vec<i64> {
    let mut v: Vec<i64> = WindowSearch::new(series, plan)
        .run()
        .unwrap()
        .feasible
        .iter()
        .map(|w| w.start_ms / H)
        .collect();
    v.sort();
    v
}

#[test]
fn no_gap_means_back_to_back() {
    let s = winds(&[1.0, 1.0, 9.0, 1.0, 1.0]);
    let plan = Plan::new(
        "p",
        vec![Stage::new("a", H, calm(5.0)), Stage::new("b", H, calm(5.0))],
    );
    assert_eq!(feasible_starts(&s, &plan), [0, 3]);
    assert_eq!(plan.min_span_ms(), 2 * H);
}

#[test]
fn exact_gap_skips_the_waiting_time_unchecked() {
    // Stage b starts exactly 2 h after a ends. Hour 1 and 2 are stormy but nobody checks them.
    let s = winds(&[1.0, 99.0, 99.0, 1.0, 1.0]);
    let plan = two_stage(2, 2);
    let result = WindowSearch::new(&s, &plan).run().unwrap();
    assert_eq!(feasible_starts(&s, &plan), [0]);
    let window = &result.feasible[0];
    assert_eq!(window.end_ms, 4 * H);
    assert_eq!(window.stages[1].start_ms, 3 * H);
    assert_eq!(window.stages[1].gap_ms, 2 * H);
    assert_eq!(window.stages[0].gap_ms, 0);
    // Start 1: a is stormy. Start 2 would need hour 5, which does not exist, so it is not a window.
    assert_eq!(result.rejected.len(), 1);
}

#[test]
fn range_picks_a_gap_that_works() {
    // Gap of 1 to 3 hours: only a 3 hour gap lands on calm weather.
    let s = winds(&[1.0, 9.0, 9.0, 9.0, 1.0]);
    let plan = two_stage(1, 3);
    let result = WindowSearch::new(&s, &plan).run().unwrap();
    assert_eq!(feasible_starts(&s, &plan), [0]);
    assert_eq!(result.feasible[0].stages[1].gap_ms, 3 * H);
    // A window can end later than the minimum span; the rejected ones report the minimum.
    assert_eq!(result.feasible[0].end_ms, 5 * H);
}

#[test]
fn best_suitability_wins_then_the_earliest_gap() {
    // Second stage prefers calm air; gaps 0..=2 give winds 4, 1, 1 -> gap 1 (first of the ties).
    let mut second = Stage::new("second", H, calm(5.0)).with_gap(0, 2 * H);
    second.constraints.push(Constraint::soft(
        "calm air",
        Metric::new("wind_speed"),
        Preference::Minimize {
            ideal: 1.0,
            scale: 10.0,
        },
        1.0,
    ));
    let plan = Plan::new("p", vec![Stage::new("first", H, calm(5.0)), second]);
    let s = winds(&[1.0, 4.0, 1.0, 1.0]);
    let result = WindowSearch::new(&s, &plan).run().unwrap();
    let w = result.feasible.iter().find(|w| w.start_ms == 0).unwrap();
    assert_eq!(w.stages[1].gap_ms, H);
    assert_eq!(w.suitability, 1.0);
}

#[test]
fn rejection_reports_the_layout_that_got_furthest() {
    // First stage passes at hour 0; the second fails wherever it can be placed.
    let s = winds(&[1.0, 9.0, 9.0, 9.0]);
    let plan = two_stage(1, 3);
    let result = WindowSearch::new(&s, &plan).run().unwrap();
    assert!(result.feasible.is_empty());
    let r = result.rejected.iter().find(|r| r.start_ms == 0).unwrap();
    assert_eq!(r.failure.stage, "second");
    assert_eq!(r.failure.timestamp_ms, 2 * H); // the earliest gap (1 h) is the one reported
    assert_eq!(r.end_ms, 3 * H);
    // A window whose first stage already fails reports the first stage.
    let r1 = result.rejected.iter().find(|r| r.start_ms == H).unwrap();
    assert_eq!(r1.failure.stage, "first");
}

#[test]
fn three_stages_chain_their_gaps() {
    // a at 0, b 1-2 h later, c 1-2 h after b. Calm only at hours 0, 2 and 5.
    let s = winds(&[1.0, 9.0, 1.0, 9.0, 9.0, 1.0]);
    let plan = Plan::new(
        "p",
        vec![
            Stage::new("a", H, calm(5.0)),
            Stage::new("b", H, calm(5.0)).with_gap(H, 2 * H),
            Stage::new("c", H, calm(5.0)).with_gap(H, 2 * H),
        ],
    );
    let result = WindowSearch::new(&s, &plan).run().unwrap();
    assert_eq!(feasible_starts(&s, &plan), [0]);
    let gaps: Vec<i64> = result.feasible[0]
        .stages
        .iter()
        .map(|st| st.gap_ms / H)
        .collect();
    assert_eq!(gaps, [0, 1, 2]);
}

#[test]
fn gap_plans_are_validated() {
    let s = winds(&[1.0; 6]);
    let on_first = Plan::new("p", vec![Stage::new("a", H, calm(5.0)).with_gap(H, H)]);
    assert!(matches!(
        WindowSearch::new(&s, &on_first).run(),
        Err(ValidationError::InvalidGap { .. })
    ));
    let off_grid = Plan::new(
        "p",
        vec![
            Stage::new("a", H, calm(5.0)),
            Stage::new("b", H, calm(5.0)).with_gap(H / 2, H),
        ],
    );
    assert!(matches!(
        WindowSearch::new(&s, &off_grid).run(),
        Err(ValidationError::InvalidGap { .. })
    ));
    assert_eq!(Gap::new(2, 1), None);
    assert_eq!(Gap::new(-1, 1), None);
    // Many stages with wide ranges would explode: refused up front.
    let stage = |i: usize| {
        let s = Stage::new(format!("s{i}"), H, calm(5.0));
        if i == 0 {
            s
        } else {
            s.with_gap(0, 20 * H)
        }
    };
    let wide = Plan::new("p", (0..6).map(stage).collect());
    assert_eq!(
        WindowSearch::new(&s, &wide).run().unwrap_err(),
        ValidationError::TooManyGapOptions
    );
    assert!(21u64.pow(5) > MAX_ARRANGEMENTS);
}

#[test]
fn a_series_too_short_for_the_minimum_span_gives_nothing() {
    let s = winds(&[1.0, 1.0, 1.0]);
    let plan = two_stage(3, 5);
    assert!(feasible_starts(&s, &plan).is_empty());
    assert!(WindowSearch::new(&s, &plan)
        .run()
        .unwrap()
        .rejected
        .is_empty());
}

/// The tutorial's paint-the-fence example must keep giving the answer it prints.
#[cfg(feature = "json")]
#[test]
fn tutorial_paint_example() {
    use wint::adapters::csv;
    let read = |f: &str| {
        std::fs::read_to_string(format!("{}/examples/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    };
    let series = csv::parse(&read("paint-series.csv")).unwrap();
    let plan: Plan = serde_json::from_str(&read("paint-plan.json")).unwrap();
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    let best = &result.feasible[0];
    // Saturday 2026-10-10 08:00 UTC; the clear coat goes on at 18:00 after an 8 hour wait.
    let sat_8 = series.observations[0].timestamp_ms;
    assert_eq!(best.start_ms, sat_8);
    assert_eq!(best.stages[1].start_ms, sat_8 + 10 * H);
    assert_eq!(best.stages[1].gap_ms, 8 * H);
    assert_eq!((result.feasible.len(), result.rejected.len()), (21, 9));
    // Paint starts between 09:00 and 17:00 fail while painting, not in the clear coat.
    assert!(result
        .rejected
        .iter()
        .all(|r| r.failure.stage == "paint" && r.start_ms > sat_8 && r.start_ms < sat_8 + 10 * H));
}

fn member(name: &str, s: Series) -> Member {
    Member {
        name: name.into(),
        series: s,
    }
}

#[test]
fn ensemble_members_may_choose_different_gaps() {
    // Gap 1..=2. Member x is calm 1 h later, member y only 2 h later; both fit.
    let x = member("x", winds(&[1.0, 9.0, 1.0, 9.0]));
    let y = member("y", winds(&[1.0, 9.0, 9.0, 1.0]));
    let ensemble = Ensemble::new(vec![x, y]).unwrap();
    let plan = two_stage(1, 2);
    let result = EnsembleSearch::new(&ensemble, &plan).run().unwrap();
    let w = result.windows.iter().find(|w| w.start_ms == 0).unwrap();
    assert_eq!((w.feasible, w.infeasible), (2, 0));
    assert!(w.meets_requirement);
    assert_eq!(w.end_ms, 3 * H); // earliest possible end
}

#[test]
fn ensemble_missing_reading_inside_the_gap_range_is_named() {
    // Member z lacks wind at hour 2, the only place its second stage could go with a 1 h gap...
    // but with gap 1..=2 hour 3 is also possible and calm, so z still fits.
    let z = member(
        "z",
        series(&[Some(1.0), Some(9.0), None, Some(1.0), Some(9.0)]),
    );
    let ensemble = Ensemble::new(vec![z]).unwrap();
    let plan = two_stage(1, 2);
    let result = EnsembleSearch::new(&ensemble, &plan).run().unwrap();
    let w = result.windows.iter().find(|w| w.start_ms == 0).unwrap();
    assert_eq!(w.outcomes[0].verdict, Verdict::Feasible);
    // With only a 1 h gap the missing reading decides the window: it cannot be judged.
    let strict = two_stage(1, 1);
    let result = EnsembleSearch::new(&ensemble, &strict).run().unwrap();
    let w = result.windows.iter().find(|w| w.start_ms == 0).unwrap();
    assert_eq!(w.outcomes[0].verdict, Verdict::Unknown);
    assert_eq!(w.missing[0].metric, "wind_speed");
}

#[cfg(feature = "json")]
mod json {
    use super::*;

    #[test]
    fn gap_wire_form() {
        let plan: Plan = serde_json::from_str(
            r#"{"name":"p","stages":[
                {"name":"a","duration_ms":3600000,"constraints":[]},
                {"name":"b","duration_ms":3600000,"constraints":[],
                 "gap":{"min_ms":3600000,"max_ms":7200000}},
                {"name":"c","duration_ms":3600000,"constraints":[],"gap":{"min_ms":3600000}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(plan.stages[0].gap, None);
        assert_eq!(plan.stages[1].gap, Gap::new(H, 2 * H));
        assert_eq!(plan.stages[2].gap, Gap::exactly(H)); // max defaults to min
        let text = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
        // A plan without gaps does not mention them.
        let plain = Plan::single_stage("p", H, vec![]);
        assert!(!serde_json::to_string(&plain).unwrap().contains("gap"));
        assert!(serde_json::from_str::<Stage>(
            r#"{"name":"b","duration_ms":3600000,"constraints":[],"gap":{"min_ms":5,"max_ms":1}}"#
        )
        .is_err());
    }
}
