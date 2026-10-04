# Examples

Plans and data you can run. Each example is a plan file and a series file; the commands below print the best window and the evidence behind it. The demo ([wint-demo](https://github.com/aunai-org/wint-demo)) runs them too: weather by default, the others from its Example menu. Output is trimmed, and the numbers are checked by the tests.

## Weather: drone survey (`examples/plan.json`, `examples/series.csv`)

*Fly for 2 hours with wind under 20 km/h and rain chance under 20%, preferring calm air, then recover for 1 hour with rain chance under 50% and a mild temperature.*

Data: wind, rain chance and temperature for 6 hours. The CSV header carries units (`km/h`, `F`), which wint converts. The plan has two stages, hard limits and soft preferences.

```sh
cargo run --features json -- --plan examples/plan.json --series examples/series.csv --top 1
```

```text
#1 2026-09-21T09:00Z -> 2026-09-21T12:00Z  suitability 0.74
   flight       2026-09-21T09:00Z -> 2026-09-21T11:00Z  suitability 0.67
   recovery     2026-09-21T11:00Z -> 2026-09-21T12:00Z  suitability 1.00
   - flight/safe wind: 4.17 m/s at 2026-09-21T09:00Z (expected <= 5.5556)
   - flight/low rain chance: 15 % at 2026-09-21T09:00Z (expected < 20)
```

Two start times fit and two do not. The same plan runs on a live forecast with `--preset drone --open-meteo 52.52,13.41`.

## Weather: painting a fence with a wait (`examples/paint-plan.json`, `examples/paint-series.csv`)

*Paint for 2 hours when it is dry and calm, let it cure for 4 to 12 hours, then apply a clear coat for 1 hour when it is dry.*

Data: a Saturday that is dry from 08:00 to 10:00, rainy until 18:00, and dry again. The clear coat stage has a **gap**: it starts 4 to 12 hours after painting ends, and the weather during the wait is not checked.

```sh
cargo run --features json -- --plan examples/paint-plan.json --series examples/paint-series.csv --format csv --top 1
```

```text
#1 2026-10-10T08:00Z -> 2026-10-10T19:00Z  suitability 1.00
   paint        2026-10-10T08:00Z -> 2026-10-10T10:00Z  suitability 1.00
   clear coat   2026-10-10T18:00Z -> 2026-10-10T19:00Z  suitability 1.00
```

Painting at 08:00 works because the clear coat can wait until 18:00, when the rain stops. The [tutorial](TUTORIAL.md#6-jobs-with-a-wait-in-the-middle-gaps) walks through it step by step.

## Server deploy window (`examples/domains/server-deploy`)

*Deploy for 2 hours, only overnight (20:00 to 06:00), when CPU load is under 60 and the job queue is short; prefer the quietest hour.*

Data: `cpu_load` and `queue_depth` every hour for 3 days (busy 09:00-17:00, queue spike at 12:00-13:00). The plan has a time-of-day window, two hard limits and one soft preference.

```sh
cargo run --features json -- --plan examples/domains/server-deploy/plan.json --series examples/domains/server-deploy/series.csv --format csv --top 1
```

```text
#1 2026-10-05T00:00Z -> 2026-10-05T02:00Z  suitability 0.93
   - deploy/time of day: mon 00:00 to 02:00 local (UTC+00:00) ... (expected within 20:00-06:00 local)
   - deploy/low load: 28 ... (expected < 60)
   - deploy/short queue: 4 ... (expected <= 20)
```

26 start times fit, 45 do not. Nothing starts in business hours, because both the clock window and the load limit rule them out, and `--rejected` says which one stopped each start.

## EV charging (`examples/domains/ev-charging`)

*Charge for 4 hours while the car is plugged in (18:00 to 08:00), only when the grid is at most 300 g CO2/kWh; prefer the cheapest power.*

Data: electricity price and grid carbon intensity per hour for 2 days (cheap at night, dirty and expensive 17:00-21:00).

```sh
cargo run --features json -- --plan examples/domains/ev-charging/plan.json --series examples/domains/ev-charging/series.csv --format csv --top 1
```

```text
#1 2026-10-05T00:00Z -> 2026-10-05T04:00Z  suitability 0.90
   - charge/cleaner grid: 250 ... (expected <= 300)
   - charge/cheap power: 0.12 ... (expected minimize: ideal 0.1, scale 0.2)
```

12 start times fit: 22:00 through 04:00. Charging that would run into the dirty evening hours is rejected, and the soft preference picks the cheapest of the rest.

## Bakery batch with a proofing wait (`examples/domains/bakery-batch`)

*Mix the dough (1 h, between 05:00 and 13:00, only when the room is dry), let it proof for 2 to 4 hours, then bake (1 h, only when the oven is free; prefer cheap power).*

Data: room humidity, an `oven_free` flag (1 or 0) and power price per hour for 2 days. The second stage uses a **gap**, so the bake can start 2 to 4 hours after mixing; the proofing time itself is not checked.

```sh
cargo run --features json -- --plan examples/domains/bakery-batch/plan.json --series examples/domains/bakery-batch/series.csv --format csv --top 1
```

```text
#1 2026-10-05T10:00Z -> 2026-10-05T14:00Z  suitability 1.00
   mix          2026-10-05T10:00Z -> 2026-10-05T11:00Z  suitability 1.00
   bake         2026-10-05T13:00Z -> 2026-10-05T14:00Z  suitability 1.00
```

Only 6 start times work. Mixing at 05:00 is rejected for a reason that is easy to miss: the room is fine, but the oven is busy at every allowed bake time (08:00 to 10:00), so no bake slot exists. That is the kind of explanation the engine is for.

## Using your own data

1. Put your readings in a CSV: first column a timestamp (ISO 8601 with a zone, or Unix seconds), then one column per metric, evenly spaced, with numbers in it (a yes/no flag is 0 or 1). Name the metrics whatever you like.
2. Write a plan (see [`examples/plan.json`](../examples/plan.json) and the format summary in the README): stages with `duration_ms`, hard and soft constraints, optionally `schedule` and `gap`.
3. Run it with the commands above, or paste the plan into the demo and upload the CSV.

A few rules to know:

- A missing reading fails a hard limit instead of passing silently.
- Units are optional. wint converts the ones it knows (km/h, °F, mm, ...); anything else is used as given, so keep it consistent in the plan or put the unit in the name (`room_humidity_pct`).
- The Open-Meteo adapter, the presets (`drone`, `outdoor-event`, `field-work`) and the `is_day` helper are only for weather and are optional.
