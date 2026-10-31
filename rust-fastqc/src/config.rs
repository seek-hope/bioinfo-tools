//! FastQC 0.13.0 limits-file defaults, overrides and report switches.
use std::collections::BTreeSet;
use std::path::Path;

use crate::modules::Thresholds;

/// Module enablement and verdict thresholds for one invocation.
#[derive(Clone, Debug)]
pub struct FastqcConfig {
    /// Thresholds used by rendered module verdicts.
    pub thresholds: Thresholds,
    /// Modules suppressed by a positive `ignore` value.
    pub ignored: BTreeSet<String>,
    /// Recoverable limits diagnostics, in file order.
    pub diagnostics: Vec<String>,
}

impl FastqcConfig {
    /// Load a custom limits file, or the bundled defaults.
    ///
    /// Custom files overlay Java's hard-coded defaults, including its reversed
    /// quality_sequence defaults (warn=20, error=27). The bundled limits file
    /// overrides these to warn=27/error=20 and disables Kmer Content.
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        match path {
            Some(path) => Self::parse(
                &std::fs::read_to_string(path)
                    .map_err(|error| format!("{}: {error}", path.display()))?,
            ),
            None => Ok(Self::default()),
        }
    }

    /// Parse overrides. Unknown keys are inert, as in FastQC. Invalid levels and
    /// numeric values emit the upstream diagnostic and leave defaults intact.
    /// Too few fields fail before processing: Java aborts configuration loading
    /// and silently runs with only the overrides preceding the broken row.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut config = Self {
            thresholds: Thresholds {
                quality_sequence_warn: 20.0,
                quality_sequence_fail: 27.0,
                ..Thresholds::default()
            },
            ignored: BTreeSet::new(),
            diagnostics: Vec::new(),
        };
        for (number, line) in text.split(['\r', '\n']).enumerate() {
            if line.starts_with('#') || line.trim_matches(|c| c <= '\u{20}').is_empty() {
                continue;
            }
            // Java split("\\s+") preserves a leading empty field and removes
            // trailing ones. Its default regex whitespace is ASCII only.
            let mut fields: Vec<_> = line
                .split([' ', '\t', '\u{b}', '\u{c}'])
                .filter(|s| !s.is_empty())
                .collect();
            if line.starts_with([' ', '\t', '\u{b}', '\u{c}']) {
                fields.insert(0, "");
            }
            if fields.len() < 2 {
                return Err(format!(
                    "limits line {} must contain three fields: module, level and value",
                    number + 1
                ));
            }
            if fields.len() != 3 {
                config.diagnostics.push(format!(
                    "Config line '{line}' didn't contain the 3 required sections"
                ));
            }
            let level = fields[1];
            if !matches!(level, "warn" | "error" | "ignore") {
                config.diagnostics.push(format!(
                    "Second config field must be error, warn or ignore, not '{level}'"
                ));
                continue;
            }
            if fields.len() < 3 {
                return Err(format!(
                    "limits line {} must contain three fields: module, level and value",
                    number + 1
                ));
            }
            let Some(value) = java_number(fields[2]) else {
                config
                    .diagnostics
                    .push(format!("Value {} didn't look like a number", fields[2]));
                continue;
            };
            if level == "ignore" {
                if value > 0.0 {
                    config.ignored.insert(fields[0].into());
                } else {
                    config.ignored.remove(fields[0]);
                }
                continue;
            }
            let target = match (fields[0], level) {
                ("duplication", "warn") => &mut config.thresholds.duplication_warn,
                ("duplication", "error") => &mut config.thresholds.duplication_fail,
                ("kmer", "warn") => &mut config.thresholds.kmer_warn,
                ("kmer", "error") => &mut config.thresholds.kmer_fail,
                ("n_content", "warn") => &mut config.thresholds.n_warn,
                ("n_content", "error") => &mut config.thresholds.n_fail,
                ("overrepresented", "warn") => &mut config.thresholds.overrepresented_warn,
                ("overrepresented", "error") => &mut config.thresholds.overrepresented_fail,
                ("quality_base_lower", "warn") => &mut config.thresholds.quality_lower_warn,
                ("quality_base_lower", "error") => &mut config.thresholds.quality_lower_fail,
                ("quality_base_median", "warn") => &mut config.thresholds.quality_median_warn,
                ("quality_base_median", "error") => &mut config.thresholds.quality_median_fail,
                ("sequence", "warn") => &mut config.thresholds.sequence_warn,
                ("sequence", "error") => &mut config.thresholds.sequence_fail,
                ("gc_sequence", "warn") => &mut config.thresholds.gc_sequence_warn,
                ("gc_sequence", "error") => &mut config.thresholds.gc_sequence_fail,
                ("quality_sequence", "warn") => &mut config.thresholds.quality_sequence_warn,
                ("quality_sequence", "error") => &mut config.thresholds.quality_sequence_fail,
                ("tile", "warn") => &mut config.thresholds.tile_warn,
                ("tile", "error") => &mut config.thresholds.tile_fail,
                ("adapter", "warn") => &mut config.thresholds.adapter_warn,
                ("adapter", "error") => &mut config.thresholds.adapter_fail,
                ("sequence_length", "warn") => &mut config.thresholds.length_warn,
                ("sequence_length", "error") => &mut config.thresholds.length_fail,
                _ => continue,
            };
            *target = value;
        }
        Ok(config)
    }

    /// Whether the report omits a module.
    pub fn is_ignored(&self, module: &str) -> bool {
        self.ignored.contains(module)
    }
}

impl Default for FastqcConfig {
    fn default() -> Self {
        Self {
            thresholds: Thresholds::default(),
            ignored: BTreeSet::from(["kmer".into()]),
            diagnostics: Vec::new(),
        }
    }
}

// Java accepts signed NaN/Infinity, decimal exponents, hex binary exponents and
// optional floating-point suffixes. Rust additionally accepts inf/lowercase nan;
// these must be rejected to preserve the upstream diagnostics.
fn java_number(text: &str) -> Option<f64> {
    let text = text.trim_matches(|c| c <= '\u{20}');
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if matches!(unsigned, "NaN" | "Infinity") {
        return match unsigned {
            "NaN" => Some(f64::NAN),
            _ => Some(if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }),
        };
    }
    let numeric = text.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(text);
    let unsigned = numeric.strip_prefix(['+', '-']).unwrap_or(numeric);
    if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        let value = hex_number(hex)?;
        return Some(if numeric.starts_with('-') {
            -value
        } else {
            value
        });
    }
    if !numeric
        .bytes()
        .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
    {
        return None;
    }
    numeric.parse().ok()
}

// Round hexadecimal literals directly to IEEE binary64, including subnormals.
// No intermediate floating multiplication may overflow before exponent scaling.
fn hex_number(hex: &str) -> Option<f64> {
    let (mantissa, exponent) = hex.split_once(['p', 'P'])?;
    let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut exp = digits.bytes().fold(0i64, |n, b| {
        n.saturating_mul(10).saturating_add(i64::from(b - b'0'))
    });
    if exponent.starts_with('-') {
        exp = -exp;
    }
    let mut bits = Vec::new();
    let mut fraction = false;
    let mut fractional_bits = 0i64;
    for c in mantissa.chars() {
        if c == '.' && !fraction {
            fraction = true;
            continue;
        }
        let digit = c.to_digit(16)?;
        for shift in (0..4).rev() {
            bits.push(((digit >> shift) & 1) as u64);
        }
        if fraction {
            fractional_bits = fractional_bits.saturating_add(4);
        }
    }
    if bits.is_empty() {
        return None;
    }
    let Some(first) = bits.iter().position(|b| *b != 0) else {
        return Some(0.0);
    };
    let bits = &bits[first..];
    let mut highest = exp
        .saturating_sub(fractional_bits)
        .saturating_add(bits.len() as i64 - 1);
    if highest > 1023 {
        return Some(f64::INFINITY);
    }
    if highest < -1075 {
        return Some(0.0);
    }
    let keep = if highest >= -1022 {
        53
    } else {
        (highest + 1075) as usize
    };
    let mut significant = (0..keep).fold(0u64, |n, i| (n << 1) | bits.get(i).copied().unwrap_or(0));
    let round = bits.get(keep) == Some(&1);
    let sticky = bits.get(keep + 1..).is_some_and(|tail| tail.contains(&1));
    if round && (sticky || significant & 1 != 0) {
        significant += 1;
    }
    if highest < -1022 {
        return Some(f64::from_bits(significant));
    }
    if significant == 1 << 53 {
        significant >>= 1;
        highest += 1;
    }
    if highest > 1023 {
        return Some(f64::INFINITY);
    }
    Some(f64::from_bits(
        ((highest + 1023) as u64) << 52 | (significant - (1 << 52)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_custom_files_use_distinct_upstream_defaults() {
        let bundled = FastqcConfig::default();
        let custom = FastqcConfig::parse("").unwrap();
        assert!(bundled.is_ignored("kmer"));
        assert!(!custom.is_ignored("kmer"));
        assert_eq!(bundled.thresholds.quality_sequence_warn, 27.0);
        assert_eq!(custom.thresholds.quality_sequence_warn, 20.0);
        assert_eq!(custom.thresholds.quality_sequence_fail, 27.0);
    }

    #[test]
    fn malformed_limits_fail_before_later_overrides_are_ignored() {
        assert!(FastqcConfig::parse("kmer ignore\nsequence ignore 1\n")
            .unwrap_err()
            .contains("three fields"));
    }

    #[test]
    fn limits_follow_java_number_syntax_and_last_assignment() {
        let c = FastqcConfig::parse("tile error 0x1.8p3\rsequence ignore 1\rsequence ignore -1\rn_content warn 2.5f\nkmer ignore NaN\n").unwrap();
        assert_eq!(c.thresholds.tile_fail, 12.0);
        assert_eq!(c.thresholds.n_warn, 2.5);
        assert!(!c.is_ignored("sequence"));
        assert!(!c.is_ignored("kmer"));
        assert!(java_number("inf").is_none());
        assert!(java_number("nan").is_none());
        assert_eq!(java_number("0x1p-1074"), Some(f64::from_bits(1)));
        assert_eq!(java_number("0x0p2048"), Some(0.0));
        assert_eq!(java_number("0x1p-1075"), Some(0.0));
        assert_eq!(java_number("0x1.00001p-1075"), Some(f64::from_bits(1)));
        assert_eq!(java_number("0x1.fffffffffffffp1023"), Some(f64::MAX));
        let large = format!("0x1{}p-2048", "0".repeat(512));
        assert_eq!(java_number(&large), Some(1.0));
    }
}
