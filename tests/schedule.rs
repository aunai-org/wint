use env_operability::{
    present, ClockSpan, Comparison, Constraint, Expectation, Metric, Observation, Plan, Schedule,
    Series, Stage, ValidationError, Weekday, WindowSearch,
};

const H: i64 = 3_600_000;
const DAY: i64 = 24 * H;

/// 48 hourly samples from a UTC midnight, calm the whole time.
fn two_days(offset_minutes: i32) -> Series {
    let obs = (0..48)
        .map(|i| Observation::at(1_790_035_200_000 + i * H).with("wind_speed", 2.0))
        .collect();
    assert_eq!(
        1_790_035_200_000 % DAY,
        0,
        "fixture must start at UTC midnight"
    );
    Series::new(H, obs)
        .unwrap()
        .with_utc_offset(offset_minutes)
        .unwrap()
}
fn calm() -> Vec<Constraint> {
    vec![Constraint::hard(
        "wind",
        Metric::new("wind_speed"),
        Comparison::LessThan,
        10.0,
    )]
}
fn starts(series: &Series, plan: &Plan) -> Vec<i64> {
    let mut v: Vec<i64> = WindowSearch::new(series, plan)
        .run()
        .unwrap()
        .feasible
        .iter()
        .map(|w| (w.start_ms.rem_euclid(DAY)) / H)
        .collect();
    v.sort();
    v
}

#[test]
fn morning_window_allows_only_spans_that_fit() {
    let plan = Plan::single_stage("p", 2 * H, calm())
        .with_schedule(Schedule::parse("09:00", "12:00").unwrap());
    // 2-hour operations fit at 09:00-11:00 and 10:00-12:00, on each of the two days.
    assert_eq!(starts(&two_days(0), &plan), [9, 9, 10, 10]);
}

#[test]
fn until_evening_and_full_day() {
    let until = Plan::single_stage("p", H, calm())
        .with_schedule(Schedule::parse("00:00", "20:00").unwrap());
    let hours = starts(&two_days(0), &until);
    assert_eq!(hours.len(), 40); // 20 start hours x 2 days
    assert!(hours.iter().all(|h| *h < 20));
    let always = Plan::single_stage("p", H, calm())
        .with_schedule(Schedule::parse("00:00", "24:00").unwrap());
    assert_eq!(starts(&two_days(0), &always).len(), 48);
}

#[test]
fn utc_offset_shifts_the_local_clock() {
    let plan = Plan::single_stage("p", 2 * H, calm())
        .with_schedule(Schedule::parse("09:00", "12:00").unwrap());
    // UTC+02:00: 09:00-12:00 local is 07:00-10:00 UTC, so starts at 07 and 08 UTC.
    assert_eq!(starts(&two_days(120), &plan), [7, 7, 8, 8]);
    // UTC-05:30: hourly samples sit at :30 on the local clock. A 2 h span from 09:30 local (15:00 UTC)
    // ends at 11:30 and fits; one from 10:30 would end at 12:30 and does not.
    assert_eq!(starts(&two_days(-330), &plan), [15, 15]);
}

#[test]
fn overnight_window_wraps_midnight() {
    let night = Plan::single_stage("p", 3 * H, calm())
        .with_schedule(Schedule::parse("20:00", "06:00").unwrap());
    let hours = starts(&two_days(0), &night);
    // A 3 h span fits inside 20:00-06:00 when it starts at 20-23 or 00-03 (03:00-06:00 ends exactly on
    // the boundary). Day 2's 22:00 and 23:00 starts would need data past the 48 h provided.
    assert_eq!(hours, [0, 0, 1, 1, 2, 2, 3, 3, 20, 20, 21, 21, 22, 23]);
}

#[test]
fn schedule_failure_explains_itself_in_time_order() {
    let plan = Plan::single_stage("p", 2 * H, calm())
        .with_schedule(Schedule::parse("09:00", "12:00").unwrap());
    let result = WindowSearch::new(&two_days(120), &plan).run().unwrap();
    // The window starting 00:00 UTC (02:00 local) fails at its first sample.
    let first = result
        .rejected
        .iter()
        .find(|r| r.start_ms.rem_euclid(DAY) == 0)
        .unwrap();
    assert_eq!(first.failure.constraint, "time of day");
    // The evidence is data: the window that was required and the local span that was examined.
    assert_eq!(
        first.failure.expectation,
        Expectation::ClockWindow {
            from_minute: 540,
            to_minute: 720,
            days: Weekday::ALL.to_vec()
        }
    );
    assert_eq!(
        first.failure.clock,
        Some(ClockSpan {
            weekday: Weekday::Tue,
            start_minute: 120,
            end_minute: 180,
            utc_offset_minutes: 120
        })
    );
    // Wording is the presenter's job; the optional default reads as before.
    assert_eq!(
        present::describe_expectation(&first.failure.expectation),
        "within 09:00-12:00 local"
    );
    assert_eq!(
        present::describe_reading(&first.failure),
        "tue 02:00 to 03:00 local (UTC+02:00)"
    );
    assert_eq!(first.failure.actual, None);
    // A passing window reports the local span it occupies.
    let ok = result
        .feasible
        .iter()
        .find(|w| w.start_ms.rem_euclid(DAY) == 7 * H)
        .unwrap();
    let row = ok
        .evidence
        .iter()
        .find(|e| e.constraint == "time of day")
        .unwrap();
    assert!(row.passed);
    assert_eq!(
        row.clock,
        Some(ClockSpan {
            weekday: Weekday::Tue,
            start_minute: 540,
            end_minute: 660,
            utc_offset_minutes: 120
        })
    );
}

#[test]
fn schedules_apply_per_stage() {
    let day = Schedule::parse("09:00", "12:00").unwrap();
    let plan = Plan::new(
        "two-stage",
        vec![
            Stage::new("survey", H, calm()).with_schedule(day),
            Stage::new("recovery", H, calm()), // any time
        ],
    );
    // Survey must be 09-12; recovery is the next hour, unconstrained: starts 9, 10, 11 (11:00 survey, 12:00 recovery).
    assert_eq!(starts(&two_days(0), &plan), [9, 9, 10, 10, 11, 11]);
}

#[test]
fn day_and_night_come_from_the_is_day_metric() {
    let obs = (0..24)
        .map(|i| {
            Observation::at(i * H)
                .with("wind_speed", 2.0)
                .with("is_day", if (6..18).contains(&i) { 1.0 } else { 0.0 })
        })
        .collect();
    let series = Series::new(H, obs).unwrap();
    let day_only = Plan::single_stage(
        "p",
        2 * H,
        vec![Constraint::hard(
            "daylight",
            Metric::new("is_day"),
            Comparison::Equal,
            1.0,
        )],
    );
    // Daylight hours 06..18, so 2-hour windows start 6..=16.
    assert_eq!(starts(&series, &day_only), (6..=16).collect::<Vec<_>>());
    let night_only = Plan::single_stage(
        "p",
        2 * H,
        vec![Constraint::hard(
            "night",
            Metric::new("is_day"),
            Comparison::Equal,
            0.0,
        )],
    );
    assert_eq!(
        starts(&series, &night_only),
        [0, 1, 2, 3, 4, 18, 19, 20, 21, 22]
    );
}

#[test]
fn offsets_are_validated() {
    let s = two_days(0);
    assert!(matches!(
        s.clone().with_utc_offset(841),
        Err(ValidationError::InvalidUtcOffset { minutes: 841 })
    ));
    assert!(s.clone().with_utc_offset(-721).is_err());
    assert!(s.clone().with_utc_offset(840).is_ok() && s.with_utc_offset(-720).is_ok());
    assert_eq!(present::format_offset(-720), "UTC-12:00");
}

#[test]
fn weekday_rule_keeps_only_windows_on_the_chosen_day() {
    let series = two_days(0);
    let first_day = Schedule::weekday_of(series.observations[0].timestamp_ms);
    let second_day = Schedule::weekday_of(series.observations[24].timestamp_ms);
    assert_ne!(first_day, second_day);
    let plan = Plan::single_stage("p", 3 * H, calm())
        .with_schedule(Schedule::on_days([second_day]).unwrap());
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    assert!(!result.feasible.is_empty());
    assert!(result
        .feasible
        .iter()
        .all(|w| Schedule::weekday_of(w.start_ms) == second_day));
    // The rejection says which day it was and which days were wanted.
    let rejected = result
        .rejected
        .iter()
        .find(|r| Schedule::weekday_of(r.start_ms) == first_day)
        .unwrap();
    assert_eq!(rejected.failure.clock.unwrap().weekday, first_day);
    assert!(matches!(
        &rejected.failure.expectation,
        Expectation::ClockWindow { days, .. } if days == &vec![second_day]
    ));
}

/// Berlin, 2026-03-28 20:00 UTC onwards, hourly. Clocks go forward at 01:00 UTC on the 29th
/// (02:00 CET becomes 03:00 CEST), so the offset is +60 before and +120 from then on.
const BERLIN_START: i64 = 1_774_728_000_000;
fn berlin_dst(per_sample: bool) -> Series {
    let obs = (0..14)
        .map(|i| {
            let o = Observation::at(BERLIN_START + i * H).with("wind_speed", 2.0);
            if per_sample {
                o.with_utc_offset(if i < 5 { 60 } else { 120 })
            } else {
                o
            }
        })
        .collect();
    Series::new(H, obs).unwrap().with_utc_offset(60).unwrap()
}

#[test]
fn per_sample_offsets_follow_a_daylight_saving_change() {
    let plan = Plan::single_stage("p", H, calm())
        .with_schedule(Schedule::parse("06:00", "09:00").unwrap());
    let hour_of_day_utc = |series: &Series| {
        let mut v: Vec<i64> = WindowSearch::new(series, &plan)
            .run()
            .unwrap()
            .feasible
            .iter()
            .map(|w| (w.start_ms - BERLIN_START) / H + 20)
            .map(|h| h % 24)
            .collect();
        v.sort();
        v
    };
    // After the change 06:00-09:00 local is 04:00-07:00 UTC.
    assert_eq!(hour_of_day_utc(&berlin_dst(true)), [4, 5, 6]);
    // A single +01:00 offset is wrong once the clocks have moved: it finds 05:00-08:00 UTC.
    assert_eq!(hour_of_day_utc(&berlin_dst(false)), [5, 6, 7]);
}

#[test]
fn evidence_reports_the_offset_in_force_at_that_sample() {
    let plan = Plan::single_stage("p", H, calm())
        .with_schedule(Schedule::parse("06:00", "09:00").unwrap());
    let result = WindowSearch::new(&berlin_dst(true), &plan).run().unwrap();
    let window = result
        .feasible
        .iter()
        .find(|w| w.start_ms == BERLIN_START + 8 * H)
        .unwrap();
    // 04:00 UTC on the 29th is 06:00 local at +02:00.
    let clock = window.evidence.iter().find_map(|e| e.clock).unwrap();
    assert_eq!((clock.start_minute, clock.utc_offset_minutes), (360, 120));
    // Before the change the same local hour is at +01:00.
    let early = result
        .rejected
        .iter()
        .find(|r| r.start_ms == BERLIN_START)
        .unwrap();
    assert_eq!(early.failure.clock.unwrap().utc_offset_minutes, 60);
}

#[test]
fn sample_offsets_are_range_checked_and_must_agree_across_an_ensemble() {
    let bad = Series::new(
        H,
        vec![Observation::at(0).with_utc_offset(900), Observation::at(H)],
    );
    assert!(matches!(
        bad,
        Err(ValidationError::InvalidUtcOffset { minutes: 900 })
    ));
    let member = |name: &str, series: Series| env_operability::Member {
        name: name.into(),
        series,
    };
    let same = env_operability::Ensemble::new(vec![
        member("a", berlin_dst(true)),
        member("b", berlin_dst(true)),
    ]);
    assert!(same.is_ok());
    let differ = env_operability::Ensemble::new(vec![
        member("a", berlin_dst(true)),
        member("b", berlin_dst(false)),
    ]);
    assert!(matches!(
        differ,
        Err(ValidationError::EnsembleMismatch { .. })
    ));
}

#[cfg(feature = "json")]
mod json {
    use super::*;

    #[test]
    fn observation_offsets_round_trip_through_json() {
        let series: Series = serde_json::from_str(
            r#"{"cadence_ms":3600000,"observations":[
                {"timestamp_ms":0,"values":{"wind_speed":1},"utc_offset_minutes":60},
                {"timestamp_ms":3600000,"values":{"wind_speed":1}}]}"#,
        )
        .unwrap();
        assert_eq!(series.observations[0].utc_offset_minutes, Some(60));
        assert_eq!(series.observations[1].utc_offset_minutes, None);
        assert_eq!(series.offset_at(&series.observations[1]), 0);
        let text = serde_json::to_string(&series.observations[1]).unwrap();
        assert!(!text.contains("utc_offset"));
        assert!(serde_json::from_str::<Series>(
            r#"{"cadence_ms":3600000,"observations":[
                {"timestamp_ms":0,"values":{},"utc_offset_minutes":9999}]}"#
        )
        .is_err());
    }

    #[test]
    fn weekdays_round_trip_through_json() {
        let s: Schedule =
            serde_json::from_str(r#"{"from":"09:00","to":"12:00","days":["sat","sun"]}"#).unwrap();
        assert_eq!(s.days(), [Weekday::Sat, Weekday::Sun]);
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"from":"09:00","to":"12:00","days":["sat","sun"]}"#
        );
        // Days alone mean the whole day; omitted days mean every day and are not written back.
        let only_days: Schedule = serde_json::from_str(r#"{"days":["mon"]}"#).unwrap();
        assert_eq!((only_days.from_minute(), only_days.to_minute()), (0, 1440));
        let plain: Schedule = serde_json::from_str(r#"{"from":"09:00","to":"12:00"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"from":"09:00","to":"12:00"}"#
        );
        assert!(serde_json::from_str::<Schedule>(r#"{"days":[]}"#).is_err());
        assert!(serde_json::from_str::<Schedule>(r#"{"days":["funday"]}"#).is_err());
    }

    #[test]
    fn stage_schedule_and_series_offset_round_trip() {
        let plan: Plan = serde_json::from_str(
            r#"{"name":"p","stages":[{"name":"s","duration_ms":3600000,"constraints":[],
                "schedule":{"from":"09:00","to":"12:00"}}]}"#,
        )
        .unwrap();
        assert_eq!(
            plan.stages[0].schedule,
            Some(Schedule::parse("09:00", "12:00").unwrap())
        );
        let text = serde_json::to_string(&plan).unwrap();
        assert!(
            text.contains(r#""schedule":{"from":"09:00","to":"12:00"}"#),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
        // Absent schedule stays absent (and is not serialized).
        let plain = Plan::single_stage("p", H, vec![]);
        assert!(!serde_json::to_string(&plain).unwrap().contains("schedule"));

        let series: Series = serde_json::from_str(
            r#"{"cadence_ms":3600000,"utc_offset_minutes":120,"observations":[{"timestamp_ms":0,"values":{}}]}"#,
        )
        .unwrap();
        assert_eq!(series.utc_offset_minutes, 120);
    }

    #[test]
    fn bad_schedules_and_offsets_are_rejected_with_reasons() {
        let bad = |from: &str, to: &str| {
            serde_json::from_str::<Plan>(&format!(
                r#"{{"name":"p","stages":[{{"name":"s","duration_ms":3600000,"constraints":[],"schedule":{{"from":"{from}","to":"{to}"}}}}]}}"#
            ))
            .unwrap_err()
            .to_string()
        };
        assert!(bad("09:00", "09:00").contains("must differ"));
        assert!(bad("9am", "12:00").contains("not a HH:MM clock time"));
        assert!(bad("24:00", "06:00").contains("from must be before 24:00"));
        let err = serde_json::from_str::<Series>(
            r#"{"cadence_ms":1,"utc_offset_minutes":900,"observations":[{"timestamp_ms":0,"values":{}}]}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("outside -12:00 to +14:00"),
            "{err}"
        );
    }
}
