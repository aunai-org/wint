//! Deterministic, in-memory search for the time windows when a job can run within declared limits.

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
    Comparison, Constraint, Gap, Metric, Observation, Plan, Preference, Series, Stage,
    ValidationError, MAX_ARRANGEMENTS,
};
pub use schedule::{parse_days, Schedule, ScheduleError, Weekday};
