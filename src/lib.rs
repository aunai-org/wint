//! Deterministic, in-memory environmental operability window search.

pub mod adapters;
mod engine;
mod model;
pub mod presets;
pub mod time;
pub mod units;

pub use engine::{Evidence, RejectedWindow, SearchResult, StageResult, WindowResult, WindowSearch};
pub use model::{
    Comparison, Constraint, Metric, Observation, Plan, Preference, Series, Stage, ValidationError,
};
