use crate::{Comparison, Constraint, Plan, Preference, Series, ValidationError};

/// One auditable observation behind a decision.
///
/// For a passing hard constraint the evidence is the *binding* sample: the one
/// closest to the limit. For a soft constraint it is the sample with the
/// largest penalty. For a rejection it is the first sample that failed.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Evidence {
    pub stage: String,
    pub constraint: String,
    pub timestamp_ms: i64,
    pub metric: String,
    pub actual: Option<f64>,
    pub expected: String,
    pub passed: bool,
    /// Soft constraints only: penalty in `[0, 1]` for this sample.
    pub penalty: Option<f64>,
}
/// Per-stage outcome of a feasible window.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct StageResult {
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Soft-preference suitability within this stage alone (1.0 if none apply).
    pub suitability: f64,
}
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct WindowResult {
    pub start_ms: i64,
    pub end_ms: i64,
    pub suitability: f64,
    pub stages: Vec<StageResult>,
    pub evidence: Vec<Evidence>,
}
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct RejectedWindow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub failure: Evidence,
}
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct SearchResult {
    pub feasible: Vec<WindowResult>,
    pub rejected: Vec<RejectedWindow>,
}
pub struct WindowSearch<'a> {
    series: &'a Series,
    plan: &'a Plan,
}
impl<'a> WindowSearch<'a> {
    pub fn new(series: &'a Series, plan: &'a Plan) -> Self {
        Self { series, plan }
    }
    /// Runs the search. Returns an error if the plan is invalid for this
    /// series; a series shorter than the plan yields an empty result.
    pub fn run(&self) -> Result<SearchResult, ValidationError> {
        self.plan.validate(self.series.cadence_ms)?;
        let samples = (self.plan.duration_ms() / self.series.cadence_ms) as usize;
        let mut result = SearchResult::default();
        if samples > self.series.observations.len() {
            return Ok(result);
        }
        for start_index in 0..=self.series.observations.len() - samples {
            let start_ms = self.series.observations[start_index].timestamp_ms;
            let end_ms = start_ms + self.plan.duration_ms();
            match self.evaluate(start_index) {
                Ok(window) => result.feasible.push(WindowResult {
                    start_ms,
                    end_ms,
                    ..window
                }),
                Err(failure) => result.rejected.push(RejectedWindow {
                    start_ms,
                    end_ms,
                    failure: *failure,
                }),
            }
        }
        result.feasible.sort_by(|a, b| {
            b.suitability
                .total_cmp(&a.suitability)
                .then(a.start_ms.cmp(&b.start_ms))
        });
        Ok(result)
    }
    fn evaluate(&self, start: usize) -> Result<WindowResult, Box<Evidence>> {
        let mut evidence = Vec::new();
        let mut stages = Vec::new();
        let mut weighted_penalty = 0.0;
        let mut total_weight = 0.0;
        let mut offset = 0;
        for stage in &self.plan.stages {
            let count = (stage.duration_ms / self.series.cadence_ms) as usize;
            let samples = &self.series.observations[start + offset..start + offset + count];
            // Binding evidence per constraint: (rank, evidence); lower rank binds harder.
            let mut binding: Vec<Option<(f64, Evidence)>> = vec![None; stage.constraints.len()];
            let (mut stage_penalty, mut stage_weight) = (0.0, 0.0);
            for observation in samples {
                for (index, constraint) in stage.constraints.iter().enumerate() {
                    match constraint {
                        Constraint::Hard {
                            name,
                            metric,
                            comparison,
                            threshold,
                        } => {
                            let actual = observation.values.get(&metric.0).copied();
                            let passed = actual.is_some_and(|v| comparison.matches(v, *threshold));
                            let item = Evidence {
                                stage: stage.name.clone(),
                                constraint: name.clone(),
                                timestamp_ms: observation.timestamp_ms,
                                metric: metric.0.clone(),
                                actual,
                                expected: format_comparison(*comparison, *threshold),
                                passed,
                                penalty: None,
                            };
                            if !passed {
                                return Err(Box::new(item));
                            }
                            let margin =
                                hard_margin(*comparison, actual.unwrap_or(0.0), *threshold);
                            keep_binding(&mut binding[index], margin, item);
                        }
                        Constraint::Soft {
                            name,
                            metric,
                            preference,
                            weight,
                        } if *weight > 0.0 => {
                            if let Some(actual) = observation.values.get(&metric.0).copied() {
                                let penalty = preference_penalty(preference, actual);
                                stage_penalty += penalty * weight;
                                stage_weight += weight;
                                let item = Evidence {
                                    stage: stage.name.clone(),
                                    constraint: name.clone(),
                                    timestamp_ms: observation.timestamp_ms,
                                    metric: metric.0.clone(),
                                    actual: Some(actual),
                                    expected: format_preference(preference),
                                    passed: true,
                                    penalty: Some(penalty),
                                };
                                keep_binding(&mut binding[index], -penalty, item);
                            }
                        }
                        Constraint::Soft { .. } => {}
                    }
                }
            }
            evidence.extend(binding.into_iter().flatten().map(|(_, item)| item));
            weighted_penalty += stage_penalty;
            total_weight += stage_weight;
            let stage_start = samples[0].timestamp_ms;
            stages.push(StageResult {
                name: stage.name.clone(),
                start_ms: stage_start,
                end_ms: stage_start + stage.duration_ms,
                suitability: suitability(stage_penalty, stage_weight),
            });
            offset += count;
        }
        Ok(WindowResult {
            start_ms: 0,
            end_ms: 0,
            suitability: suitability(weighted_penalty, total_weight),
            stages,
            evidence,
        })
    }
}
fn suitability(weighted_penalty: f64, total_weight: f64) -> f64 {
    if total_weight == 0.0 {
        1.0
    } else {
        (1.0 - weighted_penalty / total_weight).clamp(0.0, 1.0)
    }
}
/// Keeps the lowest-rank item; ties keep the earliest sample.
fn keep_binding(slot: &mut Option<(f64, Evidence)>, rank: f64, item: Evidence) {
    if slot.as_ref().is_none_or(|(best, _)| rank < *best) {
        *slot = Some((rank, item));
    }
}
/// Distance a passing value sits inside its limit (0 means right at it).
fn hard_margin(c: Comparison, actual: f64, threshold: f64) -> f64 {
    match c {
        Comparison::LessThan | Comparison::LessThanOrEqual => threshold - actual,
        Comparison::GreaterThan | Comparison::GreaterThanOrEqual => actual - threshold,
        Comparison::Equal => 0.0,
    }
}
fn format_preference(p: &Preference) -> String {
    match *p {
        Preference::Minimize { ideal, scale } => format!("minimize: ideal {ideal}, scale {scale}"),
        Preference::Maximize { ideal, scale } => format!("maximize: ideal {ideal}, scale {scale}"),
        Preference::Range { min, max, scale } => {
            format!("range: {min}..={max}, scale {scale}")
        }
    }
}
fn format_comparison(c: Comparison, t: f64) -> String {
    let symbol = match c {
        Comparison::LessThan => "<",
        Comparison::LessThanOrEqual => "<=",
        Comparison::GreaterThan => ">",
        Comparison::GreaterThanOrEqual => ">=",
        Comparison::Equal => "==",
    };
    format!("{symbol} {t}")
}
fn preference_penalty(preference: &Preference, value: f64) -> f64 {
    let (deviation, scale) = match *preference {
        Preference::Minimize { ideal, scale } => ((value - ideal).max(0.0), scale),
        Preference::Maximize { ideal, scale } => ((ideal - value).max(0.0), scale),
        Preference::Range { min, max, scale } => ((min - value).max(value - max).max(0.0), scale),
    };
    (deviation / scale).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Metric, Observation, Stage};
    fn series() -> Series {
        Series::new(
            1,
            vec![
                Observation::at(0).with("wind", 5.0),
                Observation::at(1).with("wind", 12.0),
                Observation::at(2).with("wind", 4.0),
            ],
        )
        .unwrap()
    }
    #[test]
    fn finds_only_contiguous_safe_window() {
        let plan = Plan::single_stage(
            "test",
            2,
            vec![Constraint::hard(
                "wind",
                Metric::new("wind"),
                Comparison::LessThan,
                10.0,
            )],
        );
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        assert_eq!(found.feasible.len(), 0);
        assert_eq!(found.rejected.len(), 2);
    }
    #[test]
    fn stage_constraints_are_applied_in_order() {
        let plan = Plan::new(
            "staged",
            vec![
                Stage::new(
                    "launch",
                    1,
                    vec![Constraint::hard(
                        "launch wind",
                        Metric::new("wind"),
                        Comparison::LessThan,
                        10.0,
                    )],
                ),
                Stage::new(
                    "work",
                    1,
                    vec![Constraint::hard(
                        "work wind",
                        Metric::new("wind"),
                        Comparison::LessThan,
                        10.0,
                    )],
                ),
            ],
        );
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        assert_eq!(found.rejected[0].failure.stage, "work");
    }
    #[test]
    fn ranks_soft_preferences() {
        let plan = Plan::single_stage(
            "score",
            1,
            vec![Constraint::soft(
                "calm",
                Metric::new("wind"),
                Preference::Minimize {
                    ideal: 0.0,
                    scale: 20.0,
                },
                1.0,
            )],
        );
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        assert_eq!(found.feasible[0].start_ms, 2);
        assert!(found.feasible[0].suitability > found.feasible[1].suitability);
    }
    fn soft_plan(preference: Preference) -> Plan {
        Plan::single_stage(
            "p",
            1,
            vec![Constraint::soft("c", Metric::new("wind"), preference, 1.0)],
        )
    }
    #[test]
    fn scale_sets_the_full_penalty_distance() {
        let p = Preference::Minimize {
            ideal: 10.0,
            scale: 20.0,
        };
        assert_eq!(preference_penalty(&p, 10.0), 0.0);
        assert_eq!(preference_penalty(&p, 5.0), 0.0);
        assert_eq!(preference_penalty(&p, 20.0), 0.5);
        assert_eq!(preference_penalty(&p, 100.0), 1.0);
        let m = Preference::Maximize {
            ideal: 10.0,
            scale: 5.0,
        };
        assert_eq!(preference_penalty(&m, 7.5), 0.5);
        let r = Preference::Range {
            min: 10.0,
            max: 20.0,
            scale: 10.0,
        };
        assert_eq!(preference_penalty(&r, 15.0), 0.0);
        assert_eq!(preference_penalty(&r, 25.0), 0.5);
        assert_eq!(preference_penalty(&r, 0.0), 1.0);
    }
    #[test]
    fn invalid_plans_are_errors() {
        let s = series();
        let empty = Plan::new("e", vec![]);
        assert_eq!(
            WindowSearch::new(&s, &empty).run(),
            Err(ValidationError::EmptyPlan)
        );
        let zero = Plan::single_stage("z", 0, vec![]);
        assert!(matches!(
            WindowSearch::new(&s, &zero).run(),
            Err(ValidationError::InvalidStageDuration { .. })
        ));
        let bad_scale = soft_plan(Preference::Minimize {
            ideal: 0.0,
            scale: 0.0,
        });
        assert!(matches!(
            WindowSearch::new(&s, &bad_scale).run(),
            Err(ValidationError::InvalidConstraint { .. })
        ));
        let bad_range = soft_plan(Preference::Range {
            min: 2.0,
            max: 1.0,
            scale: 1.0,
        });
        assert!(WindowSearch::new(&s, &bad_range).run().is_err());
    }
    #[test]
    fn plan_longer_than_series_yields_empty_result() {
        let plan = Plan::single_stage("long", 10, vec![]);
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        assert!(found.feasible.is_empty() && found.rejected.is_empty());
    }
    #[test]
    fn evidence_reports_binding_sample_not_every_sample() {
        // Windows of 3 samples over wind 5, 12, 4: the binding (closest to the
        // limit) sample for "wind < 20" is the 12 at t=1, and only one evidence
        // row is produced for the constraint.
        let plan = Plan::single_stage(
            "p",
            3,
            vec![Constraint::hard(
                "wind",
                Metric::new("wind"),
                Comparison::LessThan,
                20.0,
            )],
        );
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        let window = &found.feasible[0];
        assert_eq!(window.evidence.len(), 1);
        assert_eq!(window.evidence[0].timestamp_ms, 1);
        assert_eq!(window.evidence[0].actual, Some(12.0));
    }
    #[test]
    fn stage_breakdown_scores_each_stage_separately() {
        let calm = |name: &str| {
            Constraint::soft(
                name,
                Metric::new("wind"),
                Preference::Minimize {
                    ideal: 0.0,
                    scale: 10.0,
                },
                1.0,
            )
        };
        let plan = Plan::new(
            "s",
            vec![
                Stage::new("a", 1, vec![calm("a calm")]),
                Stage::new("b", 1, vec![]),
            ],
        );
        let found = WindowSearch::new(&series(), &plan).run().unwrap();
        let first = found.feasible.iter().find(|w| w.start_ms == 0).unwrap();
        assert_eq!(first.stages.len(), 2);
        assert_eq!((first.stages[0].start_ms, first.stages[0].end_ms), (0, 1));
        assert_eq!((first.stages[1].start_ms, first.stages[1].end_ms), (1, 2));
        assert!((first.stages[0].suitability - 0.5).abs() < 1e-9);
        assert_eq!(first.stages[1].suitability, 1.0);
    }
    #[test]
    fn rejects_irregular_series() {
        assert!(Series::new(1, vec![Observation::at(0), Observation::at(2)]).is_err());
    }
}
