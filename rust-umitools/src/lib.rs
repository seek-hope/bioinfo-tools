//! Rust high-performance implementation of UMI-tools workflows.
//!
//! Reference: UMI-tools 1.1.6 (commit 3f2cb4f41e07fe4a6cea3b3389f534bee957176b).
//! See docs/IMPLEMENTATION_PLAN.md for the compatibility contract and
//! docs/COMPATIBILITY.md for the current support matrix.

pub mod cli;
pub mod commands;
pub mod compat;
pub mod core;
pub mod error;
pub mod io;
pub mod runtime;

pub use error::{Error, Result};
