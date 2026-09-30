# wint - Weather Intelligence

`wint` is a deterministic Rust engine for finding and explaining time windows in environmental time-series data that meet declared operating limits.

It deliberately starts below data ingestion and user interfaces: callers supply normalized observations and receive ranked feasible windows. Version 0.1 has no network access, weather-provider coupling, AI, or geospatial file parsers.

See [the specification](SPEC.md) for the product and implementation plan.
New to the field? Start with the [plain-language learning guide](LEARNING_GUIDE.md).

## Command line

The CLI is behind the optional `json` feature (the library core has no dependencies); add `net` to fetch live forecasts.

```sh
# a plan file and a series file (JSON or CSV)
cargo run --features json -- --plan examples/plan.json --series examples/series.csv

# a built-in starting-point plan on a live Open-Meteo forecast
cargo run --features net -- --preset drone --hours 2 --open-meteo 52.52,13.41

cargo run --features json -- --list-presets
```

Options: `--top <n>`, `--rejected` (why each rejected window failed), `--json` (full result), `--between 09:00-12:00` (only operate inside a daily window of the data's local time; `20:00-06:00` wraps midnight, `00:00-20:00` means until 8pm) and `--utc-offset +02:00` (the local clock; Open-Meteo data already carries it). New here? Follow the [tutorial](docs/TUTORIAL.md). Input formats:

- **Series JSON** `{cadence_ms, units?, observations: [{timestamp_ms, values}]}`, **series CSV** (see [`examples/series.csv`](examples/series.csv)), or a saved **Open-Meteo** response (`--format open-meteo`). All are validated and converted to canonical units.
- **Plan JSON** `{name, stages: [{name, duration_ms, constraints}]}`; see [`examples/plan.json`](examples/plan.json). Hard constraints use `"comparison": "<" | "<=" | ">" | ">=" | "=="`; soft constraints take a `preference` (`minimize`, `maximize` or `range`, each with a `scale`) and an optional `weight` (default 1). Any constraint may add a `"unit"` for its limits. A stage may add `"schedule": {"from": "09:00", "to": "12:00"}`. For daytime or night only, use an `is_day` constraint (`== 1` / `== 0`).

**Units.** Metrics from the shared vocabulary (`wind_speed`, `temperature`, `visibility`, ... see [`src/units.rs`](src/units.rs)) have one canonical unit each (SI-leaning: m/s, °C, mm, %, m, hPa). Adapters and unit fields convert to it; the engine itself never sees mixed units.

**Presets** (`drone`, `outdoor-event`, `field-work`) are illustrative starting points, **not safety guidance**; check their limits against your own equipment and regulations.

## JavaScript / WASM

The engine compiles to WebAssembly (feature `wasm`), so it can run in a browser or Node with no backend. The interface is strings in, strings out, using the same JSON formats as the CLI:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match the wasm-bindgen crate
scripts/build-wasm.sh          # writes ./pkg (wint.js, wint_bg.wasm, wint.d.ts; about 280 KB)
node scripts/wasm-smoke.mjs    # checks the package against the same fixtures as the Rust tests
```

```js
import init, { search, presetPlan, parseOpenMeteo, openMeteoUrl } from './pkg/wint.js';
await init();
const series = parseOpenMeteo(await (await fetch(openMeteoUrl(52.52, 13.41, 3))).text());
const result = JSON.parse(search(series, presetPlan('drone', 2)));
```

Also exported: `parseCsv`, `listPresets`, `listMetrics`, `version`. Errors are thrown as readable messages. A browser demo built on this lives in a separate repository, `wint-demo`, which vendors the built package.

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
