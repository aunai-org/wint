//! Deterministic, in-memory environmental operability window search.

pub mod adapters;
mod engine;
mod ensemble;
mod model;
pub mod present;
pub mod presets;
mod schedule;
pub mod time;
pub mod units;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use engine::{
    ClockSpan, Evidence, Expectation, RejectedWindow, SearchResult, StageResult, WindowResult,
    WindowSearch, SCHEMA_VERSION,
};
pub use ensemble::{
    Blocker, Ensemble, EnsembleResult, EnsembleSearch, EnsembleWindow, Member, MemberOutcome,
    MissingData, Verdict,
};
pub use model::{
    Comparison, Constraint, Metric, Observation, Plan, Preference, Series, Stage, ValidationError,
};
pub use schedule::{parse_days, Schedule, ScheduleError, Weekday};
