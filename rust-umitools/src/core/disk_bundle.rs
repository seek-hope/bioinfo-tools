//! Disk-backed exact neighbour queries for a single oversized bundle.
//!
//! The in-memory path materialises every candidate pair and then duplicates them
//! into adjacency lists. For a very large bundle that is the memory peak. This
//! module enumerates the same exact edge set block by block (pigeonhole over
//! `threshold + 1` UMI blocks), writes the pairs to a temporary file, sorts and
//! deduplicates them externally, and then either streams them into a disjoint-set
//! (undirected methods) or builds a compact CSR whose neighbour array lives on
//! disk (directed and adjacency methods). Memory is bounded by one block index,
//! one sort chunk and the offset table; the edge array stays on disk.

use crate::core::distance::hamming;
use crate::core::umi::Umi;
use crate::error::{Error, Result};
use crate::io::spill::TempOutput;
use rustc_hash::{FxHashMap, FxHashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read as IoRead, Seek, SeekFrom, Write as IoWrite};
use std::path::{Path, PathBuf};

const SORT_CHUNK: usize = 1 << 19;

fn hash_block(umi: &Umi, range: std::ops::Range<usize>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for position in range {
        let value = umi.symbol(position).map(u64::from).unwrap_or(255);
        hash ^= value.wrapping_add(1);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Enumerate exact pairs block by block. Duplicates are possible when two UMIs
/// share more than one block; the external sort removes them.
fn write_pairs(umis: &[Umi], threshold: u32, directory: &Path) -> Result<TempOutput> {
    let temp = TempOutput::create(directory, "umitools-pairs")?;
    let mut writer =
        BufWriter::new(File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?);
    let blocks = (threshold as usize).saturating_add(1).max(1);
    let length = umis[0].len();
    let base = length / blocks;
    let extra = length % blocks;
    let mut offset = 0usize;
    for block in 0..blocks {
        let size = base + usize::from(block < extra);
        let range = offset..offset + size;
        offset += size;
        let mut index: FxHashMap<u64, Vec<u32>> = FxHashMap::default();
        for (position, umi) in umis.iter().enumerate() {
            index
                .entry(hash_block(umi, range.clone()))
                .or_default()
                .push(position as u32);
        }
        let mut seen: FxHashSet<u32> = FxHashSet::default();
        for (position, umi) in umis.iter().enumerate() {
            seen.clear();
            let Some(bucket) = index.get(&hash_block(umi, range.clone())) else {
                continue;
            };
            for &candidate in bucket {
                let candidate = candidate as usize;
                if candidate <= position || !seen.insert(candidate as u32) {
                    continue;
                }
                if matches!(hamming(umi, &umis[candidate]), Some(distance) if distance <= threshold)
                {
                    let pair = ((position as u64) << 32) | candidate as u64;
                    writer
                        .write_all(&pair.to_le_bytes())
                        .map_err(|e| Error::Format(format!("cannot write pair: {e}")))?;
                }
            }
        }
    }
    writer
        .flush()
        .map_err(|e| Error::Format(format!("cannot flush pairs: {e}")))?;
    Ok(temp)
}

fn read_pair(reader: &mut impl IoRead) -> std::io::Result<Option<u64>> {
    let mut buffer = [0u8; 8];
    match reader.read_exact(&mut buffer) {
        Ok(()) => Ok(Some(u64::from_le_bytes(buffer))),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

/// Sort and deduplicate the pair file with bounded memory.
fn sort_unique(input: &Path, directory: &Path) -> Result<(TempOutput, u64)> {
    let mut reader = BufReader::new(File::open(input).map_err(|e| Error::io(input, e))?);
    let mut runs: Vec<TempOutput> = Vec::new();
    loop {
        let mut chunk = Vec::with_capacity(SORT_CHUNK);
        while chunk.len() < SORT_CHUNK {
            match read_pair(&mut reader)
                .map_err(|e| Error::Format(format!("cannot read pairs: {e}")))?
            {
                Some(pair) => chunk.push(pair),
                None => break,
            }
        }
        if chunk.is_empty() {
            break;
        }
        chunk.sort_unstable();
        let temp = TempOutput::create(directory, "umitools-pairrun")?;
        let mut writer =
            BufWriter::new(File::create(temp.path()).map_err(|e| Error::io(temp.path(), e))?);
        for pair in &chunk {
            writer
                .write_all(&pair.to_le_bytes())
                .map_err(|e| Error::Format(format!("cannot write pair run: {e}")))?;
        }
        writer
            .flush()
            .map_err(|e| Error::Format(format!("cannot flush pair run: {e}")))?;
        runs.push(temp);
    }

    let output = TempOutput::create(directory, "umitools-pairsorted")?;
    let mut writer =
        BufWriter::new(File::create(output.path()).map_err(|e| Error::io(output.path(), e))?);
    let mut readers: Vec<BufReader<File>> = Vec::with_capacity(runs.len());
    for run in &runs {
        readers.push(BufReader::new(
            File::open(run.path()).map_err(|e| Error::io(run.path(), e))?,
        ));
    }
    let mut heads: Vec<Option<u64>> = Vec::with_capacity(readers.len());
    for reader in readers.iter_mut() {
        heads.push(read_pair(reader).map_err(|e| Error::Format(format!("cannot read run: {e}")))?);
    }
    let mut count = 0u64;
    let mut last: Option<u64> = None;
    while let Some((index, value)) = heads
        .iter()
        .enumerate()
        .filter_map(|(index, head)| head.map(|value| (index, value)))
        .min_by_key(|(_, value)| *value)
    {
        if last != Some(value) {
            writer
                .write_all(&value.to_le_bytes())
                .map_err(|e| Error::Format(format!("cannot write sorted pairs: {e}")))?;
            count += 1;
            last = Some(value);
        }
        heads[index] = read_pair(&mut readers[index])
            .map_err(|e| Error::Format(format!("cannot read run: {e}")))?;
    }
    writer
        .flush()
        .map_err(|e| Error::Format(format!("cannot flush sorted pairs: {e}")))?;
    drop(runs);
    Ok((output, count))
}

/// Undirected components by streaming the sorted unique pairs into a DSU.
fn union_find_components(pairs: &Path, umis: &[Umi], counts: &[u64]) -> Result<Vec<Vec<usize>>> {
    let mut parent: Vec<usize> = (0..umis.len()).collect();
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    let mut reader = BufReader::new(File::open(pairs).map_err(|e| Error::io(pairs, e))?);
    while let Some(pair) = read_pair(&mut reader)
        .map_err(|e| Error::Format(format!("cannot read sorted pairs: {e}")))?
    {
        let left = find(&mut parent, (pair >> 32) as usize);
        let right = find(&mut parent, (pair & 0xffff_ffff) as usize);
        if left != right {
            parent[right] = left;
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut index: FxHashMap<usize, usize> = FxHashMap::default();
    for node in 0..umis.len() {
        let root = find(&mut parent, node);
        match index.get(&root) {
            Some(&position) => groups[position].push(node),
            None => {
                index.insert(root, groups.len());
                groups.push(vec![node]);
            }
        }
    }
    for group in groups.iter_mut() {
        group.sort_by(|a, b| umis[*a].cmp(&umis[*b]));
    }
    let seed = |group: &Vec<usize>| -> usize {
        *group
            .iter()
            .max_by(|left, right| counts[**left].cmp(&counts[**right]).then(right.cmp(left)))
            .expect("non-empty group")
    };
    groups.sort_by(|left, right| {
        let (left_seed, right_seed) = (seed(left), seed(right));
        counts[right_seed]
            .cmp(&counts[left_seed])
            .then(left_seed.cmp(&right_seed))
    });
    Ok(groups)
}

fn directional_edge(high: u64, low: u64) -> bool {
    u128::from(high) + 1 >= 2 * u128::from(low)
}

/// Compact adjacency whose neighbour array lives on disk. `directed` supplies
/// the counts for the directional rule; `None` builds an undirected graph.
pub struct Csr {
    offsets: Vec<u64>,
    _neighbours: TempOutput,
    path: PathBuf,
    file: File,
}

impl Csr {
    fn build(
        pairs: &Path,
        nodes: usize,
        directory: &Path,
        directed: Option<&[u64]>,
    ) -> Result<Csr> {
        let mut degrees = vec![0u32; nodes];
        let mut reader = BufReader::new(File::open(pairs).map_err(|e| Error::io(pairs, e))?);
        while let Some(pair) = read_pair(&mut reader)
            .map_err(|e| Error::Format(format!("cannot read sorted pairs: {e}")))?
        {
            let left = (pair >> 32) as usize;
            let right = (pair & 0xffff_ffff) as usize;
            match directed {
                Some(counts) => {
                    if directional_edge(counts[left], counts[right]) {
                        degrees[left] += 1;
                    }
                    if directional_edge(counts[right], counts[left]) {
                        degrees[right] += 1;
                    }
                }
                None => {
                    degrees[left] += 1;
                    degrees[right] += 1;
                }
            }
        }
        let mut offsets = vec![0u64; nodes + 1];
        for node in 0..nodes {
            offsets[node + 1] = offsets[node] + u64::from(degrees[node]);
        }
        let temp = TempOutput::create(directory, "umitools-csr")?;
        let mut file = File::options()
            .read(true)
            .write(true)
            .truncate(true)
            .open(temp.path())
            .map_err(|e| Error::io(temp.path(), e))?;
        file.set_len(offsets[nodes] * 4)
            .map_err(|e| Error::io(temp.path(), e))?;
        let mut cursor = offsets.clone();
        let mut reader = BufReader::new(File::open(pairs).map_err(|e| Error::io(pairs, e))?);
        while let Some(pair) = read_pair(&mut reader)
            .map_err(|e| Error::Format(format!("cannot read sorted pairs: {e}")))?
        {
            let left = (pair >> 32) as usize;
            let right = (pair & 0xffff_ffff) as usize;
            let mut edges = Vec::with_capacity(2);
            match directed {
                Some(counts) => {
                    if directional_edge(counts[left], counts[right]) {
                        edges.push((left, right));
                    }
                    if directional_edge(counts[right], counts[left]) {
                        edges.push((right, left));
                    }
                }
                None => edges.extend([(left, right), (right, left)]),
            }
            for (node, other) in edges {
                let slot = cursor[node];
                file.seek(SeekFrom::Start(slot * 4))
                    .map_err(|e| Error::io(temp.path(), e))?;
                file.write_all(&(other as u32).to_le_bytes())
                    .map_err(|e| Error::io(temp.path(), e))?;
                cursor[node] = slot + 1;
            }
        }
        file.flush().map_err(|e| Error::io(temp.path(), e))?;
        Ok(Csr {
            offsets,
            path: PathBuf::from(temp.path()),
            _neighbours: temp,
            file,
        })
    }

    fn neighbours(&mut self, node: usize) -> Result<Vec<u32>> {
        let start = self.offsets[node];
        let end = self.offsets[node + 1];
        let mut values = Vec::with_capacity((end - start) as usize);
        self.file
            .seek(SeekFrom::Start(start * 4))
            .map_err(|e| Error::io(&self.path, e))?;
        for _ in start..end {
            let mut buffer = [0u8; 4];
            self.file
                .read_exact(&mut buffer)
                .map_err(|e| Error::io(&self.path, e))?;
            values.push(u32::from_le_bytes(buffer));
        }
        Ok(values)
    }
}

fn mark_covered(
    node: usize,
    position: &FxHashMap<usize, usize>,
    covered: &mut [u32],
    uncovered: &mut usize,
) {
    if let Some(&index) = position.get(&node) {
        if covered[index] == 0 {
            *uncovered = uncovered.saturating_sub(1);
        }
        covered[index] += 1;
    }
}

fn sort_by_count_desc(nodes: &mut [usize], counts: &[u64]) {
    nodes.sort_by(|a, b| counts[*b].cmp(&counts[*a]));
}

fn directed_components(csr: &mut Csr, umis: &[Umi], counts: &[u64]) -> Result<Vec<Vec<usize>>> {
    let nodes = umis.len();
    let mut seeds: Vec<usize> = (0..nodes).collect();
    seeds.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));
    let mut visited = vec![false; nodes];
    let mut out = Vec::new();
    for seed in seeds {
        if visited[seed] {
            continue;
        }
        let mut stack = vec![seed];
        visited[seed] = true;
        let mut members = Vec::new();
        while let Some(node) = stack.pop() {
            members.push(node);
            for next in csr.neighbours(node)? {
                let next = next as usize;
                if !visited[next] {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        members.sort_by(|a, b| umis[*a].cmp(&umis[*b]));
        out.push(members);
    }
    Ok(out)
}

fn group_adjacency(
    csr: &mut Csr,
    components: &[Vec<usize>],
    umis: &[Umi],
    counts: &[u64],
) -> Result<Vec<Vec<usize>>> {
    let mut groups = Vec::new();
    for component in components {
        if component.len() == 1 {
            groups.push(component.clone());
            continue;
        }
        let mut sorted = component.clone();
        sort_by_count_desc(&mut sorted, counts);

        let position: FxHashMap<usize, usize> = component
            .iter()
            .enumerate()
            .map(|(index, node)| (*node, index))
            .collect();
        let mut covered = vec![0u32; component.len()];
        let mut uncovered = component.len();
        let mut leads: Vec<usize> = Vec::new();
        for take in 1..sorted.len() {
            let node = sorted[take - 1];
            mark_covered(node, &position, &mut covered, &mut uncovered);
            for next in csr.neighbours(node)? {
                mark_covered(next as usize, &position, &mut covered, &mut uncovered);
            }
            if uncovered == 0 {
                leads = sorted[..take].to_vec();
                break;
            }
        }
        if leads.is_empty() {
            leads = sorted[..sorted.len() - 1].to_vec();
        }

        let mut observed: FxHashSet<usize> = leads.iter().copied().collect();
        for &lead in &leads {
            let mut tail: Vec<usize> = csr
                .neighbours(lead)?
                .into_iter()
                .map(|value| value as usize)
                .filter(|node| !observed.contains(node))
                .collect();
            tail.sort_by(|a, b| counts[*b].cmp(&counts[*a]).then(umis[*a].cmp(&umis[*b])));
            let mut group = Vec::with_capacity(tail.len() + 1);
            group.push(lead);
            group.extend(tail);
            groups.push(group);
            for next in csr.neighbours(lead)? {
                observed.insert(next as usize);
            }
        }
    }
    Ok(groups)
}

/// Cluster a bundle through the disk-backed exact neighbour path.
pub fn disk_cluster(
    umis: &[Umi],
    counts: &[u64],
    method: crate::core::cluster::Method,
    threshold: u32,
    directory: &Path,
) -> Result<Vec<Vec<usize>>> {
    use crate::core::cluster::Method;
    let pairs = write_pairs(umis, threshold, directory)?;
    let (sorted, _) = sort_unique(pairs.path(), directory)?;
    let mut groups = match method {
        Method::Cluster => union_find_components(sorted.path(), umis, counts)?,
        Method::Directional => {
            let mut csr = Csr::build(sorted.path(), umis.len(), directory, Some(counts))?;
            let components = directed_components(&mut csr, umis, counts)?;
            let mut groups = components;
            for group in groups.iter_mut() {
                sort_by_count_desc(group, counts);
            }
            groups
        }
        Method::Adjacency => {
            let mut csr = Csr::build(sorted.path(), umis.len(), directory, None)?;
            let components = directed_components(&mut csr, umis, counts)?;
            group_adjacency(&mut csr, &components, umis, counts)?
        }
        Method::Unique | Method::Percentile => unreachable!("handled in memory"),
    };
    if matches!(method, Method::Cluster) {
        for group in groups.iter_mut() {
            sort_by_count_desc(group, counts);
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cluster::{cluster, Method};

    fn bundle(count: usize) -> (Vec<Umi>, Vec<u64>) {
        let umis = (0..count)
            .map(|value| {
                let bytes: Vec<u8> = (0..8)
                    .map(|position| b"ACGT"[(value >> (2 * position)) & 3])
                    .collect();
                Umi::from_bytes(&bytes)
            })
            .collect();
        let counts = (0..count).map(|index| (index % 7 + 1) as u64).collect();
        (umis, counts)
    }

    #[test]
    fn disk_matches_memory() {
        let directory =
            std::env::temp_dir().join(format!("umitools-disk-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let (umis, counts) = bundle(300);
        for method in [Method::Cluster, Method::Directional, Method::Adjacency] {
            for threshold in [0u32, 1, 2] {
                let memory = cluster(&umis, &counts, method, threshold).unwrap();
                let disk = disk_cluster(&umis, &counts, method, threshold, &directory).unwrap();
                assert_eq!(memory, disk, "method={method:?} threshold={threshold}");
            }
        }
        std::fs::remove_dir_all(&directory).ok();
    }
}
