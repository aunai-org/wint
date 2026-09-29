//! Deterministic, in-memory environmental operability window search.

mod engine;
mod model;

pub use engine::{Evidence, RejectedWindow, SearchResult, StageResult, WindowResult, WindowSearch};
pub use model::{
    Comparison, Constraint, Metric, Observation, Plan, Preference, Series, Stage, ValidationError,
};
