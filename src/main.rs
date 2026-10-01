//! Command-line front end. Run with `--help` for usage.

use env_operability::adapters::{csv, AdapterError};
use env_operability::time::format_utc;
use env_operability::units::canonical_unit;
use env_operability::{
    presets, Ensemble, EnsembleResult, EnsembleSearch, Evidence, Plan, Schedule, SearchResult,
    Series, WindowSearch,
};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: env-operability <plan> <series> [options]

Plan (one of):
  --plan <plan.json>        Plan file (see examples/plan.json)
  --preset <name>           Built-in starting-point plan (see --list-presets)
      --hours <h>           Operation length for a preset (default 2)

Series (one of):
  --series <file>           Series file; format from extension (.csv) or --format
      --format <f>          json (default), csv, open-meteo (a saved API response),
                            open-meteo-ensemble (a saved multi-model or ensemble response)
                            or ensemble-json
  --open-meteo <lat,lon>    Fetch a live forecast (needs a build with --features net)
      --days <n>            Forecast days to fetch, 1-16 (default 3)
      --models <a,b,..>     Fetch several weather models, e.g. ecmwf_ifs025,gfs_seamless
      --ensemble <model>    Fetch an ensemble (about 40 members), e.g. icon_seamless

Time of day (local time of the data):
  --between <HH:MM-HH:MM>   Only operate inside a daily window, e.g. 09:00-12:00,
                            00:00-20:00 (until 8pm) or 20:00-06:00 (overnight)
  --utc-offset <+HH:MM>     Local clock offset from UTC (default: from the data, else +00:00)

Several forecast versions (models or ensemble members) report 'fits in k of n':
  --min-agreement <0-1>     Share of answering members a window must fit (default 1)
  --min-coverage <0-1>      Share of all members that must be able to answer (default 0.5)
  Agreement is not a probability: models are not independent and members are not calibrated.

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
    OpenMeteoEnsemble,
    EnsembleJson,
}

/// Which Open-Meteo service and how many forecast versions to fetch.
enum Versions {
    Single,
    Models(Vec<String>),
    Ensemble(Vec<String>),
}

/// A single series or several forecast versions.
enum Data {
    Single(Series),
    Many(Ensemble),
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
        versions: Versions,
    },
}

struct Args {
    plan: PlanSource,
    series: SeriesSource,
    between: Option<Schedule>,
    utc_offset: Option<i32>,
    min_agreement: Option<f64>,
    min_coverage: Option<f64>,
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
    let (mut between, mut utc_offset) = (None, None);
    let (mut models, mut ensemble) = (None, None);
    let (mut min_agreement, mut min_coverage) = (None, None);
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
                    "open-meteo-ensemble" => Format::OpenMeteoEnsemble,
                    "ensemble-json" => Format::EnsembleJson,
                    other => {
                        return Err(format!(
                            "unknown format `{other}` (json, csv, open-meteo, open-meteo-ensemble, ensemble-json)"
                        ))
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
            "--models" => models = Some(parse_list("--models", &value("--models")?)?),
            "--ensemble" => ensemble = Some(parse_list("--ensemble", &value("--ensemble")?)?),
            "--min-agreement" => {
                min_agreement = Some(parse_fraction("--min-agreement", &value("--min-agreement")?)?)
            }
            "--min-coverage" => {
                min_coverage = Some(parse_fraction("--min-coverage", &value("--min-coverage")?)?)
            }
            "--between" => between = Some(parse_between(&value("--between")?)?),
            "--utc-offset" => utc_offset = Some(parse_offset(&value("--utc-offset")?)?),
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
    let versions = match (models, ensemble) {
        (Some(_), Some(_)) => return Err("use either --models or --ensemble, not both".into()),
        (Some(list), None) => Versions::Models(list),
        (None, Some(list)) => Versions::Ensemble(list),
        (None, None) => Versions::Single,
    };
    if open_meteo.is_none() && !matches!(versions, Versions::Single) {
        return Err(
            "--models and --ensemble fetch from Open-Meteo, so they need --open-meteo".into(),
        );
    }
    let series = match (series, open_meteo) {
        (Some(_), Some(_)) => return Err("use either --series or --open-meteo, not both".into()),
        (Some(path), None) => SeriesSource::File { path, format },
        (None, Some((latitude, longitude))) => SeriesSource::OpenMeteo {
            latitude,
            longitude,
            days,
            versions,
        },
        (None, None) => return Err("--series or --open-meteo is required".into()),
    };
    Ok(Command::Run(Args {
        plan,
        series,
        between,
        utc_offset,
        min_agreement,
        min_coverage,
        top,
        rejected,
        json,
    }))
}

/// Parses a comma-separated list such as `ecmwf_ifs025,gfs_seamless`.
fn parse_list(flag: &str, text: &str) -> Result<Vec<String>, String> {
    let items: Vec<String> = text.split(',').map(|s| s.trim().to_string()).collect();
    if items.iter().any(String::is_empty) {
        return Err(format!("{flag} expects a comma-separated list, e.g. a,b"));
    }
    Ok(items)
}

/// Parses a number in (0, 1].
fn parse_fraction(flag: &str, text: &str) -> Result<f64, String> {
    text.parse()
        .ok()
        .filter(|v: &f64| *v > 0.0 && *v <= 1.0)
        .ok_or_else(|| format!("{flag} must be greater than 0 and at most 1"))
}

/// Parses `HH:MM-HH:MM` into a daily window.
fn parse_between(text: &str) -> Result<Schedule, String> {
    let (from, to) = text
        .split_once('-')
        .ok_or("--between expects HH:MM-HH:MM, e.g. 09:00-12:00")?;
    Schedule::parse(from, to).map_err(|e| format!("--between: {e}"))
}

/// Parses `+HH:MM`, `-HH:MM`, `Z` or `UTC` into minutes from UTC.
fn parse_offset(text: &str) -> Result<i32, String> {
    let bad = || "--utc-offset expects +HH:MM or -HH:MM, e.g. +02:00".to_string();
    let text = text.trim();
    if text.eq_ignore_ascii_case("z") || text.eq_ignore_ascii_case("utc") {
        return Ok(0);
    }
    let sign = match text.chars().next() {
        Some('+') => 1,
        Some('-') => -1,
        _ => return Err(bad()),
    };
    let (h, m) = text[1..].split_once(':').ok_or_else(bad)?;
    let (h, m): (i32, i32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    let minutes = sign * (h * 60 + m);
    if m > 59 || !(-720..=840).contains(&minutes) {
        return Err("--utc-offset must be between -12:00 and +14:00".into());
    }
    Ok(minutes)
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

fn load_data(source: &SeriesSource) -> Result<Data, String> {
    use env_operability::adapters::open_meteo as om;
    match source {
        SeriesSource::OpenMeteo {
            latitude,
            longitude,
            days,
            versions,
        } => match versions {
            Versions::Single => {
                let body = fetch(&om::request_url(*latitude, *longitude, *days))?;
                om::parse(&body)
                    .map(Data::Single)
                    .map_err(|e| adapter_error("forecast", e))
            }
            Versions::Models(list) | Versions::Ensemble(list) => {
                let names: Vec<&str> = list.iter().map(String::as_str).collect();
                let url = if matches!(versions, Versions::Models(_)) {
                    om::multi_model_url(*latitude, *longitude, *days, &names)
                } else {
                    om::ensemble_url(*latitude, *longitude, *days, &names)
                }
                .map_err(|e| adapter_error("request", e))?;
                om::parse_ensemble(&fetch(&url)?)
                    .map(Data::Many)
                    .map_err(|e| adapter_error("forecast", e))
            }
        },
        SeriesSource::File { path, format } => {
            let format = format.unwrap_or(if path.ends_with(".csv") {
                Format::Csv
            } else {
                Format::Json
            });
            let text = read(path, "series")?;
            let at = format!("`{path}`");
            match format {
                Format::Csv => csv::parse(&text)
                    .map(Data::Single)
                    .map_err(|e| adapter_error(&at, e)),
                Format::OpenMeteo => om::parse(&text)
                    .map(Data::Single)
                    .map_err(|e| adapter_error(&at, e)),
                Format::OpenMeteoEnsemble => om::parse_ensemble(&text)
                    .map(Data::Many)
                    .map_err(|e| adapter_error(&at, e)),
                Format::Json => serde_json::from_str(&text)
                    .map(Data::Single)
                    .map_err(|e| format!("invalid series `{path}`: {e}")),
                Format::EnsembleJson => serde_json::from_str(&text)
                    .map(Data::Many)
                    .map_err(|e| format!("invalid ensemble `{path}`: {e}")),
            }
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let mut plan = load_plan(&args.plan)?;
    let mut data = load_data(&args.series)?;
    if let Some(window) = args.between {
        plan = plan.with_schedule(window);
    }
    if let Some(minutes) = args.utc_offset {
        let bad = |e| format!("invalid offset: {e}");
        data = match data {
            Data::Single(s) => Data::Single(s.with_utc_offset(minutes).map_err(bad)?),
            Data::Many(mut e) => {
                for member in &mut e.members {
                    member.series = member
                        .series
                        .clone()
                        .with_utc_offset(minutes)
                        .map_err(bad)?;
                }
                Data::Many(e)
            }
        };
    }
    match data {
        Data::Single(series) => {
            if args.min_agreement.is_some() || args.min_coverage.is_some() {
                return Err("--min-agreement and --min-coverage apply only to several forecast versions (--models, --ensemble or an ensemble file)".into());
            }
            let result = WindowSearch::new(&series, &plan)
                .run()
                .map_err(|e| format!("invalid plan: {e}"))?;
            if args.json {
                let out = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
                println!("{out}");
            } else {
                print_text(&plan, &series, &result, args);
            }
        }
        Data::Many(ensemble) => {
            let result = EnsembleSearch::new(&ensemble, &plan)
                .min_agreement(args.min_agreement.unwrap_or(1.0))
                .min_coverage(args.min_coverage.unwrap_or(0.5))
                .run()
                .map_err(|e| format!("invalid search: {e}"))?;
            if args.json {
                let out = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
                println!("{out}");
            } else {
                print_ensemble(&plan, &ensemble, &result, args);
            }
        }
    }
    Ok(())
}

/// What to print as the reading: the note for checks that are not a number
/// (time of day), otherwise the value with its unit.
fn show_evidence(item: &Evidence) -> String {
    match &item.note {
        Some(note) => note.clone(),
        None => show(&item.metric, item.actual),
    }
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
                show_evidence(item),
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
                show_evidence(f),
                f.expected
            );
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn print_ensemble(plan: &Plan, ensemble: &Ensemble, result: &EnsembleResult, args: &Args) {
    let grid = ensemble.grid();
    let first = grid.observations.first().map_or(0, |o| o.timestamp_ms);
    let last = grid.observations.last().map_or(0, |o| o.timestamp_ms) + grid.cadence_ms;
    let meeting = result
        .windows
        .iter()
        .filter(|w| w.meets_requirement)
        .count();
    println!(
        "Plan `{}` over data {} -> {}, {} forecast versions ({})",
        plan.name,
        format_utc(first),
        format_utc(last),
        result.members.len(),
        if result.members.len() <= 6 {
            result.members.join(", ")
        } else {
            format!(
                "{}, ... {}",
                result.members[..3].join(", "),
                result.members.last().unwrap()
            )
        }
    );
    println!(
        "Requirement: fits in {:.0}% of the members that can answer, and at least {:.0}% of all members can answer.",
        result.min_agreement * 100.0,
        result.min_coverage * 100.0
    );
    println!(
        "{} of {} windows meet it. \"Fits in k of n\" counts forecast versions; it is not a probability.",
        meeting,
        result.windows.len()
    );
    // Metrics that no member provides cannot be judged by any rule that uses them.
    let mut absent: Vec<&str> = result
        .windows
        .iter()
        .flat_map(|w| w.missing.iter())
        .filter(|m| m.members == result.members.len())
        .map(|m| m.metric.as_str())
        .collect();
    absent.sort_unstable();
    absent.dedup();
    if !absent.is_empty() {
        println!(
            "Note: no forecast version provides {}, so rules on it cannot be judged. Relax those rules or use data that includes them.",
            absent.join(", ")
        );
    }
    if meeting == 0 && !result.windows.is_empty() {
        println!("No window meets the requirement; closest first.");
    }
    for (rank, w) in result.windows.iter().take(args.top).enumerate() {
        let answering = w.feasible + w.infeasible;
        println!(
            "\n#{} {} -> {}  {}",
            rank + 1,
            format_utc(w.start_ms),
            format_utc(w.end_ms),
            if w.meets_requirement {
                "meets the requirement"
            } else {
                "does not meet the requirement"
            }
        );
        match w.agreement {
            Some(_) => println!("   fits in {} of {} that can answer", w.feasible, answering),
            None => println!("   no forecast version could answer"),
        }
        if w.unknown > 0 {
            let what: Vec<String> = w
                .missing
                .iter()
                .map(|m| {
                    format!(
                        "{} for {}",
                        m.metric,
                        plural(m.members, "member", "members")
                    )
                })
                .collect();
            println!("   cannot say: {} ({})", w.unknown, what.join(", "));
        }
        if let Some(s) = w.suitability {
            println!("   preference score {s:.2} (mean over the members where it fits)");
        }
        for b in &w.blockers {
            println!(
                "   blocked by {}/{} in {}",
                b.stage,
                b.constraint,
                plural(b.members, "member", "members")
            );
        }
    }
    if args.rejected {
        println!("\nPer-member verdicts for the windows listed above:");
        for w in result.windows.iter().take(args.top) {
            let cells: Vec<String> = w
                .outcomes
                .iter()
                .map(|o| format!("{}={}", o.member, format!("{:?}", o.verdict).to_lowercase()))
                .collect();
            println!("   {}: {}", format_utc(w.start_ms), cells.join(" "));
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
    fn between_and_offset_are_parsed_and_validated() {
        assert_eq!(
            parse_between("09:00-12:00").unwrap().to_string(),
            "09:00-12:00"
        );
        assert_eq!(
            parse_between("20:00-06:00").unwrap().to_string(),
            "20:00-06:00"
        );
        assert!(parse_between("09:00").is_err());
        assert!(parse_between("09:00-09:00")
            .unwrap_err()
            .contains("must differ"));
        assert!(parse_between("9am-12:00").is_err());
        assert_eq!(parse_offset("+02:00"), Ok(120));
        assert_eq!(parse_offset("-05:30"), Ok(-330));
        assert_eq!(parse_offset("Z"), Ok(0));
        assert!(parse_offset("02:00").is_err());
        assert!(parse_offset("+15:00").is_err());
        assert!(parse_offset("+02:75").is_err());
        assert!(parse(&["--preset", "drone", "--series", "s", "--between", "nope"]).is_err());
        assert!(matches!(
            parse(&[
                "--preset",
                "drone",
                "--series",
                "s",
                "--between",
                "09:00-12:00",
                "--utc-offset",
                "+02:00"
            ]),
            Ok(Command::Run(_))
        ));
    }
    #[test]
    fn ensemble_flags_are_parsed_and_validated() {
        let ok = parse(&[
            "--preset",
            "drone",
            "--open-meteo",
            "1,2",
            "--models",
            "a_b,c",
        ]);
        assert!(matches!(ok, Ok(Command::Run(_))));
        assert!(matches!(
            parse(&[
                "--preset",
                "drone",
                "--open-meteo",
                "1,2",
                "--ensemble",
                "icon_seamless",
                "--min-agreement",
                "0.8",
                "--min-coverage",
                "1"
            ]),
            Ok(Command::Run(_))
        ));
        // Fetch flags need a place to fetch for, and cannot be combined.
        assert!(parse(&["--preset", "drone", "--series", "s", "--models", "a"]).is_err());
        assert!(parse(&[
            "--preset",
            "drone",
            "--open-meteo",
            "1,2",
            "--models",
            "a",
            "--ensemble",
            "b"
        ])
        .is_err());
        assert!(parse(&[
            "--preset",
            "drone",
            "--open-meteo",
            "1,2",
            "--models",
            "a,,b"
        ])
        .is_err());
        for bad in ["0", "1.5", "-1", "x", "NaN"] {
            assert!(
                parse(&["--preset", "drone", "--series", "s", "--min-agreement", bad]).is_err(),
                "{bad}"
            );
        }
        assert!(parse(&[
            "--preset",
            "drone",
            "--series",
            "s",
            "--format",
            "open-meteo-ensemble"
        ])
        .is_ok());
        assert!(parse(&["--preset", "drone", "--series", "s", "--format", "nope"]).is_err());
    }
    #[test]
    fn values_show_canonical_units() {
        assert_eq!(show("wind_speed", Some(10.0)), "10 m/s");
        assert_eq!(show("wind_speed", Some(10.2889)), "10.29 m/s");
        assert_eq!(show("custom", Some(3.5)), "3.5");
        assert_eq!(show("wind_speed", None), "missing");
    }
}
