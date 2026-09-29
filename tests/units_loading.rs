use env_operability::units::Unit;
use env_operability::{
    Comparison, Constraint, Metric, Observation, Plan, Preference, Series, ValidationError,
    WindowSearch,
};
use std::collections::BTreeMap;

fn units(pairs: &[(&str, Unit)]) -> BTreeMap<String, Unit> {
    pairs.iter().map(|(k, u)| (k.to_string(), *u)).collect()
}

#[test]
fn series_readings_are_converted_to_canonical_units() {
    let series = Series::with_units(
        1,
        vec![
            Observation::at(0)
                .with("wind_speed", 36.0)
                .with("temperature", 212.0),
            Observation::at(1)
                .with("wind_speed", 72.0)
                .with("temperature", 32.0),
        ],
        &units(&[
            ("wind_speed", Unit::KilometersPerHour),
            ("temperature", Unit::Fahrenheit),
        ]),
    )
    .unwrap();
    let first = &series.observations[0].values;
    assert!((first["wind_speed"] - 10.0).abs() < 1e-9);
    assert!((first["temperature"] - 100.0).abs() < 1e-9);
    assert!((series.observations[1].values["temperature"] - 0.0).abs() < 1e-9);
}

#[test]
fn series_units_reject_unknown_metric_and_wrong_dimension() {
    let obs = || {
        vec![Observation::at(0)
            .with("wind_speed", 1.0)
            .with("custom", 1.0)]
    };
    assert_eq!(
        Series::with_units(1, obs(), &units(&[("custom", Unit::Knots)])),
        Err(ValidationError::UnknownMetricUnit {
            metric: "custom".into()
        })
    );
    assert!(matches!(
        Series::with_units(1, obs(), &units(&[("wind_speed", Unit::Meters)])),
        Err(ValidationError::Units { .. })
    ));
}

#[test]
fn constraint_limits_convert_and_scale_is_a_delta() {
    let hard = Constraint::hard(
        "wind",
        Metric::new("wind_speed"),
        Comparison::LessThanOrEqual,
        36.0,
    )
    .with_unit(Unit::KilometersPerHour)
    .unwrap();
    assert!(matches!(hard, Constraint::Hard { threshold, .. } if (threshold - 10.0).abs() < 1e-9));

    let soft = Constraint::soft(
        "mild",
        Metric::new("temperature"),
        Preference::Range {
            min: 59.0,
            max: 77.0,
            scale: 18.0,
        },
        1.0,
    )
    .with_unit(Unit::Fahrenheit)
    .unwrap();
    match soft {
        Constraint::Soft {
            preference: Preference::Range { min, max, scale },
            ..
        } => {
            assert!((min - 15.0).abs() < 1e-9);
            assert!((max - 25.0).abs() < 1e-9);
            assert!(
                (scale - 10.0).abs() < 1e-9,
                "scale must not get the 32-degree offset"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn constraint_unit_errors_are_specific() {
    let custom = Constraint::hard("c", Metric::new("custom"), Comparison::LessThan, 1.0);
    assert!(matches!(
        custom.with_unit(Unit::Knots),
        Err(ValidationError::InvalidConstraint { .. })
    ));
    let wrong = Constraint::hard("w", Metric::new("wind_speed"), Comparison::LessThan, 1.0);
    assert!(matches!(
        wrong.with_unit(Unit::Celsius),
        Err(ValidationError::Units { .. })
    ));
}

#[test]
fn converted_plan_and_series_agree_end_to_end() {
    // Data in km/h and limits in knots meet in m/s: 36 km/h = 10 m/s < 20 kn (10.29 m/s).
    let series = Series::with_units(
        3_600_000,
        vec![Observation::at(0).with("wind_speed", 36.0)],
        &units(&[("wind_speed", Unit::KilometersPerHour)]),
    )
    .unwrap();
    let limit = Constraint::hard(
        "wind",
        Metric::new("wind_speed"),
        Comparison::LessThan,
        20.0,
    )
    .with_unit(Unit::Knots)
    .unwrap();
    let plan = Plan::single_stage("p", 3_600_000, vec![limit]);
    assert_eq!(
        WindowSearch::new(&series, &plan)
            .run()
            .unwrap()
            .feasible
            .len(),
        1
    );
}
