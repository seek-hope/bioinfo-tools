//! Core UMI processing shared by the `dedup`, `group` and `count` commands.

pub mod barcode;
pub mod bundle;
pub mod cluster;
pub mod disk_bundle;
pub mod distance;
pub mod neighbors;
pub mod rng;
pub mod umi;
