//! Metric vocabulary and unit conversion.
//!
//! The engine itself is unit-blind: it compares the numbers it is given. This
//! module lets the edges (adapters, JSON loading) convert everything to one
//! canonical unit per metric so plans and series always agree.

use std::fmt;

/// A physical dimension. Units only convert within one dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Dimension {
    Speed,
    Temperature,
    Length,
    Precipitation,
    Pressure,
    Percent,
    Angle,
    /// A 0/1 indicator such as `is_day`.
    Flag,
}

/// A supported measurement unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unit {
    MetersPerSecond,
    KilometersPerHour,
    Knots,
    MilesPerHour,
    Celsius,
    Fahrenheit,
    Kelvin,
    Meters,
    Kilometers,
    Feet,
    Miles,
    Millimeters,
    Centimeters,
    Inches,
    HectoPascals,
    Pascals,
    KiloPascals,
    InchesOfMercury,
    Percent,
    Degrees,
    Flag,
}

/// `(unit, dimension, canonical symbol, accepted spellings, scale to base, offset to base)`.
/// `base = value * scale + offset`. Bases: m/s, °C, m, mm, hPa, %, degrees.
type UnitRow = (
    Unit,
    Dimension,
    &'static str,
    &'static [&'static str],
    f64,
    f64,
);

const TABLE: &[UnitRow] = &[
    (
        Unit::MetersPerSecond,
        Dimension::Speed,
        "m/s",
        &["m/s", "ms", "mps"],
        1.0,
        0.0,
    ),
    (
        Unit::KilometersPerHour,
        Dimension::Speed,
        "km/h",
        &["km/h", "kmh", "kph", "kmph"],
        1.0 / 3.6,
        0.0,
    ),
    (
        Unit::Knots,
        Dimension::Speed,
        "kn",
        &["kn", "kt", "kts", "knots"],
        1852.0 / 3600.0,
        0.0,
    ),
    (
        Unit::MilesPerHour,
        Dimension::Speed,
        "mph",
        &["mph", "mi/h"],
        0.44704,
        0.0,
    ),
    (
        Unit::Celsius,
        Dimension::Temperature,
        "°C",
        &["°c", "c", "degc", "celsius"],
        1.0,
        0.0,
    ),
    (
        Unit::Fahrenheit,
        Dimension::Temperature,
        "°F",
        &["°f", "f", "degf", "fahrenheit"],
        5.0 / 9.0,
        -160.0 / 9.0,
    ),
    (
        Unit::Kelvin,
        Dimension::Temperature,
        "K",
        &["k", "kelvin"],
        1.0,
        -273.15,
    ),
    (
        Unit::Meters,
        Dimension::Length,
        "m",
        &["m", "meter", "meters", "metre", "metres"],
        1.0,
        0.0,
    ),
    (
        Unit::Kilometers,
        Dimension::Length,
        "km",
        &["km", "kilometer", "kilometers"],
        1000.0,
        0.0,
    ),
    (
        Unit::Feet,
        Dimension::Length,
        "ft",
        &["ft", "feet", "foot"],
        0.3048,
        0.0,
    ),
    (
        Unit::Miles,
        Dimension::Length,
        "mi",
        &["mi", "mile", "miles", "sm"],
        1609.344,
        0.0,
    ),
    (
        Unit::Millimeters,
        Dimension::Precipitation,
        "mm",
        &["mm"],
        1.0,
        0.0,
    ),
    (
        Unit::Centimeters,
        Dimension::Precipitation,
        "cm",
        &["cm"],
        10.0,
        0.0,
    ),
    (
        Unit::Inches,
        Dimension::Precipitation,
        "in",
        &["in", "inch", "inches"],
        25.4,
        0.0,
    ),
    (
        Unit::HectoPascals,
        Dimension::Pressure,
        "hPa",
        &["hpa", "mb", "mbar"],
        1.0,
        0.0,
    ),
    (Unit::Pascals, Dimension::Pressure, "Pa", &["pa"], 0.01, 0.0),
    (
        Unit::KiloPascals,
        Dimension::Pressure,
        "kPa",
        &["kpa"],
        10.0,
        0.0,
    ),
    (
        Unit::InchesOfMercury,
        Dimension::Pressure,
        "inHg",
        &["inhg"],
        33.863_886_666_7,
        0.0,
    ),
    (
        Unit::Percent,
        Dimension::Percent,
        "%",
        &["%", "percent", "pct"],
        1.0,
        0.0,
    ),
    (
        Unit::Degrees,
        Dimension::Angle,
        "°",
        &["°", "deg", "degrees"],
        1.0,
        0.0,
    ),
    (
        Unit::Flag,
        Dimension::Flag,
        "0/1",
        &["0/1", "flag"],
        1.0,
        0.0,
    ),
];

impl Unit {
    fn row(self) -> &'static UnitRow {
        TABLE
            .iter()
            .find(|r| r.0 == self)
            .expect("every unit has a table row")
    }
    /// Parses a unit symbol such as `"km/h"`, `"kn"`, `"°F"` or `"%"` (case-insensitive).
    ///
    /// Note that a bare `"m"` is metres and `"mm"` is millimetres, so the
    /// symbol is matched exactly (after lowercasing), never by prefix.
    pub fn parse(symbol: &str) -> Option<Self> {
        let wanted = symbol.trim().to_lowercase();
        TABLE
            .iter()
            .find(|r| r.3.contains(&wanted.as_str()))
            .map(|r| r.0)
    }
    pub fn dimension(self) -> Dimension {
        self.row().1
    }
    pub fn symbol(self) -> &'static str {
        self.row().2
    }
    /// Converts an absolute value (a reading or a limit) into `to`.
    pub fn convert(self, value: f64, to: Unit) -> Result<f64, UnitError> {
        self.check(to)?;
        let (from, to) = (self.row(), to.row());
        Ok((value * from.4 + from.5 - to.5) / to.4)
    }
    /// Converts a *difference* between two values (for example a soft-preference
    /// scale). Unlike [`Unit::convert`], temperature offsets do not apply:
    /// a 18 °F difference is a 10 °C difference.
    pub fn convert_delta(self, delta: f64, to: Unit) -> Result<f64, UnitError> {
        self.check(to)?;
        Ok(delta * self.row().4 / to.row().4)
    }
    fn check(self, to: Unit) -> Result<(), UnitError> {
        if self.dimension() == to.dimension() {
            Ok(())
        } else {
            Err(UnitError::DimensionMismatch { from: self, to })
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnitError {
    DimensionMismatch { from: Unit, to: Unit },
}
impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch { from, to } => {
                write!(
                    f,
                    "cannot convert {from} to {to}: different kinds of quantity"
                )
            }
        }
    }
}
impl std::error::Error for UnitError {}

/// The shared metric vocabulary: `(name, canonical unit, description)`.
///
/// Canonical units are SI-leaning so any source converts cleanly. Adapters and
/// presets use these names; callers may still use their own metric names, but
/// those are not unit-checked or converted.
pub const VOCABULARY: &[(&str, Unit, &str)] = &[
    ("wind_speed", Unit::MetersPerSecond, "Sustained wind speed"),
    ("wind_gust", Unit::MetersPerSecond, "Wind gust speed"),
    ("wind_direction", Unit::Degrees, "Direction wind blows from"),
    ("temperature", Unit::Celsius, "Air temperature"),
    (
        "precipitation",
        Unit::Millimeters,
        "Precipitation accumulated over the sample interval",
    ),
    (
        "precipitation_probability",
        Unit::Percent,
        "Chance of precipitation",
    ),
    ("cloud_cover", Unit::Percent, "Total cloud cover"),
    ("relative_humidity", Unit::Percent, "Relative humidity"),
    ("visibility", Unit::Meters, "Horizontal visibility"),
    ("wave_height", Unit::Meters, "Significant wave height"),
    ("pressure", Unit::HectoPascals, "Mean sea-level pressure"),
    ("is_day", Unit::Flag, "1 in daylight, 0 at night"),
];

/// Canonical unit for a vocabulary metric, or `None` for a caller-defined metric.
pub fn canonical_unit(metric: &str) -> Option<Unit> {
    VOCABULARY.iter().find(|v| v.0 == metric).map(|v| v.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }
    #[test]
    fn parses_common_spellings_exactly() {
        assert_eq!(Unit::parse("km/h"), Some(Unit::KilometersPerHour));
        assert_eq!(Unit::parse(" KN "), Some(Unit::Knots));
        assert_eq!(Unit::parse("°F"), Some(Unit::Fahrenheit));
        assert_eq!(Unit::parse("m"), Some(Unit::Meters));
        assert_eq!(Unit::parse("mm"), Some(Unit::Millimeters));
        assert_eq!(Unit::parse("furlongs"), None);
    }
    #[test]
    fn converts_speed() {
        assert!(close(
            Unit::KilometersPerHour
                .convert(36.0, Unit::MetersPerSecond)
                .unwrap(),
            10.0
        ));
        assert!(close(
            Unit::Knots.convert(10.0, Unit::MetersPerSecond).unwrap(),
            5.144_444_444
        ));
        assert!(close(
            Unit::MetersPerSecond
                .convert(10.0, Unit::MilesPerHour)
                .unwrap(),
            22.369_362_92
        ));
    }
    #[test]
    fn converts_temperature_absolute_and_delta() {
        assert!(close(
            Unit::Fahrenheit.convert(32.0, Unit::Celsius).unwrap(),
            0.0
        ));
        assert!(close(
            Unit::Fahrenheit.convert(212.0, Unit::Celsius).unwrap(),
            100.0
        ));
        assert!(close(
            Unit::Kelvin.convert(273.15, Unit::Celsius).unwrap(),
            0.0
        ));
        assert!(close(
            Unit::Celsius.convert(100.0, Unit::Fahrenheit).unwrap(),
            212.0
        ));
        assert!(close(
            Unit::Fahrenheit.convert_delta(18.0, Unit::Celsius).unwrap(),
            10.0
        ));
    }
    #[test]
    fn converts_length_precipitation_pressure() {
        assert!(close(
            Unit::Miles.convert(3.0, Unit::Meters).unwrap(),
            4828.032
        ));
        assert!(close(
            Unit::Feet.convert(100.0, Unit::Meters).unwrap(),
            30.48
        ));
        assert!(close(
            Unit::Inches.convert(1.0, Unit::Millimeters).unwrap(),
            25.4
        ));
        assert!(
            (Unit::InchesOfMercury
                .convert(29.92, Unit::HectoPascals)
                .unwrap()
                - 1013.2075)
                .abs()
                < 1e-3
        );
        assert!(close(
            Unit::Pascals
                .convert(101_325.0, Unit::HectoPascals)
                .unwrap(),
            1013.25
        ));
    }
    #[test]
    fn rejects_cross_dimension_conversion() {
        let err = Unit::Knots.convert(1.0, Unit::Meters).unwrap_err();
        assert!(matches!(err, UnitError::DimensionMismatch { .. }));
    }
    #[test]
    fn every_unit_has_a_row_and_vocabulary_is_consistent() {
        for row in TABLE {
            assert_eq!(row.0.row().2, row.2);
            assert_eq!(Unit::parse(row.2), Some(row.0), "{}", row.2);
        }
        assert_eq!(canonical_unit("wind_speed"), Some(Unit::MetersPerSecond));
        assert_eq!(canonical_unit("my_custom_metric"), None);
    }
}
