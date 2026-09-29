# Roadmap

Guiding principle: get a trustworthy, explained answer from real data in minutes, keeping the core neutral. Domain knowledge (units, presets, adapters) lives in optional layers around it.

## M0 - Trustworthy base
- [x] Explicit `scale` on soft preferences (fixes saturating penalties)
- [x] Invalid plans return `ValidationError` instead of empty results
- [x] Plan longer than series returns an empty result (previously a slice panic)
- [x] Integration tests, CI (fmt, clippy, test), README example corrected
- [ ] Decide whether `Series` should require timestamps aligned to the cadence
- [ ] Replace placeholder `repository` URL in Cargo.toml

## M1 - Usable from anywhere
- [ ] Optional serde JSON for series, plans and results
- [ ] Working CLI: plan + series in, ranked windows out
- [ ] Per-stage breakdown in results; less noisy evidence

## M2 - Real data
- [ ] Open-Meteo and CSV adapters
- [ ] Metric vocabulary with units and conversion
- [ ] First preset plans (outdoor/drone), labelled as starting points, not safety advice
- [ ] Tutorial using real forecast data

## M3 - Reach
- [ ] WASM build and demo web app
- [ ] Python bindings

## M4 - Depth
- [ ] Uncertainty and multi-model agreement
- [ ] Temporal predicates and gaps; stable schemas
