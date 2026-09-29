# A plain-language guide to wint

This is a companion to the [technical specification](SPEC.md). It explains the ideas used by this library in everyday language. You do not need a climate-science background to use the engine: it works with any measurements that change over time.

## The big idea

Imagine you need to mow a field for two hours. You do not need a general weather forecast—you need to know **which two-hour periods are suitable**. Perhaps wind must stay below a limit, rain must be unlikely, and cooler temperatures are preferred.

This library takes a list of time-stamped measurements and a list of rules. It checks each possible time period, removes the unsafe ones, and ranks the remaining choices.

The same idea can apply to a drone flight, a boat trip, an outdoor event, telescope work, a maintenance job, or non-weather measurements such as air quality or river level.

## The data: what the engine receives

### Environmental time series

A **time series** is simply a list of measurements taken over time. A weather forecast is one example.

```text
Time        Wind speed    Rain chance    Temperature
08:00       12 km/h       10%            22 C
09:00       18 km/h       15%            24 C
10:00       31 km/h       20%            25 C
```

Each row is an **observation**. It says what we know or forecast at one specific time.

### Timestamp (`timestamp_ms`)

A **timestamp** identifies a moment in time. In the first version of this library it is a Unix timestamp measured in milliseconds: a number of milliseconds since 1 January 1970 UTC. This is a computer-friendly format, not a format people normally type. A future input adapter or UI can convert readable dates such as `2026-09-21 08:00 UTC` to and from this number.

### Metric

A **metric** is the thing being measured. Examples include `wind_speed`, `wind_gust`, `air_temperature`, `precipitation_probability`, `wave_height`, `visibility`, `river_level`, and `air_quality_index`.

In the core engine, a metric is just a name plus a number. The caller is responsible for using clear names and consistent units. For example, do not mix wind values in miles per hour and kilometres per hour under the same metric name.

### Value

A **value** is the numeric measurement for a metric at one observation. In `wind_speed = 18`, the value is `18`. Values must be real, finite numbers: no blank values, `NaN`, or infinity.

### Units

**Units** explain what a number means: `km/h`, metres, degrees Celsius, percent, and so on. A value of `20` alone is ambiguous. The v0.1 core does not store or convert units; normalize them before calling the engine. The planned unit metadata work will make this easier and safer.

### Cadence (`cadence_ms`)

The **cadence** is the regular spacing between observations. Hourly forecast rows have a one-hour cadence; a sensor reporting every 15 minutes has a 15-minute cadence.

v0.1 requires regular spacing. If a series says 08:00, 09:00, then 11:00, the missing 10:00 observation makes it invalid. This strict rule prevents the engine from quietly assuming conditions were safe during a gap.

## The plan: what you ask the engine to find

### Plan

A **plan** describes one operation you want to carry out and the conditions it needs. For example: “Find a two-hour drone survey window with gentle wind and low rain risk.”

### Stage

A **stage** is one consecutive part of a plan. A drone job might have a 15-minute launch stage, a 90-minute survey stage, and a 15-minute recovery stage. Each stage can have its own rules.

Stages are ordered and touch each other in v0.1: recovery begins as soon as survey ends. This is called a **staged operation**.

### Duration (`duration_ms`)

**Duration** is how long a stage or whole operation lasts. It is also represented as milliseconds in the Rust API. It must be positive and a whole multiple of the series cadence. For hourly data, a two-hour stage is valid; a 90-minute stage is not, because the engine has no 09:30 sample to evaluate.

### Candidate window

A **candidate window** is one possible start and end time for the full operation. If the operation lasts two hours and the data is hourly, the engine tries 08:00–10:00, then 09:00–11:00, and so on.

### Contiguous window

**Contiguous** means unbroken. A valid two-hour window needs two connected hours that meet the applicable rules. One safe hour in the morning and another safe hour in the afternoon do not make a valid two-hour window.

### Half-open interval (`[start, end)`)

The engine treats a window as including its start but excluding its end. Thus `[08:00, 10:00)` includes observations at 08:00 and 09:00, not 10:00. This avoids accidentally counting a boundary sample twice when stages meet.

## Rules: hard limits and preferences

### Constraint

A **constraint** is a rule about a metric. The library has two kinds: hard constraints and soft constraints.

### Hard constraint

A **hard constraint** is a non-negotiable limit. If it fails once during its stage, the whole candidate window is rejected.

Examples:

- Wind gust must be below 35 km/h.
- Visibility must be at least 5 km.
- Rain probability must be under 20%.

The engine supports these comparisons: less than (`<`), less than or equal to (`<=`), greater than (`>`), greater than or equal to (`>=`), and exactly equal to (`==`).

### Threshold

The **threshold** is the number in a hard rule. For “wind gust below 35 km/h,” `35` is the threshold. The **comparison** says how the measured value relates to that threshold.

### Missing data

If a hard rule needs a value that is missing, v0.1 rejects that candidate. For safety-sensitive work, “we do not know” should not silently mean “safe.” An application can choose a different policy in a later version, but it should make that choice explicit.

### Soft constraint / preference

A **soft constraint** is a preference, not a safety limit. It cannot reject an otherwise feasible window; it helps choose the nicer option among feasible ones.

For example, an event may be acceptable at 10–30 C but prefer 20–25 C. Or a boat trip may be allowed at moderate wind but prefer calmer wind.

### Weight

A **weight** says how important one preference is compared with another. A preference with weight `2` affects the score twice as much as a preference with weight `1`. A zero or negative weight has no effect in v0.1.

### Minimize, maximize, and range preferences

- **Minimize**: lower is better, such as wind speed or wave height.
- **Maximize**: higher is better, such as visibility.
- **Range**: values inside a preferred interval are best, such as a comfortable temperature range.

An **ideal** is the best target for minimize/maximize. The engine converts distance from the ideal into a penalty between 0 and 1. A range has zero penalty while the value stays inside the range; penalty grows outside it.

Every preference also has a **scale**: the distance from the ideal that counts as the worst possible result. For example, `Minimize { ideal: 10.0, scale: 20.0 }` for wind gives 10 km/h a penalty of 0, 20 km/h a penalty of 0.5, and 30 km/h or more a penalty of 1. You choose the scale, so it always makes sense in your metric's unit.

## Results: how the engine answers

### Feasible window

A **feasible window** meets every hard constraint for every relevant observation. It is possible according to the supplied data and rules. It is not a guarantee of safety: the data may be forecast data, conditions can change, and real operations require human judgement and domain-specific safeguards.

### Rejected window

A **rejected window** is a candidate that failed a hard constraint or lacked a required value. v0.1 records the first deciding failure, rather than every failure, so applications can explain quickly why a period was excluded.

### Evidence / explainability

**Evidence** is the small audit trail attached to a result: which stage and constraint were checked, at what time, what value was seen, what was expected, and whether it passed. This lets an application say “09:00–11:00 was rejected because wind at 10:00 was 31 km/h, exceeding the 25 km/h limit,” rather than simply “not suitable.”

### Suitability score

The **suitability score** ranks feasible windows from 0 to 1. A score closer to 1 means the supplied soft preferences were met more closely. It is calculated as `1 - weighted mean of penalties`.

It is deliberately not a weather probability, a confidence percentage, or a safety rating. A window with score 0.90 is preferable under the supplied preferences; it does not mean there is a 90% chance that the operation will succeed.

### Deterministic ranking

**Deterministic** means identical inputs always produce identical outputs. Windows are ranked by suitability score, then by earlier start time when scores tie. This makes results testable and auditable.

## Terms likely to appear in later versions

### Temporal constraint

A **temporal constraint** describes a timing relationship, rather than a measurement limit: for example, stage A must happen before stage B, or a recovery phase must begin within 30 minutes of a survey phase. In v0.1, stages are already consecutive, so their before/after relationship is automatic. More flexible gaps and named relationships are planned for a later release.

### Forecast

A **forecast** is a prediction of future conditions. The engine does not produce forecasts; it evaluates whatever time series it is given. That series could be a forecast, a live sensor feed, historical data, or a simulation.

### Model and ensemble

In environmental science, a **model** is a mathematical simulation used to estimate future conditions. An **ensemble** is a group of model runs or forecasts. Agreement between them can provide useful information about uncertainty. This is intentionally outside v0.1; future versions can represent this explicitly instead of pretending a single forecast is certain.

### Provenance

**Provenance** means where data came from and how it was transformed: for example, a named weather provider, model run time, original unit, and conversion. It matters for trust and auditing. v0.1 expects the caller to manage it; planned metadata support will carry it with the data.

### Adapter

An **adapter** converts data at the edge of the system into the core engine’s simple `Series` format. A future CSV adapter might read a file; a weather-provider adapter might fetch a forecast and convert units. Keeping adapters outside the core means the decision logic stays reusable and easy to test.

## A complete small example

Suppose a team needs a two-hour outdoor job. Their rule is wind below 25 km/h at every hourly observation. They prefer the calmest option.

```text
08:00  wind 12 km/h
09:00  wind 18 km/h
10:00  wind 31 km/h
11:00  wind 10 km/h
12:00  wind  8 km/h
```

The engine considers:

- `08:00–10:00`: feasible; both 12 and 18 are below 25.
- `09:00–11:00`: rejected; 31 at 10:00 breaks the hard limit.
- `10:00–12:00`: rejected; it also contains 31.
- `11:00–13:00`: feasible; 10 and 8 are below 25, and it ranks above the earlier window because it is calmer.

That is the essential job of `wint`: apply declared rules consistently, then make the result understandable.
