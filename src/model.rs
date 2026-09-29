use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
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
pub struct Metric(pub String);
impl Metric {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
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
pub enum Preference {
    Minimize { ideal: f64 },
    Maximize { ideal: f64 },
    Range { min: f64, max: f64 },
}

#[derive(Clone, Debug, PartialEq)]
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
        weight: f64,
    },
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub name: String,
    pub duration_ms: i64,
    pub constraints: Vec<Constraint>,
}
impl Stage {
    pub fn new(name: impl Into<String>, duration_ms: i64, constraints: Vec<Constraint>) -> Self {
        Self {
            name: name.into(),
            duration_ms,
            constraints,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
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
    pub fn duration_ms(&self) -> i64 {
        self.stages.iter().map(|s| s.duration_ms).sum()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub cadence_ms: i64,
    pub observations: Vec<Observation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    NonPositiveCadence,
    EmptySeries,
    NonFiniteValue { timestamp_ms: i64, metric: String },
    IrregularTimestamp { expected: i64, actual: i64 },
}
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ValidationError {}
impl Series {
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
        })
    }
}
