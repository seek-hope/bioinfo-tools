//! UMI clustering: the five UMI-tools methods.
//!
//! This reproduces `umi_tools.network.UMIClusterer` step by step:
//! adjacency construction, connected components, then grouping. Group order and
//! the representative (first) UMI of each group are part of the observable
//! contract, so both are kept stable and documented in the implementation plan.
//!
//! Ordering rules taken from upstream:
//! * components are seeded by descending count, ties in bundle insertion order;
//! * the members of a component are sorted by UMI byte order;
//! * groups are sorted by descending count, ties keep the byte order.

use crate::core::neighbors::candidate_pairs;
use crate::core::umi::Umi;
use crate::error::{InputError, Result};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Unique,
    Percentile,
    Cluster,
    Adjacency,
    Directional,
}

impl Method {
    pub fn parse(raw: &str) -> Result<Method> {
        match raw {
            "unique" => Ok(Method::Unique),
            "percentile" => Ok(Method::Percentile),
            "cluster" => Ok(Method::Cluster),
            "adjacency" => Ok(Method::Adjacency),
            "directional" => Ok(Method::Directional),
            other => Err(crate::error::Error::usage_with_hint(
                format!("unknown --method '{other}'"),
                "choose one of: unique, percentile, cluster, adjacency, directional",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Unique => "unique",
            Method::Percentile => "percentile",
            Method::Cluster => "cluster",
            Method::Adjacency => "adjacency",
            Method::Directional => "directional",
        }
    }
}

/// UMI-tools asserts that every UMI in a bundle has the same length. We reject
/// the input with an actionable message instead of aborting inside the library.
pub fn check_uniform_lengths(umis: &[Umi]) -> Result<()> {
    if umis.len() < 2 {
        return Ok(());
    }
    let mut min = usize::MAX;
    let mut max = 0usize;
    let mut longest_index = 0usize;
    for (index, umi) in umis.iter().enumerate() {
        let len = umi.len();
        min = min.min(len);
        // `>=` keeps the last longest UMI, matching the previous error text.
        if len >= max {
            max = len;
            longest_index = index;
        }
    }
    if min != max {
        return Err(crate::error::Error::Input(InputError::MixedUmiLengths {
            min,
            max,
            example: umis[longest_index].display(),
        }));
    }
    Ok(())
}

/// `count(high) >= 2 * count(low) - 1`, evaluated in u128 so large counts cannot
/// overflow.
fn directional_edge(high: u64, low: u64) -> bool {
    u128::from(high) + 1 >= 2 * u128::from(low)
}

fn build_adjacency(
    umis: &[Umi],
    counts: &[u64],
    threshold: u32,
    directional: bool,
) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); umis.len()];
    for (i, j) in candidate_pairs(umis, threshold) {
        if directional {
            if directional_edge(counts[i], counts[j]) {
                adjacency[i].push(j);
            }
            if directional_edge(counts[j], counts[i]) {
                adjacency[j].push(i);
            }
        } else {
            adjacency[i].push(j);
            adjacency[j].push(i);
        }
    }
    adjacency
}

/// Seeded traversal, one component per seed, members sorted by UMI bytes.
fn components(adjacency: &[Vec<usize>], counts: &[u64], umis: &[Umi]) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut seeds: Vec<usize> = (0..n).collect();
    seeds.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));

    let mut visited = vec![false; n];
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
            for &next in &adjacency[node] {
                if !visited[next] {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        members.sort_by(|&a, &b| umis[a].cmp(&umis[b]));
        out.push(members);
    }
    out
}

/// Count-only variant of `components`: same seeded traversal, but no member
/// vectors and no member sorting. `count` only needs the number of clusters.
fn components_count(adjacency: &[Vec<usize>], counts: &[u64]) -> usize {
    let n = adjacency.len();
    let mut seeds: Vec<usize> = (0..n).collect();
    seeds.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));

    let mut visited = vec![false; n];
    let mut components = 0usize;
    for seed in seeds {
        if visited[seed] {
            continue;
        }
        components += 1;
        let mut stack = vec![seed];
        visited[seed] = true;
        while let Some(node) = stack.pop() {
            for &next in &adjacency[node] {
                if !visited[next] {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
    }
    components
}

/// Count-only variant of `components_union_find`. The component count is
/// `n - successful unions`; no group or member vectors are materialised.
fn components_union_find_count(adjacency: &[Vec<usize>]) -> usize {
    let n = adjacency.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }

    let mut components = n;
    for (node, neighbours) in adjacency.iter().enumerate() {
        for &other in neighbours {
            let left = find(&mut parent, node);
            let right = find(&mut parent, other);
            if left != right {
                parent[right] = left;
                components -= 1;
            }
        }
    }
    components
}

/// Union-find components for undirected methods. The observable order is the
/// same as `components`: groups by the count of their highest-count seed (ties
/// by insertion index), members by UMI bytes.
fn components_union_find(
    adjacency: &[Vec<usize>],
    counts: &[u64],
    umis: &[Umi],
) -> Vec<Vec<usize>> {
    let n = adjacency.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }

    for (node, neighbours) in adjacency.iter().enumerate() {
        for &other in neighbours {
            let left = find(&mut parent, node);
            let right = find(&mut parent, other);
            if left != right {
                parent[right] = left;
            }
        }
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut index: FxHashMap<usize, usize> = FxHashMap::default();
    for node in 0..n {
        let root = find(&mut parent, node);
        match index.get(&root) {
            Some(&position) => groups[position].push(node),
            None => {
                index.insert(root, groups.len());
                groups.push(vec![node]);
            }
        }
    }

    // Seed = highest count, ties to the earliest insertion index.
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
    for group in groups.iter_mut() {
        group.sort_by(|a, b| umis[*a].cmp(&umis[*b]));
    }
    groups
}

fn sort_by_count_desc(nodes: &mut [usize], counts: &[u64]) {
    // Stable: equal counts keep the UMI byte order established above.
    nodes.sort_by(|&a, &b| counts[b].cmp(&counts[a]));
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

fn group_adjacency(
    adjacency: &[Vec<usize>],
    comps: &[Vec<usize>],
    counts: &[u64],
    umis: &[Umi],
) -> Vec<Vec<usize>> {
    let mut groups = Vec::new();
    for component in comps {
        if component.len() == 1 {
            groups.push(component.clone());
            continue;
        }
        let mut sorted = component.clone();
        sort_by_count_desc(&mut sorted, counts);

        // Incremental coverage: adding a lead covers itself and its neighbours.
        // `uncovered == 0` is equivalent to an empty remaining set, without
        // rebuilding a set for every candidate prefix.
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
            for &next in &adjacency[node] {
                mark_covered(next, &position, &mut covered, &mut uncovered);
            }
            if uncovered == 0 {
                leads = sorted[..take].to_vec();
                break;
            }
        }
        if leads.is_empty() {
            // Upstream dereferences None here and crashes; take all but the last
            // node instead of failing, which keeps the run reproducible.
            leads = sorted[..sorted.len() - 1].to_vec();
        }

        let mut observed: FxHashSet<usize> = leads.iter().copied().collect();
        for &lead in &leads {
            let mut tail: Vec<usize> = adjacency[lead]
                .iter()
                .copied()
                .filter(|node| !observed.contains(node))
                .collect();
            // Upstream emits this set in Python hash order, which is not
            // reproducible even between upstream runs; sort for determinism.
            tail.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(umis[a].cmp(&umis[b])));
            let mut group = Vec::with_capacity(tail.len() + 1);
            group.push(lead);
            group.extend(tail);
            groups.push(group);
            observed.extend(adjacency[lead].iter().copied());
        }
    }
    groups
}

fn percentile_groups(counts: &[u64]) -> Vec<Vec<usize>> {
    if counts.len() == 1 {
        return vec![vec![0]];
    }
    let mut sorted = counts.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    let median = if sorted.len() % 2 == 1 {
        sorted[mid] as f64
    } else {
        (sorted[mid - 1] as f64 + sorted[mid] as f64) / 2.0
    };
    let threshold = median / 100.0;
    (0..counts.len())
        .filter(|&i| counts[i] as f64 > threshold)
        .map(|i| vec![i])
        .collect()
}

fn percentile_count(counts: &[u64]) -> usize {
    if counts.len() == 1 {
        return 1;
    }
    let mut sorted = counts.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    let median = if sorted.len() % 2 == 1 {
        sorted[mid] as f64
    } else {
        (sorted[mid - 1] as f64 + sorted[mid] as f64) / 2.0
    };
    let threshold = median / 100.0;
    counts.iter().filter(|&&c| c as f64 > threshold).count()
}

/// Cluster `umis` (bundle insertion order) with their counts.
///
/// Returns groups as indices into `umis`; the first element of each group is the
/// representative UMI, exactly as `ReadDeduplicator` uses `cluster[0]`.
pub fn cluster(
    umis: &[Umi],
    counts: &[u64],
    method: Method,
    threshold: u32,
) -> Result<Vec<Vec<usize>>> {
    debug_assert_eq!(umis.len(), counts.len());
    if umis.is_empty() {
        return Ok(Vec::new());
    }
    check_uniform_lengths(umis)?;

    match method {
        Method::Unique => Ok((0..umis.len()).map(|i| vec![i]).collect()),
        Method::Percentile => Ok(percentile_groups(counts)),
        _ => {
            let directional = method == Method::Directional;
            let adjacency = build_adjacency(umis, counts, threshold, directional);
            let comps = if directional {
                components(&adjacency, counts, umis)
            } else {
                components_union_find(&adjacency, counts, umis)
            };
            if method == Method::Adjacency {
                return Ok(group_adjacency(&adjacency, &comps, counts, umis));
            }
            Ok(comps
                .into_iter()
                .map(|mut component| {
                    sort_by_count_desc(&mut component, counts);
                    component
                })
                .collect())
        }
    }
}

/// Count clusters without materialising groups for the methods where the
/// group contents are irrelevant to `count`. `Adjacency` still needs its
/// groups, so it falls back to `cluster`.
pub fn cluster_count(
    umis: &[Umi],
    counts: &[u64],
    method: Method,
    threshold: u32,
) -> Result<usize> {
    debug_assert_eq!(umis.len(), counts.len());
    if umis.is_empty() {
        return Ok(0);
    }
    check_uniform_lengths(umis)?;

    match method {
        Method::Unique => Ok(umis.len()),
        Method::Percentile => Ok(percentile_count(counts)),
        Method::Directional => {
            let adjacency = build_adjacency(umis, counts, threshold, true);
            Ok(components_count(&adjacency, counts))
        }
        Method::Cluster => {
            let adjacency = build_adjacency(umis, counts, threshold, false);
            Ok(components_union_find_count(&adjacency))
        }
        Method::Adjacency => Ok(cluster(umis, counts, method, threshold)?.len()),
    }
}

/// Read the diagnostic threshold once; `cluster_with_scratch` runs for every
/// bundle and a per-call `getenv` showed up in profiles.
fn disk_umis_limit() -> usize {
    static LIMIT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("RUST_UMITOOLS_DISK_UMIS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(200_000)
    })
}

/// Cluster with an optional scratch directory: bundles larger than the
/// calibrated limit use the disk-backed exact neighbour path.
pub fn cluster_with_scratch(
    umis: &[Umi],
    counts: &[u64],
    method: Method,
    threshold: u32,
    scratch: Option<&std::path::Path>,
) -> Result<Vec<Vec<usize>>> {
    debug_assert_eq!(umis.len(), counts.len());
    if umis.is_empty() {
        return Ok(Vec::new());
    }
    check_uniform_lengths(umis)?;
    let limit = disk_umis_limit();
    if let Some(directory) = scratch {
        if umis.len() > limit && !matches!(method, Method::Unique | Method::Percentile) {
            return crate::core::disk_bundle::disk_cluster(
                umis, counts, method, threshold, directory,
            );
        }
    }
    cluster(umis, counts, method, threshold)
}

/// Count-only counterpart of `cluster_with_scratch`. The disk-backed path
/// still materialises groups, but only because its exact neighbour algorithm
/// is shared with dedup/group; non-disk directional/cluster bundles avoid the
/// group construction entirely.
pub fn cluster_count_with_scratch(
    umis: &[Umi],
    counts: &[u64],
    method: Method,
    threshold: u32,
    scratch: Option<&std::path::Path>,
) -> Result<usize> {
    debug_assert_eq!(umis.len(), counts.len());
    if umis.is_empty() {
        return Ok(0);
    }
    check_uniform_lengths(umis)?;
    let limit = disk_umis_limit();
    if let Some(directory) = scratch {
        if umis.len() > limit && !matches!(method, Method::Unique | Method::Percentile) {
            return crate::core::disk_bundle::disk_cluster(
                umis, counts, method, threshold, directory,
            )
            .map(|groups| groups.len());
        }
    }
    cluster_count(umis, counts, method, threshold)
}

/// Convenience: representative UMI index of every group.
pub fn representatives(groups: &[Vec<usize>]) -> Vec<usize> {
    groups.iter().map(|g| g[0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(raws: &[&str], counts: &[u64]) -> (Vec<Umi>, Vec<u64>) {
        (
            raws.iter().map(|r| Umi::from_bytes(r.as_bytes())).collect(),
            counts.to_vec(),
        )
    }

    #[test]
    fn unique_keeps_insertion_order() {
        let (umis, counts) = bundle(&["TTTT", "AAAA", "CCCC"], &[1, 1, 1]);
        let groups = cluster(&umis, &counts, Method::Unique, 1).unwrap();
        assert_eq!(groups, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn directional_collapses_singletons_at_distance_one() {
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA"], &[1, 1]);
        let groups = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0][0], 0, "first inserted UMI wins the count tie");
    }

    #[test]
    fn directional_requires_two_to_one_support() {
        // 2 vs 2: neither side satisfies count(a) >= 2*count(b)-1.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA"], &[2, 2]);
        let groups = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        assert_eq!(groups.len(), 2, "equal counts must not collapse");

        // 3 vs 2: 3 >= 2*2-1 holds.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA"], &[3, 2]);
        let groups = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn cluster_uses_undirected_components() {
        // A-B at distance 1, B-C at distance 1, A-C at distance 2.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA", "CCAAAAAA"], &[5, 1, 1]);
        let directional = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        assert_eq!(
            directional.len(),
            1,
            "directional: 5 -> 1 -> 1 is reachable"
        );
        let undirected = cluster(&umis, &counts, Method::Cluster, 1).unwrap();
        assert_eq!(undirected.len(), 1);
        assert_eq!(undirected[0][0], 0);
    }

    #[test]
    fn directional_is_not_symmetric() {
        // 1 vs 1 vs 1 chain: A-B, B-C but A and C differ in 2 positions.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA", "CCAAAAAA"], &[1, 1, 1]);
        let groups = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        // Each pair satisfies 1 >= 2*1-1, so the directed graph is symmetric and
        // the whole chain is one component.
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn percentile_keeps_umis_above_one_percent_of_median() {
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA", "CCAAAAAA"], &[1000, 100, 1]);
        let groups = cluster(&umis, &counts, Method::Percentile, 1).unwrap();
        // median = 100, threshold = 1.0 -> counts must be strictly greater.
        assert_eq!(groups, vec![vec![0], vec![1]]);
    }

    #[test]
    fn percentile_single_umi_is_retained() {
        let (umis, counts) = bundle(&["AAAAAAAA"], &[1]);
        let groups = cluster(&umis, &counts, Method::Percentile, 1).unwrap();
        assert_eq!(groups, vec![vec![0]]);
    }

    #[test]
    fn adjacency_splits_by_lead_umi_coverage() {
        // 10 vs 1 vs 1: the high-count UMI does not cover the second singleton
        // on its own, so upstream returns two groups.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA", "CCAAAAAA"], &[10, 1, 1]);
        let groups = cluster(&umis, &counts, Method::Adjacency, 1).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec![0]);
        assert_eq!(groups[1][0], 1);
        assert_eq!(groups[1].len(), 2);
    }

    #[test]
    fn adjacency_covers_the_whole_component_when_one_lead_suffices() {
        // Star topology: the high-count UMI is adjacent to both singletons.
        let (umis, counts) = bundle(&["AAAAAAAA", "CAAAAAAA", "GAAAAAAA"], &[100, 1, 1]);
        let groups = cluster(&umis, &counts, Method::Adjacency, 1).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0][0], 0);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn union_find_matches_traversal_components() {
        let raws = [
            "AAAA", "AAAC", "AAAG", "AACC", "ACCC", "CCCC", "CCCA", "GAAA", "GACA", "TTTT", "TTTA",
            "TTAA", "AGAG", "AGAC", "CACA",
        ];
        let umis: Vec<Umi> = raws
            .iter()
            .map(|raw| Umi::from_bytes(raw.as_bytes()))
            .collect();
        let counts: Vec<u64> = vec![9, 8, 7, 6, 5, 4, 3, 2, 2, 1, 1, 1, 3, 3, 2];
        for threshold in 0..=2u32 {
            let adjacency = build_adjacency(&umis, &counts, threshold, false);
            let traversal = components(&adjacency, &counts, &umis);
            let union = components_union_find(&adjacency, &counts, &umis);
            assert_eq!(traversal, union, "threshold {threshold}");
        }
    }

    #[test]
    fn mixed_lengths_are_rejected() {
        let (umis, counts) = bundle(&["A", "AA"], &[1, 1]);
        assert!(cluster(&umis, &counts, Method::Unique, 1).is_err());
    }

    #[test]
    fn representative_is_highest_count_then_lowest_bytes() {
        // Undirected method so equal counts still form one component.
        let (umis, counts) = bundle(&["CAAAAAAA", "AAAAAAAA"], &[5, 5]);
        let groups = cluster(&umis, &counts, Method::Cluster, 1).unwrap();
        assert_eq!(groups.len(), 1);
        // Tie on count: the byte-smaller UMI leads.
        assert_eq!(umis[groups[0][0]].display(), "AAAAAAAA");
    }

    #[test]
    fn equal_counts_do_not_collapse_in_directional() {
        let (umis, counts) = bundle(&["CAAAAAAA", "AAAAAAAA"], &[5, 5]);
        let groups = cluster(&umis, &counts, Method::Directional, 1).unwrap();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn count_only_matches_full_cluster_for_all_methods() {
        let (umis, counts) = bundle(
            &["AAAA", "AAAT", "AATT", "ATTT", "CCCC", "CCCA", "CCAA"],
            &[100, 50, 25, 12, 7, 3, 1],
        );
        for method in [
            Method::Unique,
            Method::Percentile,
            Method::Directional,
            Method::Cluster,
            Method::Adjacency,
        ] {
            let full = cluster(&umis, &counts, method, 1).unwrap();
            let only = cluster_count(&umis, &counts, method, 1).unwrap();
            assert_eq!(only, full.len(), "method {method:?}");
        }
    }

    #[test]
    fn count_only_matches_union_find_fixture() {
        let raws = [
            "AAAA", "AAAC", "AAAG", "AACC", "ACCC", "CCCC", "CCCA", "GAAA", "GACA", "TTTT", "TTTA",
            "TTAA", "AGAG", "AGAC", "CACA",
        ];
        let umis: Vec<Umi> = raws
            .iter()
            .map(|raw| Umi::from_bytes(raw.as_bytes()))
            .collect();
        let counts: Vec<u64> = vec![9, 8, 7, 6, 5, 4, 3, 2, 2, 1, 1, 1, 3, 3, 2];
        for method in [Method::Directional, Method::Cluster] {
            for threshold in 0..=2u32 {
                let full = cluster(&umis, &counts, method, threshold).unwrap();
                let only = cluster_count(&umis, &counts, method, threshold).unwrap();
                assert_eq!(only, full.len(), "method {method:?}, threshold {threshold}");
            }
        }
    }
}
