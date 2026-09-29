# wint - Weather Intelligence

`wint` is a deterministic Rust engine for finding and explaining time windows in environmental time-series data that meet declared operating limits.

It deliberately starts below data ingestion and user interfaces: callers supply normalized observations and receive ranked feasible windows. Version 0.1 has no network access, weather-provider coupling, AI, or geospatial file parsers.

See [the specification](SPEC.md) for the product and implementation plan.
New to the field? Start with the [plain-language learning guide](LEARNING_GUIDE.md).

## Command line

The CLI reads JSON and is behind the optional `json` feature (the library core has no dependencies):

```sh
cargo run --features json -- --plan examples/plan.json --series examples/series.json
cargo run --features json -- --plan examples/plan.json --series examples/series.json --json
```

Options: `--top <n>`, `--rejected` (why each rejected window failed), `--json` (full machine-readable result). See [`examples/`](examples) for the input format: a series is `{cadence_ms, observations: [{timestamp_ms, values}]}` and a plan is `{name, stages: [{name, duration_ms, constraints}]}`. Hard constraints use `"comparison": "<" | "<=" | ">" | ">=" | "=="`; soft constraints take a `preference` of kind `minimize`, `maximize` or `range` (each with a `scale`) and an optional `weight` (default 1). JSON series go through the same validation as `Series::new`.

## Quick example

```rust
use wint::{Comparison, Constraint, Metric, Observation, Plan, Series, WindowSearch};

let series = Series::new(3_600_000, vec![
    Observation::at(0).with("wind", 12.0).with("rain_probability", 10.0),
    Observation::at(3_600_000).with("wind", 15.0).with("rain_probability", 15.0),
    Observation::at(7_200_000).with("wind", 8.0).with("rain_probability", 10.0),
]).unwrap();
let plan = Plan::single_stage("field-work", 7_200_000, vec![
    Constraint::hard("safe wind", Metric::new("wind"), Comparison::LessThanOrEqual, 20.0),
    Constraint::hard("low rain chance", Metric::new("rain_probability"), Comparison::LessThan, 20.0),
]);

let result = WindowSearch::new(&series, &plan).run().unwrap();
// Windows starting at 0h and 1h both satisfy every limit.
assert_eq!(result.feasible.len(), 2);
```

`run()` returns an error if the plan is invalid for the series (empty plan, misaligned stage durations, malformed constraints).

Timestamps and durations are signed Unix milliseconds. All ranges are half-open: `[start, end)`.
