# Roadmap

Guiding principle: get a trustworthy, explained answer from real data in minutes, keeping the core neutral. Domain knowledge (units, presets, adapters) lives in optional layers around it.

## M0 - Trustworthy base
- [x] Explicit `scale` on soft preferences (fixes saturating penalties)
- [x] Invalid plans return `ValidationError` instead of empty results
- [x] Plan longer than series returns an empty result (previously a slice panic)
- [x] Integration tests, CI (fmt, clippy, test), README example corrected
- [x] Decided: use data as given. No cadence-boundary alignment and no resampling (see SPEC)
- [ ] Replace placeholder `repository` URL in Cargo.toml

## M1 - Usable from anywhere
- [x] Optional serde JSON (`json` feature) for series, plans and results; JSON series are validated
- [x] Working CLI: plan + series in, ranked windows out (text or `--json`), with example files
- [x] Per-stage breakdown in results; evidence reduced to the binding sample per constraint

## M2 - Real data
- [x] Open-Meteo (parser + URL builder, CLI fetch behind `net`) and CSV adapters
- [x] Metric vocabulary with units and conversion
- [x] First preset plans (drone, outdoor-event, field-work), labelled as starting points, not safety advice
- [x] Tutorial (docs/TUTORIAL.md)
- [x] Live Open-Meteo fetch verified from the CLI against the real API (all 11 variables, units as assumed); the `net` build trusts the OS certificate store so TLS-intercepting proxies work
- [ ] Verify the live fetch from a real browser (demo)
- [x] Time-of-day windows: per-stage local clock windows (incl. overnight), series UTC offset, `is_day` from Open-Meteo
- [ ] Weekday rules ("weekends only") and per-observation UTC offsets across daylight-saving changes
- [ ] Opt-in resampling/alignment step, only once a real use case needs it
- [ ] Marine data (wave height) adapter, e.g. Open-Meteo Marine API
- [ ] Review preset thresholds with domain practitioners

## M3 - Reach
- [x] WASM build (`wasm` feature, `scripts/build-wasm.sh`, Node smoke test, CI job)
- [x] Demo web app, in its own repository (`wint-demo`), vendoring the built package
- [ ] Publish an npm package so the demo need not vendor files
- [ ] Host the demo (e.g. GitHub Pages) and verify live forecasts from a browser
- [ ] Python bindings

## M4 - Depth (branch `mile4`)
Uncertainty first, then the smaller items.

**M4.1 Engine: ensembles and agreement**
- [x] `Ensemble` / `EnsembleSearch`: per-window agreement across members, three verdicts (fits / does not fit / cannot say), coverage, blockers, missing data, explicit `min_agreement` and `min_coverage`
- [x] Tolerant missing-reading mode so abstaining members are not counted as disagreement
- [x] SPEC wording rules: agreement is not a probability

**M4.2 Data: multi-model and ensemble input**
- [x] Open-Meteo multi-model forecast adapter (`models=` returns per-model keys; some models omit variables or end early, both handled as "cannot say")
- [x] Open-Meteo ensemble API adapter (31-51 members; shared `is_day`; all-null columns with unit `undefined` skipped, real values with an unknown unit are an error)
- [x] CLI: `--models`, `--ensemble`, `--min-agreement`, `--min-coverage`, ensemble report, saved-response formats
- [x] Verified live against the real multi-model and ensemble APIs, and fixtures are trimmed real responses
- [ ] Ensemble service lacks visibility and rain probability: consider a second source for those, or presets with ensemble-friendly rules

**M4.3 Reach and demo**
- [ ] WASM: ensemble search; npm-ready package
- [ ] Demo (branch `mile4` in wint-demo): agreement colours on the rail and strip, "fits in k of n", spread bands on the chart, per-model lines, what blocked each window

**M4.4 Remaining**
- [ ] Temporal predicates and gaps between stages
- [ ] Stable schemas and API (freeze JSON formats and the Rust/JS interfaces)
- [ ] Weekday rules; daylight-saving-aware offsets
