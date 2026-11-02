//! DuckDB-independent conversion policy over a pinned UCUM implementation.
//! Only multiplicative and affine temperature units are admitted in this release.
#![forbid(unsafe_code)]

pub mod catalog;
pub mod quantity;

use octofhir_ucum::{Dimension, ErrorKind, Number, SpecialKind};
use std::fmt::{self, Write};

pub const BACKEND: &str = concat!(
    "octofhir-ucum 0.9.2; UCUM 2.2; units policy ",
    env!("CARGO_PKG_VERSION")
);
pub const MAX_UNIT_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitError {
    pub status: &'static str,
    pub message: String,
}

impl UnitError {
    pub fn new(status: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}
impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}
impl std::error::Error for UnitError {}
impl From<octofhir_ucum::UcumError> for UnitError {
    fn from(error: octofhir_ucum::UcumError) -> Self {
        let status = match &error.kind {
            ErrorKind::UnitNotFound { .. } => "unknown_unit",
            ErrorKind::DimensionMismatch { .. } => "incompatible_dimension",
            ErrorKind::SpecialUnitError { .. } => "unsupported_unit",
            ErrorKind::PrecisionOverflow { .. } => "overflow",
            ErrorKind::NestingTooDeep { .. } => "resource_limit",
            _ => "invalid_syntax",
        };
        Self::new(status, error.to_string())
    }
}

type Result<T> = std::result::Result<T, UnitError>;

#[derive(Clone, Debug)]
pub struct ParsedUnit {
    pub dimension: String,
    pub base_unit: String,
    /// base_value = value * factor + offset, in UCUM base units (mass is grams).
    pub factor: f64,
    pub offset: f64,
    pub is_affine: bool,
    dims: Dimension,
    exact_factor: Number,
    exact_offset: Number,
}

fn overflow() -> UnitError {
    UnitError::new(
        "overflow",
        "conversion is outside the supported numeric range",
    )
}

fn finite_factor(number: Number) -> Result<f64> {
    // The common identity factor is exact; avoid the backend's decimal-format /
    // decimal-parse round trip without changing rounding for any other factor.
    let value = if number == Number::ONE {
        1.0
    } else {
        number.to_f64()
    };
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(overflow())
    }
}

pub fn parse(unit: &str) -> Result<ParsedUnit> {
    if unit.len() > MAX_UNIT_BYTES {
        return Err(UnitError::new(
            "resource_limit",
            "unit expressions are limited to 4096 bytes",
        ));
    }
    if unit.is_empty() {
        return Err(UnitError::new(
            "invalid_syntax",
            "empty unit; use '1' for dimensionless quantities",
        ));
    }
    let info = octofhir_ucum::analyse(unit)?;
    if !info.arbitrary.is_empty()
        || !matches!(
            info.special,
            None | Some(SpecialKind::Celsius | SpecialKind::Fahrenheit | SpecialKind::Reaumur)
        )
    {
        return Err(UnitError::new(
            "unsupported_unit",
            "only multiplicative units and affine temperatures are supported; no logarithmic or arbitrary units",
        ));
    }
    // Prefixes/numeric factors on special units apply to their input scale.
    let exact_factor = if info.scale == Number::ONE {
        info.factor
    } else {
        info.factor.checked_mul(info.scale).ok_or_else(overflow)?
    };
    let factor = finite_factor(exact_factor)?;
    let offset = info.offset.to_f64();
    if !offset.is_finite() {
        return Err(overflow());
    }
    // Render directly, rather than allocating a Vec and a String per base unit
    // and then joining them into a second output allocation.
    let mut base_unit = String::new();
    for (symbol, exp) in Dimension::BASE_UNITS.iter().zip(info.dimension.0) {
        if exp == 0 {
            continue;
        }
        if !base_unit.is_empty() {
            base_unit.push('.');
        }
        base_unit.push_str(symbol);
        if exp != 1 {
            write!(&mut base_unit, "{exp}").expect("writing to a String cannot fail");
        }
    }
    Ok(ParsedUnit {
        dimension: info.dimension.to_string(),
        base_unit: if base_unit.is_empty() {
            "1".to_owned()
        } else {
            base_unit
        },
        factor,
        offset,
        is_affine: info.special.is_some(),
        dims: info.dimension,
        exact_factor,
        exact_offset: info.offset,
    })
}

pub fn compatible(from: &ParsedUnit, to: &ParsedUnit) -> bool {
    from.dims == to.dims
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConversionPlan {
    scale: Number,
    offset: Number,
    fast: Option<(f64, f64)>,
}

impl ConversionPlan {
    pub fn new(from: &ParsedUnit, to: &ParsedUnit, delta: bool) -> Result<Self> {
        if !compatible(from, to) {
            return Err(UnitError::new(
                "incompatible_dimension",
                format!("{} cannot convert to {}", from.dimension, to.dimension),
            ));
        }
        if delta && from.dims != Dimension([0, 0, 0, 0, 1, 0, 0]) {
            return Err(UnitError::new(
                "invalid_delta_unit",
                "delta conversion requires temperature units",
            ));
        }
        // Keep the wide decimal ratio: two representable unit factors can have a
        // non-representable/subnormal ratio even when the final value is ordinary.
        let scale = from
            .exact_factor
            .checked_div(to.exact_factor)
            .ok_or_else(overflow)?;
        let offset = if delta {
            Number::ZERO
        } else {
            from.exact_offset
                .checked_sub(to.exact_offset)
                .and_then(|n| n.checked_div(to.exact_factor))
                .ok_or_else(overflow)?
        };
        let (s, o) = (scale.to_f64(), offset.to_f64());
        let fast = (s.is_normal() && (offset.is_zero() || o.is_normal())).then_some((s, o));
        Ok(Self {
            scale,
            offset,
            fast,
        })
    }

    pub fn convert(&self, value: f64) -> Result<f64> {
        if !value.is_finite() {
            return Err(UnitError::new(
                "non_finite_input",
                "measurement must be finite",
            ));
        }
        if let Some((scale, offset)) = self.fast {
            let result = value.mul_add(scale, offset);
            if result.is_normal() {
                return Ok(result);
            }
        }
        // Fall back at numeric boundaries, validating the final result rather
        // than a prematurely narrowed intermediate. Ordinary rows use one FMA.
        let wide = Number::from_f64(value)
            .and_then(|n| n.checked_mul(self.scale))
            .and_then(|n| n.checked_add(self.offset))
            .ok_or_else(overflow)?;
        let result = wide.to_f64();
        if !result.is_finite() || (result == 0.0 && !wide.is_zero()) {
            return Err(overflow());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(value: f64, from: &str, to: &str, delta: bool) -> Result<f64> {
        ConversionPlan::new(&parse(from)?, &parse(to)?, delta)?.convert(value)
    }
    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }
    #[test]
    fn base_unit_rendering_preserves_symbols_and_exponents() {
        for (unit, base) in [
            ("1", "1"),
            ("m", "m"),
            ("Pa", "m-1.s-2.g"),
            ("V", "m2.s-2.g.C-1"),
            ("m-128", "m-128"),
            ("m127", "m127"),
        ] {
            assert_eq!(parse(unit).unwrap().base_unit, base, "{unit}");
        }
    }

    #[test]
    fn compounds_and_prefixes() {
        for (v, a, b, expected) in [
            (1.0, "kg/m3", "g/L", 1.0),
            (1.0, "kW.h", "J", 3_600_000.0),
            (1.0, "Pa", "kg/(m.s2)", 1.0),
            (1.0, "MW", "mW", 1e9),
            (1.0, "[in_i]", "cm", 2.54),
            (25.0, "%", "1", 0.25),
            (1.0, "mg{total}", "mg", 1.0),
            (1.0, "m120", "m120", 1.0),
        ] {
            near(convert(v, a, b, false).unwrap(), expected);
        }
        assert!(!compatible(&parse("kg").unwrap(), &parse("m").unwrap()));
        assert!(compatible(&parse("J").unwrap(), &parse("N.m").unwrap()));
        assert!(!compatible(&parse("Hz").unwrap(), &parse("rad/s").unwrap()));
    }
    #[test]
    fn temperature_absolute_and_delta() {
        for (v, a, b, delta, expected) in [
            (0.0, "Cel", "K", false, 273.15),
            (0.0, "Cel", "[degF]", false, 32.0),
            (32.0, "[degF]", "Cel", false, 0.0),
            (10.0, "Cel", "[degF]", true, 18.0),
            (18.0, "[degF]", "K", true, 10.0),
            (20.0, "Cel", "K", true, 20.0),
            (1.0, "mCel", "K", false, 273.151),
            (1000.0, "mCel", "Cel", true, 1.0),
            (80.0, "[degRe]", "Cel", false, 100.0),
        ] {
            near(convert(v, a, b, delta).unwrap(), expected);
        }
        assert_eq!(
            convert(1.0, "m", "cm", true).unwrap_err().status,
            "invalid_delta_unit"
        );
    }
    #[test]
    fn rejects_invalid_and_unsupported() {
        for unit in [
            "", "kg/m^3", "kWh", "watts", " kg", "m s", "m//s", "µm", "m\0",
        ] {
            assert!(parse(unit).is_err(), "accepted {unit:?}");
        }
        for unit in ["Cel/s", "Cel2", "Cel/Cel", "dB", "[pH]", "[IU]"] {
            assert!(parse(unit).is_err(), "accepted {unit}");
        }
        assert_eq!(parse("madeup").unwrap_err().status, "unknown_unit");
        assert_eq!(
            convert(1.0, "kg", "m", false).unwrap_err().status,
            "incompatible_dimension"
        );
    }
    #[test]
    fn numeric_and_resource_limits() {
        assert!(parse("m120.m120").is_err());
        assert!(parse("(m100)100").is_err());
        assert!(parse("10*1000").is_err());
        assert!(parse("10*-1000").is_err());
        assert!(parse(&format!("{}m{}", "(".repeat(150), ")".repeat(150))).is_err());
        assert_eq!(
            parse(&"m".repeat(MAX_UNIT_BYTES + 1)).unwrap_err().status,
            "resource_limit"
        );
        for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                convert(n, "m", "m", false).unwrap_err().status,
                "non_finite_input"
            );
        }
        assert_eq!(
            convert(f64::MAX, "km", "m", false).unwrap_err().status,
            "overflow"
        );
        assert_eq!(
            convert(f64::from_bits(1), "m", "km", false)
                .unwrap_err()
                .status,
            "overflow"
        );
    }
    #[test]
    fn extreme_ratios_keep_significant_digits() {
        for (value, from, to, expected) in [
            (1e308, "4.10*-300.m", "10*24.m", 4e-16),
            (1e-300, "10*300.m", "10*-300.m", 1e300),
            (1e300, "10*-300.m", "10*300.m", 1e-300),
        ] {
            let actual = convert(value, from, to, false).unwrap();
            assert!(
                (actual / expected - 1.0).abs() < 1e-12,
                "{actual} != {expected}"
            );
        }
        assert_eq!(convert(0.0, "10*300.m", "10*-300.m", false).unwrap(), 0.0);
    }

    #[test]
    fn roundtrips() {
        for (a, b) in [
            ("kW.h", "J"),
            ("Cel", "[degF]"),
            ("kg/m3", "g/L"),
            ("km/h", "m/s"),
        ] {
            for value in [-100.0, -1.0, 0.0, 0.001, 1.0, 1e6] {
                near(
                    convert(convert(value, a, b, false).unwrap(), b, a, false).unwrap(),
                    value,
                );
            }
        }
    }
}
