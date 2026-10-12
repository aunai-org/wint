# Changelog

All notable changes to wint are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and wint is 0.x: the Rust API can still change between minor versions. The JSON result shape has its own `schema_version` (see the Stability section of the [specification](SPEC.md)); a change to it is always listed here.

## [Unreleased]

### Added
- README badges for crates.io, docs.rs and npm.
- The CLI text output shows the wait between stages (for example `wait 8 h`).
- This changelog.

## [0.1.0] - 2026-10-06

First release. Published as the Rust crate `wint` and the npm package `wint-core`. Result `schema_version` is `1`.

### Added
- Operability engine: finds the time windows in which a plan of staged hard and soft constraints is satisfied, ranks them, and returns the evidence behind every accepted and rejected window as data.
- Hard constraints (`<`, `<=`, `>`, `>=`, `==`) and soft preferences (`minimize`, `maximize`, `range`) with an explicit `scale`; invalid plans return a validation error.
- Per-stage results, with evidence reduced to the binding sample per constraint.
- Time of day: per-stage local clock windows (including overnight), weekday filters, series UTC offset and `is_day`.
- Gaps between stages: a minimum and maximum wait before a stage.
- Ensembles: per-window agreement across several forecast versions, with fits / does not fit / cannot say verdicts, coverage, blockers and missing data. Agreement is not a probability.
- Adapters for series JSON, CSV and Open-Meteo (single model, multi-model and ensemble), a shared metric vocabulary with unit conversion, and preset plans (`drone`, `outdoor-event`, `field-work`) that are starting points, not safety guidance.
- Command line (`json` feature, `net` for live forecasts) and WebAssembly bindings (`wasm` feature, `scripts/build-wasm.sh`).
- Documentation: specification, tutorial, worked examples and a concepts guide.

[Unreleased]: https://github.com/aunai-org/wint/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/aunai-org/wint/releases/tag/v0.1.0
