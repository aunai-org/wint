//! Command-line front end: `env-operability --plan plan.json --series series.json`.

use env_operability::time::format_utc;
use env_operability::{Plan, SearchResult, Series, WindowSearch};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: env-operability --plan <plan.json> --series <series.json> [options]

Options:
  --top <n>      Show at most n feasible windows in text output (default 5)
  --rejected     Also list why each rejected window failed (text output)
  --json         Print the full result as JSON instead of text
  -h, --help     Show this help

Timestamps in text output are UTC. Scores describe preference only; they are
not probabilities and not safety certifications.";

struct Args {
    plan: String,
    series: String,
    top: usize,
    rejected: bool,
    json: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let (mut plan, mut series) = (None, None);
    let (mut top, mut rejected, mut json) = (5, false, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--plan" => plan = Some(value("--plan")?),
            "--series" => series = Some(value("--series")?),
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
    Ok(Some(Args {
        plan: plan.ok_or("--plan is required")?,
        series: series.ok_or("--series is required")?,
        top,
        rejected,
        json,
    }))
}

fn load<T: serde::de::DeserializeOwned>(path: &str, what: &str) -> Result<T, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {what} `{path}`: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid {what} `{path}`: {e}"))
}

fn run(args: &Args) -> Result<(), String> {
    let series: Series = load(&args.series, "series")?;
    let plan: Plan = load(&args.plan, "plan")?;
    let result = WindowSearch::new(&series, &plan)
        .run()
        .map_err(|e| format!("invalid plan: {e}"))?;
    if args.json {
        let out = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
        println!("{out}");
    } else {
        print_text(&plan, &result, args);
    }
    Ok(())
}

fn print_text(plan: &Plan, result: &SearchResult, args: &Args) {
    println!(
        "Plan `{}`: {} feasible, {} rejected",
        plan.name,
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
                item.actual.map_or("missing".into(), |v| v.to_string()),
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
                f.actual.map_or("missing".into(), |v| v.to_string()),
                f.expected
            );
        }
    }
}

fn main() -> ExitCode {
    match parse_args() {
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(args)) => match run(&args) {
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
