//! Property tests: whatever the data and plan, the engine's verdicts agree with a direct re-check
//! of the raw readings. Feasible windows satisfy every hard constraint; rejections name a real failure.

use proptest::prelude::*;
use wint::{
    Comparison, Constraint, Evidence, Metric, Observation, Plan, Preference, Series, Stage,
    WindowSearch,
};

const H: i64 = 3_600_000;
const METRICS: [&str; 2] = ["wind", "rain"];

fn comparison() -> impl Strategy<Value = Comparison> {
    prop_oneof![
        Just(Comparison::LessThan),
        Just(Comparison::LessThanOrEqual),
        Just(Comparison::GreaterThan),
        Just(Comparison::GreaterThanOrEqual),
        Just(Comparison::Equal),
    ]
}

/// A hard constraint on one of the metrics. Small integer thresholds make equality and ties likely.
fn hard(index: usize) -> impl Strategy<Value = Constraint> {
    (0..METRICS.len(), comparison(), 0u8..8).prop_map(move |(m, cmp, t)| {
        Constraint::hard(
            format!("hard{index}"),
            Metric::new(METRICS[m]),
            cmp,
            f64::from(t),
        )
    })
}

fn soft() -> impl Strategy<Value = Constraint> {
    (0..METRICS.len(), 0u8..8, 1u8..8).prop_map(|(m, ideal, scale)| {
        Constraint::soft(
            "soft",
            Metric::new(METRICS[m]),
            Preference::Minimize {
                ideal: f64::from(ideal),
                scale: f64::from(scale),
            },
            1.0,
        )
    })
}

/// One to three stages of one to three hours; later stages may wait up to two hours.
fn plan() -> impl Strategy<Value = Plan> {
    let stage = (
        1i64..=3,
        0i64..=2,
        prop::collection::vec(hard(0), 0..3),
        prop::option::of(soft()),
    );
    prop::collection::vec(stage, 1..=3).prop_map(|stages| {
        let stages = stages
            .into_iter()
            .enumerate()
            .map(|(i, (hours, wait, hards, soft))| {
                let mut constraints: Vec<Constraint> = hards
                    .into_iter()
                    .enumerate()
                    .map(|(j, c)| match c {
                        Constraint::Hard {
                            metric,
                            comparison,
                            threshold,
                            ..
                        } => Constraint::hard(format!("s{i}h{j}"), metric, comparison, threshold),
                        other => other,
                    })
                    .collect();
                constraints.extend(soft);
                let stage = Stage::new(format!("stage{i}"), hours * H, constraints);
                if i > 0 {
                    stage.with_gap(0, wait * H)
                } else {
                    stage
                }
            })
            .collect();
        Plan::new("p", stages)
    })
}

/// Hourly readings for both metrics, about one in six missing.
fn series() -> impl Strategy<Value = Series> {
    let reading = prop::option::weighted(0.85, 0u8..10);
    prop::collection::vec((reading.clone(), reading), 1..24).prop_map(|rows| {
        let observations = rows
            .into_iter()
            .enumerate()
            .map(|(i, (wind, rain))| {
                let mut o = Observation::at(i as i64 * H);
                if let Some(v) = wind {
                    o = o.with("wind", f64::from(v));
                }
                if let Some(v) = rain {
                    o = o.with("rain", f64::from(v));
                }
                o
            })
            .collect();
        Series::new(H, observations).unwrap()
    })
}

/// Hard constraints of `stage` that a reading at `timestamp_ms` violates (a missing reading violates).
fn violated(series: &Series, stage: &Stage, timestamp_ms: i64) -> Vec<String> {
    let obs = series
        .observations
        .iter()
        .find(|o| o.timestamp_ms == timestamp_ms)
        .expect("timestamp belongs to the series");
    stage
        .constraints
        .iter()
        .filter_map(|c| match c {
            Constraint::Hard {
                name,
                metric,
                comparison,
                threshold,
            } => {
                let ok = obs
                    .values
                    .get(&metric.0)
                    .is_some_and(|v| comparison_holds(*comparison, *v, *threshold));
                (!ok).then(|| name.clone())
            }
            _ => None,
        })
        .collect()
}

fn comparison_holds(c: Comparison, actual: f64, threshold: f64) -> bool {
    match c {
        Comparison::LessThan => actual < threshold,
        Comparison::LessThanOrEqual => actual <= threshold,
        Comparison::GreaterThan => actual > threshold,
        Comparison::GreaterThanOrEqual => actual >= threshold,
        Comparison::Equal => actual == threshold,
        _ => unreachable!("test only generates known comparisons"),
    }
}

fn assert_real_failure(series: &Series, plan: &Plan, start_ms: i64, failure: &Evidence) {
    assert!(!failure.passed, "a rejection's evidence must be a failure");
    let stage = plan
        .stages
        .iter()
        .find(|s| s.name == failure.stage)
        .expect("failure names a stage of the plan");
    assert!(
        failure.timestamp_ms >= start_ms,
        "failure lies before its window"
    );
    let broken = violated(series, stage, failure.timestamp_ms);
    assert!(
        broken.contains(&failure.constraint),
        "{} did not fail at {} (violated: {broken:?})",
        failure.constraint,
        failure.timestamp_ms
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every feasible window has every stage inside the series, in order, and every sample of every
    /// stage satisfies every hard constraint of that stage.
    #[test]
    fn feasible_windows_satisfy_every_hard_constraint(series in series(), plan in plan()) {
        let result = WindowSearch::new(&series, &plan).run().unwrap();
        for window in &result.feasible {
            prop_assert_eq!(window.stages.len(), plan.stages.len());
            let mut previous_end = window.start_ms;
            for (stage, got) in plan.stages.iter().zip(&window.stages) {
                prop_assert_eq!(&got.name, &stage.name);
                prop_assert!(got.start_ms >= previous_end);
                prop_assert_eq!(got.end_ms - got.start_ms, stage.duration_ms);
                let mut t = got.start_ms;
                while t < got.end_ms {
                    let broken = violated(&series, stage, t);
                    prop_assert!(broken.is_empty(), "stage {} broke {:?} at {}", stage.name, broken, t);
                    t += H;
                }
                previous_end = got.end_ms;
            }
            prop_assert_eq!(window.stages.last().unwrap().end_ms, window.end_ms);
        }
    }

    /// Every rejected window carries evidence of a hard constraint that really fails at a real sample.
    #[test]
    fn rejected_windows_name_a_real_failure(series in series(), plan in plan()) {
        let result = WindowSearch::new(&series, &plan).run().unwrap();
        for rejected in &result.rejected {
            assert_real_failure(&series, &plan, rejected.start_ms, &rejected.failure);
        }
    }

    /// Each possible start is either feasible or rejected, never both and never dropped.
    #[test]
    fn every_start_is_decided_once(series in series(), plan in plan()) {
        let result = WindowSearch::new(&series, &plan).run().unwrap();
        let mut starts: Vec<i64> = result
            .feasible
            .iter()
            .map(|w| w.start_ms)
            .chain(result.rejected.iter().map(|w| w.start_ms))
            .collect();
        starts.sort_unstable();
        let before = starts.len();
        starts.dedup();
        prop_assert_eq!(before, starts.len());
        let fits = series.observations.len() as i64 >= plan.min_span_ms() / H;
        let expected = if fits {
            series.observations.len() - (plan.min_span_ms() / H) as usize + 1
        } else {
            0
        };
        prop_assert_eq!(before, expected);
    }
}
