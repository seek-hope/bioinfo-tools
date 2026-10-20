//! Resource budget: threads, memory and temporary storage.
//!
//! The plan requires one budget that covers buffered bundles, batches, sorting
//! and I/O workers instead of several independent limits.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Smallest budget that can hold the working set of one bundle batch.
pub const MIN_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
/// Runs merged in a single pass; bounds open files and reader threads.
pub const MAX_MERGE_FAN_IN: usize = 32;

#[derive(Clone, Debug)]
pub struct Budget {
    pub threads: usize,
    pub memory_bytes: u64,
    pub tmpdir: PathBuf,
    /// Upper bound on UMIs buffered in the bundle window.
    pub max_pending_umis: usize,
    /// Upper bound on UMIs in one parallel batch.
    pub max_batch_umis: usize,
    /// Upper bound on outstanding paired-end mate requests.
    pub max_mates: usize,
    /// Maximum number of sort runs merged in one pass.
    pub max_merge_fanin: usize,
    /// Sort chunk size chosen from memory, cache size and free temporary space.
    pub chunk_bytes: u64,
}

pub fn parse_size(raw: &str) -> Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::usage("empty memory size"));
    }
    let digits_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '_')
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(digits_end);
    let number: f64 = number.replace('_', "").parse().map_err(|_| {
        Error::usage_with_hint(
            format!("cannot parse size '{raw}'"),
            "use e.g. 4GiB, 512MiB or 1000000",
        )
    })?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1e3,
        "ki" | "kib" => 1024.0,
        "m" | "mb" => 1e6,
        "mi" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" => 1e9,
        "gi" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" => 1e12,
        "ti" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        other => {
            return Err(Error::usage_with_hint(
                format!("unknown size unit '{other}'"),
                "use B, KiB, MiB, GiB or TiB",
            ))
        }
    };
    Ok((number * multiplier) as u64)
}

/// Parse a cgroup v2 `memory.max` value; `max` means unlimited.
fn parse_cgroup_memory(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed == "max" || trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<u64>()
        .ok()
        .filter(|value| *value < (1u64 << 60))
}

/// Parse a cgroup v2 `cpu.max` value (`quota period`), rounded up to CPUs.
fn parse_cgroup_cpu(text: &str) -> Option<usize> {
    let mut parts = text.split_whitespace();
    let quota = parts.next()?;
    let period = parts.next()?.parse::<u64>().ok()?;
    if quota == "max" || period == 0 {
        return None;
    }
    let quota = quota.parse::<u64>().ok()?;
    if quota == 0 {
        return None;
    }
    Some(quota.div_ceil(period).max(1) as usize)
}

fn read_text(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn read_u64(path: &str) -> Option<u64> {
    read_text(path)?.trim().parse().ok()
}

/// Memory limit imposed by the container runtime, if any (v2 first, then v1).
fn cgroup_memory_limit() -> Option<u64> {
    if let Some(text) = read_text("/sys/fs/cgroup/memory.max") {
        return parse_cgroup_memory(&text);
    }
    read_u64("/sys/fs/cgroup/memory/memory.limit_in_bytes").filter(|value| *value < (1u64 << 60))
}

fn cgroup_memory_usage() -> u64 {
    read_u64("/sys/fs/cgroup/memory.current")
        .or_else(|| read_u64("/sys/fs/cgroup/memory/memory.usage_in_bytes"))
        .unwrap_or(0)
}

/// CPU limit imposed by the container runtime, if any (v2 first, then v1).
fn cgroup_cpu_limit() -> Option<usize> {
    if let Some(text) = read_text("/sys/fs/cgroup/cpu.max") {
        return parse_cgroup_cpu(&text);
    }
    let quota = read_u64("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")?;
    let period = read_u64("/sys/fs/cgroup/cpu/cpu.cfs_period_us").unwrap_or(100_000);
    if quota == 0 || period == 0 {
        return None;
    }
    Some(quota.div_ceil(period).max(1) as usize)
}

/// `MemAvailable` from /proc/meminfo, reduced by a cgroup memory limit so a
/// container does not size its buffers from the host.
pub fn available_memory() -> u64 {
    let mut available = 2 * 1024 * 1024 * 1024;
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemAvailable:") {
                if let Some(value) = rest.split_whitespace().next() {
                    if let Ok(kib) = value.parse::<u64>() {
                        available = kib * 1024;
                        break;
                    }
                }
            }
        }
    }
    if let Some(limit) = cgroup_memory_limit() {
        let headroom = limit
            .saturating_sub(cgroup_memory_usage())
            .max(64 * 1024 * 1024);
        available = available.min(headroom);
    }
    available
}

pub fn default_threads() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let allowed = cgroup_cpu_limit().map_or(logical, |quota| logical.min(quota));
    allowed.min(16)
}

/// Startup probe used to explain the automatic resource choices.
pub struct HardwareProbe {
    pub logical_cpus: usize,
    pub physical_cpus: usize,
    pub cache_bytes: Option<u64>,
    pub available_memory: u64,
    pub tmp_free_bytes: Option<u64>,
    pub cgroup_memory_limit: Option<u64>,
    pub cgroup_cpu_limit: Option<usize>,
    pub chunk_bytes: u64,
    pub threads: usize,
}

impl HardwareProbe {
    pub fn detect(budget: &Budget) -> HardwareProbe {
        let logical_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);
        HardwareProbe {
            logical_cpus,
            physical_cpus: physical_cpus().unwrap_or(logical_cpus),
            cache_bytes: largest_cache_bytes(),
            available_memory: available_memory(),
            tmp_free_bytes: statvfs_free(&budget.tmpdir),
            cgroup_memory_limit: cgroup_memory_limit(),
            cgroup_cpu_limit: cgroup_cpu_limit(),
            chunk_bytes: budget.chunk_bytes,
            threads: budget.threads,
        }
    }

    pub fn summary(&self) -> String {
        let mib = |value: u64| value / 1_048_576;
        let limit_mem = self
            .cgroup_memory_limit
            .map(|value| format!("{}MiB", mib(value)))
            .unwrap_or_else(|| "unlimited".to_string());
        let limit_cpu = self
            .cgroup_cpu_limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unlimited".to_string());
        format!(
            "hardware            : logical_cpus={} physical_cpus={} cache={}MiB available_memory={}MiB tmp_free={}MiB chunk={}MiB threads={}\ncontainer limits     : memory={} cpu={}",
            self.logical_cpus,
            self.physical_cpus,
            self.cache_bytes.map(mib).unwrap_or(0),
            mib(self.available_memory),
            self.tmp_free_bytes.map(mib).unwrap_or(0),
            mib(self.chunk_bytes),
            self.threads,
            limit_mem,
            limit_cpu
        )
    }
}

/// BGZF reader/writer workers. These threads mostly wait on block I/O, so a
/// small fixed share keeps the total close to the declared thread budget
/// instead of adding a second full-size pool on top of the compute pool.
pub fn io_worker_threads(threads: usize) -> usize {
    std::env::var("RUST_UMITOOLS_IO_THREADS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| (threads / 2).max(1))
}

/// Count unique (physical id, core id) pairs; `None` when the kernel does not
/// expose the topology.
fn physical_cpus() -> Option<usize> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let mut seen = std::collections::HashSet::new();
    for block in text.split("\n\n") {
        let mut physical = None;
        let mut core = None;
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("physical id") {
                physical = rest
                    .split(':')
                    .nth(1)
                    .and_then(|value| value.trim().parse::<u32>().ok());
            } else if let Some(rest) = line.strip_prefix("core id") {
                core = rest
                    .split(':')
                    .nth(1)
                    .and_then(|value| value.trim().parse::<u32>().ok());
            }
        }
        if let (Some(physical), Some(core)) = (physical, core) {
            seen.insert((physical, core));
        }
    }
    (!seen.is_empty()).then_some(seen.len())
}

fn largest_cache_bytes() -> Option<u64> {
    let mut largest = 0u64;
    for index in 0..8 {
        let path = format!("/sys/devices/system/cpu/cpu0/cache/index{index}/size");
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let text = text.trim();
        let split = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let Ok(value) = number.parse::<u64>() else {
            continue;
        };
        let bytes = match unit {
            "K" | "kB" => value * 1024,
            "M" | "MB" => value * 1024 * 1024,
            _ => value,
        };
        largest = largest.max(bytes);
    }
    (largest > 0).then_some(largest)
}

fn statvfs_free(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) };
    (rc == 0).then(|| stats.f_bavail as u64 * stats.f_frsize as u64)
}

/// Free bytes in `path`, used by the sorter before it creates a run.
pub fn tmp_free_bytes(path: &Path) -> Option<u64> {
    statvfs_free(path)
}

/// Fail with an actionable resource error when a temporary run cannot fit.
pub fn ensure_tmp_space(dir: &Path, needed: u64) -> Result<()> {
    if let Some(free) = statvfs_free(dir) {
        if free < needed {
            return Err(Error::Resource(format!(
                "not enough free space in {} for a sort run: {} MiB available, {} MiB needed; \
                 pass --tmpdir with more space",
                dir.display(),
                free / 1_048_576,
                needed / 1_048_576
            )));
        }
    }
    Ok(())
}

impl Budget {
    /// Adjust the sort budget for an explicit strategy. `auto` keeps the
    /// heuristic values chosen from the hardware probe.
    pub fn apply_sort_strategy(&mut self, strategy: &str) -> Result<()> {
        match strategy {
            "auto" => {}
            "low-memory" => {
                self.chunk_bytes = (self.chunk_bytes / 4).max(1024 * 1024);
                self.max_batch_umis = (self.max_batch_umis / 2).max(256);
            }
            "high-throughput" => {
                self.chunk_bytes = self
                    .chunk_bytes
                    .max(self.memory_bytes / 8)
                    .min(192 * 1024 * 1024);
            }
            other => {
                return Err(Error::usage_with_hint(
                    format!("unknown --sort-strategy '{other}'"),
                    "choose one of: auto, low-memory, high-throughput",
                ))
            }
        }
        Ok(())
    }

    pub fn resolve(
        threads: Option<usize>,
        memory: Option<&str>,
        tmpdir: Option<&Path>,
    ) -> Result<Budget> {
        let threads = match threads {
            Some(0) => {
                return Err(Error::usage_with_hint(
                    "--threads must be a positive integer",
                    "omit the option to use the automatic default",
                ))
            }
            Some(value) => value,
            None => default_threads(),
        };
        let memory_bytes = match memory {
            Some(raw) => parse_size(raw)?,
            None => (available_memory() / 2).clamp(512 * 1024 * 1024, 16 * 1024 * 1024 * 1024),
        };
        if memory_bytes < MIN_MEMORY_BYTES {
            return Err(Error::Resource(format!(
                "memory budget {} MiB is below the {} MiB working set this build needs; \
                 raise --memory-limit",
                memory_bytes / 1_048_576,
                MIN_MEMORY_BYTES / 1_048_576
            )));
        }
        let tmpdir = match tmpdir {
            Some(path) => path.to_path_buf(),
            None => std::env::temp_dir(),
        };
        if !tmpdir.is_dir() {
            return Err(Error::usage(format!(
                "temporary directory {} does not exist",
                tmpdir.display()
            )));
        }
        // The limit covers buffered bundles, batches, sorting and I/O workers.
        // HTSlib, the allocator and per-thread stacks need a fixed baseline, so
        // reserve part of the limit for them instead of treating the whole
        // number as data. This keeps the measured RSS close to the requested
        // limit; it is still not a hard whole-process cap.
        let reserve = (memory_bytes / 4).clamp(8 * 1024 * 1024, 32 * 1024 * 1024);
        let working_bytes = memory_bytes - reserve;
        // Roughly 512 bytes of live state per buffered UMI for the hard limit;
        // batches stay far smaller so the pending window and a batch cannot both
        // grow to the limit at once.
        // Diagnostics knob: tests force a small window to exercise the
        // bounded path and the single-bundle resource error.
        let max_pending_umis = std::env::var("RUST_UMITOOLS_MAX_PENDING_UMIS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| (working_bytes / 512).max(1024) as usize);
        let max_batch_umis = ((working_bytes / 16_384).clamp(256, 131_072)) as usize;
        let max_mates = std::env::var("RUST_UMITOOLS_MAX_MATES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(max_pending_umis);

        // The sort chunk is chosen from memory, the largest cache and the free
        // space in the temporary directory. Every cap is clamped to a quarter of
        // the declared budget, so a small --memory-limit cannot be silently
        // exceeded by a fixed floor.
        let quarter = (working_bytes / 4).max(1024 * 1024);
        let mut chunk_bytes = (working_bytes / 32).clamp(1024 * 1024, 192 * 1024 * 1024);
        if let Some(cache) = largest_cache_bytes() {
            // Keep a chunk within an order of magnitude of the last-level cache.
            chunk_bytes = chunk_bytes.min((cache.saturating_mul(8)).max(1024 * 1024));
        }
        if let Some(free) = statvfs_free(&tmpdir) {
            // Leave room for several runs plus the final output.
            chunk_bytes = chunk_bytes.min((free / 8).max(1024 * 1024));
        }
        // Temporary space is checked when a spill actually needs it, so a run
        // that fits in memory is not rejected for an unrelated full disk.
        let chunk_bytes = chunk_bytes.min(quarter).max(1024 * 1024);
        Ok(Budget {
            threads,
            memory_bytes,
            tmpdir,
            max_pending_umis,
            max_batch_umis,
            max_mates,
            max_merge_fanin: MAX_MERGE_FAN_IN,
            chunk_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("512").unwrap(), 512);
        assert_eq!(parse_size("1KiB").unwrap(), 1024);
        assert_eq!(parse_size("4GiB").unwrap(), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("2.5MiB").unwrap(), 2621440);
        assert!(parse_size("4XiB").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn rejects_zero_threads_and_tiny_budget() {
        assert!(Budget::resolve(Some(0), None, None).is_err());
        assert!(Budget::resolve(Some(4), Some("1MiB"), None).is_err());
        assert!(Budget::resolve(Some(4), Some("32MiB"), None).is_err());
        assert!(Budget::resolve(Some(4), Some("1GiB"), None).is_ok());
    }

    #[test]
    fn parses_container_limits() {
        assert_eq!(parse_cgroup_memory("1073741824"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_cgroup_memory("max"), None);
        assert_eq!(parse_cgroup_cpu("200000 100000"), Some(2));
        assert_eq!(parse_cgroup_cpu("150000 100000"), Some(2));
        assert_eq!(parse_cgroup_cpu("max 100000"), None);
        assert_eq!(parse_cgroup_cpu("0 100000"), None);
    }

    #[test]
    fn chunk_stays_inside_the_declared_budget() {
        let budget = Budget::resolve(Some(4), Some("64MiB"), None).unwrap();
        assert!(budget.chunk_bytes <= budget.memory_bytes / 4);
        assert!(budget.chunk_bytes >= 1024 * 1024);
        assert!(budget.max_mates >= 1024);
        assert_eq!(budget.max_merge_fanin, MAX_MERGE_FAN_IN);
    }
}
