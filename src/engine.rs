use crate::{Comparison, Constraint, Plan, Preference, Series};

#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub stage: String,
    pub constraint: String,
    pub timestamp_ms: i64,
    pub metric: String,
    pub actual: Option<f64>,
    pub expected: String,
    pub passed: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct WindowResult {
    pub start_ms: i64,
    pub end_ms: i64,
    pub suitability: f64,
    pub evidence: Vec<Evidence>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedWindow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub failure: Evidence,
}
#[derive(Clone, Debug, Default, PartialEq)]
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
    pub fn run(&self) -> SearchResult {
        let samples = match self.sample_count() {
            Some(value) => value,
            None => return SearchResult::default(),
        };
        let mut result = SearchResult::default();
        for start_index in 0..=self.series.observations.len().saturating_sub(samples) {
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
        result
    }
    fn sample_count(&self) -> Option<usize> {
        let duration = self.plan.duration_ms();
        if duration <= 0 || duration % self.series.cadence_ms != 0 {
            None
        } else {
            Some((duration / self.series.cadence_ms) as usize)
        }
    }
    fn evaluate(&self, start: usize) -> Result<WindowResult, Box<Evidence>> {
        let mut evidence = Vec::new();
        let mut weighted_penalty = 0.0;
        let mut total_weight = 0.0;
        let mut offset = 0;
        for stage in &self.plan.stages {
            if stage.duration_ms <= 0 || stage.duration_ms % self.series.cadence_ms != 0 {
                return Err(Box::new(invalid_stage(stage)));
            }
            let count = (stage.duration_ms / self.series.cadence_ms) as usize;
            for observation in &self.series.observations[start + offset..start + offset + count] {
                for constraint in &stage.constraints {
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
                            };
                            if !passed {
                                return Err(Box::new(item));
                            }
                            evidence.push(item);
                        }
                        Constraint::Soft {
                            name,
                            metric,
                            preference,
                            weight,
                        } if *weight > 0.0 => {
                            if let Some(actual) = observation.values.get(&metric.0).copied() {
                                let penalty = preference_penalty(preference, actual);
                                weighted_penalty += penalty * weight;
                                total_weight += weight;
                                evidence.push(Evidence {
                                    stage: stage.name.clone(),
                                    constraint: name.clone(),
                                    timestamp_ms: observation.timestamp_ms,
                                    metric: metric.0.clone(),
                                    actual: Some(actual),
                                    expected: "soft preference".into(),
                                    passed: true,
                                });
                            }
                        }
                        Constraint::Soft { .. } => {}
                    }
                }
            }
            offset += count;
        }
        Ok(WindowResult {
            start_ms: 0,
            end_ms: 0,
            suitability: if total_weight == 0.0 {
                1.0
            } else {
                (1.0 - weighted_penalty / total_weight).clamp(0.0, 1.0)
            },
            evidence,
        })
    }
}
fn invalid_stage(stage: &crate::Stage) -> Evidence {
    Evidence {
        stage: stage.name.clone(),
        constraint: "stage duration".into(),
        timestamp_ms: 0,
        metric: String::new(),
        actual: None,
        expected: "positive cadence multiple".into(),
        passed: false,
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
    match *preference {
        Preference::Minimize { ideal } => {
            ((value - ideal).max(0.0) / ideal.abs().max(1.0)).min(1.0)
        }
        Preference::Maximize { ideal } => {
            ((ideal - value).max(0.0) / ideal.abs().max(1.0)).min(1.0)
        }
        Preference::Range { min, max } => {
            let scale = (max - min).abs().max(1.0);
            if value < min {
                ((min - value) / scale).min(1.0)
            } else if value > max {
                ((value - max) / scale).min(1.0)
            } else {
                0.0
            }
        }
    }
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
        let found = WindowSearch::new(&series(), &plan).run();
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
        let found = WindowSearch::new(&series(), &plan).run();
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
                Preference::Minimize { ideal: 0.0 },
                1.0,
            )],
        );
        let found = WindowSearch::new(&series(), &plan).run();
        assert_eq!(found.feasible[0].start_ms, 2);
        assert!(found.feasible[0].suitability > found.feasible[1].suitability);
    }
    #[test]
    fn rejects_irregular_series() {
        assert!(Series::new(1, vec![Observation::at(0), Observation::at(2)]).is_err());
    }
}
