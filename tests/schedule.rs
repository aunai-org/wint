use env_operability::{
    present, ClockSpan, Comparison, Constraint, Expectation, Metric, Observation, Plan, Schedule,
    Series, Stage, ValidationError, WindowSearch,
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
            to_minute: 720
        }
    );
    assert_eq!(
        first.failure.clock,
        Some(ClockSpan {
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
        "02:00 to 03:00 local (UTC+02:00)"
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

#[cfg(feature = "json")]
mod json {
    use super::*;

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
