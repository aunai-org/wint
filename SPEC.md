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

## Time of day

Two separate mechanisms cover "daytime only", "until 8pm", "9am-12pm" and "overnight":

- **Clock windows.** A stage may carry a `schedule` (`{"from": "09:00", "to": "12:00"}`). The whole stage must lie inside the window on the *local clock of the data*. `from` later than `to` wraps midnight (`20:00`-`06:00`); `00:00`-`20:00` means "until 8pm"; `to` may be `24:00`. Edges are half-open like every range in wint, so a 2-hour stage fits `09:00-12:00` when it starts at 09:00 or 10:00, never at 11:00. The check runs per sample interval, in time order with the other checks, so the first failure is reported with a readable note (`08:00 to 09:00 local (UTC+02:00)`). Schedules are per stage, so a survey stage can be daytime-only while recovery is unrestricted.
- **Sunrise/sunset.** Day and night depend on place and date, so they are data, not clock windows: use the `is_day` metric (1 in daylight, 0 at night; Open-Meteo supplies it) in an ordinary hard constraint (`is_day == 1` or `== 0`). A missing value rejects the window like any other metric.

The local clock is a property of where the data is, so the **series** carries `utc_offset_minutes` (default 0, range -12:00 to +14:00) and the Open-Meteo adapter fills it from the API (`timezone=auto`). Only one fixed offset is modelled: a forecast that crosses a daylight-saving change shifts by an hour after it. Weekday rules ("weekends only") are not included yet.

## Uncertainty: ensembles and agreement

A forecast is an estimate, so "the window fits" depends on which forecast you believe. An **ensemble** is several parallel versions of one forecast on a shared time grid: different weather models, or the members of one model's ensemble. `EnsembleSearch` runs the ordinary window search on every member and reports, per window, how many members it fits.

Each member gives a window one of three **verdicts**:

- **fits**: every hard constraint passes in that member;
- **does not fit**: some hard constraint, or the time-of-day check, is violated;
- **cannot say**: the member lacks a reading the plan needs (a model that does not forecast visibility, or whose horizon ends before the window) and nothing else rules the window out. A definite violation elsewhere in the window still counts as "does not fit".

"Cannot say" is not disagreement. A single series with a missing reading still fails (missing data is never a silent pass), but in an ensemble an abstaining member simply does not vote, and the gap is shown as **coverage**.

Per window the result reports `fits`, `does not fit` and `cannot say` counts; **agreement** = fits / (fits + does not fit), absent if nobody could answer; **coverage** = members that could answer / all members; the mean preference score over the members where it fits; the **blockers** (which rules ruled it out, and in how many members) and the **missing data** (every hard-rule metric some members lacked in that window, with counts). The result also lists the **unprovided** metrics: those the plan's hard rules need and that no member provides in any hour, so no window can be judged on them. A window **meets the requirement** when agreement is at least `min_agreement` (default 1.0: every member that can answer) and coverage is at least `min_coverage` (default 0.5). Both are explicit and must be in (0, 1]. Windows are ordered by: meets the requirement, agreement, coverage, preference score, earliest start. Windows that fail are kept in the list, because "fits in 4 of 5 models" is useful information.

**What agreement is not.** It is not the probability that the operation will be possible. Ensemble members are not calibrated frequencies, different models share data and assumptions so they are not independent, and an ensemble can be too narrow. Always present it as "fits in `k` of `n`" and never as a percentage chance. All members are weighted equally.

### Presenting results (guidance for applications)

The engine returns facts; an application decides how to show them. These rules keep a display honest, and the [demo](https://github.com/aunai-org/wint-demo#how-to-read-the-display-the-rules) follows them (its README lists the exact colors):

1. **Count, do not percent.** Say "fits in 3 of 4 forecast versions that can answer". Do not show agreement as a percentage chance or a confidence.
2. **Keep abstentions separate.** "Cannot say" is not disagreement. Show how many versions could not answer and name every metric they lacked (`missing`), so a window that "fits in 2 of 2" with two silent versions is not read as "fits in 4 of 4".
3. **Say what bar you used.** State the `min_agreement` and `min_coverage` in force next to the result, and keep windows that miss the bar visible, because "fits in 4 of 5" is information.
4. **Color by agreement; mark the requirement separately.** Let color describe how much the versions agree (all, some, none, nobody could answer) and show "meets your requirement" as a second, independent mark. Then changing the bar never makes a split window look unanimous.
5. **Name unprovided metrics first.** If `unprovided` is not empty, say so before listing windows: no window can be judged on those rules, and an empty result there is not good news.
6. **Explain on demand.** Offer the `blockers` for each window and every version's verdict and reason (`outcomes`) one click away.
7. **A missing reading in one series cannot be approved.** In a single series the engine never passes a window on missing data. Show it as "cannot be approved", neither safe nor unsafe.
8. **Derive colors from the number the user reads.** If a score is shown with two decimals, apply any threshold to the rounded value, so a window labelled 0.80 is never colored "under 0.8".
9. **Never present a score or an agreement as a safety certification.** Scores rank windows that already pass the hard limits; agreement counts forecast versions.

Members must share cadence, timestamps and UTC offset exactly (`Ensemble::new` checks this), so that a window is the same span of time in every member.

**Open-Meteo input.** `adapters::open_meteo::parse_ensemble` reads both services that return parallel versions. The *multi-model* forecast (`models=a,b,c`) names columns `<variable>_<model>`; the *ensemble* API names them `<variable>_member01` and so on, plus an unsuffixed control run. A variable with a single unsuffixed column (`is_day`) is shared by every member. A column that is entirely `null` carries no readings and is skipped (the ensemble API returns visibility and rain probability this way, with the unit `undefined`); a column with real values and an unrecognised unit is an error, never a guess. The consequence is visible and intended: rules that use a metric nobody provides cannot be judged, every window reports "cannot say", and the tools say so by name instead of silently passing or failing. Real examples seen at the time of writing: in a 4-model forecast ECMWF and Meteo-France provided no visibility; the ensemble API provided none for any member.

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

`Stage::with_schedule` / `Plan::with_schedule` attach clock windows, and `Series::with_utc_offset` sets the local clock. Key API: `Series::new(cadence_ms, observations) -> Result<Series, ValidationError>`, `Plan::single_stage(...)`, `Plan::new(...)`, and `WindowSearch::new(&series, &plan).run() -> Result<SearchResult, ValidationError>`. `run` validates the plan first (`Plan::validate`), so malformed plans are errors, never silent empty results.

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
