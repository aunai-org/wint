# wint - An Environmental Operability Engine — product and technical specification

## Vision and problem

Environmental data is commonly presented as forecasts, charts, or domain-specific go/no-go advice. Operators instead need an auditable answer to: **when can this operation be performed, for how long, and why?**

`wint` is an open-source, domain-neutral Rust library and CLI that turns normalized environmental time series plus declared operating rules into feasible, ranked, explainable time windows. It is an engine, not a forecast provider or a vertical application. It can power marine workability, field work, drone operations, observatory scheduling, events, maintenance, or industrial planning without embedding any one domain’s policy.

## Scope

The engine accepts regular, normalized point observations and declarative plans; searches contiguous windows; enforces hard limits; ranks survivors with deterministic soft preferences; and returns per-window evidence. It supports sequential stages so different phases may have different operating envelopes.

Non-goals for v0.1: data collection or weather APIs; probabilistic forecasting/model ensembles; UI; AI advice; geographic interpolation; authentication; scheduling/resources; GRIB, NetCDF, or vendor-format parsing. These are adapter or application concerns.

## Users and examples

Library/application developers integrate the search primitive. Domain engineers author reusable plans. Operations users consume the application built on top.

Examples: a drone team requires two safe flight hours followed by one recovery hour; a marine operator needs four hours below wave and gust limits; a venue selects dry event windows while preferring milder temperatures; a research team searches a measurement phase before a low-wind collection phase.

## Core concepts and semantics

- **Series**: regular cadence, ordered observations. An observation maps metric names to finite numeric values. Time is Unix milliseconds.
- **Plan**: ordered, contiguous stages. The operation duration is the sum of stage durations.
- **Candidate window**: a half-open interval `[start, end)` beginning at an observation timestamp. A stage covers a contiguous subinterval.
- **Hard constraint**: every sample in its stage must provide the metric and satisfy its comparison. A failed/missing value rejects the candidate.
- **Soft constraint**: an ideal value/range with a non-negative weight. Each relevant sample contributes a penalty `min(deviation / scale, 1)`, where `scale` is the caller-declared deviation that counts as a full penalty (positive, in the metric's unit); final suitability is `1 - weighted_mean(penalty)`.
- **Temporal constraint**: an explicit relationship between stages/intervals, such as `Before`, `After`, or a permitted gap. Adjacent stages in v0.1 are inherently `before/after`; richer named temporal predicates are planned for v0.2.
- **Contiguous duration**: all samples whose timestamps fall in the interval must satisfy applicable constraints. A candidate is only considered when the full duration is covered by the series.
- **Staged operation**: sequential stages with independently declared limits. No idle gaps exist in v0.1.

Observations use a regular cadence. A stage duration must be a positive multiple of the cadence. `Series::new` validates strictly increasing, exactly regular timestamps; this makes coverage and outcomes unambiguous.

**Data is used as given.** The engine does not resample, interpolate, or require timestamps to sit on clock boundaries: a series may start at 08:17 as long as every step is exactly one cadence, and windows start at the observation times supplied. Adapters (CSV, Open-Meteo) pass data through unchanged for the same reason, so nothing is silently assumed. Resampling or alignment is deferred until a real use case needs it and would arrive as an explicit, opt-in adapter step.

## Data model

```text
Series { cadence_ms, observations: [Observation { timestamp_ms, values }] }
Plan { name, stages: [Stage { name, duration_ms, hard, soft }] }
HardConstraint { name, metric, comparison, threshold }
SoftConstraint { name, metric, preference, weight }
Preference: Minimize { ideal, scale } | Maximize { ideal, scale } | Range { min, max, scale }
WindowResult { start_ms, end_ms, suitability, stages: [StageResult { name, start_ms, end_ms, suitability }], evidence }
```

Metrics are strings. The engine treats them as opaque and never sees units: it compares numbers. Alongside it, a shared **vocabulary** (`units::VOCABULARY`: `wind_speed`, `wind_gust`, `temperature`, `precipitation`, `precipitation_probability`, `cloud_cover`, `relative_humidity`, `visibility`, `wave_height`, `pressure`, `wind_direction`) fixes one canonical, SI-leaning unit per metric (m/s, °C, mm, %, m, hPa, °). Conversion happens only at the edges: `Series::with_units` / the series `units` JSON map for readings, and `Constraint::with_unit` / the constraint `unit` JSON field for limits (absolute values convert with offsets, a preference `scale` converts as a difference, so 18 °F of scale is 10 °C). Caller-defined metrics outside the vocabulary are allowed but are never converted, and attaching a unit to one is an error. Provenance, quality flags and ensemble members remain future work.

## Outputs, scoring, and explainability

The result separates feasible windows from rejected candidates. Feasible windows are sorted by descending suitability then ascending start time. Evidence records the stage, constraint, sample timestamp, observed value, expected relation, pass/fail status and (for soft constraints) the penalty. To keep output bounded, a feasible window reports one *binding* sample per constraint: the sample closest to a hard limit, or with the largest soft penalty (earliest on ties). Each feasible window also carries a per-stage breakdown (time range and stage-local suitability). Rejections preserve the first decisive failure in time order. Score calculations are deterministic and use only supplied values; score is not a probability or safety certification.

## Architecture

The core has no dependencies and is pure/in-memory:

```text
adapters (future) -> normalized Series -> search/evaluation -> WindowSearchResult
                                  plans -> constraints/scoring --^
CLI (example) --------------------------------------------------^
```

Separating adapters protects the core from provider-specific units, interpolation, and forecast policy. The optional `wasm` feature (implies `json`) exposes the same operations to JavaScript via wasm-bindgen; it adds no logic of its own. The optional `json` feature adds serde (de)serialization of series, plans and results plus the CLI; JSON series deserialize through `Series::new`, so validation cannot be bypassed. Later crates may add CSV adapters and source-specific integrations.

## Crate layout and API

```text
src/
  lib.rs          public API
  model.rs        Series, observations, plans, constraint definitions
  engine.rs       candidate search, validation, evidence, ranking
  units.rs        metric vocabulary, unit conversion
  time.rs         ISO 8601 <-> Unix ms helpers
  adapters/       csv (no deps), open_meteo (json feature): series from external data
  presets.rs      illustrative starting-point plans
  wasm.rs         JavaScript bindings (`wasm` feature): JSON in, JSON out
  main.rs         CLI (requires the `json` feature; `net` adds live fetching)
```

Key API: `Series::new(cadence_ms, observations) -> Result<Series, ValidationError>`, `Plan::single_stage(...)`, `Plan::new(...)`, and `WindowSearch::new(&series, &plan).run() -> Result<SearchResult, ValidationError>`. `run` validates the plan first (`Plan::validate`), so malformed plans are errors, never silent empty results.

## MVP v0.1 acceptance requirements

1. Validate regular, finite, strictly ordered in-memory observations and cadence-aligned positive stage durations.
2. Search every cadence-aligned candidate that has complete coverage.
3. Implement numeric `<`, `<=`, `>`, `>=`, and `==` hard limits; a missing metric rejects a candidate.
4. Implement soft minimization, maximization, and ideal range scoring with weights.
5. Implement ordered multi-stage operations and stage-level evidence.
6. Return deterministic ranking and first-failure rejection evidence.
7. Expose a documented dependency-free Rust library with unit and integration-style tests.

## Testing strategy

Unit tests cover series validation, each comparison, boundary semantics, missing metrics, duration coverage, soft score math, sorting ties, and staged boundaries. Property tests (after adding `proptest`) will assert that a feasible output always satisfies every applicable hard constraint and that invalid series cannot construct. Fixture tests will hold canonical marine/drone/event examples. Fuzzing and benchmark suites follow when parsing and larger datasets arrive.

## Roadmap

- **v0.1:** deterministic in-memory core described above.
- **v0.2:** serde JSON schema, CSV adapter, named stages, temporal predicates/gaps, unit metadata, richer diagnostics.
- **v0.3:** uncertainty and multi-model agreement, aggregation/interpolation policies, calendar/resource constraints, CLI.
- **v1.0:** stable schemas/API, adapter ecosystem, performance benchmarks, operational audit export.

## Risks and decisions

Threshold semantics, sampling boundaries, units, and missing data can create false confidence. v0.1 addresses this by strict regular-series validation, half-open ranges, caller-owned normalized units, explicit missing-data failure, and deterministic evidence. It does not infer safety margins. Forecast uncertainty will be represented explicitly before it affects ranking. Keep the core numeric and policy-neutral to avoid locking into weather terminology.

## License and repository conventions

Use dual `Apache-2.0 OR MIT` licensing, a Rust 2021 library-first crate, `rustfmt` formatting, `clippy -D warnings` in CI, conventional commits, and a changelog. Public behavior changes require tests and specification updates. Keep core modules dependency-free; optional integrations belong in feature-gated or separate crates. Include `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, and `SECURITY.md` before public release.

## Immediate implementation tasks

1. Establish the crate, public types, validation invariants, and deterministic search loop.
2. Add hard/soft evaluation, evidence, scoring, sorting, and test fixtures.
3. Add a JSON/CSV boundary only after the in-memory API is reviewed.
4. Design v0.2 temporal predicate and unit/provenance schemas against real domain adapters.
