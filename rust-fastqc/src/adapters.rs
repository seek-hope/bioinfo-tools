//! FastQC adapter definitions and its tab-separated custom-list format.

/// A named, literal adapter sequence. Read sequences are uppercased; custom
/// adapter bytes retain their case, matching FastQC's exact substring search.
#[derive(Clone, Debug)]
pub struct Adapter {
    /// Name used as the report column heading.
    pub name: String,
    /// Literal sequence to search for.
    pub sequence: Vec<u8>,
}

/// The six default adapter motifs in FastQC 0.13.0.
pub const ADAPTERS: [(&str, &str); 6] = [
    ("Illumina Universal Adapter", "AGATCGGAAGAG"),
    ("Illumina Small RNA 3' Adapter", "TGGAATTCTCGG"),
    ("Illumina Small RNA 5' Adapter", "GATCGTCGGACT"),
    ("Nextera Transposase Sequence", "CTGTCTCTTATA"),
    ("PolyA", "AAAAAAAAAAAA"),
    ("PolyG", "GGGGGGGGGGGG"),
];

/// Construct the default list without external resources.
pub fn defaults() -> Vec<Adapter> {
    ADAPTERS
        .iter()
        .map(|(name, sequence)| Adapter {
            name: (*name).into(),
            sequence: sequence.as_bytes().to_vec(),
        })
        .collect()
}

/// Parse the upstream tab-separated format and collect its row diagnostics.
///
/// Tab runs delimit fields; trailing empty fields are discarded, as with Java
/// String.split. Malformed rows are skipped, preserving valid rows and order.
/// A list with no usable adapter is rejected before processing reads; the
/// upstream crashes while building a report for that configuration.
pub fn parse(text: &str) -> Result<(Vec<Adapter>, Vec<String>), String> {
    let mut adapters = Vec::new();
    let mut warnings = Vec::new();
    // BufferedReader.readLine accepts CR, LF and CRLF. An extra empty slice
    // from CRLF is harmless; Java String.trim treats only U+0000..U+0020 as
    // blank, leaving other whitespace available for row diagnostics.
    for line in text.split(['\r', '\n']) {
        if line.starts_with('#') || line.trim_matches(|c: char| c <= '\u{20}').is_empty() {
            continue;
        }
        let mut sections = Vec::new();
        let mut rest = line;
        while let Some(tab) = rest.find('\t') {
            sections.push(&rest[..tab]);
            rest = rest[tab..].trim_start_matches('\t');
        }
        if !rest.is_empty() {
            sections.push(rest);
        }
        if sections.len() != 2 {
            warnings.push(format!(
                "Expected 2 sections for contaminant line but got {} from {line}",
                sections.len()
            ));
            continue;
        }
        adapters.push(Adapter {
            name: sections[0].into(),
            sequence: sections[1].as_bytes().to_vec(),
        });
    }
    if adapters.is_empty() {
        return Err("adapter list contains no usable name<TAB>sequence entries".into());
    }
    if adapters
        .iter()
        .any(|adapter| adapter.sequence.len() != adapters[0].sequence.len())
    {
        warnings.push("[Warning] You are using adapter sequences with different lengths. Matches will only be reported up to the position where the longest adapter could match. Matches to shorter adapters at the end of sequences will not be recorded.".into());
    }
    Ok((adapters, warnings))
}
