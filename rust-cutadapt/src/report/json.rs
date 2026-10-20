//! cutadapt 4.6 `--json` report.
//!
//! This is a port of `cutadapt/cli.py` (`json_report`) and
//! `cutadapt/report.py` (`Statistics.as_json`,
//! `Statistics._adapter_statistics_as_json`) together with
//! `cutadapt/json.py` (`dumps` + `OneLine`).
//!
//! # Schema
//!
//! ```text
//! tag, schema_version, cutadapt_version, python_version,
//! command_line_arguments, cores, input{path1,path2,paired},
//! read_counts{...}, basepair_counts{...},
//! adapters_read1[], adapters_read2, poly_a_trimmed_read1, poly_a_trimmed_read2
//! ```
//!
//! # Null semantics
//!
//! cutadapt leaves a counter `null` when the corresponding option was not used
//! (for example `filtered.too_short` without `-m`), and writes `0` when the
//! option was used but nothing matched. [`crate::report::stats`] models that with
//! `Option`, and [`JsonValue::Null`] is emitted for `None`.
//!
//! # Layout
//!
//! cutadapt indents the document with 2 spaces per level, but marks selected
//! nodes `OneLine` so that they are emitted without line breaks:
//!
//! * `schema_version`, which is `[0, 3]` on one line;
//! * each `error_lengths` list;
//! * each `trimmed_lengths` entry (`{"len": ..., "expect": ..., "counts": [...]}`);
//! * each `poly_a_trimmed_readN` entry.
//!
//! The document ends with a single `"\n"` (cutadapt writes
//! `f.write(json_dumps(d)); f.write("\n")`).

use crate::report::fmt::{json_float, json_string};
use crate::report::stats::{AdapterStats, EndStats, Stats};

/// Schema version emitted by cutadapt 4.6 (`OneLine([0, 3])`).
pub const SCHEMA_VERSION: [u64; 2] = [0, 3];

/// Metadata that cutadapt reads from the environment rather than from the run
/// statistics.
///
/// The differential harness ignores these fields, so a Rust implementation
/// cannot (and should not) claim cutadapt's values; they are explicit inputs so
/// the JSON structure stays complete and testable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// `"Cutadapt report"` in cutadapt; overridable.
    pub tag: String,
    /// cutadapt's `__version__`.
    pub cutadapt_version: String,
    /// Python's `platform.python_version()`.
    pub python_version: String,
    /// `sys.argv[1:]`.
    pub command_line_arguments: Vec<String>,
    /// Number of cores cutadapt reports.
    pub cores: u64,
    /// Input path 1 (`inpaths.paths[0]`).
    pub path1: String,
    /// Input path 2 for paired-end input.
    pub path2: Option<String>,
}

impl Meta {
    /// Metadata with cutadapt 4.6's defaults for the fields that are constant
    /// across runs.
    pub fn new(path1: impl Into<String>, command_line_arguments: Vec<String>) -> Self {
        Self {
            tag: "Cutadapt report".to_string(),
            cutadapt_version: "4.6".to_string(),
            python_version: String::new(),
            command_line_arguments,
            cores: 1,
            path1: path1.into(),
            path2: None,
        }
    }
}

/// A JSON value.
///
/// Only the subset cutadapt's report needs is modelled. `Object` keeps
/// insertion order because the report's field order is part of its output.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    /// A float, formatted like Python's `json.dumps`.
    Float(f64),
    /// A string, escaped like Python's `json.dumps`.
    String(String),
    /// An ordered list.
    Array(Vec<JsonValue>),
    /// An ordered map.
    Object(Vec<(String, JsonValue)>),
    /// A value that cutadapt wraps in `OneLine`: rendered without line breaks.
    OneLine(Box<JsonValue>),
}

impl JsonValue {
    /// Mark this value as `OneLine`.
    pub fn one_line(self) -> Self {
        JsonValue::OneLine(Box::new(self))
    }

    /// `Some(&value)` for an object key, if this is an object.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Whether this is `Null` (possibly wrapped in `OneLine`).
    pub fn is_null(&self) -> bool {
        match self {
            JsonValue::Null => true,
            JsonValue::OneLine(inner) => inner.is_null(),
            _ => false,
        }
    }

    /// Render exactly like `cutadapt.json.dumps(value)`.
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        write_value(self, 0, &mut out);
        out
    }
}

fn indent(level: usize) -> String {
    "  ".repeat(level)
}

fn write_value(value: &JsonValue, level: usize, out: &mut String) {
    write_value_with(value, level, out, false);
}

fn write_value_with(value: &JsonValue, level: usize, out: &mut String, one_line: bool) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(true) => out.push_str("true"),
        JsonValue::Bool(false) => out.push_str("false"),
        JsonValue::Int(v) => out.push_str(&v.to_string()),
        JsonValue::UInt(v) => out.push_str(&v.to_string()),
        JsonValue::Float(v) => out.push_str(&json_float(*v)),
        JsonValue::String(s) => out.push_str(&json_string(s)),
        // cutadapt's `dumps` returns `json.dumps(value)` directly for OneLine
        // nodes, so no line break is inserted between the key and the value.
        JsonValue::OneLine(inner) => match inner.as_ref() {
            JsonValue::Array(items) => write_array(items, level, out, true),
            JsonValue::Object(fields) => write_object(fields, level, out, true),
            other => write_value_with(other, level, out, one_line),
        },
        JsonValue::Array(items) => write_array(items, level, out, one_line),
        JsonValue::Object(fields) => write_object(fields, level, out, one_line),
    }
}

fn write_array(items: &[JsonValue], level: usize, out: &mut String, one_line: bool) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    let inner_level = if one_line { level } else { level + 1 };
    let start = if one_line {
        String::new()
    } else {
        format!("\n{}", indent(inner_level))
    };
    let sep = if one_line {
        ", ".to_string()
    } else {
        format!(",{start}")
    };
    out.push('[');
    out.push_str(&start);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        write_value_with(item, inner_level, out, one_line);
    }
    if !one_line {
        out.push_str(&format!("\n{}", indent(level)));
    }
    out.push(']');
}

fn write_object(fields: &[(String, JsonValue)], level: usize, out: &mut String, one_line: bool) {
    if fields.is_empty() {
        out.push_str("{}");
        return;
    }
    let inner_level = if one_line { level } else { level + 1 };
    let start = if one_line {
        String::new()
    } else {
        format!("\n{}", indent(inner_level))
    };
    let sep = if one_line {
        ", ".to_string()
    } else {
        format!(",{start}")
    };
    out.push('{');
    out.push_str(&start);
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        out.push_str(&json_string(key));
        out.push_str(": ");
        // Everything nested inside a OneLine node stays on one line, matching
        // cutadapt's json.dumps fallback for the node's children.
        write_value_with(value, inner_level, out, one_line);
    }
    if !one_line {
        out.push_str(&format!("\n{}", indent(level)));
    }
    out.push('}');
}

fn opt_u64(value: Option<u64>) -> JsonValue {
    value.map_or(JsonValue::Null, JsonValue::UInt)
}

fn obj(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// Build the complete JSON report document (without the trailing newline).
pub fn report_value(stats: &Stats, meta: &Meta) -> JsonValue {
    let filtered = obj(vec![
        ("too_short", opt_u64(stats.filtered.too_short)),
        ("too_long", opt_u64(stats.filtered.too_long)),
        ("too_many_n", opt_u64(stats.filtered.too_many_n)),
        (
            "too_many_expected_errors",
            opt_u64(stats.filtered.too_many_expected_errors),
        ),
        ("casava_filtered", opt_u64(stats.filtered.casava_filtered)),
        ("discard_trimmed", opt_u64(stats.filtered.discard_trimmed)),
        (
            "discard_untrimmed",
            opt_u64(stats.filtered.discard_untrimmed),
        ),
    ]);

    let read_counts = obj(vec![
        ("input", JsonValue::UInt(stats.n)),
        ("filtered", filtered),
        ("output", JsonValue::UInt(stats.written)),
        ("reverse_complemented", opt_u64(stats.reverse_complemented)),
        ("read1_with_adapter", opt_u64(stats.with_adapters1)),
        (
            "read2_with_adapter",
            if stats.paired {
                opt_u64(stats.with_adapters2)
            } else {
                JsonValue::Null
            },
        ),
    ]);

    let basepair_counts = obj(vec![
        ("input", JsonValue::UInt(stats.total_bp())),
        ("input_read1", JsonValue::UInt(stats.total_bp1)),
        (
            "input_read2",
            if stats.paired {
                opt_u64(stats.total_bp2)
            } else {
                JsonValue::Null
            },
        ),
        ("quality_trimmed", opt_u64(stats.quality_trimmed())),
        ("quality_trimmed_read1", opt_u64(stats.quality_trimmed_bp1)),
        ("quality_trimmed_read2", opt_u64(stats.quality_trimmed_bp2)),
        ("poly_a_trimmed", opt_u64(stats.poly_a_trimmed())),
        ("poly_a_trimmed_read1", opt_u64(stats.poly_a_trimmed_bp1)),
        ("poly_a_trimmed_read2", opt_u64(stats.poly_a_trimmed_bp2)),
        ("output", JsonValue::UInt(stats.total_written_bp())),
        ("output_read1", JsonValue::UInt(stats.written_bp1)),
        (
            "output_read2",
            if stats.paired {
                opt_u64(stats.written_bp2)
            } else {
                JsonValue::Null
            },
        ),
    ]);

    let adapters_read1 = JsonValue::Array(
        stats
            .adapters1
            .iter()
            .map(|a| adapter_value(a, stats.n, stats.gc_content))
            .collect(),
    );
    let adapters_read2 = if stats.paired {
        JsonValue::Array(
            stats
                .adapters2
                .iter()
                .map(|a| adapter_value(a, stats.n, stats.gc_content))
                .collect(),
        )
    } else {
        JsonValue::Null
    };

    obj(vec![
        ("tag", JsonValue::String(meta.tag.clone())),
        (
            "schema_version",
            JsonValue::Array(SCHEMA_VERSION.iter().map(|v| JsonValue::UInt(*v)).collect())
                .one_line(),
        ),
        (
            "cutadapt_version",
            JsonValue::String(meta.cutadapt_version.clone()),
        ),
        (
            "python_version",
            JsonValue::String(meta.python_version.clone()),
        ),
        (
            "command_line_arguments",
            JsonValue::Array(
                meta.command_line_arguments
                    .iter()
                    .map(|a| JsonValue::String(a.clone()))
                    .collect(),
            ),
        ),
        ("cores", JsonValue::UInt(meta.cores)),
        (
            "input",
            obj(vec![
                ("path1", JsonValue::String(meta.path1.clone())),
                (
                    "path2",
                    meta.path2
                        .clone()
                        .map_or(JsonValue::Null, JsonValue::String),
                ),
                ("paired", JsonValue::Bool(stats.paired)),
            ]),
        ),
        ("read_counts", read_counts),
        ("basepair_counts", basepair_counts),
        ("adapters_read1", adapters_read1),
        ("adapters_read2", adapters_read2),
        (
            "poly_a_trimmed_read1",
            poly_a_value(stats.poly_a_lengths1.as_deref()),
        ),
        (
            "poly_a_trimmed_read2",
            poly_a_value(stats.poly_a_lengths2.as_deref()),
        ),
    ])
}

/// Render the complete JSON report, including cutadapt's trailing newline.
pub fn to_json(stats: &Stats, meta: &Meta) -> String {
    let mut out = report_value(stats, meta).to_json_string();
    out.push('\n');
    out
}

fn poly_a_value(lengths: Option<&[crate::report::stats::PolyALength]>) -> JsonValue {
    match lengths {
        None => JsonValue::Null,
        Some(entries) => JsonValue::Array(
            entries
                .iter()
                .map(|entry| {
                    obj(vec![
                        ("len", JsonValue::UInt(entry.length as u64)),
                        ("count", JsonValue::UInt(entry.count)),
                    ])
                    .one_line()
                })
                .collect(),
        ),
    }
}

fn adapter_value(adapter: &AdapterStats, n: u64, gc_content: f64) -> JsonValue {
    let on_reverse_complement = if adapter.reverse_complemented.is_some() {
        opt_u64(adapter.reverse_complemented)
    } else {
        JsonValue::Null
    };
    obj(vec![
        ("name", JsonValue::String(adapter.name.clone())),
        ("total_matches", JsonValue::UInt(adapter.total_matches())),
        ("on_reverse_complement", on_reverse_complement),
        ("linked", JsonValue::Bool(adapter.linked)),
        (
            "five_prime_end",
            end_value(adapter.front.as_ref(), n, gc_content),
        ),
        (
            "three_prime_end",
            end_value(adapter.back.as_ref(), n, gc_content),
        ),
    ])
}

fn end_value(end: Option<&EndStats>, n: u64, gc_content: f64) -> JsonValue {
    let Some(end) = end else {
        return JsonValue::Null;
    };
    let error_lengths = match end.error_lengths() {
        Some(lengths) => JsonValue::Array(
            lengths
                .into_iter()
                .map(|l| JsonValue::UInt(l as u64))
                .collect(),
        )
        .one_line(),
        None => JsonValue::Null,
    };
    let adjacent_bases = match &end.adjacent_bases {
        Some(bases) if bases.total() > 0 => obj(vec![
            ("A", JsonValue::UInt(bases.a)),
            ("C", JsonValue::UInt(bases.c)),
            ("G", JsonValue::UInt(bases.g)),
            ("T", JsonValue::UInt(bases.t)),
            ("", JsonValue::UInt(bases.none)),
        ]),
        _ => JsonValue::Null,
    };
    let dominant_adjacent_base = match &end.adjacent_bases {
        Some(bases) => bases
            .warnbase()
            .map_or(JsonValue::Null, |b| JsonValue::String(b.to_string())),
        None => JsonValue::Null,
    };
    let probabilities = crate::report::text::random_match_probabilities(&end.sequence, gc_content);
    let trimmed_lengths = JsonValue::Array(
        end.rows
            .iter()
            .map(|row| {
                let index = row.length.min(end.sequence.len());
                let expect = n as f64 * probabilities[index];
                obj(vec![
                    ("len", JsonValue::UInt(row.length as u64)),
                    (
                        "expect",
                        JsonValue::Float(crate::report::fmt::round1(expect)),
                    ),
                    (
                        "counts",
                        JsonValue::Array(
                            row.error_counts
                                .iter()
                                .map(|c| JsonValue::UInt(*c))
                                .collect(),
                        ),
                    ),
                ])
                .one_line()
            })
            .collect(),
    );
    obj(vec![
        ("type", JsonValue::String(end.adapter_type.clone())),
        ("sequence", JsonValue::String(end.sequence.clone())),
        ("error_rate", JsonValue::Float(end.error_rate)),
        ("indels", JsonValue::Bool(end.indels)),
        ("error_lengths", error_lengths),
        ("matches", JsonValue::UInt(end.total())),
        ("adjacent_bases", adjacent_bases),
        ("dominant_adjacent_base", dominant_adjacent_base),
        ("trimmed_lengths", trimmed_lengths),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::stats::{AdjacentBases, FilterCounts, HistogramRow, PolyALength};

    fn end_3prime() -> EndStats {
        EndStats {
            adapter_type: "regular_three_prime".to_string(),
            sequence: "AGATCGGAAGAGC".to_string(),
            error_rate: 0.1,
            indels: true,
            allows_partial_matches: true,
            effective_length: 13,
            minimum_overlap: 3,
            adjacent_bases: Some(AdjacentBases {
                a: 2,
                c: 1,
                g: 0,
                t: 3,
                none: 1,
            }),
            rows: vec![
                HistogramRow {
                    length: 6,
                    count: 1,
                    expect: 0.0,
                    max_err: 0,
                    error_counts: vec![1],
                },
                HistogramRow {
                    length: 13,
                    count: 4,
                    expect: 0.0,
                    max_err: 1,
                    error_counts: vec![3, 1],
                },
            ],
            five_prime_overlaps: None,
            five_prime_rows: Vec::new(),
            matches_total: None,
            three_prime_matches: None,
        }
    }

    fn stats_plain() -> Stats {
        Stats {
            n: 10,
            total_bp1: 460,
            with_adapters1: Some(7),
            written: 10,
            written_bp1: 326,
            adapters1: vec![AdapterStats {
                name: "1".to_string(),
                linked: false,
                reverse_complemented: None,
                front: None,
                back: Some(end_3prime()),
            }],
            ..Default::default()
        }
    }

    fn meta() -> Meta {
        Meta::new(
            "in.fastq",
            vec!["-a".to_string(), "AGATCGGAAGAGC".to_string()],
        )
    }

    /// The exact `--json` document for
    /// `cutadapt -a AGATCGGAAGAGC -o out.fastq in.fastq --json out.json`
    /// (verified against the oracle; volatile fields are dropped by the
    /// harness, so they are shown with the values this implementation emits).
    #[test]
    fn json_matches_oracle_structure_for_plain_invocation() {
        let expected = r#"{
  "tag": "Cutadapt report",
  "schema_version": [0, 3],
  "cutadapt_version": "4.6",
  "python_version": "",
  "command_line_arguments": [
    "-a",
    "AGATCGGAAGAGC"
  ],
  "cores": 1,
  "input": {
    "path1": "in.fastq",
    "path2": null,
    "paired": false
  },
  "read_counts": {
    "input": 10,
    "filtered": {
      "too_short": null,
      "too_long": null,
      "too_many_n": null,
      "too_many_expected_errors": null,
      "casava_filtered": null,
      "discard_trimmed": null,
      "discard_untrimmed": null
    },
    "output": 10,
    "reverse_complemented": null,
    "read1_with_adapter": 7,
    "read2_with_adapter": null
  },
  "basepair_counts": {
    "input": 460,
    "input_read1": 460,
    "input_read2": null,
    "quality_trimmed": null,
    "quality_trimmed_read1": null,
    "quality_trimmed_read2": null,
    "poly_a_trimmed": null,
    "poly_a_trimmed_read1": null,
    "poly_a_trimmed_read2": null,
    "output": 326,
    "output_read1": 326,
    "output_read2": null
  },
  "adapters_read1": [
    {
      "name": "1",
      "total_matches": 5,
      "on_reverse_complement": null,
      "linked": false,
      "five_prime_end": null,
      "three_prime_end": {
        "type": "regular_three_prime",
        "sequence": "AGATCGGAAGAGC",
        "error_rate": 0.1,
        "indels": true,
        "error_lengths": [9, 13],
        "matches": 5,
        "adjacent_bases": {
          "A": 2,
          "C": 1,
          "G": 0,
          "T": 3,
          "": 1
        },
        "dominant_adjacent_base": null,
        "trimmed_lengths": [
          {"len": 6, "expect": 0.0, "counts": [1]},
          {"len": 13, "expect": 0.0, "counts": [3, 1]}
        ]
      }
    }
  ],
  "adapters_read2": null,
  "poly_a_trimmed_read1": null,
  "poly_a_trimmed_read2": null
}
"#;
        assert_eq!(to_json(&stats_plain(), &meta()), expected);
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -m 10 \
    //     -o .rp/c.fastq --json .rp/c.json src/tests/fixtures/simple.fastq
    //   "filtered": {"too_short": 3, "too_long": null, ...}, "output": 7
    #[test]
    fn json_sets_filter_counter_when_option_used() {
        let stats = Stats {
            written: 7,
            written_bp1: 313,
            filtered: FilterCounts {
                too_short: Some(3),
                ..Default::default()
            },
            ..stats_plain()
        };
        let value = report_value(&stats, &meta());
        let filtered = value.get("read_counts").unwrap().get("filtered").unwrap();
        assert_eq!(filtered.get("too_short").unwrap(), &JsonValue::UInt(3));
        assert!(filtered.get("too_long").unwrap().is_null());
        assert_eq!(
            value.get("read_counts").unwrap().get("output").unwrap(),
            &JsonValue::UInt(7)
        );
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC --rc \
    //     -o .rp/x.fastq --json .rp/x.json src/tests/fixtures/simple.fastq
    //   "reverse_complemented": 1
    #[test]
    fn json_reports_reverse_complemented_when_rc_used() {
        let mut stats = stats_plain();
        stats.reverse_complemented = Some(1);
        stats.adapters1[0].reverse_complemented = Some(0);
        let value = report_value(&stats, &meta());
        assert_eq!(
            value
                .get("read_counts")
                .unwrap()
                .get("reverse_complemented")
                .unwrap(),
            &JsonValue::UInt(1)
        );
        let adapter = &value.get("adapters_read1").unwrap().clone();
        let JsonValue::Array(items) = adapter else {
            panic!("adapters_read1 must be an array");
        };
        assert_eq!(
            items[0].get("on_reverse_complement").unwrap(),
            &JsonValue::UInt(0)
        );
    }

    // $ cd .work && ./oracle-venv/bin/cutadapt -a AGATCGGAAGAGC -q 20 \
    //     -o .rp/x.fastq --json .rp/x.json src/tests/fixtures/simple.fastq
    //   "quality_trimmed": 0, "quality_trimmed_read1": 0
    #[test]
    fn json_reports_zero_quality_trimmed_when_q_used() {
        let stats = Stats {
            quality_trimmed_bp1: Some(0),
            ..stats_plain()
        };
        let value = report_value(&stats, &meta());
        let basepair = value.get("basepair_counts").unwrap();
        assert_eq!(
            basepair.get("quality_trimmed").unwrap(),
            &JsonValue::UInt(0)
        );
        assert_eq!(
            basepair.get("quality_trimmed_read1").unwrap(),
            &JsonValue::UInt(0)
        );
        assert!(basepair.get("quality_trimmed_read2").unwrap().is_null());
    }

    // $ cutadapt ... --json -> "error_lengths": [9, 13] on a single line
    #[test]
    fn error_lengths_and_trimmed_lengths_are_one_line() {
        let text = to_json(&stats_plain(), &meta());
        assert!(text.contains("\"error_lengths\": [9, 13],"), "{text}");
        assert!(
            text.contains("{\"len\": 6, \"expect\": 0.0, \"counts\": [1]},"),
            "{text}"
        );
    }

    // $ cutadapt ... --json -> "schema_version": [0, 3]
    #[test]
    fn schema_version_is_one_line() {
        assert!(to_json(&stats_plain(), &meta()).contains("\"schema_version\": [0, 3],"));
    }

    // $ cutadapt ... --json -> "indels": true (indels are allowed by default)
    #[test]
    fn indels_flag_is_serialised() {
        let text = to_json(&stats_plain(), &meta());
        assert!(text.contains("\"indels\": true,"), "{text}");
        let mut stats = stats_plain();
        stats.adapters1[0].back.as_mut().unwrap().indels = false;
        let text = to_json(&stats, &meta());
        assert!(text.contains("\"indels\": false,"), "{text}");
    }

    // $ cutadapt ... --json -> adjacent_bases uses the empty string key
    #[test]
    fn adjacent_bases_uses_empty_string_key_for_none() {
        let text = to_json(&stats_plain(), &meta());
        assert!(text.contains("\"\": 1"), "{text}");
    }

    // $ cutadapt ... --poly-a --json -> "poly_a_trimmed_read1": [{"len": 5, "count": 2}]
    #[test]
    fn poly_a_lengths_are_one_line_objects() {
        let stats = Stats {
            poly_a_trimmed_bp1: Some(10),
            poly_a_lengths1: Some(vec![
                PolyALength {
                    length: 5,
                    count: 1,
                },
                PolyALength {
                    length: 10,
                    count: 1,
                },
            ]),
            ..stats_plain()
        };
        let text = to_json(&stats, &meta());
        assert!(
            text.contains("\"poly_a_trimmed_read1\": [\n    {\"len\": 5, \"count\": 1},\n    {\"len\": 10, \"count\": 1}\n  ],"),
            "{text}"
        );
    }

    // $ cutadapt ... --json (paired-end) -> adapters_read2 is a list, path2 set
    #[test]
    fn paired_end_sets_read2_fields() {
        let mut meta = meta();
        meta.path2 = Some("in.r2.fastq".to_string());
        let stats = Stats {
            paired: true,
            total_bp2: Some(100),
            with_adapters2: Some(2),
            written_bp2: Some(80),
            adapters2: vec![AdapterStats {
                name: "1".to_string(),
                linked: false,
                reverse_complemented: None,
                front: None,
                back: Some(end_3prime()),
            }],
            ..stats_plain()
        };
        let value = report_value(&stats, &meta);
        assert_eq!(
            value.get("input").unwrap().get("path2").unwrap(),
            &JsonValue::String("in.r2.fastq".to_string())
        );
        assert_eq!(
            value.get("input").unwrap().get("paired").unwrap(),
            &JsonValue::Bool(true)
        );
        assert_eq!(
            value
                .get("read_counts")
                .unwrap()
                .get("read2_with_adapter")
                .unwrap(),
            &JsonValue::UInt(2)
        );
        assert!(matches!(
            value.get("adapters_read2").unwrap(),
            JsonValue::Array(items) if items.len() == 1
        ));
    }

    #[test]
    fn json_string_escaping_is_applied_to_names_and_paths() {
        let mut meta = meta();
        meta.path1 = "a\"b.fastq".to_string();
        let text = to_json(&stats_plain(), &meta);
        assert!(text.contains("\"path1\": \"a\\\"b.fastq\""), "{text}");
    }

    #[test]
    fn document_ends_with_exactly_one_newline() {
        let text = to_json(&stats_plain(), &meta());
        assert!(text.ends_with("}\n"));
        assert!(!text.ends_with("}\n\n"));
    }
}
