//! Command-line front end. Run with `--help` for usage.

use env_operability::adapters::{csv, AdapterError};
use env_operability::time::format_utc;
use env_operability::units::canonical_unit;
use env_operability::{presets, Plan, SearchResult, Series, WindowSearch};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: env-operability <plan> <series> [options]

Plan (one of):
  --plan <plan.json>        Plan file (see examples/plan.json)
  --preset <name>           Built-in starting-point plan (see --list-presets)
      --hours <h>           Operation length for a preset (default 2)

Series (one of):
  --series <file>           Series file; format from extension (.csv) or --format
      --format <f>          json (default), csv, or open-meteo (a saved API response)
  --open-meteo <lat,lon>    Fetch a live forecast (needs a build with --features net)
      --days <n>            Forecast days to fetch, 1-16 (default 3)

Output:
  --top <n>                 Show at most n feasible windows (default 5)
  --rejected                Also list why each rejected window failed
  --json                    Print the full result as JSON
  --list-presets            List built-in presets and exit
  -h, --help                Show this help

Times are UTC. Values are shown in canonical units (m/s, C, mm, %, m, hPa).
Scores describe preference only; they are not probabilities or safety
certifications, and presets are illustrative, not safety guidance.";

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Json,
    Csv,
    OpenMeteo,
}

enum PlanSource {
    File(String),
    Preset { name: String, hours: f64 },
}

enum SeriesSource {
    File {
        path: String,
        format: Option<Format>,
    },
    OpenMeteo {
        latitude: f64,
        longitude: f64,
        days: u32,
    },
}

struct Args {
    plan: PlanSource,
    series: SeriesSource,
    top: usize,
    rejected: bool,
    json: bool,
}

enum Command {
    Help,
    ListPresets,
    Run(Args),
}

fn parse_args(raw: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let (mut plan, mut preset, mut hours) = (None, None, 2.0);
    let (mut series, mut format, mut open_meteo, mut days) = (None, None, None, 3u32);
    let (mut top, mut rejected, mut json) = (5, false, false);
    let mut args = raw.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--list-presets" => return Ok(Command::ListPresets),
            "--plan" => plan = Some(value("--plan")?),
            "--preset" => preset = Some(value("--preset")?),
            "--hours" => {
                hours = value("--hours")?
                    .parse()
                    .ok()
                    .filter(|h: &f64| h.is_finite() && *h > 0.0)
                    .ok_or("--hours must be a positive number")?
            }
            "--series" => series = Some(value("--series")?),
            "--format" => {
                format = Some(match value("--format")?.as_str() {
                    "json" => Format::Json,
                    "csv" => Format::Csv,
                    "open-meteo" => Format::OpenMeteo,
                    other => {
                        return Err(format!("unknown format `{other}` (json, csv, open-meteo)"))
                    }
                })
            }
            "--open-meteo" => open_meteo = Some(parse_coordinates(&value("--open-meteo")?)?),
            "--days" => {
                days = value("--days")?
                    .parse()
                    .ok()
                    .filter(|d| (1..=16).contains(d))
                    .ok_or("--days must be between 1 and 16")?
            }
            "--top" => {
                top = value("--top")?
                    .parse()
                    .map_err(|_| "--top must be a non-negative integer".to_string())?
            }
            "--rejected" => rejected = true,
            "--json" => json = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    let plan = match (plan, preset) {
        (Some(_), Some(_)) => return Err("use either --plan or --preset, not both".into()),
        (Some(path), None) => PlanSource::File(path),
        (None, Some(name)) => PlanSource::Preset { name, hours },
        (None, None) => return Err("--plan or --preset is required".into()),
    };
    let series = match (series, open_meteo) {
        (Some(_), Some(_)) => return Err("use either --series or --open-meteo, not both".into()),
        (Some(path), None) => SeriesSource::File { path, format },
        (None, Some((latitude, longitude))) => SeriesSource::OpenMeteo {
            latitude,
            longitude,
            days,
        },
        (None, None) => return Err("--series or --open-meteo is required".into()),
    };
    Ok(Command::Run(Args {
        plan,
        series,
        top,
        rejected,
        json,
    }))
}

fn parse_coordinates(text: &str) -> Result<(f64, f64), String> {
    let bad = || "--open-meteo expects `latitude,longitude`, e.g. 52.52,13.41".to_string();
    let (lat, lon) = text.split_once(',').ok_or_else(bad)?;
    let (lat, lon): (f64, f64) = (
        lat.trim().parse().map_err(|_| bad())?,
        lon.trim().parse().map_err(|_| bad())?,
    );
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("latitude must be within ±90 and longitude within ±180".into());
    }
    Ok((lat, lon))
}

fn read(path: &str, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {what} `{path}`: {e}"))
}

fn load_plan(source: &PlanSource) -> Result<Plan, String> {
    match source {
        PlanSource::File(path) => serde_json::from_str(&read(path, "plan")?)
            .map_err(|e| format!("invalid plan `{path}`: {e}")),
        PlanSource::Preset { name, hours } => {
            let preset = presets::by_name(name).ok_or_else(|| {
                let names: Vec<&str> = presets::ALL.iter().map(|p| p.name).collect();
                format!("unknown preset `{name}` (available: {})", names.join(", "))
            })?;
            Ok(preset.plan((hours * 3_600_000.0).round() as i64))
        }
    }
}

#[cfg(feature = "net")]
fn fetch(url: &str) -> Result<String, String> {
    // Trust the operating system's certificate store (not a bundled list), so
    // corporate and other TLS-intercepting proxies that install their own CA work.
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls).build().into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {e}"))
}
#[cfg(not(feature = "net"))]
fn fetch(_url: &str) -> Result<String, String> {
    Err(
        "live fetching needs a build with `--features net`; alternatively save the API response \
         (see the tutorial) and pass it with `--series file.json --format open-meteo`"
            .into(),
    )
}

fn adapter_error(what: &str, error: AdapterError) -> String {
    format!("cannot load {what}: {error}")
}

fn load_series(source: &SeriesSource) -> Result<Series, String> {
    match source {
        SeriesSource::OpenMeteo {
            latitude,
            longitude,
            days,
        } => {
            let body = fetch(&env_operability::adapters::open_meteo::request_url(
                *latitude, *longitude, *days,
            ))?;
            env_operability::adapters::open_meteo::parse(&body)
                .map_err(|e| adapter_error("forecast", e))
        }
        SeriesSource::File { path, format } => {
            let format = format.unwrap_or(if path.ends_with(".csv") {
                Format::Csv
            } else {
                Format::Json
            });
            let text = read(path, "series")?;
            match format {
                Format::Csv => {
                    csv::parse(&text).map_err(|e| adapter_error(&format!("`{path}`"), e))
                }
                Format::OpenMeteo => env_operability::adapters::open_meteo::parse(&text)
                    .map_err(|e| adapter_error(&format!("`{path}`"), e)),
                Format::Json => {
                    serde_json::from_str(&text).map_err(|e| format!("invalid series `{path}`: {e}"))
                }
            }
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let plan = load_plan(&args.plan)?;
    let series = load_series(&args.series)?;
    let result = WindowSearch::new(&series, &plan)
        .run()
        .map_err(|e| format!("invalid plan: {e}"))?;
    if args.json {
        let out = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
        println!("{out}");
    } else {
        print_text(&plan, &series, &result, args);
    }
    Ok(())
}

/// A reading with its canonical unit, e.g. `10.29 m/s`.
fn show(metric: &str, value: Option<f64>) -> String {
    match value {
        None => "missing".to_string(),
        Some(v) => {
            let number = format!("{v:.2}");
            let number = number.trim_end_matches('0').trim_end_matches('.');
            match canonical_unit(metric) {
                Some(unit) => format!("{number} {unit}"),
                None => number.to_string(),
            }
        }
    }
}

fn print_text(plan: &Plan, series: &Series, result: &SearchResult, args: &Args) {
    let first = series.observations.first().map_or(0, |o| o.timestamp_ms);
    let last = series.observations.last().map_or(0, |o| o.timestamp_ms) + series.cadence_ms;
    println!(
        "Plan `{}` over data {} -> {}: {} feasible, {} rejected",
        plan.name,
        format_utc(first),
        format_utc(last),
        result.feasible.len(),
        result.rejected.len()
    );
    for (rank, window) in result.feasible.iter().take(args.top).enumerate() {
        println!(
            "\n#{} {} -> {}  suitability {:.2}",
            rank + 1,
            format_utc(window.start_ms),
            format_utc(window.end_ms),
            window.suitability
        );
        if window.stages.len() > 1 {
            for stage in &window.stages {
                println!(
                    "   {:<12} {} -> {}  suitability {:.2}",
                    stage.name,
                    format_utc(stage.start_ms),
                    format_utc(stage.end_ms),
                    stage.suitability
                );
            }
        }
        for item in &window.evidence {
            println!(
                "   - {}/{}: {} at {} (expected {})",
                item.stage,
                item.constraint,
                show(&item.metric, item.actual),
                format_utc(item.timestamp_ms),
                item.expected
            );
        }
    }
    if args.rejected {
        println!();
        for window in &result.rejected {
            let f = &window.failure;
            println!(
                "rejected {}: {}/{} at {}: {} (expected {})",
                format_utc(window.start_ms),
                f.stage,
                f.constraint,
                format_utc(f.timestamp_ms),
                show(&f.metric, f.actual),
                f.expected
            );
        }
    }
}

fn print_presets() {
    println!("Built-in presets (illustrative starting points, not safety guidance):\n");
    for preset in presets::ALL {
        println!("  {:<14} {}", preset.name, preset.description);
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::ListPresets) => {
            print_presets();
            ExitCode::SUCCESS
        }
        Ok(Command::Run(args)) => match run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::from(1)
            }
        },
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn coordinates_are_validated() {
        assert_eq!(parse_coordinates("52.52, 13.41"), Ok((52.52, 13.41)));
        assert!(parse_coordinates("52.52").is_err());
        assert!(parse_coordinates("91,0").is_err());
        assert!(parse_coordinates("0,181").is_err());
    }
    #[test]
    fn plan_and_series_sources_are_exclusive_and_required() {
        assert!(parse(&["--series", "s.json"]).is_err());
        assert!(parse(&["--plan", "p.json"]).is_err());
        assert!(parse(&["--plan", "p", "--preset", "drone", "--series", "s"]).is_err());
        assert!(parse(&["--plan", "p", "--series", "s", "--open-meteo", "1,1"]).is_err());
        assert!(matches!(
            parse(&[
                "--preset",
                "drone",
                "--hours",
                "1.5",
                "--open-meteo",
                "1,2",
                "--days",
                "2"
            ]),
            Ok(Command::Run(_))
        ));
    }
    #[test]
    fn bad_numbers_are_rejected() {
        for flag in [
            ["--hours", "0"],
            ["--hours", "abc"],
            ["--days", "17"],
            ["--days", "0"],
            ["--top", "-1"],
        ] {
            assert!(
                parse(&["--preset", "drone", "--series", "s", flag[0], flag[1]]).is_err(),
                "{flag:?}"
            );
        }
    }
    #[test]
    fn values_show_canonical_units() {
        assert_eq!(show("wind_speed", Some(10.0)), "10 m/s");
        assert_eq!(show("wind_speed", Some(10.2889)), "10.29 m/s");
        assert_eq!(show("custom", Some(3.5)), "3.5");
        assert_eq!(show("wind_speed", None), "missing");
    }
}
