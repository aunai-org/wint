# Examples: the same engine, different domains

wint answers one question: **when can this job run, given the limits I set?** It does not know what a "wind speed" or a "CPU load" is. The data is a list of named numbers at regular times; the plan is a list of limits on those names. So the weather examples in the [README](../README.md) and [tutorial](TUTORIAL.md) are one use case among many.

This page shows three that have nothing to do with weather. Each one is a pair of files you can run today:

```sh
cargo run --features json -- --plan examples/domains/<name>/plan.json \
  --series examples/domains/<name>/series.csv --format csv --top 1
```

The demo can load all of them from its **Example** menu ([wint-demo](https://github.com/aunai-org/wint-demo)). Every number printed below is produced by these files and is checked by `tests/examples.rs`.

## What "domain-neutral" means

- **Metric names are yours.** Any text works as a metric (`cpu_load`, `price_eur_kwh`, `oven_free`). The engine only compares numbers; it never interprets the name.
- **Units are optional.** wint knows a small vocabulary (km/h, °F, mm, ...) and converts it for you, which is handy for weather. A metric outside the vocabulary is used as given, so state its unit in the metric name (`room_humidity_pct`) or keep it consistent in the plan.
- **The structure is generic.** Hard limits (must hold), soft preferences (rank what passes), several stages with a wait between them, a daily clock window, weekdays. None of it is tied to a field.
- **A few rules apply everywhere:** values must be finite numbers (a yes/no flag is 0 or 1), the data must be evenly spaced, and a missing reading fails a hard limit instead of passing silently.
- **What is weather-specific** is only at the edges: the Open-Meteo adapter, the preset plans (`drone`, `outdoor-event`, `field-work`) and the day/night helper `is_day`. You can ignore all of them.

If you can produce a table of timestamps and numbers (a CSV export, a metrics query, a price feed), you can use wint.

## 1. Server deploy window (`examples/domains/server-deploy`)

*Deploy for 2 hours, only overnight (20:00 to 06:00), when CPU load is under 60 and the job queue is short; prefer the quietest hour.*

Data: `cpu_load` and `queue_depth` every hour for 3 days (busy 09:00-17:00, queue spike at 12:00-13:00). The plan has a time-of-day window, two hard limits and one soft preference.

```text
#1 2026-10-05T00:00Z -> 2026-10-05T02:00Z  suitability 0.93
   - deploy/time of day: mon 00:00 to 02:00 local (UTC+00:00) ... (expected within 20:00-06:00 local)
   - deploy/low load: 28 ... (expected < 60)
   - deploy/short queue: 4 ... (expected <= 20)
```

26 start times fit, 45 do not. Nothing starts in business hours, because both the clock window and the load limit rule them out, and `--rejected` says which one stopped each start.

## 2. EV charging (`examples/domains/ev-charging`)

*Charge for 4 hours while the car is plugged in (18:00 to 08:00), only when the grid is at most 300 g CO2/kWh; prefer the cheapest power.*

Data: electricity price and grid carbon intensity per hour for 2 days (cheap at night, dirty and expensive 17:00-21:00).

```text
#1 2026-10-05T00:00Z -> 2026-10-05T04:00Z  suitability 0.90
   - charge/cleaner grid: 250 ... (expected <= 300)
   - charge/cheap power: 0.12 ... (expected minimize: ideal 0.1, scale 0.2)
```

12 start times fit: 22:00 through 04:00. Charging that would run into the dirty evening hours is rejected, and the soft preference picks the cheapest of the rest.

## 3. Bakery batch with a proofing wait (`examples/domains/bakery-batch`)

*Mix the dough (1 h, between 05:00 and 13:00, only when the room is dry), let it proof for 2 to 4 hours, then bake (1 h, only when the oven is free; prefer cheap power).*

Data: room humidity, an `oven_free` flag (1 or 0) and power price per hour for 2 days. The second stage uses a **gap**, so the bake can start 2 to 4 hours after mixing; the proofing time itself is not checked.

```text
#1 2026-10-05T10:00Z -> 2026-10-05T14:00Z  suitability 1.00
   mix          2026-10-05T10:00Z -> 2026-10-05T11:00Z  suitability 1.00
   bake         2026-10-05T13:00Z -> 2026-10-05T14:00Z  suitability 1.00
```

Only 6 start times work. Mixing at 05:00 is rejected for a reason that is easy to miss: the room is fine, but the oven is busy at every allowed bake time (08:00 to 10:00), so no bake slot exists. That is the kind of explanation the engine is for.

## Try your own

1. Put your data in a CSV: first column a timestamp (ISO 8601 with a zone, or Unix seconds), then one column per metric, evenly spaced.
2. Write a plan (see [`examples/plan.json`](../examples/plan.json) and the format summary in the README): stages with `duration_ms`, hard and soft constraints, optionally `schedule` and `gap`.
3. Run it with the command above, or paste the plan into the demo and upload the CSV.

Domains that fit well: maintenance and deploy windows, energy and EV scheduling, greenhouse and irrigation timing, lab and factory batch steps, event planning, anything that is "find me a good stretch of time given these readings".
