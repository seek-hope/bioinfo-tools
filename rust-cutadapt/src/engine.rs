//! Per-chunk trimming: parse records, apply the adapter set, serialize output.

use crate::adapter::matcher::{AdapterSet, MatchConfig};
use crate::adapter::{AdapterSpec, Match};
use crate::fastq::Parser;

/// Outcome of processing one chunk.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkStats {
    pub records_in: u64,
    pub records_out: u64,
    pub bp_in: u64,
    pub bp_out: u64,
    /// Reads in which an adapter was removed (whatever its length).
    pub reads_with_adapter: u64,
    /// Reads discarded because they were shorter than `-m` after trimming.
    pub reads_too_short: u64,
    /// Reads discarded because they were longer than `-M`.
    pub reads_too_long: u64,
    /// Reads discarded by --max-n.
    pub reads_too_many_n: u64,
    /// Reads discarded by --max-ee.
    pub reads_too_many_expected_errors: u64,
    /// Reads discarded by --max-aer.
    pub reads_too_many_average_errors: u64,
    /// Reads discarded by --discard-casava.
    pub reads_casava_filtered: u64,
    /// Reads discarded by --discard-trimmed.
    pub reads_discard_trimmed: u64,
    /// Reads discarded by --discard-untrimmed.
    pub reads_discard_untrimmed: u64,
    /// Bases removed by quality trimming (-q/--nextseq-trim).
    pub quality_trimmed_bp: u64,
    /// Bases removed by poly-A trimming.
    pub poly_a_trimmed_bp: u64,
    /// Length of each poly-A tail that was removed (for the report table).
    pub poly_a_lengths: Vec<usize>,
    /// Reads that matched only on the reverse complement (`--rc`).
    pub reverse_complemented: u64,
    /// Per-adapter count of reverse-complement matches.
    pub adapter_reverse_complemented: Vec<u64>,
    /// Lines for --info-file.
    pub info_lines: Vec<String>,
    /// Lines for --rest-file.
    pub rest_lines: Vec<String>,
    /// Lines for --wildcard-file.
    pub wildcard_lines: Vec<String>,
    /// Reads diverted to --untrimmed-output.
    pub reads_untrimmed_output: u64,
    /// Per-adapter demultiplexed output, indexed like the adapter set. Only
    /// populated when `-o` contains `{name}`.
    pub demux_records: Vec<Vec<u8>>,
    /// Records for the demultiplexed "unknown" file (reads with no adapter).
    pub demux_unknown: Vec<u8>,
    /// Reads diverted to that file.
    pub demux_unknown_records: u64,
    /// Records for --too-short-output.
    pub too_short_records: Vec<u8>,
    /// Records for --too-long-output.
    pub too_long_records: Vec<u8>,
    /// Records for --untrimmed-output.
    pub untrimmed_records: Vec<u8>,
    /// Per-adapter trim measurements, indexed like the adapter set.
    pub adapter_stats: Vec<AdapterChunkStats>,
}

/// What one chunk contributes to one adapter's report section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdapterChunkStats {
    /// How many times this adapter was the winning match.
    pub matches: u64,
    /// `(removed length, errors)` for every winning match.
    pub trimmed: Vec<(usize, usize)>,
    /// Base immediately preceding a 3' match: `Some(byte)`, or `None` when the
    /// match starts at position 0 (cutadapt's "none/other" bucket).
    pub adjacent: Vec<Option<u8>>,
    /// Matches that overlapped the read's 5' end (used by `-b` reporting).
    pub five_prime_overlaps: u64,
    /// `(removed length, errors)` for matches overlapping the 5' end.
    pub five_prime_trimmed: Vec<(usize, usize)>,
    /// For linked adapters: how many matches removed a 3' part as well.
    pub three_prime_hits: u64,
    /// Removed length of each 5' removal, for per-end histograms.
    pub five_prime_lengths: Vec<usize>,
    /// Removed length of each 3' removal, for per-end histograms.
    pub three_prime_lengths: Vec<usize>,
}

/// Trims records from a byte chunk, appending retained records to `out`.
#[derive(Clone, Debug)]
pub struct Trimmer {
    adapters: AdapterSet,
    config: MatchConfig,
    /// Maximum number of adapters to remove per read (`-n`/`--times`).
    times: usize,
    /// Read modifications applied around adapter trimming (`-u`, `-q`, ...).
    modifications: crate::modify::Modifications,
    /// Read filters (`-m`, `-M`, `--max-n`, `--discard-*`, ...).
    filters: crate::filter::Filters,
    /// What to do with a matched adapter (`--action`).
    action: crate::modify::Action,
    /// Auxiliary outputs written alongside the main output.
    aux: crate::auxout::AuxOutputs,
    /// When set, demultiplexing uses THIS adapter index for every record instead
    /// of the adapter that matched. Paired mode uses it so R2's file follows the
    /// pair's R1 adapter assignment, which is what cutadapt does.
    demux_index_override: Option<usize>,
    /// Read-name transformations.
    renaming: crate::modify::Renaming,
    /// `--rc`: also try the read's reverse complement.
    reverse_complement: bool,
}

/// Prefix that marks a parse-channel error as a FATAL condition rather than a
/// malformed-input problem (the pipeline transports both through one channel).
pub const FATAL_MARKER: &str = "\u{1}";

impl Trimmer {
    pub fn new(adapters: AdapterSet, config: MatchConfig) -> Self {
        Self {
            adapters,
            config,
            times: 1,
            modifications: crate::modify::Modifications::default(),
            filters: crate::filter::Filters::default(),
            action: crate::modify::Action::Trim,
            aux: crate::auxout::AuxOutputs::default(),
            demux_index_override: None,
            renaming: crate::modify::Renaming::default(),
            reverse_complement: false,
        }
    }

    /// Convenience constructor for the single regular 3' adapter case.
    pub fn single(adapter: AdapterSpec, config: MatchConfig) -> Self {
        let mut set = AdapterSet::new();
        set.push(adapter, &config);
        Self::new(set, config)
    }

    pub fn with_times(mut self, times: usize) -> Self {
        self.times = times;
        self
    }

    /// Configure the read modifications applied around adapter trimming.
    pub fn with_modifications(mut self, modifications: crate::modify::Modifications) -> Self {
        self.modifications = modifications;
        self
    }

    /// Configure the read filters.
    pub fn with_filters(mut self, filters: crate::filter::Filters) -> Self {
        self.filters = filters;
        self
    }

    /// Configure what happens to a read once an adapter matched.
    pub fn with_action(mut self, action: crate::modify::Action) -> Self {
        self.action = action;
        self
    }

    /// Configure the auxiliary outputs.
    pub fn with_aux(mut self, aux: crate::auxout::AuxOutputs) -> Self {
        self.aux = aux;
        self
    }

    /// Route every record to this adapter's demultiplexed file (paired mode).
    pub fn with_demux_index_override(mut self, index: Option<usize>) -> Self {
        self.demux_index_override = index;
        self
    }

    /// Configure the read-name transformations.
    pub fn with_renaming(mut self, renaming: crate::modify::Renaming) -> Self {
        self.renaming = renaming;
        self
    }

    /// Also try the reverse complement (`--rc`).
    pub fn with_reverse_complement(mut self, enabled: bool) -> Self {
        self.reverse_complement = enabled;
        self
    }

    pub fn adapters(&self) -> &AdapterSet {
        &self.adapters
    }

    pub fn config(&self) -> &MatchConfig {
        &self.config
    }

    /// Trim one read, returning the retained range as `(from, to)`.
    ///
    /// Repeated adapter removals (`-n`) are applied on the retained slice, which
    /// is how cutadapt's multiple rounds behave.
    fn trim_range(&mut self, sequence: &[u8]) -> (usize, usize, Option<Match>) {
        let mut from = 0usize;
        let mut to = sequence.len();
        let mut first_match = None;
        for _ in 0..self.times {
            let slice = &sequence[from..to];
            match self.adapters.best_match(slice) {
                Some(m) => {
                    if first_match.is_none() {
                        first_match = Some(m);
                    }
                    from += m.removed_5prime;
                    to -= m.removed_3prime;
                    if from >= to {
                        break;
                    }
                }
                None => break,
            }
        }
        (from, to, first_match)
    }

    /// Parse `input`, trim each record, and append records that are at least
    /// `minimum_length` bases long after trimming to `out`.
    ///
    /// `out` is cleared first. `base_offset`/`base_line` locate `input` within
    /// the whole input stream so parse errors can report real coordinates.
    pub fn process_chunk(
        &mut self,
        input: &[u8],
        base_offset: u64,
        base_line: u64,
        minimum_length: usize,
        out: &mut Vec<u8>,
    ) -> Result<ChunkStats, crate::fastq::ParseError> {
        out.clear();
        let mut parser = Parser::new(input, base_offset, base_line);
        let mut stats = ChunkStats {
            adapter_stats: vec![AdapterChunkStats::default(); self.adapters.len()],
            demux_records: vec![
                Vec::new();
                self.demux_index_override
                    .map_or(self.adapters.len(), |i| (i + 1)
                        .max(self.adapters.len()))
            ],
            ..ChunkStats::default()
        };
        while let Some(record) = parser.next_record()? {
            stats.records_in += 1;
            let original = record.sequence.len();
            stats.bp_in += original as u64;
            // cutadapt's order: -u, then quality trimming, then adapter
            // trimming, then the post-trim modifications.
            let mods = self.modifications.clone();
            let (mut from, mut to) = (0usize, original);
            if !mods.is_empty() {
                (from, to) = crate::modify::cut(from, to, mods.cut_5prime, mods.cut_3prime);
                if mods.quality_5prime.is_some() || mods.quality_3prime.is_some() {
                    let before = to - from;
                    let (qf, qt) = crate::modify::quality_trim(record.qualities, from, to, &mods);
                    // Count REMOVED bases for the "Quality-trimmed" report line.
                    stats.quality_trimmed_bp += (before - (qt - qf)) as u64;
                    (from, to) = (qf, qt);
                }
                if let Some(cutoff) = mods.nextseq_trim {
                    let before = to - from;
                    let (nf, nt) = crate::modify::nextseq_trim_with_sequence(
                        record.sequence,
                        record.qualities,
                        from,
                        to,
                        cutoff,
                        mods.quality_base,
                    );
                    // Count REMOVED bases, not retained ones.
                    stats.quality_trimmed_bp += (before - (nt - nf)) as u64;
                    (from, to) = (nf, nt);
                }
            }
            // `slice_from` is where adapter matching starts within the read,
            // after -u and quality trimming but before any post-trim modifier.
            let slice_from = from;
            let slice = &record.sequence[from..to];
            let (mut trim_from, mut trim_to, mut matched) = self.trim_range(slice);
            // `--rc`: when the forward strand has no adapter, try the reverse
            // complement and, if that matches, write the reverse-complemented
            // read trimmed on that strand.
            let mut used_rc = false;
            let mut rc_sequence: Option<Vec<u8>> = None;
            if matched.is_none() && self.reverse_complement {
                let rc = crate::modify::reverse_complement(&record.sequence[from..to]);
                if let Some(m) = self.adapters.best_match(&rc) {
                    used_rc = true;
                    let rc_to = (rc.len() - m.removed_3prime).max(m.removed_5prime);
                    trim_from = m.removed_5prime;
                    trim_to = rc_to;
                    matched = Some(m);
                    rc_sequence = Some(rc);
                }
            }
            if used_rc {
                stats.reverse_complemented += 1;
                if stats.adapter_reverse_complemented.len() <= self.adapters.len() {
                    stats
                        .adapter_reverse_complemented
                        .resize(self.adapters.len(), 0);
                }
                if let Some(slot) = matched
                    .and_then(|m| stats.adapter_reverse_complemented.get_mut(m.adapter_index))
                {
                    *slot += 1;
                }
            }
            from += trim_from;
            to = from + (trim_to - trim_from);
            // Where the 3' adapter begins. -u has already shifted `from`, so
            // this is the position within the read as trimmed: with `-u 3` on
            // AAAAGATCGGAAGAGC the adapter starts at 0 and cutadapt reports the
            // preceding base as none/other. Post-trim modifiers run later, so
            // recording it now keeps their effect out of the statistic.
            let _ = slice_from;
            // Absolute position where the 3' adapter starts, in the ORIGINAL
            // read. Oracle-verified: on AAAAGATCGGAAGAGC the preceding base is A
            // with no -u and with -u 1; with -u 3 (which removes the whole
            // prefix) cutadapt reports none/other, which the `>= slice_from`
            // guard below reproduces.
            let adapter_start = to;
            if !mods.is_empty() {
                // Oracle: -l is applied AFTER adapter trimming (a 53 bp read
                // trimmed to 40 bp then `-l -20` keeps its last 20 bases).
                if let Some(length) = mods.length {
                    (from, to) = crate::modify::length(from, to, length);
                }
                if mods.poly_a {
                    let before = to - from;
                    let (pf, pt) = crate::modify::poly_a(from, to, record.sequence);
                    let removed = before - (pt - pf);
                    // cutadapt records EVERY read in the Poly-A length table,
                    // including a 0 row for reads with nothing trimmed (oracle
                    // shows "0  12" next to "12  1").
                    stats.poly_a_trimmed_bp += removed as u64;
                    stats.poly_a_lengths.push(removed);
                    (from, to) = (pf, pt);
                }
                if mods.trim_n {
                    (from, to) = crate::modify::trim_n(from, to, record.sequence);
                }
            }
            if let Some(m) = matched {
                stats.reads_with_adapter += 1;
                // `adapter_stats` is indexed LOCALLY (R2 adapters carry a global index for
                // the report headings but their statistics start at 0), so map the
                // match index through the set's first adapter index.
                let local_index = m.adapter_index.saturating_sub(self.adapters.first_index());
                if let Some(entry) = stats.adapter_stats.get_mut(local_index) {
                    entry.matches += 1;
                    // Record each end separately: a linked match removes a 5'
                    // part and a 3' part with different lengths, and cutadapt
                    // histograms them independently.
                    if m.removed_5prime > 0 {
                        entry.five_prime_lengths.push(m.removed_5prime);
                    }
                    if m.removed_3prime > 0 {
                        entry.three_prime_lengths.push(m.removed_3prime);
                    }
                    if m.removed_3prime > 0 {
                        entry.three_prime_hits += 1;
                    }
                    if m.removed_5prime > 0 {
                        entry.five_prime_overlaps += 1;
                        entry.five_prime_trimmed.push((m.removed_5prime, m.errors));
                    }
                    // The histogram length is the number of bases the ADAPTER
                    // removed, measured from the adapter's own match. Using the
                    // post-modification `to` wrongly folds in -u/-l/--trim-n
                    // removals (oracle: -u 5 changes 13 to 6, not to 11).
                    let removed = m.removed_5prime + m.removed_3prime;
                    entry.trimmed.push((removed, m.errors));
                    // cutadapt records the base immediately preceding the
                    // removed 3' adapter. The adapter starts at index `to`, so
                    // that base is at `to - 1`; when the adapter starts at
                    // position 0 there is none and cutadapt counts it in the
                    // "none/other" bucket.
                    if m.removed_3prime > 0 {
                        entry.adjacent.push(if adapter_start > slice_from {
                            Some(record.sequence[adapter_start - 1])
                        } else {
                            None
                        });
                    }
                }
            }
            let fate = crate::filter::classify(
                record.header,
                &record.sequence[from..to],
                &record.qualities[from..to],
                matched.is_some(),
                &self.filters,
            );
            // Auxiliary per-read files. They are written regardless of the
            // read's fate, exactly like cutadapt (the info-file covers every
            // input read, the rest-file every read with a 3' match).
            if self.aux.info_file.is_some() {
                // The adapter's coordinates in the read: it starts where the
                // retained 3' region ends and spans `adapter_bases` bases.
                let matched = matched.map(|m| {
                    let start = to;
                    let end = start + m.adapter_bases;
                    (m.errors, start, end.min(original), m.adapter_bases)
                });
                stats.info_lines.push(crate::auxout::info_line(
                    &record.header[1..],
                    record.sequence,
                    record.qualities,
                    matched,
                    matched.map(|_| false).unwrap_or(false),
                ));
            }
            if self.aux.wildcard_file.is_some() {
                // Oracle format: "<wildcard bases> <read name>", where the
                // wildcard bases are what replaced the adapter's wildcard
                // positions; empty when the adapter has no wildcard.
                // Oracle: the wildcard file records only the read base at each
                // WILDCARD position of the adapter (one character), not the whole
                // matched region.
                let wildcards = matched.map(|m| {
                    let mut bases = Vec::new();
                    for (offset, _adapter_base) in self
                        .adapters
                        .wildcard_positions(m.adapter_index)
                        .into_iter()
                    {
                        if let Some(base) = record.sequence.get(to + offset) {
                            bases.push(*base);
                        }
                    }
                    String::from_utf8_lossy(&bases).into_owned()
                });
                if let Some(bases) = wildcards {
                    stats.wildcard_lines.push(format!(
                        "{} {}",
                        bases,
                        String::from_utf8_lossy(&record.header[1..])
                    ));
                }
            }
            if self.aux.rest_file.is_some() {
                if let Some(m) = matched {
                    // The rest file holds the bases AFTER a 3' adapter, and only
                    // for reads where some remain (oracle lists 3 of 13 reads).
                    let adapter_end = to + m.adapter_bases;
                    if m.removed_3prime > 0 && adapter_end < original {
                        stats.rest_lines.push(crate::auxout::rest_line(
                            &record.header[1..],
                            &record.sequence[adapter_end..],
                        ));
                    }
                }
            }
            // `--untrimmed-output` redirects UNMATCHED reads to a separate
            // file; matched reads still go to the main output.
            if self.aux.untrimmed_output.is_some() && matched.is_none() {
                crate::auxout::write_record(
                    &mut stats.untrimmed_records,
                    record.header,
                    &record.sequence[from..to],
                    record.plus,
                    &record.qualities[from..to],
                    self.aux.fasta,
                )
                .expect("writing to memory cannot fail");
                // Untrimmed reads are DIVERTED to that file, so they do not also
                // appear in the main output.
                stats.reads_untrimmed_output += 1;
                continue;
            }
            match fate {
                crate::filter::Fate::Written => {}
                crate::filter::Fate::TooShort => {
                    stats.reads_too_short += 1;
                    // Always capture the record: paired mode may still need it
                    // when the other mate passes (--pair-filter both).
                    crate::auxout::write_record(
                        &mut stats.too_short_records,
                        record.header,
                        &record.sequence[from..to],
                        record.plus,
                        &record.qualities[from..to],
                        self.aux.fasta,
                    )
                    .expect("writing to memory cannot fail");
                    continue;
                }
                crate::filter::Fate::TooLong => {
                    stats.reads_too_long += 1;
                    if self.aux.too_long_output.is_some() {
                        crate::auxout::write_record(
                            &mut stats.too_long_records,
                            record.header,
                            &record.sequence[from..to],
                            record.plus,
                            &record.qualities[from..to],
                            self.aux.fasta,
                        )
                        .expect("writing to memory cannot fail");
                    }
                    continue;
                }
                crate::filter::Fate::TooManyN => {
                    stats.reads_too_many_n += 1;
                    continue;
                }
                crate::filter::Fate::TooManyExpectedErrors => {
                    stats.reads_too_many_expected_errors += 1;
                    continue;
                }
                crate::filter::Fate::TooManyAverageErrors => {
                    stats.reads_too_many_average_errors += 1;
                    continue;
                }
                crate::filter::Fate::CasavaFiltered => {
                    stats.reads_casava_filtered += 1;
                    continue;
                }
                crate::filter::Fate::DiscardTrimmed => {
                    stats.reads_discard_trimmed += 1;
                    continue;
                }
                crate::filter::Fate::DiscardUntrimmed => {
                    stats.reads_discard_untrimmed += 1;
                    continue;
                }
                crate::filter::Fate::AverageErrorRateOnEmptyRead => {
                    // cutadapt flushes the reads processed before the crash, so
                    // the caller must keep `out` (see `PartialOutput`) rather
                    // than discarding it.
                    // Fail closed: cutadapt dies here (ZeroDivisionError), so we
                    // abort the whole run with exit 1 rather than silently
                    // writing a different set of reads.
                    return Err(crate::fastq::ParseError {
                        message: format!(
                            "{FATAL_MARKER}--max-average-error-rate cannot be computed for a \
                             read that became empty (cutadapt 4.6 crashes here); aborting so no \
                             incorrect output is produced"
                        ),
                        line: base_line,
                        offset: base_offset,
                    });
                }
            }
            let _ = minimum_length;
            // `--action` decides what the matched region looks like. `trim` (the
            // default) already happened; the others keep the read intact apart
            // from masking or lowercasing the adapter bases, and `retain`/`none`
            // keep the read unchanged. Oracle-verified on AAAAGATCGGAAGAGC.
            let (write_from, write_to, transformed) = match self.action {
                crate::modify::Action::Trim => (from, to, None),
                // `retain` keeps the read through the END of the adapter, not
                // the whole read: the oracle keeps the first 43 bases of a 53 bp
                // read whose 13 bp adapter starts at 30. `none` leaves the read
                // untouched.
                crate::modify::Action::Retain => {
                    let adapter_end = match matched {
                        // 3' match: adapter runs from `to` for adapter_bases.
                        Some(m) if m.removed_3prime > 0 => (to + m.adapter_bases).min(original),
                        // 5' match: everything up to the cut is the adapter.
                        Some(_) => from.min(original),
                        None => original,
                    };
                    (0, adapter_end, None)
                }
                crate::modify::Action::None => (0, original, None),
                crate::modify::Action::Mask => {
                    // The *removed* bases (outside from..to, and the 3' tail
                    // after `to`) are the adapter; mask exactly those.
                    let mut seq = record.sequence.to_vec();
                    for (index, base) in seq.iter_mut().enumerate() {
                        if index < from || index >= to {
                            *base = b'N';
                        }
                    }
                    (0, original, Some(seq))
                }
                crate::modify::Action::Lowercase => {
                    let mut seq = record.sequence.to_vec();
                    for (index, base) in seq.iter_mut().enumerate() {
                        if index < from || index >= to {
                            *base = base.to_ascii_lowercase();
                        }
                    }
                    (0, original, Some(seq))
                }
            };
            let written_len = write_to.saturating_sub(write_from);
            stats.records_out += 1;
            stats.bp_out += written_len as u64;
            // Build the record once, then append it to every destination: the
            // main output and, when demultiplexing, the per-adapter file (or the
            // "unknown" file). Building it once keeps the copies byte-identical.
            let mut one: Vec<u8> = Vec::with_capacity(record.header.len() + written_len * 2 + 8);
            // `--strip-suffix`, `--length-tag`, `--rename`, `-x` and `-y` all
            // rewrite the header name; the sigil is kept.
            let renamed = if self.renaming.is_empty() {
                record.header[1..].to_vec()
            } else {
                // Unmatched reads use the literal adapter name "no_adapter",
                // which is what {adapter_name}/{name} expand to.
                let adapter_name = Some(matched.map_or_else(
                    || "no_adapter".to_string(),
                    |m| {
                        self.adapters
                            .name_of(m.adapter_index)
                            .unwrap_or_else(|| (m.adapter_index + 1).to_string())
                    },
                ));
                crate::modify::rename(
                    &record.header[1..],
                    &self.renaming,
                    adapter_name.as_deref(),
                    written_len,
                )
            };
            // FASTA output drops the qualities and uses '>' as the header sigil.
            if self.aux.fasta {
                one.push(b'>');
            } else if record.header.first() == Some(&b'@') {
                one.push(b'@');
            }
            one.extend_from_slice(&renamed);
            // cutadapt marks reverse-complemented reads with a trailing " rc".
            if used_rc {
                one.extend_from_slice(b" rc");
            }
            one.push(b'\n');
            match (&transformed, &rc_sequence) {
                (Some(seq), _) => one.extend_from_slice(&seq[write_from..write_to]),
                (None, Some(rc)) => one.extend_from_slice(&rc[write_from..write_to]),
                (None, None) => one.extend_from_slice(&record.sequence[write_from..write_to]),
            }
            one.push(b'\n');
            if !self.aux.fasta {
                one.extend_from_slice(record.plus);
                one.push(b'\n');
                one.extend_from_slice(&record.qualities[write_from..write_to]);
                one.push(b'\n');
            }
            // Demultiplexing target: the matched adapter, or "unknown".
            if self.aux.demultiplex {
                let local = match self.demux_index_override {
                    Some(index) => Some(index),
                    None => {
                        matched.map(|m| m.adapter_index.saturating_sub(self.adapters.first_index()))
                    }
                };
                match local.and_then(|i| stats.demux_records.get_mut(i)) {
                    Some(buffer) => buffer.extend_from_slice(&one),
                    None => {
                        stats.demux_unknown.extend_from_slice(&one);
                        stats.demux_unknown_records += 1;
                    }
                }
            }
            // In demultiplexing mode the per-adapter files are the output, so the
            // `-o` template itself receives nothing.
            if !self.aux.demultiplex {
                out.extend_from_slice(&one);
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::parse;

    fn trimmer(option: char, adapter: &str) -> Trimmer {
        let config = MatchConfig::default();
        Trimmer::single(parse(option, adapter, 0, None).unwrap(), config)
    }

    fn process(input: &[u8], minimum_length: usize) -> (String, ChunkStats) {
        // Filters now own -m, so the helper configures them explicitly.
        let mut trimmer = trimmer('a', "AGATCGGAAGAGC").with_filters(crate::filter::Filters {
            minimum_length: (minimum_length > 0).then_some(minimum_length),
            ..Default::default()
        });
        let mut out = Vec::new();
        let stats = trimmer
            .process_chunk(input, 0, 1, minimum_length, &mut out)
            .unwrap();
        (String::from_utf8(out).unwrap(), stats)
    }

    fn run(trimmer: &mut Trimmer, input: &str) -> String {
        let mut out = Vec::new();
        trimmer
            .process_chunk(input.as_bytes(), 0, 1, 0, &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn keeps_plus_and_quality_lines() {
        let (out, stats) = process(
            b"@r1 note\nACGTAGATCGGAAGAGC\n+r1 note\nIIIIIIIIIIIIIIIII\n",
            0,
        );
        assert_eq!(out, "@r1 note\nACGT\n+r1 note\nIIII\n");
        assert_eq!(stats.records_in, 1);
        assert_eq!(stats.records_out, 1);
        assert_eq!(stats.reads_with_adapter, 1);
        assert_eq!(stats.bp_in, 17);
        assert_eq!(stats.bp_out, 4);
    }

    #[test]
    fn filters_short_reads_after_trimming() {
        let input = b"@a\nACGTAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIII\n@b\nACGTACGTAC\n+\nIIIIIIIIII\n";
        let (out, stats) = process(input, 5);
        assert_eq!(out, "@b\nACGTACGTAC\n+\nIIIIIIIIII\n");
        assert_eq!(stats.records_in, 2);
        assert_eq!(stats.records_out, 1);
        assert_eq!(stats.reads_too_short, 1);
    }

    #[test]
    fn keeps_reads_equal_to_minimum_length_exact() {
        let input = b"@a\nACGTAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIII\n";
        let (out, _stats) = process(input, 4);
        assert_eq!(out, "@a\nACGT\n+\nIIII\n");
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let (out, stats) = process(b"", 10);
        assert_eq!(out, "");
        assert_eq!(stats.records_in, 0);
        assert_eq!(stats.records_out, 0);
        // one (empty) measurement slot per configured adapter
        assert_eq!(stats.adapter_stats.len(), 1);
        assert_eq!(stats.adapter_stats[0].matches, 0);
    }

    #[test]
    fn malformed_input_reports_error() {
        let mut trimmer = trimmer('a', "AGATCGGAAGAGC");
        let mut out = Vec::new();
        let err = trimmer
            .process_chunk(b"@a\nACGT\n+\nII\n", 0, 1, 0, &mut out)
            .unwrap_err();
        assert!(err.message.contains("lengths differ"), "{}", err.message);
    }

    #[test]
    fn untrimmed_quality_is_preserved_byte_for_byte() {
        let input = b"@a\nACGTACGT\n+\n!\"#$%&'(\n";
        let (out, stats) = process(input, 0);
        assert_eq!(out, "@a\nACGTACGT\n+\n!\"#$%&'(\n");
        assert_eq!(stats.reads_with_adapter, 0);
    }

    // ---- adapter types through the engine, oracle-verified ----------------

    #[test]
    fn five_prime_adapter_trims_the_prefix() {
        let mut t = trimmer('g', "ACGTACGT");
        assert_eq!(
            run(&mut t, "@r\nACGTACGTGGGG\n+\nIIIIIIIIIIII\n"),
            "@r\nGGGG\n+\nIIII\n"
        );
    }

    #[test]
    fn anywhere_adapter_trims_both_ends() {
        let mut t = trimmer('b', "ACGTACGT");
        assert_eq!(
            run(&mut t, "@r\nACGTACGTGGGG\n+\nIIIIIIIIIIII\n"),
            "@r\nGGGG\n+\nIIII\n"
        );
        assert_eq!(
            run(&mut t, "@r\nGGGGACGTACGT\n+\nIIIIIIIIIIII\n"),
            "@r\nGGGG\n+\nIIII\n"
        );
    }

    #[test]
    fn linked_adapter_trims_both_parts() {
        let mut t = trimmer('a', "^AAA...TTT");
        assert_eq!(
            run(&mut t, "@r\nAAAGGGGGTTT\n+\nIIIIIIIIIII\n"),
            "@r\nGGGGG\n+\nIIIII\n"
        );
    }

    #[test]
    fn times_removes_multiple_adapters() {
        // Oracle (`-n 1` and `-n 2` both give AAA for this read): the best 3'
        // match wins and cuts to the alignment start, so the second adapter is
        // already removed together with the tail.
        let read = "@r\nAAAAGATCGGAAGAGCGGGGAGATCGGAAGAGC\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII\n";
        let mut t1 = trimmer('a', "AGATCGGAAGAGC");
        assert_eq!(run(&mut t1, read), "@r\nAAA\n+\nIII\n");
        let mut t2 = trimmer('a', "AGATCGGAAGAGC").with_times(2);
        assert_eq!(run(&mut t2, read), "@r\nAAA\n+\nIII\n");
    }
}
