//! Deterministic, in-memory search for the time windows when a job can run within declared limits.
//!
//! Give it a [`Series`] of readings and a [`Plan`] of limits; [`WindowSearch`] returns every
//! window where the plan fits, best first, and every window that does not, with the evidence.
//!
//! ```
//! use wint::{Comparison, Constraint, Metric, Observation, Plan, Series, WindowSearch};
//!
//! const HOUR: i64 = 3_600_000;
//! let series = Series::new(HOUR, vec![
//!     Observation::at(0).with("wind", 12.0),
//!     Observation::at(HOUR).with("wind", 25.0),
//!     Observation::at(2 * HOUR).with("wind", 8.0),
//! ]).unwrap();
//! let plan = Plan::single_stage("flight", HOUR, vec![
//!     Constraint::hard("safe wind", Metric::new("wind"), Comparison::LessThanOrEqual, 20.0),
//! ]);
//!
//! let result = WindowSearch::new(&series, &plan).run().unwrap();
//! assert_eq!(result.feasible.len(), 2); // the hours starting at 0h and 2h
//! assert_eq!(result.rejected.len(), 1); // 1h is too windy
//! ```

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
