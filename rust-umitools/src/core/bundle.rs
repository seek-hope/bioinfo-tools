//! Bundle keys, bundle state and the UMI-tools buffer lifecycle.
//!
//! `Bundler` reproduces `umi_tools.sam_methods.get_bundles`: reads are grouped by
//! `(position, key)` where `key` is `(reverse, spliced, tlen, read_length, cell)`,
//! and bundles are released exactly when upstream's `check_output` releases them.
//! Emission order therefore matches upstream, which lets parallel clustering write
//! output in the same order without a reordering buffer.

use crate::core::barcode::{extract_cell, extract_umi, UmiSource};
use crate::core::rng::Rng;
use crate::core::umi::Umi;
use crate::error::{Error, Result};
use crate::io::alignment::{aux_int, aux_str, read_position};
use crate::io::bam::Record;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;

pub const FLUSH_WINDOW: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detection {
    None,
    Integer([u8; 2]),
    Xt,
}

impl Detection {
    pub fn parse(raw: &str) -> Result<Detection> {
        match raw {
            "NH" => Ok(Detection::Integer(*b"NH")),
            "X0" => Ok(Detection::Integer(*b"X0")),
            "XT" => Ok(Detection::Xt),
            other => Err(Error::usage_with_hint(
                format!("unknown --multimapping-detection-method '{other}'"),
                "choose one of: NH, X0, XT",
            )),
        }
    }
}

/// How many records the bundle keeps per UMI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    /// One best record per UMI (`dedup`).
    Best,
    /// Every record per UMI (`group`).
    All,
    /// Counts only, no records (`count`).
    CountOnly,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BundleKey {
    pub reverse: bool,
    pub spliced: i64,
    pub tlen: i64,
    pub read_length: i64,
    pub cell: Option<Box<[u8]>>,
}

impl Ord for BundleKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.reverse
            .cmp(&other.reverse)
            .then_with(|| self.spliced.cmp(&other.spliced))
            .then_with(|| self.tlen.cmp(&other.tlen))
            .then_with(|| self.read_length.cmp(&other.read_length))
            .then_with(|| self.cell.cmp(&other.cell))
    }
}

impl PartialOrd for BundleKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct Entry {
    pub count: u64,
    pub ties: u64,
    pub reads: Vec<Record>,
}

/// Gene (and optional cell) label carried by gene keyed bundles.
pub type GeneLabel = (Box<[u8]>, Option<Box<[u8]>>);

#[derive(Default)]
pub struct Bundle {
    /// UMI insertion order: upstream relies on it for `unique`/`percentile` and
    /// for tie-breaking in component seeding.
    pub order: Vec<Umi>,
    pub entries: FxHashMap<Umi, Entry>,
    /// Gene keyed bundles carry their gene (and cell) so the command can label
    /// its output without re-deriving it.
    pub label: Option<GeneLabel>,
}

impl Bundle {
    pub fn umis(&self) -> &[Umi] {
        &self.order
    }

    pub fn counts(&self) -> Vec<u64> {
        self.order.iter().map(|u| self.entries[u].count).collect()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn best_read(&self, umi: &Umi) -> &Record {
        &self.entries[umi].reads[0]
    }

    /// Port of `sam_methods.update_dicts`.
    pub fn update(
        &mut self,
        umi: Umi,
        read: Record,
        rng: &mut dyn Rng,
        detection: Detection,
        storage: Storage,
    ) -> Result<bool> {
        let entry = match self.entries.get_mut(&umi) {
            Some(entry) => entry,
            None => {
                self.order.push(umi.clone());
                let entry = self.entries.entry(umi).or_insert(Entry {
                    count: 0,
                    ties: 0,
                    reads: Vec::new(),
                });
                entry.count = 1;
                if storage != Storage::CountOnly {
                    entry.reads.push(read);
                }
                return Ok(false);
            }
        };
        entry.count = entry
            .count
            .checked_add(1)
            .ok_or_else(|| Error::Resource("UMI count overflowed u64".to_string()))?;
        match storage {
            Storage::CountOnly => return Ok(false),
            Storage::All => {
                entry.reads.push(read);
                return Ok(false);
            }
            Storage::Best => {}
        }

        let current = entry.reads[0].mapq();
        let candidate = read.mapq();
        if current > candidate {
            return Ok(false);
        }
        if current < candidate {
            entry.reads[0] = read;
            entry.ties = 0;
            return Ok(false);
        }
        let mut reset = false;
        match detection {
            Detection::Integer(tag) => {
                let held = aux_int(&entry.reads[0], &tag)?;
                let new = aux_int(&read, &tag)?;
                if held < new {
                    return Ok(false);
                }
                if held > new {
                    reset = true;
                }
            }
            Detection::Xt => {
                if aux_str(&entry.reads[0], b"XT")? == "U" {
                    return Ok(false);
                }
                if aux_str(&read, b"XT")? == "U" {
                    reset = true;
                }
            }
            Detection::None => {}
        }
        if reset {
            entry.ties = 0;
        }
        // Upstream falls through to the tie draw even after replacing on a
        // better multimapping score: read_counts becomes 1, the draw has
        // probability 1.0 (so the read stays) and one RNG value is consumed.
        // That consumption is observable in every later tie, so it must match.
        entry.ties += 1;
        if rng.next_f64() < 1.0 / entry.ties as f64 {
            entry.reads[0] = read;
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub struct BundleConfig {
    pub spliced: bool,
    pub ignore_tlen: bool,
    pub paired: bool,
    pub read_length: bool,
    pub per_cell: bool,
    pub source: UmiSource,
    pub cell_tag: [u8; 2],
    pub cell_tag_split: Option<String>,
    pub cell_tag_delim: Option<String>,
    pub soft_clip_threshold: i64,
    pub detection: Detection,
    pub storage: Storage,
    /// Release bundles only when the contig changes, as upstream's
    /// `--buffer-whole-contig` does. Memory then grows with the contig.
    pub whole_contig: bool,
    /// Tagged reads without a UMI are skipped (upstream warns); otherwise the
    /// missing value is a hard input error.
    pub skip_missing_umi: bool,
}

#[derive(Default)]
pub struct BundleStats {
    pub skipped_missing_umi: u64,
    pub ties: u64,
    pub bundles: u64,
    pub missing_umi_example: Option<String>,
}

/// Bundle state for position keyed commands (`dedup`, `group`).
pub struct Bundler {
    pending: BTreeMap<i64, BTreeMap<BundleKey, Bundle>>,
    last_pos: i64,
    last_chr: Option<i32>,
    current_chr: Option<i32>,
    /// Running count so the memory guard is O(1) per read.
    live_umis: usize,
    pub stats: BundleStats,
}

impl Default for Bundler {
    fn default() -> Self {
        Bundler::new()
    }
}

impl Bundler {
    pub fn new() -> Bundler {
        Bundler {
            pending: BTreeMap::new(),
            last_pos: 0,
            last_chr: None,
            current_chr: None,
            live_umis: 0,
            stats: BundleStats::default(),
        }
    }

    pub fn live_umis(&self) -> usize {
        self.live_umis
    }

    pub fn live_bundles(&self) -> usize {
        self.pending.values().map(|b| b.len()).sum()
    }

    fn emit_ready(&mut self, start: i64, tid: i32, whole_contig: bool, out: &mut Vec<Bundle>) {
        self.current_chr = Some(tid);
        let contig_changed = self.current_chr != self.last_chr;
        let advanced = !whole_contig && start > self.last_pos + FLUSH_WINDOW;
        if !(contig_changed || advanced) {
            return;
        }
        // Take only the releasable prefix instead of scanning and allocating a
        // vector with every buffered position on each flush.
        // `split_off` returns the tail (keys >= split point) and keeps the
        // prefix in `pending`; we want to release the prefix and keep the tail.
        let released = if contig_changed {
            std::mem::take(&mut self.pending)
        } else {
            let keep = self.pending.split_off(&(start - FLUSH_WINDOW + 1));
            std::mem::replace(&mut self.pending, keep)
        };
        let mut released_umis = 0usize;
        for (_, bundles) in released {
            for (_, bundle) in bundles {
                released_umis += bundle.len();
                self.stats.bundles += 1;
                out.push(bundle);
            }
        }
        self.live_umis = self.live_umis.saturating_sub(released_umis);
    }

    /// Feed one surviving mapped record.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        record: Record,
        cfg: &BundleConfig,
        rng: &mut dyn Rng,
        out: &mut Vec<Bundle>,
    ) -> Result<()> {
        let (start, pos, spliced_value) =
            read_position(&record, cfg.soft_clip_threshold, cfg.spliced);
        self.emit_ready(start, record.tid(), cfg.whole_contig, out);

        let cell = if cfg.per_cell {
            Some(extract_cell(
                &cfg.source,
                &record,
                &cfg.cell_tag,
                &cfg.cell_tag_split,
                &cfg.cell_tag_delim,
            )?)
        } else {
            None
        };
        let tlen = if cfg.paired && !cfg.ignore_tlen {
            record.insert_size()
        } else {
            0
        };
        let read_length = if cfg.read_length {
            record.seq_len() as i64
        } else {
            0
        };
        let key = BundleKey {
            reverse: record.is_reverse(),
            spliced: spliced_value,
            tlen,
            read_length,
            cell,
        };

        let umi = match extract_umi(&cfg.source, &record) {
            Ok(umi) => umi,
            Err(error) => {
                if cfg.skip_missing_umi {
                    self.stats.skipped_missing_umi += 1;
                    if self.stats.missing_umi_example.is_none() {
                        self.stats.missing_umi_example = Some(error.to_string());
                    }
                    return Ok(());
                }
                return Err(error);
            }
        };

        self.last_pos = start;
        self.last_chr = self.current_chr;

        let bundle = self.pending.entry(pos).or_default().entry(key).or_default();
        let before = bundle.entries.len();
        let used_rng = bundle.update(umi, record, rng, cfg.detection, cfg.storage)?;
        if bundle.entries.len() > before {
            self.live_umis += 1;
        }
        if used_rng {
            self.stats.ties += 1;
        }
        Ok(())
    }

    /// Release everything left, as upstream does after the last read.
    pub fn finish(&mut self, out: &mut Vec<Bundle>) {
        for (_, bundles) in std::mem::take(&mut self.pending) {
            for (_, bundle) in bundles {
                self.stats.bundles += 1;
                out.push(bundle);
            }
        }
        self.live_umis = 0;
    }
}

/// Cell barcode and UMI extracted from one record.
type ExtractedKey = (Option<Box<[u8]>>, Umi);

/// Extract the cell barcode (when requested) and UMI for a record.
fn extract_key(
    record: &Record,
    cfg: &BundleConfig,
    stats: &mut BundleStats,
) -> Result<Option<ExtractedKey>> {
    let cell = if cfg.per_cell {
        Some(extract_cell(
            &cfg.source,
            record,
            &cfg.cell_tag,
            &cfg.cell_tag_split,
            &cfg.cell_tag_delim,
        )?)
    } else {
        None
    };
    match extract_umi(&cfg.source, record) {
        Ok(umi) => Ok(Some((cell, umi))),
        Err(error) => {
            if cfg.skip_missing_umi {
                stats.skipped_missing_umi += 1;
                if stats.missing_umi_example.is_none() {
                    stats.missing_umi_example = Some(error.to_string());
                }
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

/// Gene keyed bundling used by `count --per-gene` and `group --per-gene`.
///
/// Upstream keys `reads_dict` by the gene string and only flushes when the
/// contig changes, so an entire contig of genes is held at once.
pub struct GeneBundler {
    pending: BTreeMap<GeneLabel, Bundle>,
    last_chr: Option<i32>,
    live_umis: usize,
    flush_on_contig: bool,
    pub stats: BundleStats,
}

impl Default for GeneBundler {
    fn default() -> Self {
        GeneBundler::new()
    }
}

impl GeneBundler {
    pub fn new() -> GeneBundler {
        GeneBundler {
            pending: BTreeMap::new(),
            last_chr: None,
            live_umis: 0,
            flush_on_contig: true,
            stats: BundleStats::default(),
        }
    }

    /// A gene may map to transcripts on more than one contig; hold every gene
    /// until the end when the input is grouped into gene partitions.
    pub fn set_flush_on_contig(&mut self, flush: bool) {
        self.flush_on_contig = flush;
    }

    pub fn live_umis(&self) -> usize {
        self.live_umis
    }

    pub fn live_bundles(&self) -> usize {
        self.pending.len()
    }

    pub fn push(
        &mut self,
        record: Record,
        gene: Box<[u8]>,
        cfg: &BundleConfig,
        rng: &mut dyn Rng,
        out: &mut Vec<Bundle>,
    ) -> Result<()> {
        let contig = record.tid();
        if self.flush_on_contig && self.last_chr != Some(contig) && self.last_chr.is_some() {
            self.finish(out);
        }
        self.last_chr = Some(contig);

        let Some((cell, umi)) = extract_key(&record, cfg, &mut self.stats)? else {
            return Ok(());
        };
        let bundle = self
            .pending
            .entry((gene.clone(), cell.clone()))
            .or_default();
        if bundle.label.is_none() {
            bundle.label = Some((gene, cell));
        }
        let before = bundle.entries.len();
        let used_rng = bundle.update(umi, record, rng, cfg.detection, cfg.storage)?;
        if bundle.entries.len() > before {
            self.live_umis += 1;
        }
        if used_rng {
            self.stats.ties += 1;
        }
        Ok(())
    }

    pub fn finish(&mut self, out: &mut Vec<Bundle>) {
        for (_, bundle) in std::mem::take(&mut self.pending) {
            self.stats.bundles += 1;
            out.push(bundle);
        }
        self.live_umis = 0;
    }
}

/// Count-only bundle: no representative reads, no tie state and no duplicate
/// order vector. A gene with millions of UMIs would otherwise retain an
/// `Entry` (reads/ties) for every UMI even though `count` only needs counts.
#[derive(Default)]
pub struct CountBundle {
    pub label: Option<GeneLabel>,
    pub counts: FxHashMap<Umi, u64>,
    /// Insertion order, kept to avoid sorting millions of UMIs at flush.
    pub order: Vec<Umi>,
}

impl CountBundle {
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Returns true when the UMI was new (for the live-UMI guard).
    pub fn update(&mut self, umi: Umi) -> Result<bool> {
        match self.counts.get_mut(&umi) {
            Some(count) => {
                *count = count
                    .checked_add(1)
                    .ok_or_else(|| Error::Resource("UMI count overflowed u64".to_string()))?;
                Ok(false)
            }
            None => {
                self.counts.insert(umi.clone(), 1);
                self.order.push(umi);
                Ok(true)
            }
        }
    }

    /// Materialise the insertion order for clustering. `count` does not use
    /// representative reads, so the order only has to be reproducible.
    pub fn into_sorted(self) -> (Option<GeneLabel>, Vec<Umi>, Vec<u64>) {
        let counts = self.order.iter().map(|umi| self.counts[umi]).collect();
        (self.label, self.order, counts)
    }
}

/// Compact gene bundler used by `count`; mirrors `GeneBundler`'s contig flush
/// and extraction rules without the record/tie storage.
pub struct CountGeneBundler {
    pending: BTreeMap<GeneLabel, CountBundle>,
    last_chr: Option<i32>,
    live_umis: usize,
    flush_on_contig: bool,
    pub stats: BundleStats,
}

impl Default for CountGeneBundler {
    fn default() -> Self {
        CountGeneBundler::new()
    }
}

impl CountGeneBundler {
    pub fn new() -> CountGeneBundler {
        CountGeneBundler {
            pending: BTreeMap::new(),
            last_chr: None,
            live_umis: 0,
            flush_on_contig: true,
            stats: BundleStats::default(),
        }
    }

    pub fn set_flush_on_contig(&mut self, flush: bool) {
        self.flush_on_contig = flush;
    }

    pub fn live_umis(&self) -> usize {
        self.live_umis
    }

    pub fn live_bundles(&self) -> usize {
        self.pending.len()
    }

    pub fn push(
        &mut self,
        record: Record,
        gene: Box<[u8]>,
        cfg: &BundleConfig,
        out: &mut Vec<CountBundle>,
    ) -> Result<()> {
        let contig = record.tid();
        if self.flush_on_contig && self.last_chr != Some(contig) && self.last_chr.is_some() {
            self.finish(out);
        }
        self.last_chr = Some(contig);

        let Some((cell, umi)) = extract_key(&record, cfg, &mut self.stats)? else {
            return Ok(());
        };
        let bundle = self
            .pending
            .entry((gene.clone(), cell.clone()))
            .or_default();
        if bundle.label.is_none() {
            bundle.label = Some((gene, cell));
        }
        if bundle.update(umi)? {
            self.live_umis += 1;
        }
        Ok(())
    }

    pub fn finish(&mut self, out: &mut Vec<CountBundle>) {
        for (_, bundle) in std::mem::take(&mut self.pending) {
            self.stats.bundles += 1;
            out.push(bundle);
        }
        self.live_umis = 0;
    }
}
