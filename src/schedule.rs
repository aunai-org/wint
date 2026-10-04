//! Local time-of-day windows ("daytime only", "09:00-12:00", "until 20:00",
//! "night 20:00-06:00").
//!
//! A [`Schedule`] is a daily clock window. The clock it refers to is the local
//! time of the data: a series carries a UTC offset (see
//! [`Series::utc_offset_minutes`](crate::Series)), and a stage's whole span must
//! lie inside the window. A window whose `from` is later than its `to` wraps
//! midnight (`20:00-06:00`).
//!
//! A window can also be limited to some weekdays ("Saturday and Sunday", "Monday to Friday").
//! The weekday is the local day the window *starts* on, so a Friday `20:00-06:00` night runs into
//! Saturday morning and still counts as Friday's.
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
#[non_exhaustive]
pub enum ScheduleError {
    /// Not a `HH:MM` clock time (`24:00` is allowed only as an end time).
    BadClock(String),
    /// `from` must be before 24:00 and `to` after 00:00 and at most 24:00.
    OutOfRange,
    /// `from` and `to` are equal, which would describe an empty window.
    Empty,
    /// Not a list of weekdays (`mon-fri`, `sat,sun`, `weekend` ...), or an empty one.
    BadDays(String),
}
impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadClock(text) => write!(f, "`{text}` is not a HH:MM clock time"),
            Self::OutOfRange => write!(f, "from must be before 24:00 and to within 00:01-24:00"),
            Self::Empty => write!(f, "from and to must differ"),
            Self::BadDays(text) => write!(
                f,
                "`{text}` is not a weekday list (e.g. mon-fri, sat,sun, weekend)"
            ),
        }
    }
}
impl std::error::Error for ScheduleError {}

/// A day of the week, Monday first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}
impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    /// Lower-case three-letter name (`mon` ... `sun`), the wire and CLI form.
    pub fn name(self) -> &'static str {
        ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][self.index()]
    }
    /// Parses `mon`, `monday`, ... in any case.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|d| t == d.name() || (t.len() > 3 && FULL[d.index()] == t))
    }
    fn index(self) -> usize {
        self as usize
    }
    fn bit(self) -> u8 {
        1 << self.index()
    }
    fn from_index(i: i64) -> Self {
        Self::ALL[i.rem_euclid(7) as usize]
    }
}
const FULL: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const ALL_DAYS: u8 = 0b111_1111;

/// Parses a day list: `mon-fri`, `sat,sun`, `weekdays`, `weekend`, `daily`, `fri-mon` (wraps).
pub fn parse_days(text: &str) -> Result<Vec<Weekday>, ScheduleError> {
    let bad = || ScheduleError::BadDays(text.to_string());
    let mut mask = 0u8;
    for part in text.split(',') {
        let part = part.trim().to_ascii_lowercase();
        match part.as_str() {
            "weekdays" => mask |= 0b001_1111,
            "weekend" | "weekends" => mask |= 0b110_0000,
            "daily" | "all" => mask |= ALL_DAYS,
            _ => match part.split_once('-') {
                Some((a, b)) => {
                    let (a, b) = (
                        Weekday::parse(a).ok_or_else(bad)?,
                        Weekday::parse(b).ok_or_else(bad)?,
                    );
                    let mut i = a.index();
                    loop {
                        mask |= 1 << i;
                        if i == b.index() {
                            break;
                        }
                        i = (i + 1) % 7;
                    }
                }
                None => mask |= Weekday::parse(&part).ok_or_else(bad)?.bit(),
            },
        }
    }
    if mask == 0 {
        return Err(bad());
    }
    Ok(Weekday::ALL
        .into_iter()
        .filter(|d| mask & d.bit() != 0)
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "json",
    serde(try_from = "ScheduleWire", into = "ScheduleWire")
)]
pub struct Schedule {
    from_minute: u16,
    to_minute: u16,
    days: u8,
}

/// Wire form: `{"from": "09:00", "to": "12:00", "days": ["sat", "sun"]}`; `days` is optional
/// (every day) and `from`/`to` default to the whole day.
#[cfg(feature = "json")]
#[derive(serde::Serialize, serde::Deserialize)]
struct ScheduleWire {
    #[serde(default = "start_of_day")]
    from: String,
    #[serde(default = "end_of_day")]
    to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    days: Option<Vec<Weekday>>,
}
#[cfg(feature = "json")]
fn start_of_day() -> String {
    "00:00".into()
}
#[cfg(feature = "json")]
fn end_of_day() -> String {
    "24:00".into()
}
#[cfg(feature = "json")]
impl TryFrom<ScheduleWire> for Schedule {
    type Error = ScheduleError;
    fn try_from(wire: ScheduleWire) -> Result<Self, Self::Error> {
        let schedule = Schedule::parse(&wire.from, &wire.to)?;
        match wire.days {
            Some(days) => schedule.with_days(days),
            None => Ok(schedule),
        }
    }
}
#[cfg(feature = "json")]
impl From<Schedule> for ScheduleWire {
    fn from(schedule: Schedule) -> Self {
        Self {
            from: format_clock(schedule.from_minute),
            to: format_clock(schedule.to_minute),
            days: (schedule.days != ALL_DAYS).then(|| schedule.days()),
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
            days: ALL_DAYS,
        })
    }
    /// A whole-day schedule limited to the given weekdays.
    pub fn on_days(days: impl IntoIterator<Item = Weekday>) -> Result<Self, ScheduleError> {
        Self::new(0, DAY_MINUTES)?.with_days(days)
    }
    /// Limits the window to the given weekdays (the day the window starts on).
    pub fn with_days(
        mut self,
        days: impl IntoIterator<Item = Weekday>,
    ) -> Result<Self, ScheduleError> {
        let mask = days.into_iter().fold(0, |m, d| m | d.bit());
        if mask == 0 {
            return Err(ScheduleError::BadDays(String::new()));
        }
        self.days = mask;
        Ok(self)
    }
    /// The weekdays the window applies to, Monday first.
    pub fn days(&self) -> Vec<Weekday> {
        Weekday::ALL
            .into_iter()
            .filter(|d| self.days & d.bit() != 0)
            .collect()
    }
    fn every_day(&self) -> bool {
        self.days == ALL_DAYS
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
    /// True for `00:00-24:00` on every day, which every span satisfies.
    pub fn is_always(&self) -> bool {
        self.from_minute == 0 && self.to_minute == DAY_MINUTES && self.every_day()
    }
    /// The weekday of a local-clock instant (Unix ms already shifted by the UTC offset).
    pub fn weekday_of(local_ms: i64) -> Weekday {
        // 1970-01-01 was a Thursday.
        Weekday::from_index(local_ms.div_euclid(DAY_MS) + 3)
    }
    /// The weekday a span starting at `local_start_ms` belongs to: the day its window started.
    /// The after-midnight part of an overnight window belongs to the previous day.
    pub fn owner_day(&self, local_start_ms: i64) -> Weekday {
        let a = local_start_ms.rem_euclid(DAY_MS);
        let after_midnight =
            self.from_minute > self.to_minute && a < i64::from(self.to_minute) * MINUTE_MS;
        Self::weekday_of(if after_midnight {
            local_start_ms - DAY_MS
        } else {
            local_start_ms
        })
    }
    /// Whether the span `[start, start + len)` lies inside the window.
    /// `local_start_ms` is a Unix-millisecond instant already shifted by the
    /// series' UTC offset, so its time of day is the local clock.
    pub fn contains_span(&self, local_start_ms: i64, len_ms: i64) -> bool {
        if self.is_always() {
            return true;
        }
        let a = local_start_ms.rem_euclid(DAY_MS);
        if self.from_minute == 0 && self.to_minute == DAY_MINUTES {
            // Whole day on chosen weekdays: both ends of the span must fall on allowed days.
            let last = Self::weekday_of(local_start_ms + len_ms - 1);
            return self.days & Self::weekday_of(local_start_ms).bit() != 0
                && self.days & last.bit() != 0;
        }
        if self.days & self.owner_day(local_start_ms).bit() == 0 {
            return false;
        }
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
        )?;
        if !self.every_day() {
            let names: Vec<_> = self.days().into_iter().map(Weekday::name).collect();
            write!(f, " {}", names.join(","))?;
        }
        Ok(())
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
    const DAY: i64 = 86_400_000;
    // 1970-01-05 was a Monday.
    const MON: i64 = 4 * DAY;

    #[test]
    fn weekday_of_known_dates() {
        assert_eq!(Schedule::weekday_of(0), Weekday::Thu);
        assert_eq!(Schedule::weekday_of(MON), Weekday::Mon);
        assert_eq!(Schedule::weekday_of(MON - 1), Weekday::Sun);
        assert_eq!(Schedule::weekday_of(-DAY), Weekday::Wed); // before 1970
    }
    #[test]
    fn day_lists_parse() {
        use Weekday::*;
        assert_eq!(parse_days("mon-fri").unwrap(), [Mon, Tue, Wed, Thu, Fri]);
        assert_eq!(parse_days("weekend").unwrap(), [Sat, Sun]);
        assert_eq!(parse_days("Sat, sunday").unwrap(), [Sat, Sun]);
        assert_eq!(parse_days("fri-mon").unwrap(), [Mon, Fri, Sat, Sun]);
        assert_eq!(parse_days("daily").unwrap().len(), 7);
        for bad in ["", "funday", "mon-", "mon,,"] {
            assert!(
                matches!(parse_days(bad), Err(ScheduleError::BadDays(_))),
                "{bad}"
            );
        }
    }
    #[test]
    fn weekday_limits_a_daytime_window() {
        let s = Schedule::parse("09:00", "12:00")
            .unwrap()
            .with_days([Weekday::Sat, Weekday::Sun])
            .unwrap();
        assert!(!s.contains_span(MON + 10 * H, H));
        assert!(s.contains_span(MON + 5 * DAY + 10 * H, H)); // Saturday
        assert!(s.contains_span(MON + 6 * DAY + 10 * H, H));
        assert!(!s.contains_span(MON + 5 * DAY + 13 * H, H)); // right day, wrong time
        assert_eq!(s.to_string(), "09:00-12:00 sat,sun");
    }
    #[test]
    fn overnight_window_belongs_to_the_day_it_starts() {
        let fri_night = Schedule::parse("20:00", "06:00")
            .unwrap()
            .with_days([Weekday::Fri])
            .unwrap();
        let fri = MON + 4 * DAY;
        assert!(fri_night.contains_span(fri + 22 * H, H)); // Fri 22:00
        assert!(fri_night.contains_span(fri + DAY + 2 * H, H)); // Sat 02:00 still Friday's night
        assert!(!fri_night.contains_span(fri + 2 * H, H)); // Fri 02:00 is Thursday's night
        assert!(!fri_night.contains_span(fri + DAY + 22 * H, H)); // Sat night
    }
    #[test]
    fn whole_day_on_weekdays_only() {
        let weekend = Schedule::on_days([Weekday::Sat, Weekday::Sun]).unwrap();
        assert!(!weekend.is_always());
        assert!(!weekend.contains_span(MON + 12 * H, H));
        assert!(weekend.contains_span(MON + 5 * DAY + 12 * H, H));
        assert!(weekend.contains_span(MON + 5 * DAY + 23 * H + H / 2, H)); // Sat 23:30 into Sun
        assert!(!weekend.contains_span(MON + 4 * DAY + 23 * H + H / 2, H)); // Fri into Sat
        assert!(Schedule::on_days([]).is_err());
    }
    #[test]
    fn clock_text_is_the_wire_form() {
        assert_eq!(format_clock(1440), "24:00");
        assert_eq!(format_clock(545), "09:05");
    }
}
