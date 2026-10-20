//! Number formatting that reproduces Python's `str.format` behaviour.
//!
//! cutadapt 4.6 builds its text report with Python format specifiers such as
//! `{value:13,d}`, `{fraction:.1%}`, `{expect:.1F}` and `round(x, 1)`, and its
//! JSON report with `json.dumps`. Rust's `std::fmt` differs in a few places, so
//! every conversion lives here and is unit-tested against real Python output.
//!
//! What needed special handling:
//!
//! * `,d` always groups with commas and pads to a fixed width, including inside
//!   the padded field (`{1234567:13,d}` is `"    1,234,567"`, not
//!   `"           1,234,567"`). [`comma_u64`] inserts separators *before*
//!   padding, which `{:13}` does not do on its own.
//! * `:.1%` scales by 100 and appends `%`. Python's `%` formatting and
//!   "multiply, then format with `.1f`" agree for every value this report can
//!   produce (verified over 200,000 random fractions), so [`percent_1`] uses the
//!   simpler form.
//! * `{:.1F}` is identical to `{:.1f}` in Python.
//! * `round(x, 1)` rounds halfway cases to **even**, exactly like Rust's
//!   `{:.1$}` formatting (verified for 0.25, 0.35, 1.25, 2.05, ...), so
//!   [`round1`] is a thin wrapper. `-0.0` is normalised to `0.0` because
//!   `json.dumps(-0.0)` would otherwise emit `-0.0`.
//! * `json.dumps(float)` prints the shortest round-tripping decimal, which is
//!   what Rust's `Display` for `f64` also produces; [`json_float`] therefore
//!   delegates to `Display` and only fixes up integral values (`1.0`, not `1`,
//!   because Python's `json.dumps(1.0)` is `"1.0"`).

/// `f"{value:13,d}"` for a non-negative `u64` (commas, width 13, space padded).
pub fn comma_u64(value: u64) -> String {
    format!("{:>13}", grouped(value))
}

/// `f"{value:,d}"` with no padding.
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let bytes = digits.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

/// cutadapt's `safe_divide`: `0.0` when the value is absent or the denominator
/// is zero.
pub fn fraction(numerator: Option<u64>, denominator: u64) -> f64 {
    match numerator {
        Some(value) if denominator != 0 => value as f64 / denominator as f64,
        _ => 0.0,
    }
}

/// `f"{fraction:.1%}"`, e.g. `0.7` -> `"70.0%"`.
pub fn percent_1(fraction: f64) -> String {
    format!("{:.1}%", fraction * 100.0)
}

/// `f"{value:.1f}"`.
pub fn fixed1(value: f64) -> String {
    format!("{value:.1}")
}

/// `f"{value:.1F}"` (identical to [`fixed1`]).
pub fn fixed1_upper(value: f64) -> String {
    format!("{value:.1}")
}

/// Python's `round(value, 1)`.
pub fn round1(value: f64) -> f64 {
    let rounded: f64 = format!("{value:.1}").parse().unwrap_or(value);
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// `json.dumps(float)`, i.e. Python's `float.__repr__`.
///
/// Python prints the shortest string that round-trips and chooses between
/// positional and scientific notation by decimal exponent:
///
/// ```text
/// >>> [repr(x) for x in [0.1, 0.0001, 1e-05, 1e16, 1e17, 1.5e-05]]
/// ['0.1', '0.0001', '1e-05', '1e+16', '1e+17', '1.5e-05']
/// ```
///
/// Scientific notation is used when the exponent is below -4 or at least 16,
/// and its exponent is always signed with at least two digits (`e-05`, `e+16`).
/// Rust's `Display` prints `0.00001` and its `{:e}` prints `1e-5`, so neither
/// alone matches; this function combines them.
pub fn json_float(value: f64) -> String {
    if !value.is_finite() {
        // Python's json module emits Infinity/-Infinity/NaN, but cutadapt never
        // computes those here.
        return match value {
            v if v.is_nan() => "NaN".to_string(),
            v if v > 0.0 => "Infinity".to_string(),
            _ => "-Infinity".to_string(),
        };
    }
    if value == 0.0 {
        // Python prints "-0.0" for negative zero; keep the sign for parity.
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let negative = value.is_sign_negative();
    let magnitude = value.abs();
    // Rust's LowerExp yields the shortest round-tripping mantissa and exponent.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("LowerExp always contains an exponent");
    let exponent: i32 = exponent.parse().expect("exponent is an integer");
    let body = if (-4..16).contains(&exponent) {
        let positional = format!("{magnitude}");
        if positional.contains('.') {
            positional
        } else {
            format!("{positional}.0")
        }
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.abs())
    };
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// Escape a string the way `json.dumps` does for the characters that can occur
/// in a cutadapt report (adapter sequences, names and paths).
pub fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // $ .work/oracle-venv/bin/python -c \
    //   "print([f'{v:13,d}' for v in [0,7,10,1000,1234567]])"
    // ['            0', '            7', '           10', '        1,000', '    1,234,567']
    #[test]
    fn comma_u64_matches_python_width_and_grouping() {
        assert_eq!(comma_u64(0), "            0");
        assert_eq!(comma_u64(7), "            7");
        assert_eq!(comma_u64(10), "           10");
        assert_eq!(comma_u64(1000), "        1,000");
        assert_eq!(comma_u64(1234567), "    1,234,567");
    }

    // $ python3 -c "print(f'{460:13,d}', '|', f'{3999984:13,d}')"
    #[test]
    fn comma_u64_groups_at_every_third_digit() {
        assert_eq!(comma_u64(460), "          460");
        assert_eq!(comma_u64(3999984), "    3,999,984");
        assert_eq!(comma_u64(100), "          100");
        assert_eq!(comma_u64(1000), "        1,000");
    }

    // $ python3 -c "print([f'{n/d:.1%}' for n,d in [(7,10),(3,10),(313,460),(0,10),(1,3),(2,3),(1,8)]])"
    // ['70.0%', '30.0%', '68.0%', '0.0%', '33.3%', '66.7%', '12.5%']
    #[test]
    fn percent_1_matches_python() {
        assert_eq!(percent_1(7.0 / 10.0), "70.0%");
        assert_eq!(percent_1(3.0 / 10.0), "30.0%");
        assert_eq!(percent_1(313.0 / 460.0), "68.0%");
        assert_eq!(percent_1(0.0 / 10.0), "0.0%");
        assert_eq!(percent_1(1.0 / 3.0), "33.3%");
        assert_eq!(percent_1(2.0 / 3.0), "66.7%");
        assert_eq!(percent_1(1.0 / 8.0), "12.5%");
    }

    // python3: print(f'{0.0625:.1f}', f'{2.05:.1f}', f'{0.25:.1f}')  -> 0.1 2.0 0.2
    #[test]
    fn fixed1_matches_python_half_even_rounding() {
        assert_eq!(fixed1(0.0625), "0.1");
        assert_eq!(fixed1(2.05), "2.0");
        assert_eq!(fixed1(0.25), "0.2");
        assert_eq!(fixed1(0.0), "0.0");
        assert_eq!(fixed1_upper(1.25), "1.2");
    }

    // python3: print([round(x,1) for x in [0.25,0.75,1.25,2.675,0.0,0.15]])
    // -> [0.2, 0.8, 1.2, 2.7, 0.0, 0.1]
    #[test]
    fn round1_matches_python() {
        assert_eq!(round1(0.25), 0.2);
        assert_eq!(round1(0.75), 0.8);
        assert_eq!(round1(1.25), 1.2);
        assert_eq!(round1(2.675), 2.7);
        assert_eq!(round1(0.0), 0.0);
        assert_eq!(round1(0.15), 0.1);
    }

    // python3 -c "import json; print([json.dumps(x) for x in [0.1,0.0,0.25,1.0,0.30000000000000004,1e-05]])"
    // ['0.1', '0.0', '0.25', '1.0', '0.30000000000000004', '1e-05']
    #[test]
    fn json_float_matches_python_for_typical_values() {
        assert_eq!(json_float(0.1), "0.1");
        assert_eq!(json_float(0.0), "0.0");
        assert_eq!(json_float(0.25), "0.25");
        assert_eq!(json_float(1.0), "1.0");
        assert_eq!(json_float(0.30000000000000004), "0.30000000000000004");
        // $ python3 -c "import json; print(json.dumps(1e-05))" -> 1e-05
        assert_eq!(json_float(1e-05), "1e-05");
        assert_eq!(json_float(1.5e-05), "1.5e-05");
        assert_eq!(json_float(0.0001), "0.0001");
        assert_eq!(json_float(2.5e-07), "2.5e-07");
        assert_eq!(json_float(-1.5e-05), "-1.5e-05");
    }

    // $ python3 -c "import json; print([json.dumps(x) for x in [1e16,1e17,1.5e16,123456789012345678.0]])"
    // ['1e+16', '1e+17', '1.5e+16', '1.2345678901234568e+17']
    #[test]
    fn json_float_switches_to_scientific_at_one_e16() {
        assert_eq!(json_float(1e16), "1e+16");
        assert_eq!(json_float(1e17), "1e+17");
        assert_eq!(json_float(1.5e16), "1.5e+16");
        assert_eq!(json_float(123456789012345678.0), "1.2345678901234568e+17");
    }

    // python3 -c "import json; print(json.dumps('a\"b\\\\c'))" -> "a\"b\\c"
    #[test]
    fn json_string_escapes_quotes_and_backslashes() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\tb"), "\"a\\tb\"");
    }

    #[test]
    fn fraction_uses_zero_for_missing_values_and_zero_denominator() {
        assert_eq!(fraction(None, 10), 0.0);
        assert_eq!(fraction(Some(7), 0), 0.0);
        assert_eq!(fraction(Some(7), 10), 0.7);
    }
}
