//! Deterministic bounded SQL-to-Alloy compilation and isolated local orchestration.
#![forbid(unsafe_code)]

pub mod alloy;
pub mod bridge;
mod extended;
pub mod finite;
mod instances;
pub mod pipeline;
pub mod process;
