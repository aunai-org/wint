//! Ready-made starting-point plans.
//!
//! **These are illustrative defaults, not safety guidance.** Limits differ by
//! aircraft, vessel, equipment, venue, jurisdiction and risk appetite. Read
//! every threshold, compare it with your manufacturer's limits and your
//! regulator's rules, and adjust it before relying on the result. Presets use
//! the canonical units of the metric vocabulary ([`crate::units`]).

use crate::{Comparison, Constraint, Metric, Plan, Preference};

/// A named, documented plan template.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    build: fn(i64) -> Plan,
}

impl Preset {
    /// Builds the plan for an operation lasting `duration_ms`.
    pub fn plan(&self, duration_ms: i64) -> Plan {
        (self.build)(duration_ms)
    }
}

/// Every preset, in a stable order.
pub const ALL: &[Preset] = &[
    Preset {
        name: "drone",
        description: "Small multirotor flight: wind and gusts under 10/12 m/s, no rain, 4.8 km (3 mi) visibility, 0-40 C; prefers calm air.",
        build: drone,
    },
    Preset {
        name: "outdoor-event",
        description: "Outdoor gathering: rain chance under 30%, wind under 8 m/s (gusts 14), 5-35 C; prefers dry, mild (16-26 C) weather.",
        build: outdoor_event,
    },
    Preset {
        name: "field-work",
        description: "General field work (e.g. mowing, hay): almost no rain, wind under 12 m/s, at least 2 C; prefers dry, mild, moderately humid days.",
        build: field_work,
    },
];

/// Looks a preset up by name.
pub fn by_name(name: &str) -> Option<&'static Preset> {
    ALL.iter().find(|p| p.name == name)
}

fn hard(name: &str, metric: &str, comparison: Comparison, threshold: f64) -> Constraint {
    Constraint::hard(name, Metric::new(metric), comparison, threshold)
}
fn soft(name: &str, metric: &str, preference: Preference, weight: f64) -> Constraint {
    Constraint::soft(name, Metric::new(metric), preference, weight)
}

fn drone(duration_ms: i64) -> Plan {
    use Comparison::*;
    Plan::single_stage(
        "drone",
        duration_ms,
        vec![
            hard("wind limit", "wind_speed", LessThanOrEqual, 10.0),
            hard("gust limit", "wind_gust", LessThanOrEqual, 12.0),
            hard("no precipitation", "precipitation", LessThanOrEqual, 0.1),
            hard(
                "visibility (3 statute miles)",
                "visibility",
                GreaterThanOrEqual,
                4_828.0,
            ),
            hard(
                "operating temperature min",
                "temperature",
                GreaterThanOrEqual,
                0.0,
            ),
            hard(
                "operating temperature max",
                "temperature",
                LessThanOrEqual,
                40.0,
            ),
            soft(
                "calm air",
                "wind_speed",
                Preference::Minimize {
                    ideal: 2.0,
                    scale: 8.0,
                },
                2.0,
            ),
            soft(
                "low rain chance",
                "precipitation_probability",
                Preference::Minimize {
                    ideal: 0.0,
                    scale: 50.0,
                },
                1.0,
            ),
        ],
    )
}

fn outdoor_event(duration_ms: i64) -> Plan {
    use Comparison::*;
    Plan::single_stage(
        "outdoor-event",
        duration_ms,
        vec![
            hard("rain chance", "precipitation_probability", LessThan, 30.0),
            hard("wind limit", "wind_speed", LessThanOrEqual, 8.0),
            hard("gust limit", "wind_gust", LessThanOrEqual, 14.0),
            hard("temperature min", "temperature", GreaterThanOrEqual, 5.0),
            hard("temperature max", "temperature", LessThanOrEqual, 35.0),
            soft(
                "mild temperature",
                "temperature",
                Preference::Range {
                    min: 16.0,
                    max: 26.0,
                    scale: 10.0,
                },
                2.0,
            ),
            soft(
                "dry",
                "precipitation",
                Preference::Minimize {
                    ideal: 0.0,
                    scale: 1.0,
                },
                1.0,
            ),
        ],
    )
}

fn field_work(duration_ms: i64) -> Plan {
    use Comparison::*;
    Plan::single_stage(
        "field-work",
        duration_ms,
        vec![
            hard("almost no rain", "precipitation", LessThanOrEqual, 0.2),
            hard("wind limit", "wind_speed", LessThanOrEqual, 12.0),
            hard("temperature min", "temperature", GreaterThanOrEqual, 2.0),
            soft(
                "low rain chance",
                "precipitation_probability",
                Preference::Minimize {
                    ideal: 0.0,
                    scale: 60.0,
                },
                2.0,
            ),
            soft(
                "mild temperature",
                "temperature",
                Preference::Range {
                    min: 10.0,
                    max: 28.0,
                    scale: 12.0,
                },
                1.0,
            ),
            soft(
                "moderate humidity",
                "relative_humidity",
                Preference::Range {
                    min: 30.0,
                    max: 75.0,
                    scale: 30.0,
                },
                1.0,
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::canonical_unit;
    use crate::{Observation, Series, WindowSearch};

    const HOUR: i64 = 3_600_000;

    #[test]
    fn every_preset_is_valid_and_uses_only_vocabulary_metrics() {
        for preset in ALL {
            let plan = preset.plan(2 * HOUR);
            plan.validate(HOUR).unwrap();
            assert_eq!(plan.duration_ms(), 2 * HOUR);
            assert!(!preset.description.is_empty());
            for stage in &plan.stages {
                for constraint in &stage.constraints {
                    let metric = match constraint {
                        Constraint::Hard { metric, .. } | Constraint::Soft { metric, .. } => {
                            &metric.0
                        }
                    };
                    assert!(
                        canonical_unit(metric).is_some(),
                        "{}: {metric}",
                        preset.name
                    );
                }
            }
        }
        assert!(by_name("drone").is_some() && by_name("nope").is_none());
    }

    fn hour(t: i64, wind: f64, gust: f64, rain: f64, vis: f64, temp: f64) -> Observation {
        Observation::at(t * HOUR)
            .with("wind_speed", wind)
            .with("wind_gust", gust)
            .with("precipitation", rain)
            .with("visibility", vis)
            .with("temperature", temp)
            .with("precipitation_probability", 5.0)
    }

    #[test]
    fn drone_accepts_calm_clear_and_rejects_gusty_or_low_visibility() {
        let series = Series::new(
            HOUR,
            vec![
                hour(0, 3.0, 5.0, 0.0, 20_000.0, 15.0),
                hour(1, 3.0, 5.0, 0.0, 20_000.0, 15.0),
                hour(2, 3.0, 13.0, 0.0, 20_000.0, 15.0), // gust over limit
                hour(3, 3.0, 5.0, 0.0, 2_000.0, 15.0),   // visibility under limit
            ],
        )
        .unwrap();
        let result = WindowSearch::new(&series, &by_name("drone").unwrap().plan(HOUR))
            .run()
            .unwrap();
        assert_eq!(result.feasible.len(), 2);
        let rejected: Vec<&str> = result
            .rejected
            .iter()
            .map(|r| r.failure.constraint.as_str())
            .collect();
        assert_eq!(rejected, ["gust limit", "visibility (3 statute miles)"]);
    }

    #[test]
    fn missing_metric_rejects_instead_of_passing() {
        let series = Series::new(
            HOUR,
            vec![
                Observation::at(0).with("wind_speed", 1.0),
                Observation::at(HOUR).with("wind_speed", 1.0),
            ],
        )
        .unwrap();
        let result = WindowSearch::new(&series, &by_name("drone").unwrap().plan(HOUR))
            .run()
            .unwrap();
        assert!(result.feasible.is_empty());
        assert_eq!(result.rejected[0].failure.actual, None);
    }
}
