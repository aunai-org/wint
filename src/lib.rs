//! Deterministic, in-memory environmental operability window search.

pub mod adapters;
mod engine;
mod model;
pub mod presets;
mod schedule;
pub mod time;
pub mod units;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use engine::{Evidence, RejectedWindow, SearchResult, StageResult, WindowResult, WindowSearch};
pub use model::{
    Comparison, Constraint, Metric, Observation, Plan, Preference, Series, Stage, ValidationError,
};
pub use schedule::{format_clock, format_offset, Schedule, ScheduleError};
