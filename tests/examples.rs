//! The non-weather examples in `examples/domains/` are documented in `docs/EXAMPLES.md`; these tests
//! keep the numbers printed there true, and show that nothing in the engine is about weather.
#![cfg(feature = "json")]

use wint::adapters::csv;
use wint::{Plan, SearchResult, Series, WindowSearch};

const H: i64 = 3_600_000;

fn run(domain: &str) -> (Series, SearchResult) {
    let dir = format!("{}/examples/domains/{domain}", env!("CARGO_MANIFEST_DIR"));
    let series =
        csv::parse(&std::fs::read_to_string(format!("{dir}/series.csv")).unwrap()).unwrap();
    let plan: Plan =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/plan.json")).unwrap())
            .unwrap();
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    (series, result)
}
/// Hour of the (UTC, offset 0) day at which a window starts.
fn hour(ms: i64) -> i64 {
    ms.rem_euclid(24 * H) / H
}

#[test]
fn server_deploy_only_runs_overnight_when_quiet() {
    let (_, result) = run("server-deploy");
    assert_eq!((result.feasible.len(), result.rejected.len()), (26, 45));
    // The deploy window is 20:00-06:00 and takes 2 h: starts 20:00..04:00, never in business hours.
    assert!(result
        .feasible
        .iter()
        .all(|w| hour(w.start_ms) >= 20 || hour(w.start_ms) <= 4));
    // The quietest start wins: busy-hour load is 82, evening shoulder 34, deep night 28.
    assert_eq!(result.feasible[0].suitability, 0.925);
}

#[test]
fn ev_charging_uses_cheap_and_clean_hours() {
    let (_, result) = run("ev-charging");
    assert_eq!((result.feasible.len(), result.rejected.len()), (12, 33));
    // 18:00 on, the grid is dirtier than 300 g/kWh until 22:00, so evening starts fail the rule.
    let best = &result.feasible[0];
    assert_eq!(hour(best.start_ms), 0);
    assert!(result.feasible.iter().all(|w| {
        let h = hour(w.start_ms);
        h <= 4 || h >= 22
    }));
}

#[test]
fn bakery_batch_waits_between_mixing_and_baking() {
    let (series, result) = run("bakery-batch");
    assert_eq!((result.feasible.len(), result.rejected.len()), (6, 39));
    let first = &result.feasible[0];
    assert_eq!(first.start_ms, series.observations[10].timestamp_ms); // mix at 10:00
    assert_eq!(first.stages[1].gap_ms, 2 * H); // bake at 13:00, after the shortest allowed proof
                                               // Mixing at 05:00 is rejected: the oven is busy for every allowed bake slot (08:00-10:00).
    let five = result
        .rejected
        .iter()
        .find(|r| hour(r.start_ms) == 5)
        .unwrap();
    assert_eq!(five.failure.stage, "bake");
    assert_eq!(five.failure.constraint, "oven free");
}
