//! Forecast ensembles and window agreement.
//!
//! An [`Ensemble`] is several parallel versions of one forecast: different weather models, or the
//! members of one model's ensemble. [`EnsembleSearch`] runs the ordinary window search on each
//! member and reports, per window, how many members it fits.
//!
//! # What the numbers mean (and do not)
//!
//! *Agreement* is the share of members, among those able to answer, in which the whole window
//! fits. It is **not** a probability that the operation will be possible. Ensemble members are not
//! calibrated frequencies, models share data and assumptions so they are not independent, and an
//! ensemble can be too narrow. Always present it as "fits in `k` of `n` members".
//!
//! A member that lacks a reading the plan needs (a model that does not forecast visibility, or one
//! whose horizon ends before the window) *cannot say*. It is reported separately instead of being
//! counted as disagreement, and shows up in `coverage`. A definite violation elsewhere in the
//! window still counts as "does not fit" even if a reading was also missing.

use crate::engine::{Evidence, WindowSearch};
use crate::{Plan, Series, ValidationError};
use std::collections::BTreeMap;

/// One forecast version: a model name or an ensemble member id, and its series.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Member {
    pub name: String,
    pub series: Series,
}

/// Parallel forecast versions on one shared time grid.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(try_from = "EnsembleData"))]
pub struct Ensemble {
    pub members: Vec<Member>,
}

/// Unvalidated wire form, so JSON cannot bypass [`Ensemble::new`].
#[cfg(feature = "json")]
#[derive(serde::Deserialize)]
struct EnsembleData {
    members: Vec<Member>,
}
#[cfg(feature = "json")]
impl TryFrom<EnsembleData> for Ensemble {
    type Error = ValidationError;
    fn try_from(data: EnsembleData) -> Result<Self, Self::Error> {
        Ensemble::new(data.members)
    }
}

impl Ensemble {
    /// Builds an ensemble. Members need unique names and must share cadence, timestamps and UTC
    /// offset exactly, so a window means the same span in every member.
    pub fn new(members: Vec<Member>) -> Result<Self, ValidationError> {
        let first = members.first().ok_or(ValidationError::EmptyEnsemble)?;
        for (i, member) in members.iter().enumerate() {
            if members[..i].iter().any(|m| m.name == member.name) {
                return Err(ValidationError::DuplicateMember {
                    name: member.name.clone(),
                });
            }
            let mismatch = |reason| ValidationError::EnsembleMismatch {
                member: member.name.clone(),
                reason,
            };
            let (a, b) = (&first.series, &member.series);
            if a.cadence_ms != b.cadence_ms {
                return Err(mismatch("has a different cadence"));
            }
            if a.utc_offset_minutes != b.utc_offset_minutes {
                return Err(mismatch("has a different UTC offset"));
            }
            if a.observations.len() != b.observations.len() {
                return Err(mismatch("has a different number of observations"));
            }
            if a.observations
                .iter()
                .zip(&b.observations)
                .any(|(x, y)| x.timestamp_ms != y.timestamp_ms)
            {
                return Err(mismatch("has different timestamps"));
            }
        }
        Ok(Self { members })
    }
    /// A one-member ensemble, so a plain series can use the ensemble search.
    pub fn single(name: impl Into<String>, series: Series) -> Self {
        Self {
            members: vec![Member {
                name: name.into(),
                series,
            }],
        }
    }
    /// The shared series geometry (cadence, timestamps, offset), taken from the first member.
    pub fn grid(&self) -> &Series {
        &self.members[0].series
    }
}

/// A member's verdict on one window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
pub enum Verdict {
    /// The window passes every hard constraint in this member.
    Feasible,
    /// The window breaks a hard constraint in this member.
    Infeasible,
    /// This member lacks a reading the plan needs, and nothing else rules the window out.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct MemberOutcome {
    pub member: String,
    pub verdict: Verdict,
    /// Preference score in this member, for feasible windows.
    pub suitability: Option<f64>,
    /// Why not: the first violation (infeasible) or the first missing reading (unknown).
    pub failure: Option<Evidence>,
}

/// A rule that ruled a window out, and in how many members.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct Blocker {
    pub stage: String,
    pub constraint: String,
    pub members: usize,
}

/// A metric some members could not provide, and how many.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct MissingData {
    pub metric: String,
    pub members: usize,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct EnsembleWindow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub members_total: usize,
    pub feasible: usize,
    pub infeasible: usize,
    pub unknown: usize,
    /// `feasible / (feasible + infeasible)`: share of answering members it fits. `None` if none answered.
    pub agreement: Option<f64>,
    /// Share of all members that could answer: `(feasible + infeasible) / members_total`.
    pub coverage: f64,
    /// Whether the window meets the search's `min_agreement` and `min_coverage`.
    pub meets_requirement: bool,
    /// Mean preference score over the members where the window fits.
    pub suitability: Option<f64>,
    /// Rules that ruled the window out, most common first.
    pub blockers: Vec<Blocker>,
    /// Metrics that some members lacked, most common first.
    pub missing: Vec<MissingData>,
    /// One entry per member, in member order.
    pub outcomes: Vec<MemberOutcome>,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct EnsembleResult {
    pub members: Vec<String>,
    pub min_agreement: f64,
    pub min_coverage: f64,
    /// Every candidate window, best first: those meeting the requirement, then by agreement,
    /// coverage and preference score, then earliest start.
    pub windows: Vec<EnsembleWindow>,
}

/// Searches every member and combines the results per window.
pub struct EnsembleSearch<'a> {
    ensemble: &'a Ensemble,
    plan: &'a Plan,
    min_agreement: f64,
    min_coverage: f64,
}

impl<'a> EnsembleSearch<'a> {
    /// Defaults are conservative: a window must fit in *every* member that can answer, and at least
    /// half of all members must be able to answer.
    pub fn new(ensemble: &'a Ensemble, plan: &'a Plan) -> Self {
        Self {
            ensemble,
            plan,
            min_agreement: 1.0,
            min_coverage: 0.5,
        }
    }
    /// Share of answering members a window must fit to meet the requirement, in (0, 1].
    pub fn min_agreement(mut self, value: f64) -> Self {
        self.min_agreement = value;
        self
    }
    /// Share of all members that must be able to answer, in (0, 1].
    pub fn min_coverage(mut self, value: f64) -> Self {
        self.min_coverage = value;
        self
    }

    pub fn run(&self) -> Result<EnsembleResult, ValidationError> {
        for (name, value) in [
            ("min_agreement", self.min_agreement),
            ("min_coverage", self.min_coverage),
        ] {
            if !(value > 0.0 && value <= 1.0) {
                return Err(ValidationError::InvalidRequirement { name });
            }
        }
        let members = &self.ensemble.members;
        // start_ms -> (end_ms, outcomes in member order)
        let mut by_start: BTreeMap<i64, (i64, Vec<MemberOutcome>)> = BTreeMap::new();
        for member in members {
            let result = WindowSearch::new(&member.series, self.plan).run_tolerant()?;
            for w in result.feasible {
                by_start
                    .entry(w.start_ms)
                    .or_insert_with(|| (w.end_ms, Vec::new()))
                    .1
                    .push(MemberOutcome {
                        member: member.name.clone(),
                        verdict: Verdict::Feasible,
                        suitability: Some(w.suitability),
                        failure: None,
                    });
            }
            for w in result.rejected {
                // A reading-less failure that is not the clock check means "cannot say".
                let missing = w.failure.actual.is_none() && w.failure.note.is_none();
                by_start
                    .entry(w.start_ms)
                    .or_insert_with(|| (w.end_ms, Vec::new()))
                    .1
                    .push(MemberOutcome {
                        member: member.name.clone(),
                        verdict: if missing {
                            Verdict::Unknown
                        } else {
                            Verdict::Infeasible
                        },
                        suitability: None,
                        failure: Some(w.failure),
                    });
            }
        }
        let order: BTreeMap<&str, usize> = members
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.as_str(), i))
            .collect();
        let mut windows: Vec<EnsembleWindow> = by_start
            .into_iter()
            .map(|(start_ms, (end_ms, mut outcomes))| {
                outcomes.sort_by_key(|o| order[o.member.as_str()]);
                self.combine(start_ms, end_ms, outcomes)
            })
            .collect();
        windows.sort_by(|a, b| {
            b.meets_requirement
                .cmp(&a.meets_requirement)
                .then(cmp_desc_opt(a.agreement, b.agreement))
                .then(b.coverage.total_cmp(&a.coverage))
                .then(cmp_desc_opt(a.suitability, b.suitability))
                .then(a.start_ms.cmp(&b.start_ms))
        });
        Ok(EnsembleResult {
            members: members.iter().map(|m| m.name.clone()).collect(),
            min_agreement: self.min_agreement,
            min_coverage: self.min_coverage,
            windows,
        })
    }

    fn combine(&self, start_ms: i64, end_ms: i64, outcomes: Vec<MemberOutcome>) -> EnsembleWindow {
        let count = |v: Verdict| outcomes.iter().filter(|o| o.verdict == v).count();
        let (feasible, infeasible, unknown) = (
            count(Verdict::Feasible),
            count(Verdict::Infeasible),
            count(Verdict::Unknown),
        );
        let total = outcomes.len();
        let answering = feasible + infeasible;
        let agreement = (answering > 0).then(|| feasible as f64 / answering as f64);
        let coverage = answering as f64 / total as f64;
        let scores: Vec<f64> = outcomes.iter().filter_map(|o| o.suitability).collect();
        let suitability =
            (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64);
        // Tolerance so that e.g. 4 of 5 meets a requirement of 0.8 despite float rounding.
        let eps = 1e-9;
        let meets_requirement = agreement.is_some_and(|a| a + eps >= self.min_agreement)
            && coverage + eps >= self.min_coverage;
        let mut blockers: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut missing: BTreeMap<String, usize> = BTreeMap::new();
        for o in &outcomes {
            if let Some(f) = &o.failure {
                match o.verdict {
                    Verdict::Infeasible => {
                        *blockers
                            .entry((f.stage.clone(), f.constraint.clone()))
                            .or_default() += 1
                    }
                    Verdict::Unknown => *missing.entry(f.metric.clone()).or_default() += 1,
                    Verdict::Feasible => {}
                }
            }
        }
        let mut blockers: Vec<Blocker> = blockers
            .into_iter()
            .map(|((stage, constraint), members)| Blocker {
                stage,
                constraint,
                members,
            })
            .collect();
        blockers.sort_by(|a, b| b.members.cmp(&a.members));
        let mut missing: Vec<MissingData> = missing
            .into_iter()
            .map(|(metric, members)| MissingData { metric, members })
            .collect();
        missing.sort_by(|a, b| b.members.cmp(&a.members));
        EnsembleWindow {
            start_ms,
            end_ms,
            members_total: total,
            feasible,
            infeasible,
            unknown,
            agreement,
            coverage,
            meets_requirement,
            suitability,
            blockers,
            missing,
            outcomes,
        }
    }
}

/// Descending order for optional numbers, with `None` last.
fn cmp_desc_opt(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}
