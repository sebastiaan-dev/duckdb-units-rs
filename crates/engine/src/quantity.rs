//! The text envelope around a UCUM quantity: a decimal number, a horizontal
//! whitespace separator, and a unit expression. Unit resolution is deliberately
//! separate so SQL execution can reuse its bounded per-unit cache.
use crate::{Result, UnitError};

pub const MAX_QUANTITY_BYTES: usize = 8192;

#[derive(Debug, PartialEq)]
pub struct QuantityParts<'a> {
    pub value: f64,
    /// Original unit spelling with envelope whitespace removed; not an alias or canonicalization.
    pub unit: &'a str,
}

fn invalid() -> UnitError {
    UnitError::new(
        "invalid_quantity",
        "expected a decimal number, a space or tab, and a UCUM unit (for example '12.5 kg/m3'); use '1' for dimensionless units",
    )
}

fn decimal_number(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut digits = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if start == i {
            return false;
        }
    }
    i == bytes.len()
}

/// Split and validate the numeric envelope, without resolving the unit. Callers
/// must also pass `unit` through the unit parser before returning a valid quantity.
/// Requiring a separator avoids interpreting `1eV` as an exponent or an alias.
pub fn split(text: &str) -> Result<QuantityParts<'_>> {
    if text.len() > MAX_QUANTITY_BYTES {
        return Err(UnitError::new(
            "resource_limit",
            "quantity strings are limited to 8192 bytes",
        ));
    }
    if text.contains(['\r', '\n']) {
        return Err(invalid());
    }
    let text = text.trim_matches([' ', '\t']);
    let separator = text.find([' ', '\t']).ok_or_else(invalid)?;
    let (number, rest) = text.split_at(separator);
    let unit = rest.trim_start_matches([' ', '\t']);
    if unit.is_empty() {
        return Err(invalid());
    }
    let unsigned = number.strip_prefix(['+', '-']).unwrap_or(number);
    if ["nan", "inf", "infinity"]
        .iter()
        .any(|token| unsigned.eq_ignore_ascii_case(token))
    {
        return Err(UnitError::new(
            "non_finite_input",
            "measurement must be finite",
        ));
    }
    if !decimal_number(number) {
        return Err(invalid());
    }
    let value = number.parse::<f64>().map_err(|_| invalid())?;
    // f64 parsing otherwise silently maps out-of-range decimal literals to
    // infinity or zero. Distinguish a genuine zero mantissa from underflow.
    let nonzero_mantissa = number
        .split(['e', 'E'])
        .next()
        .unwrap_or("")
        .bytes()
        .any(|byte| matches!(byte, b'1'..=b'9'));
    if !value.is_finite() || (value == 0.0 && nonzero_mantissa) {
        return Err(UnitError::new(
            "overflow",
            "quantity value is outside the supported DOUBLE range",
        ));
    }
    Ok(QuantityParts { value, unit })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_envelopes_and_unit_spelling() {
        for (text, value, unit) in [
            ("12.5 kg/m3", 12.5, "kg/m3"),
            ("  +.5\tkg/m3  ", 0.5, "kg/m3"),
            ("-1.25E+3 J", -1250.0, "J"),
            ("2. W.h", 2.0, "W.h"),
            ("25 %", 25.0, "%"),
            ("3 1", 3.0, "1"),
            ("1 eV", 1.0, "eV"),
            ("2 10*3.m", 2.0, "10*3.m"),
            ("5 mg{total}", 5.0, "mg{total}"),
            ("-0 K", -0.0, "K"),
            ("0e-9999 m", 0.0, "m"),
        ] {
            let parts = split(text).unwrap();
            assert_eq!(parts.value, value, "{text}");
            assert_eq!(parts.unit, unit, "{text}");
            assert!(crate::parse(parts.unit).is_ok(), "{text}");
        }
        assert!(split("-0 K").unwrap().value.is_sign_negative());
        assert_eq!(split("5e-324 m").unwrap().value, f64::from_bits(1));
    }

    #[test]
    fn rejects_ambiguous_or_locale_dependent_numbers() {
        for text in [
            "",
            " ",
            "kg",
            "12.5",
            "12.5 ",
            "12.5kg",
            "1eV",
            "1,234 m",
            "1,5 m",
            "1_000 m",
            "0x10 m",
            ". kg",
            "+ kg",
            "1e m",
            "1e+ m",
            "1e- m",
            "1.2.3 m",
            "--1 m",
            "1e2e3 m",
            "  kg m",
            "1\nm",
            "1 m\n",
            "1\rm",
            "1\u{a0}m",
            "\u{a0}1 m",
            "١ m",
            "1\0 m",
        ] {
            assert_eq!(
                split(text).unwrap_err().status,
                "invalid_quantity",
                "{text:?}"
            );
        }
    }

    #[test]
    fn nonfinite_range_and_resource_errors() {
        for token in ["NaN", "nan", "Inf", "+Infinity", "-inf", "-NaN"] {
            assert_eq!(
                split(&format!("{token} m")).unwrap_err().status,
                "non_finite_input"
            );
        }
        for token in ["1e309", "-1e999", "1e-999", "-0.0001e-999"] {
            assert_eq!(split(&format!("{token} m")).unwrap_err().status, "overflow");
        }
        assert_eq!(
            split(&" ".repeat(MAX_QUANTITY_BYTES + 1))
                .unwrap_err()
                .status,
            "resource_limit"
        );
        // Boundaries do not allocate or normalize an unbounded unit string.
        let text = format!("1 {}", "m".repeat(MAX_QUANTITY_BYTES - 2));
        assert_eq!(split(&text).unwrap().unit.len(), MAX_QUANTITY_BYTES - 2);
        assert_eq!(
            crate::parse(split(&text).unwrap().unit).unwrap_err().status,
            "resource_limit"
        );
    }
}
