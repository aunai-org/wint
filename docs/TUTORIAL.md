# Tutorial: from a forecast to ranked windows

This walks through the whole loop: get hourly forecast data, pick a plan, read the answer. It assumes you have Rust installed and have cloned this repository.

## 1. Get forecast data

`wint` reads three kinds of input. Pick whichever suits you.

**A. Fetch a live forecast (needs the `net` feature).** [Open-Meteo](https://open-meteo.com) needs no API key for non-commercial use (check their terms before anything else).

```sh
cargo run --features net -- --preset drone --hours 2 --open-meteo 52.52,13.41 --days 3
```

**B. Save the API response yourself, then analyse it offline.** Build the URL with the library (`adapters::open_meteo::request_url`) or copy this one, download it, and pass it with `--format open-meteo`:

```sh
curl -o forecast.json "https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&hourly=temperature_2m,precipitation,precipitation_probability,wind_speed_10m,wind_gusts_10m,cloud_cover,relative_humidity_2m,visibility&wind_speed_unit=ms&timeformat=unixtime&timezone=GMT&forecast_days=3"
cargo run --features json -- --preset drone --series forecast.json --format open-meteo
```

**C. Your own CSV.** See [`examples/series.csv`](../examples/series.csv). The header can carry units, `wind_speed (km/h)` or `temperature[F]`, and they are converted for you:

```csv
timestamp,wind_speed (km/h),precipitation_probability,temperature (F)
2026-09-21T08:00Z,12,10,75
2026-09-21T09:00Z,15,15,79
```

## 2. Understand the units

Everything is converted to one canonical unit per metric, so you never compare knots with km/h by accident:

| metric | unit | metric | unit |
|---|---|---|---|
| `wind_speed`, `wind_gust` | m/s | `precipitation` | mm per sample |
| `temperature` | °C | `precipitation_probability`, `cloud_cover`, `relative_humidity` | % |
| `visibility`, `wave_height` | m | `pressure` | hPa |

Limits in a plan can be written in any unit you like by adding `"unit": "kn"` (or `km/h`, `mph`, `°F`, ...) to the constraint; the engine converts them. Results always show canonical units. Metrics outside this vocabulary work too, but they are not converted, and you cannot attach a unit to them.

## 3. Pick a plan

List the built-in starting points:

```sh
cargo run --features json -- --list-presets
```

Presets are **illustrative, not safety guidance**. Read their thresholds in [`src/presets.rs`](../src/presets.rs) and compare them with your equipment's limits and your regulator's rules. To write your own, copy [`examples/plan.json`](../examples/plan.json): a plan is a list of stages, each with hard limits (pass or fail) and soft preferences (a ranking bonus, with a `scale` saying how far from the ideal counts as the worst case).

## 4. Read the answer

Running the drone preset on a saved Open-Meteo-shaped sample (`tests/fixtures/open_meteo_hourly.json`) with `--rejected`:

```text
Plan `drone` over data 2026-09-22T08:00Z -> 2026-09-22T12:00Z: 0 feasible, 3 rejected

rejected 2026-09-22T08:00Z: operation/gust limit at 2026-09-22T08:00Z: 15 m/s (expected <= 12)
rejected 2026-09-22T09:00Z: operation/no precipitation at 2026-09-22T10:00Z: 0.2 mm (expected <= 0.1)
rejected 2026-09-22T10:00Z: operation/no precipitation at 2026-09-22T10:00Z: 0.2 mm (expected <= 0.1)
```

(That run used a 2-hour operation; `--hours 1` would find two feasible windows in the same data.) Each rejection names the first limit that failed, when, and by how much. For a feasible window, the evidence shows the *binding* reading for each constraint, the one closest to its limit, so you can see how much margin you actually had. Use `--json` for the full machine-readable result.

## 5. How sure is the forecast? Compare models

One forecast hides disagreement between weather models. Ask several at once:

```sh
cargo run --features net -- --preset field-work --hours 3 --open-meteo 52.52,13.41 \
  --models ecmwf_ifs025,gfs_seamless,icon_seamless,meteofrance_seamless
```

Each window reports how many models it fits ("fits in 3 of 4 that can answer"), which rule blocked the others, and which models could not answer at all (a model may not provide visibility, or its forecast may end early). Use `--ensemble icon_seamless` for about 40 ensemble members instead. Two cautions: this is a count of forecast versions, not a probability, because models share data and are not independent; and the ensemble service does not provide visibility or rain probability, so presets with rules on those say so instead of guessing.

## What to remember

- The score ranks windows that already pass every hard limit. It is not a probability, and not a safety certification.
- Forecasts are uncertain. A single forecast is treated as exact; compare several models or an ensemble (section 5) to see where they disagree.
- A missing reading (a `null` in the API, an empty CSV cell) fails any hard limit on that metric rather than passing silently.
