# wint

`wint` finds the time windows when a job can run, given the limits you set, and explains why the others cannot. You give it readings over time (a weather forecast, server metrics, energy prices) and a plan with your limits. It returns ranked windows and the evidence behind every decision.

Worked examples are in [docs/EXAMPLES.md](docs/EXAMPLES.md). The [tutorial](docs/TUTORIAL.md) walks through a first run, the [plain-language learning guide](LEARNING_GUIDE.md) explains the ideas, and [the specification](SPEC.md) has the details.

## Command line

The CLI is behind the optional `json` feature (the library core has no dependencies); add `net` to fetch live forecasts.

```sh
# a plan file and a series file (JSON or CSV)
cargo run --features json -- --plan examples/plan.json --series examples/series.csv

# a built-in starting-point plan on a live Open-Meteo forecast
cargo run --features net -- --preset drone --hours 2 --open-meteo 52.52,13.41

cargo run --features json -- --list-presets
```

Several forecast versions: `--models ecmwf_ifs025,gfs_seamless,icon_seamless` fetches several weather models, `--ensemble icon_seamless` fetches an ensemble (about 40 members), and saved responses are read with `--format open-meteo-ensemble`. The report says "fits in k of n" and names what blocked each window and which metrics some models lack; `--min-agreement` (default 1: every model that can answer) and `--min-coverage` (default 0.5) set the requirement. Agreement is **not** a probability.

Options: `--top <n>`, `--rejected` (why each rejected window failed), `--json` (full result), `--between 09:00-12:00` (only operate inside a daily window of the data's local time; `20:00-06:00` wraps midnight, `00:00-20:00` means until 8pm) and `--on weekend` (only some weekdays: `mon-fri`, `sat,sun`, `fri-mon`; the day a window starts on) and `--utc-offset +02:00` (the local clock; Open-Meteo data already carries it). New here? Follow the [tutorial](docs/TUTORIAL.md). Input formats:

- **Series JSON** `{cadence_ms, units?, observations: [{timestamp_ms, values}]}`, **series CSV** (see [`examples/series.csv`](examples/series.csv)), or a saved **Open-Meteo** response (`--format open-meteo`). All are validated and converted to canonical units.
- **Plan JSON** `{name, stages: [{name, duration_ms, constraints}]}`; see [`examples/plan.json`](examples/plan.json). Hard constraints use `"comparison": "<" | "<=" | ">" | ">=" | "=="`; soft constraints take a `preference` (`minimize`, `maximize` or `range`, each with a `scale`) and an optional `weight` (default 1). Any constraint may add a `"unit"` for its limits. A stage may add `"schedule": {"from": "09:00", "to": "12:00", "days": ["sat", "sun"]}` (`days` optional) and, after the first stage, `"gap": {"min_ms": 14400000, "max_ms": 43200000}` for a wait before it (see [tutorial](docs/TUTORIAL.md#6-jobs-with-a-wait-in-the-middle-gaps)). For daytime or night only, use an `is_day` constraint (`== 1` / `== 0`).

**Units.** Metrics from the shared vocabulary (`wind_speed`, `temperature`, `visibility`, ... see [`src/units.rs`](src/units.rs)) have one canonical unit each (SI-leaning: m/s, °C, mm, %, m, hPa). Adapters and unit fields convert to it; the engine itself never sees mixed units.

**Presets** (`drone`, `outdoor-event`, `field-work`) are illustrative starting points, **not safety guidance**; check their limits against your own equipment and regulations.

## JavaScript / WASM

The engine compiles to WebAssembly (feature `wasm`), so it can run in a browser or Node with no backend. The interface is strings in, strings out, using the same JSON formats as the CLI:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match the wasm-bindgen crate
scripts/build-wasm.sh          # writes ./pkg, a ready-to-publish npm package (about 390 KB, 150 KB gzipped)
node scripts/wasm-smoke.mjs    # checks the package against the same fixtures as the Rust tests
```

```js
import init, { search, presetPlan, parseOpenMeteo, openMeteoUrl } from './pkg/wint.js';
await init();
const series = parseOpenMeteo(await (await fetch(openMeteoUrl(52.52, 13.41, 3))).text());
const result = JSON.parse(search(series, presetPlan('drone', 2)));
```

Also exported: `parseCsv`, `listPresets`, `listMetrics`, `version`, and for several forecast versions `multiModelUrl`, `ensembleUrl`, `parseOpenMeteoEnsemble` and `searchEnsemble(ensembleJson, planJson, minAgreement, minCoverage)`. Errors are thrown as readable messages. The generated `pkg/package.json` is named `wint-core` by default (the name `wint` is taken on npm; set `WINT_NPM_NAME` to change it). It is not published yet. A browser demo built on this lives in a separate repository, `wint-demo`, which vendors the built package.

## Results are data

The engine returns facts and decisions (numbers, enums, identifiers), never display text: a limit comes back as `{"type": "comparison", "comparison": "<=", "threshold": 10}`, a clock check as minutes and an offset. Wording, rounding, units and colors are for your application to choose; `wint::present` offers a ready-made default for Rust programs (`describe_expectation`, `describe_reading`, `format_value`, `format_utc`), and the CLI uses it. See the layering rule in the [specification](SPEC.md#architecture).

## Uncertainty (ensembles)

`EnsembleSearch` runs the search on several forecast versions (models or ensemble members) and reports, per window, how many fit: "fits in 4 of 5 members", with what blocked the others and which members could not answer. See the [specification](SPEC.md#uncertainty-ensembles-and-agreement) for what the numbers mean. Agreement is **not** a probability.

```rust
use wint::{Ensemble, EnsembleSearch};
let ensemble = Ensemble::new(members)?;                       // same time grid for every member
let result = EnsembleSearch::new(&ensemble, &plan).min_agreement(0.8).run()?;
let best = &result.windows[0];                                 // best first
println!("fits in {} of {}", best.feasible, best.members_total);
```

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
