//! Input adapters that turn external data into a validated [`Series`](crate::Series).
//!
//! Adapters convert to the canonical units of the metric vocabulary
//! ([`crate::units`]) so a plan never has to know where the data came from.

pub mod csv;
#[cfg(feature = "json")]
pub mod open_meteo;

use crate::ValidationError;
use std::fmt;

/// Why an adapter could not produce a series.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdapterError {
    /// The input is malformed. `location` says where (a line number, a field name).
    Format { location: String, message: String },
    /// The input parsed, but the resulting series is not valid.
    Invalid(ValidationError),
}

impl AdapterError {
    pub(crate) fn format(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Format {
            location: location.into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format { location, message } => write!(f, "{location}: {message}"),
            Self::Invalid(error) => write!(f, "invalid series: {error}"),
        }
    }
}
impl std::error::Error for AdapterError {}
impl From<ValidationError> for AdapterError {
    fn from(error: ValidationError) -> Self {
        Self::Invalid(error)
    }
}
