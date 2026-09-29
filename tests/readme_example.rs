use env_operability::{Comparison, Constraint, Metric, Observation, Plan, Series, WindowSearch};

#[test]
fn readme_example_finds_two_windows() {
    let series = Series::new(
        3_600_000,
        vec![
            Observation::at(0)
                .with("wind", 12.0)
                .with("rain_probability", 10.0),
            Observation::at(3_600_000)
                .with("wind", 15.0)
                .with("rain_probability", 15.0),
            Observation::at(7_200_000)
                .with("wind", 8.0)
                .with("rain_probability", 10.0),
        ],
    )
    .unwrap();
    let plan = Plan::single_stage(
        "field-work",
        7_200_000,
        vec![
            Constraint::hard(
                "safe wind",
                Metric::new("wind"),
                Comparison::LessThanOrEqual,
                20.0,
            ),
            Constraint::hard(
                "low rain chance",
                Metric::new("rain_probability"),
                Comparison::LessThan,
                20.0,
            ),
        ],
    );
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    assert_eq!(result.feasible.len(), 2);
    assert_eq!(result.feasible[0].start_ms, 0);
}

#[test]
fn half_open_boundary_excludes_end_sample() {
    // Window [0, 2) covers samples at 0 and 1 only; the spike at 2 is outside.
    let series = Series::new(
        1,
        vec![
            Observation::at(0).with("wind", 1.0),
            Observation::at(1).with("wind", 1.0),
            Observation::at(2).with("wind", 99.0),
        ],
    )
    .unwrap();
    let plan = Plan::single_stage(
        "p",
        2,
        vec![Constraint::hard(
            "w",
            Metric::new("wind"),
            Comparison::LessThan,
            10.0,
        )],
    );
    let result = WindowSearch::new(&series, &plan).run().unwrap();
    assert_eq!(result.feasible.len(), 1);
    assert_eq!(result.rejected.len(), 1);
}
