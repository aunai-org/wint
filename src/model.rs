use crate::schedule::Schedule;
use crate::units::{canonical_unit, Unit, UnitError};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Observation {
    pub timestamp_ms: i64,
    pub values: BTreeMap<String, f64>,
}
impl Observation {
    pub fn at(timestamp_ms: i64) -> Self {
        Self {
            timestamp_ms,
            values: BTreeMap::new(),
        }
    }
    pub fn with(mut self, metric: impl Into<String>, value: f64) -> Self {
        self.values.insert(metric.into(), value);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Metric(pub String);
impl Metric {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Comparison {
    #[cfg_attr(feature = "json", serde(rename = "<"))]
    LessThan,
    #[cfg_attr(feature = "json", serde(rename = "<="))]
    LessThanOrEqual,
    #[cfg_attr(feature = "json", serde(rename = ">"))]
    GreaterThan,
    #[cfg_attr(feature = "json", serde(rename = ">="))]
    GreaterThanOrEqual,
    #[cfg_attr(feature = "json", serde(rename = "=="))]
    Equal,
}
impl Comparison {
    pub(crate) fn matches(self, actual: f64, threshold: f64) -> bool {
        match self {
            Self::LessThan => actual < threshold,
            Self::LessThanOrEqual => actual <= threshold,
            Self::GreaterThan => actual > threshold,
            Self::GreaterThanOrEqual => actual >= threshold,
            Self::Equal => actual == threshold,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(tag = "kind", rename_all = "snake_case"))]
#[non_exhaustive]
pub enum Preference {
    /// Lower is better; values at or below `ideal` are perfect. A value
    /// `scale` or more above `ideal` receives the maximum penalty.
    Minimize { ideal: f64, scale: f64 },
    /// Higher is better; values at or above `ideal` are perfect. A value
    /// `scale` or more below `ideal` receives the maximum penalty.
    Maximize { ideal: f64, scale: f64 },
    /// Values within `[min, max]` are perfect. A value `scale` or more
    /// outside the range receives the maximum penalty.
    Range { min: f64, max: f64, scale: f64 },
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(tag = "type", rename_all = "snake_case"))]
#[cfg_attr(feature = "json", serde(try_from = "ConstraintData"))]
#[non_exhaustive]
pub enum Constraint {
    Hard {
        name: String,
        metric: Metric,
        comparison: Comparison,
        threshold: f64,
    },
    Soft {
        name: String,
        metric: Metric,
        preference: Preference,
        #[cfg_attr(feature = "json", serde(default = "default_weight"))]
        weight: f64,
    },
}
#[cfg(feature = "json")]
fn default_weight() -> f64 {
    1.0
}
/// Wire form of [`Constraint`]: identical, plus an optional `unit` in which the
/// limits are written. Limits are converted to the metric's canonical unit.
#[cfg(feature = "json")]
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConstraintData {
    Hard {
        name: String,
        metric: Metric,
        comparison: Comparison,
        threshold: f64,
        #[serde(default)]
        unit: Option<String>,
    },
    Soft {
        name: String,
        metric: Metric,
        preference: Preference,
        #[serde(default = "default_weight")]
        weight: f64,
        #[serde(default)]
        unit: Option<String>,
    },
}
#[cfg(feature = "json")]
impl TryFrom<ConstraintData> for Constraint {
    type Error = ValidationError;
    fn try_from(data: ConstraintData) -> Result<Self, Self::Error> {
        let (constraint, unit) = match data {
            ConstraintData::Hard {
                name,
                metric,
                comparison,
                threshold,
                unit,
            } => (Constraint::hard(name, metric, comparison, threshold), unit),
            ConstraintData::Soft {
                name,
                metric,
                preference,
                weight,
                unit,
            } => (Constraint::soft(name, metric, preference, weight), unit),
        };
        match unit {
            None => Ok(constraint),
            Some(symbol) => {
                let unit =
                    Unit::parse(&symbol).ok_or_else(|| ValidationError::InvalidConstraint {
                        constraint: constraint.name().to_string(),
                        reason: "unknown unit",
                    })?;
                constraint.with_unit(unit)
            }
        }
    }
}
impl Constraint {
    pub fn hard(
        name: impl Into<String>,
        metric: Metric,
        comparison: Comparison,
        threshold: f64,
    ) -> Self {
        Self::Hard {
            name: name.into(),
            metric,
            comparison,
            threshold,
        }
    }
    pub fn soft(
        name: impl Into<String>,
        metric: Metric,
        preference: Preference,
        weight: f64,
    ) -> Self {
        Self::Soft {
            name: name.into(),
            metric,
            preference,
            weight,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Hard { name, .. } | Self::Soft { name, .. } => name,
        }
    }
    /// Reinterprets this constraint's limits as written in `unit` and converts
    /// them to the metric's canonical unit (see [`crate::units::VOCABULARY`]).
    /// Thresholds and preference bounds convert as absolute values; a
    /// preference `scale` converts as a difference. Fails if the metric is not
    /// in the vocabulary or `unit` measures a different kind of quantity.
    pub fn with_unit(self, unit: Unit) -> Result<Self, ValidationError> {
        let name = self.name().to_string();
        let metric = match &self {
            Self::Hard { metric, .. } | Self::Soft { metric, .. } => metric.0.clone(),
        };
        let canonical =
            canonical_unit(&metric).ok_or_else(|| ValidationError::InvalidConstraint {
                constraint: name.clone(),
                reason: "unit given for a metric outside the vocabulary",
            })?;
        let units_err = |error: UnitError| ValidationError::Units {
            metric: metric.clone(),
            error,
        };
        let abs = |v: f64| unit.convert(v, canonical).map_err(units_err);
        let delta = |v: f64| unit.convert_delta(v, canonical).map_err(units_err);
        Ok(match self {
            Self::Hard {
                name,
                metric,
                comparison,
                threshold,
            } => Self::Hard {
                name,
                metric,
                comparison,
                threshold: abs(threshold)?,
            },
            Self::Soft {
                name,
                metric,
                preference,
                weight,
            } => Self::Soft {
                name,
                metric,
                weight,
                preference: match preference {
                    Preference::Minimize { ideal, scale } => Preference::Minimize {
                        ideal: abs(ideal)?,
                        scale: delta(scale)?,
                    },
                    Preference::Maximize { ideal, scale } => Preference::Maximize {
                        ideal: abs(ideal)?,
                        scale: delta(scale)?,
                    },
                    Preference::Range { min, max, scale } => Preference::Range {
                        min: abs(min)?,
                        max: abs(max)?,
                        scale: delta(scale)?,
                    },
                },
            },
        })
    }
    fn validate(&self) -> Result<(), ValidationError> {
        let invalid = |name: &str, reason: &'static str| ValidationError::InvalidConstraint {
            constraint: name.to_string(),
            reason,
        };
        match self {
            Self::Hard {
                name, threshold, ..
            } => {
                if !threshold.is_finite() {
                    return Err(invalid(name, "threshold must be finite"));
                }
            }
            Self::Soft {
                name,
                preference,
                weight,
                ..
            } => {
                if !weight.is_finite() || *weight < 0.0 {
                    return Err(invalid(name, "weight must be finite and non-negative"));
                }
                let (finite, scale, ordered) = match *preference {
                    Preference::Minimize { ideal, scale }
                    | Preference::Maximize { ideal, scale } => (ideal.is_finite(), scale, true),
                    Preference::Range { min, max, scale } => {
                        (min.is_finite() && max.is_finite(), scale, min <= max)
                    }
                };
                if !finite {
                    return Err(invalid(name, "preference bounds must be finite"));
                }
                if !ordered {
                    return Err(invalid(name, "range min must not exceed max"));
                }
                if !scale.is_finite() || scale <= 0.0 {
                    return Err(invalid(name, "scale must be finite and positive"));
                }
            }
        }
        Ok(())
    }
}

/// How long after the previous stage ends a stage may start: any whole number of samples from
/// `min_ms` to `max_ms` inclusive. Nothing is checked during the gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(try_from = "GapWire"))]
pub struct Gap {
    pub min_ms: i64,
    pub max_ms: i64,
}
/// Wire form: `{"min_ms": 14400000, "max_ms": 43200000}`; `max_ms` defaults to `min_ms` (an exact
/// delay) and `min_ms` to 0.
#[cfg(feature = "json")]
#[derive(serde::Deserialize)]
struct GapWire {
    #[serde(default)]
    min_ms: i64,
    max_ms: Option<i64>,
}
#[cfg(feature = "json")]
impl TryFrom<GapWire> for Gap {
    type Error = String;
    fn try_from(wire: GapWire) -> Result<Self, String> {
        let max_ms = wire.max_ms.unwrap_or(wire.min_ms);
        Gap::new(wire.min_ms, max_ms).ok_or_else(|| "gap needs 0 <= min_ms <= max_ms".to_string())
    }
}
impl Gap {
    /// `None` unless `0 <= min_ms <= max_ms`.
    pub fn new(min_ms: i64, max_ms: i64) -> Option<Self> {
        (0 <= min_ms && min_ms <= max_ms).then_some(Self { min_ms, max_ms })
    }
    /// Exactly `ms` after the previous stage.
    pub fn exactly(ms: i64) -> Option<Self> {
        Self::new(ms, ms)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Stage {
    pub name: String,
    pub duration_ms: i64,
    pub constraints: Vec<Constraint>,
    /// Delay after the previous stage (not allowed on the first stage). Absent means the stage
    /// starts right when the previous one ends.
    #[cfg_attr(
        feature = "json",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub gap: Option<Gap>,
    /// Optional local time-of-day window the whole stage must fit inside.
    #[cfg_attr(
        feature = "json",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub schedule: Option<Schedule>,
}
impl Stage {
    pub fn new(name: impl Into<String>, duration_ms: i64, constraints: Vec<Constraint>) -> Self {
        Self {
            name: name.into(),
            duration_ms,
            constraints,
            gap: None,
            schedule: None,
        }
    }
    /// Lets the stage start between `min_ms` and `max_ms` after the previous one ends.
    /// Panics unless `0 <= min_ms <= max_ms`; build a [`Gap`] yourself to handle that.
    pub fn with_gap(mut self, min_ms: i64, max_ms: i64) -> Self {
        self.gap = Some(Gap::new(min_ms, max_ms).expect("gap needs 0 <= min_ms <= max_ms"));
        self
    }
    /// Restricts the stage to a local time-of-day window.
    pub fn with_schedule(mut self, schedule: Schedule) -> Self {
        self.schedule = Some(schedule);
        self
    }
}

/// Upper bound on the ways the gap ranges of one plan can lay out a window (their product).
pub const MAX_ARRANGEMENTS: u64 = 10_000;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Plan {
    pub name: String,
    pub stages: Vec<Stage>,
}
impl Plan {
    pub fn new(name: impl Into<String>, stages: Vec<Stage>) -> Self {
        Self {
            name: name.into(),
            stages,
        }
    }
    pub fn single_stage(
        name: impl Into<String>,
        duration_ms: i64,
        constraints: Vec<Constraint>,
    ) -> Self {
        Self::new(
            name,
            vec![Stage::new("operation", duration_ms, constraints)],
        )
    }
    /// Total time the stages take, gaps not counted.
    pub fn duration_ms(&self) -> i64 {
        self.stages.iter().map(|s| s.duration_ms).sum()
    }
    /// The shortest span a window can cover: the stages plus every gap at its minimum.
    pub fn min_span_ms(&self) -> i64 {
        self.duration_ms()
            + self
                .stages
                .iter()
                .filter_map(|s| s.gap)
                .map(|g| g.min_ms)
                .sum::<i64>()
    }
    /// Restricts every stage to the same local time-of-day window.
    pub fn with_schedule(mut self, schedule: Schedule) -> Self {
        for stage in &mut self.stages {
            stage.schedule = Some(schedule);
        }
        self
    }

    /// Checks the plan against a series cadence: at least one stage, positive
    /// cadence-aligned durations, and well-formed constraints.
    pub fn validate(&self, cadence_ms: i64) -> Result<(), ValidationError> {
        if self.stages.is_empty() {
            return Err(ValidationError::EmptyPlan);
        }
        let mut total: i64 = 0;
        let mut arrangements: u64 = 1;
        for (index, stage) in self.stages.iter().enumerate() {
            if let Some(gap) = stage.gap {
                let aligned = gap.min_ms % cadence_ms == 0 && gap.max_ms % cadence_ms == 0;
                if index == 0 || !aligned {
                    return Err(ValidationError::InvalidGap {
                        stage: stage.name.clone(),
                    });
                }
                total = total
                    .checked_add(gap.max_ms)
                    .ok_or(ValidationError::PlanTooLong)?;
                arrangements = arrangements
                    .saturating_mul(((gap.max_ms - gap.min_ms) / cadence_ms) as u64 + 1);
                if arrangements > MAX_ARRANGEMENTS {
                    return Err(ValidationError::TooManyGapOptions);
                }
            }
            if stage.duration_ms <= 0 || stage.duration_ms % cadence_ms != 0 {
                return Err(ValidationError::InvalidStageDuration {
                    stage: stage.name.clone(),
                });
            }
            total = total
                .checked_add(stage.duration_ms)
                .ok_or(ValidationError::PlanTooLong)?;
            for constraint in &stage.constraints {
                constraint.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(try_from = "SeriesData"))]
pub struct Series {
    pub cadence_ms: i64,
    pub observations: Vec<Observation>,
    /// Offset of the data's local clock from UTC, in minutes (for example
    /// `120` for UTC+02:00). Used only by stage [`Schedule`]s; defaults to 0.
    pub utc_offset_minutes: i32,
}
/// Unvalidated wire form of [`Series`]; deserializing goes through
/// [`Series::new`] so JSON input cannot bypass validation.
#[cfg(feature = "json")]
#[derive(serde::Deserialize)]
struct SeriesData {
    cadence_ms: i64,
    observations: Vec<Observation>,
    /// Optional source units per metric, converted to canonical on load.
    #[serde(default)]
    units: BTreeMap<String, String>,
    /// Optional offset of the local clock from UTC, in minutes.
    #[serde(default)]
    utc_offset_minutes: i32,
}
#[cfg(feature = "json")]
impl TryFrom<SeriesData> for Series {
    type Error = ValidationError;
    fn try_from(data: SeriesData) -> Result<Self, Self::Error> {
        let mut units = BTreeMap::new();
        for (metric, symbol) in data.units {
            let unit = Unit::parse(&symbol).ok_or_else(|| ValidationError::UnknownUnit {
                metric: metric.clone(),
                symbol,
            })?;
            units.insert(metric, unit);
        }
        Series::with_units(data.cadence_ms, data.observations, &units)?
            .with_utc_offset(data.utc_offset_minutes)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationError {
    NonPositiveCadence,
    EmptySeries,
    NonFiniteValue {
        timestamp_ms: i64,
        metric: String,
    },
    IrregularTimestamp {
        expected: i64,
        actual: i64,
    },
    EmptyPlan,
    InvalidStageDuration {
        stage: String,
    },
    PlanTooLong,
    /// A stage's gap is on the first stage, or not a whole number of samples.
    InvalidGap {
        stage: String,
    },
    /// The gap ranges allow more than [`MAX_ARRANGEMENTS`] ways to lay out one window.
    TooManyGapOptions,
    InvalidConstraint {
        constraint: String,
        reason: &'static str,
    },
    /// A unit conversion failed for this metric.
    Units {
        metric: String,
        error: UnitError,
    },
    /// A unit was declared for a metric outside the vocabulary, so there is
    /// no canonical unit to convert to.
    UnknownMetricUnit {
        metric: String,
    },
    /// A declared unit symbol is not recognised.
    UnknownUnit {
        metric: String,
        symbol: String,
    },
    /// A UTC offset outside -12:00..=+14:00.
    InvalidUtcOffset {
        minutes: i32,
    },
    /// An ensemble with no members.
    EmptyEnsemble,
    /// Two ensemble members share a name.
    DuplicateMember {
        name: String,
    },
    /// An ensemble member's series does not line up with the first member's.
    EnsembleMismatch {
        member: String,
        reason: &'static str,
    },
    /// A search requirement (`min_agreement`, `min_coverage`) outside (0, 1].
    InvalidRequirement {
        name: &'static str,
    },
}
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveCadence => write!(f, "cadence must be positive"),
            Self::EmptySeries => write!(f, "series has no observations"),
            Self::NonFiniteValue {
                timestamp_ms,
                metric,
            } => write!(f, "metric `{metric}` at {timestamp_ms} is not finite"),
            Self::IrregularTimestamp { expected, actual } => {
                write!(f, "expected timestamp {expected}, found {actual}")
            }
            Self::EmptyPlan => write!(f, "plan has no stages"),
            Self::InvalidStageDuration { stage } => write!(
                f,
                "stage `{stage}` duration must be a positive multiple of the cadence"
            ),
            Self::PlanTooLong => write!(f, "total plan duration overflows"),
            Self::InvalidGap { stage } => write!(
                f,
                "stage `{stage}`: a gap needs a previous stage and must be a whole number of samples"
            ),
            Self::TooManyGapOptions => write!(
                f,
                "the gap ranges allow more than {MAX_ARRANGEMENTS} ways to place the stages"
            ),
            Self::InvalidConstraint { constraint, reason } => {
                write!(f, "constraint `{constraint}`: {reason}")
            }
            Self::Units { metric, error } => write!(f, "metric `{metric}`: {error}"),
            Self::UnknownMetricUnit { metric } => write!(
                f,
                "metric `{metric}` is not in the vocabulary, so a unit cannot be converted"
            ),
            Self::UnknownUnit { metric, symbol } => {
                write!(f, "metric `{metric}`: unknown unit `{symbol}`")
            }
            Self::EmptyEnsemble => write!(f, "ensemble has no members"),
            Self::DuplicateMember { name } => write!(f, "ensemble member `{name}` appears twice"),
            Self::EnsembleMismatch { member, reason } => {
                write!(f, "ensemble member `{member}` {reason}")
            }
            Self::InvalidRequirement { name } => {
                write!(f, "{name} must be greater than 0 and at most 1")
            }
            Self::InvalidUtcOffset { minutes } => write!(
                f,
                "UTC offset of {minutes} minutes is outside -12:00 to +14:00"
            ),
        }
    }
}
impl std::error::Error for ValidationError {}
impl Series {
    /// Builds a series from readings given in `units` (metric name to unit)
    /// and converts them to each metric's canonical unit. Metrics without an
    /// entry are assumed to already be canonical or caller-defined.
    pub fn with_units(
        cadence_ms: i64,
        mut observations: Vec<Observation>,
        units: &BTreeMap<String, Unit>,
    ) -> Result<Self, ValidationError> {
        for (metric, from) in units {
            let to = canonical_unit(metric).ok_or_else(|| ValidationError::UnknownMetricUnit {
                metric: metric.clone(),
            })?;
            if from.dimension() != to.dimension() {
                return Err(ValidationError::Units {
                    metric: metric.clone(),
                    error: UnitError::DimensionMismatch { from: *from, to },
                });
            }
            for observation in &mut observations {
                if let Some(value) = observation.values.get_mut(metric) {
                    *value = from
                        .convert(*value, to)
                        .map_err(|error| ValidationError::Units {
                            metric: metric.clone(),
                            error,
                        })?;
                }
            }
        }
        Self::new(cadence_ms, observations)
    }
    pub fn new(cadence_ms: i64, observations: Vec<Observation>) -> Result<Self, ValidationError> {
        if cadence_ms <= 0 {
            return Err(ValidationError::NonPositiveCadence);
        }
        if observations.is_empty() {
            return Err(ValidationError::EmptySeries);
        }
        for (index, observation) in observations.iter().enumerate() {
            if index > 0 {
                let expected = observations[index - 1].timestamp_ms + cadence_ms;
                if observation.timestamp_ms != expected {
                    return Err(ValidationError::IrregularTimestamp {
                        expected,
                        actual: observation.timestamp_ms,
                    });
                }
            }
            for (metric, value) in &observation.values {
                if !value.is_finite() {
                    return Err(ValidationError::NonFiniteValue {
                        timestamp_ms: observation.timestamp_ms,
                        metric: metric.clone(),
                    });
                }
            }
        }
        Ok(Self {
            cadence_ms,
            observations,
            utc_offset_minutes: 0,
        })
    }
    /// Sets the local clock's offset from UTC in minutes (-12:00 to +14:00).
    pub fn with_utc_offset(mut self, minutes: i32) -> Result<Self, ValidationError> {
        if !(-720..=840).contains(&minutes) {
            return Err(ValidationError::InvalidUtcOffset { minutes });
        }
        self.utc_offset_minutes = minutes;
        Ok(self)
    }
}
