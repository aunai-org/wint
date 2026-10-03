use crate::{Comparison, Constraint, Plan, Preference, Schedule, Series, ValidationError};

/// What a piece of evidence was checked against, as data. The engine never turns this into text:
/// wording, rounding and units belong to whoever presents the result (see [`crate::present`] for an
/// optional default).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(tag = "type", rename_all = "snake_case"))]
pub enum Expectation {
    /// A hard limit: the reading must satisfy `reading <comparison> threshold`.
    Comparison {
        comparison: Comparison,
        threshold: f64,
    },
    /// A soft preference, scored rather than passed or failed.
    Preference { preference: Preference },
    /// A daily clock window (minutes after local midnight; `to_minute` may be 1440).
    ClockWindow { from_minute: u16, to_minute: u16 },
}

/// The span of local clock time a time-of-day check examined, as numbers: minutes after local
/// midnight (the end wraps at 24:00) and the local clock's offset from UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct ClockSpan {
    pub start_minute: u16,
    pub end_minute: u16,
    pub utc_offset_minutes: i32,
}

/// One auditable observation behind a decision.
///
/// For a passing hard constraint the evidence is the *binding* sample: the one
/// closest to the limit. For a soft constraint it is the sample with the
/// largest penalty. For a rejection it is the first sample that failed.
/// Everything here is data or an identifier chosen by the plan's author; no field is display text.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Evidence {
    pub stage: String,
    pub constraint: String,
    pub timestamp_ms: i64,
    pub metric: String,
    pub actual: Option<f64>,
    /// What the reading was checked against.
    pub expectation: Expectation,
    pub passed: bool,
    /// Soft constraints only: penalty in `[0, 1]` for this sample.
    pub penalty: Option<f64>,
    /// Time-of-day checks only: the local clock span that was examined.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub clock: Option<ClockSpan>,
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
        self.run_with(false)
    }
    /// Like [`run`](Self::run), but a missing reading does not stop the scan of a window: a definite
    /// violation later in the window is reported in preference, and the window fails on the missing
    /// reading only if nothing else does. Ensembles use this to tell "does not fit" from "cannot say".
    pub(crate) fn run_tolerant(&self) -> Result<SearchResult, ValidationError> {
        self.run_with(true)
    }
    fn run_with(&self, tolerate_missing: bool) -> Result<SearchResult, ValidationError> {
        self.plan.validate(self.series.cadence_ms)?;
        let samples = (self.plan.duration_ms() / self.series.cadence_ms) as usize;
        let mut result = SearchResult::default();
        if samples > self.series.observations.len() {
            return Ok(result);
        }
        for start_index in 0..=self.series.observations.len() - samples {
            let start_ms = self.series.observations[start_index].timestamp_ms;
            let end_ms = start_ms + self.plan.duration_ms();
            match self.evaluate(start_index, tolerate_missing) {
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
    fn evaluate(
        &self,
        start: usize,
        tolerate_missing: bool,
    ) -> Result<WindowResult, Box<Evidence>> {
        // First missing reading seen while tolerating them (see `run_tolerant`).
        let mut deferred_missing: Option<Evidence> = None;
        let mut evidence = Vec::new();
        let mut stages = Vec::new();
        let mut weighted_penalty = 0.0;
        let mut total_weight = 0.0;
        let mut offset = 0;
        let offset_ms = i64::from(self.series.utc_offset_minutes) * 60_000;
        for stage in &self.plan.stages {
            let count = (stage.duration_ms / self.series.cadence_ms) as usize;
            let samples = &self.series.observations[start + offset..start + offset + count];
            // Binding evidence per constraint: (rank, evidence); lower rank binds harder.
            let mut binding: Vec<Option<(f64, Evidence)>> = vec![None; stage.constraints.len()];
            let (mut stage_penalty, mut stage_weight) = (0.0, 0.0);
            for observation in samples {
                if let Some(schedule) = &stage.schedule {
                    let local = observation.timestamp_ms + offset_ms;
                    if !schedule.contains_span(local, self.series.cadence_ms) {
                        return Err(Box::new(schedule_evidence(
                            stage,
                            schedule,
                            observation.timestamp_ms,
                            local,
                            self.series.cadence_ms,
                            self.series.utc_offset_minutes,
                            false,
                        )));
                    }
                }
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
                                expectation: Expectation::Comparison {
                                    comparison: *comparison,
                                    threshold: *threshold,
                                },
                                passed,
                                penalty: None,
                                clock: None,
                            };
                            if !passed {
                                if tolerate_missing && actual.is_none() {
                                    deferred_missing.get_or_insert(item);
                                    continue;
                                }
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
                                    expectation: Expectation::Preference {
                                        preference: preference.clone(),
                                    },
                                    passed: true,
                                    penalty: Some(penalty),
                                    clock: None,
                                };
                                keep_binding(&mut binding[index], -penalty, item);
                            }
                        }
                        Constraint::Soft { .. } => {}
                    }
                }
            }
            if let Some(schedule) = &stage.schedule {
                evidence.push(schedule_evidence(
                    stage,
                    schedule,
                    samples[0].timestamp_ms,
                    samples[0].timestamp_ms + offset_ms,
                    stage.duration_ms,
                    self.series.utc_offset_minutes,
                    true,
                ));
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
        if let Some(item) = deferred_missing {
            return Err(Box::new(item));
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
/// Evidence for a time-of-day check. `local_ms` is the span start on the local clock and `len_ms`
/// its length.
fn schedule_evidence(
    stage: &crate::Stage,
    schedule: &Schedule,
    timestamp_ms: i64,
    local_ms: i64,
    len_ms: i64,
    utc_offset_minutes: i32,
    passed: bool,
) -> Evidence {
    let day_minutes = 1440;
    let start = local_ms.div_euclid(60_000).rem_euclid(day_minutes) as u16;
    let end = (local_ms + len_ms)
        .div_euclid(60_000)
        .rem_euclid(day_minutes) as u16;
    Evidence {
        stage: stage.name.clone(),
        constraint: "time of day".into(),
        timestamp_ms,
        metric: "local_time".into(),
        actual: None,
        expectation: Expectation::ClockWindow {
            from_minute: schedule.from_minute(),
            to_minute: schedule.to_minute(),
        },
        passed,
        penalty: None,
        clock: Some(ClockSpan {
            start_minute: start,
            end_minute: end,
            utc_offset_minutes,
        }),
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
