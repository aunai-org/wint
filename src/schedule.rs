//! Local time-of-day windows ("daytime only", "09:00-12:00", "until 20:00",
//! "night 20:00-06:00").
//!
//! A [`Schedule`] is a daily clock window. The clock it refers to is the local
//! time of the data: a series carries a UTC offset (see
//! [`Series::utc_offset_minutes`](crate::Series)), and a stage's whole span must
//! lie inside the window. A window whose `from` is later than its `to` wraps
//! midnight (`20:00-06:00`).
//!
//! Only a fixed offset is modelled, so a forecast that crosses a daylight-saving
//! change shifts by an hour after it. Sunrise/sunset windows are not clock
//! windows: use the `is_day` metric from the data for those.

use std::fmt;

pub(crate) const DAY_MINUTES: u16 = 1440;
const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = DAY_MINUTES as i64 * MINUTE_MS;

/// Why a [`Schedule`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// Not a `HH:MM` clock time (`24:00` is allowed only as an end time).
    BadClock(String),
    /// `from` must be before 24:00 and `to` after 00:00 and at most 24:00.
    OutOfRange,
    /// `from` and `to` are equal, which would describe an empty window.
    Empty,
}
impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadClock(text) => write!(f, "`{text}` is not a HH:MM clock time"),
            Self::OutOfRange => write!(f, "from must be before 24:00 and to within 00:01-24:00"),
            Self::Empty => write!(f, "from and to must differ"),
        }
    }
}
impl std::error::Error for ScheduleError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "json",
    serde(try_from = "ScheduleWire", into = "ScheduleWire")
)]
pub struct Schedule {
    from_minute: u16,
    to_minute: u16,
}

/// Wire form: `{"from": "09:00", "to": "12:00"}`.
#[cfg(feature = "json")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ScheduleWire {
    from: String,
    to: String,
}
#[cfg(feature = "json")]
impl TryFrom<ScheduleWire> for Schedule {
    type Error = ScheduleError;
    fn try_from(wire: ScheduleWire) -> Result<Self, Self::Error> {
        Schedule::parse(&wire.from, &wire.to)
    }
}
#[cfg(feature = "json")]
impl From<Schedule> for ScheduleWire {
    fn from(schedule: Schedule) -> Self {
        Self {
            from: format_clock(schedule.from_minute),
            to: format_clock(schedule.to_minute),
        }
    }
}

impl Schedule {
    /// Builds a window from minutes after local midnight; `to_minute` may be 1440 (24:00).
    pub fn new(from_minute: u16, to_minute: u16) -> Result<Self, ScheduleError> {
        if from_minute >= DAY_MINUTES || to_minute == 0 || to_minute > DAY_MINUTES {
            return Err(ScheduleError::OutOfRange);
        }
        if from_minute == to_minute {
            return Err(ScheduleError::Empty);
        }
        Ok(Self {
            from_minute,
            to_minute,
        })
    }
    /// Parses two `HH:MM` strings, e.g. `Schedule::parse("09:00", "12:00")`.
    pub fn parse(from: &str, to: &str) -> Result<Self, ScheduleError> {
        let clock =
            |text: &str| parse_clock(text).ok_or_else(|| ScheduleError::BadClock(text.to_string()));
        Self::new(clock(from)?, clock(to)?)
    }
    pub fn from_minute(&self) -> u16 {
        self.from_minute
    }
    pub fn to_minute(&self) -> u16 {
        self.to_minute
    }
    /// True for `00:00-24:00`, which every span satisfies.
    pub fn is_always(&self) -> bool {
        self.from_minute == 0 && self.to_minute == DAY_MINUTES
    }
    /// Whether the span `[start, start + len)` lies inside the window.
    /// `local_start_ms` is a Unix-millisecond instant already shifted by the
    /// series' UTC offset, so its time of day is the local clock.
    pub fn contains_span(&self, local_start_ms: i64, len_ms: i64) -> bool {
        if self.is_always() {
            return true;
        }
        let a = local_start_ms.rem_euclid(DAY_MS);
        let (from, to) = (
            i64::from(self.from_minute) * MINUTE_MS,
            i64::from(self.to_minute) * MINUTE_MS,
        );
        if self.from_minute < self.to_minute {
            a >= from && a + len_ms <= to
        } else {
            (a >= from && a + len_ms <= DAY_MS + to) || (a < to && a + len_ms <= to)
        }
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}",
            format_clock(self.from_minute),
            format_clock(self.to_minute)
        )
    }
}

/// Parses `H:MM` or `HH:MM`; `24:00` is accepted (it is rejected later if used as a start).
pub(crate) fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    if m.len() != 2 || h.is_empty() || h.len() > 2 {
        return None;
    }
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    let total = h * 60 + m;
    (m < 60 && total <= DAY_MINUTES).then_some(total)
}

/// `HH:MM` for minutes after midnight (1440 becomes `24:00`): the plan's JSON form and `Display`.
pub(crate) fn format_clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    const H: i64 = 3_600_000;

    #[test]
    fn builds_parses_and_rejects() {
        let s = Schedule::parse("09:00", "12:00").unwrap();
        assert_eq!((s.from_minute(), s.to_minute()), (540, 720));
        assert_eq!(s.to_string(), "09:00-12:00");
        assert_eq!(Schedule::parse("9:00", "24:00").unwrap().to_minute(), 1440);
        assert_eq!(Schedule::parse("09:00", "09:00"), Err(ScheduleError::Empty));
        assert_eq!(
            Schedule::parse("24:00", "06:00"),
            Err(ScheduleError::OutOfRange)
        );
        assert_eq!(
            Schedule::parse("09:00", "00:00"),
            Err(ScheduleError::OutOfRange)
        );
        for bad in ["9", "09:5", "09:60", "25:00", "ab:cd", ""] {
            assert!(
                matches!(
                    Schedule::parse(bad, "12:00"),
                    Err(ScheduleError::BadClock(_))
                ),
                "{bad}"
            );
        }
    }
    #[test]
    fn daytime_window_needs_the_whole_span_inside() {
        let day = Schedule::parse("09:00", "12:00").unwrap();
        assert!(day.contains_span(9 * H, H)); // 09-10
        assert!(day.contains_span(11 * H, H)); // 11-12 ends exactly at the boundary
        assert!(!day.contains_span(8 * H, H)); // before
        assert!(!day.contains_span(12 * H, H)); // starts at the end (half-open)
        assert!(!day.contains_span(11 * H + H / 2, H)); // straddles 12:00
        assert!(day.contains_span(3 * 86_400_000 + 10 * H, H)); // any day
        assert!(day.contains_span(-86_400_000 + 10 * H, H)); // before 1970
    }
    #[test]
    fn until_window_starts_at_midnight() {
        let until = Schedule::parse("00:00", "20:00").unwrap();
        assert!(until.contains_span(0, H));
        assert!(until.contains_span(19 * H, H));
        assert!(!until.contains_span(20 * H, H));
    }
    #[test]
    fn overnight_window_wraps_midnight() {
        let night = Schedule::parse("20:00", "06:00").unwrap();
        for ok in [20, 22, 23, 0, 3, 5] {
            assert!(night.contains_span(ok * H, H), "{ok}");
        }
        for bad in [6, 12, 19] {
            assert!(!night.contains_span(bad * H, H), "{bad}");
        }
        assert!(night.contains_span(23 * H + H / 2, H)); // 23:30-00:30 straddles midnight, allowed
        assert!(!night.contains_span(5 * H + H / 2, H)); // 05:30-06:30 leaves the window
    }
    #[test]
    fn full_day_is_always_true_even_across_midnight() {
        let all = Schedule::parse("00:00", "24:00").unwrap();
        assert!(all.is_always() && all.contains_span(23 * H + H / 2, H));
    }
    #[test]
    fn clock_text_is_the_wire_form() {
        assert_eq!(format_clock(1440), "24:00");
        assert_eq!(format_clock(545), "09:05");
    }
}
