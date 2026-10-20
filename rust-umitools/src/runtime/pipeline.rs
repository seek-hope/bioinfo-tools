//! Ordered parallel execution.
//!
//! Bundles are independent, but the output order is part of the contract, so
//! results are merged back in submission order. The batch itself is the bounded
//! queue: the reader stops pulling records while a batch is processed, which
//! gives natural backpressure.

use crate::error::{Error, Result};
use rayon::prelude::*;

/// Apply `f` to every item in parallel and keep the original order.
pub fn map_ordered<T, R, F>(items: Vec<T>, f: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Sync + Send,
{
    items.into_par_iter().map(f).collect()
}

/// Install the global worker pool from the unified thread budget.
pub fn configure_pool(threads: usize) -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|e| Error::Resource(format!("cannot start {threads} worker threads: {e}")))
}
