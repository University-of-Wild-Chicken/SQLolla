//! Loopback browser host for the existing deterministic pipeline and isolated AI annotations.
#![forbid(unsafe_code)]

pub mod ai;
pub mod command;
pub mod config;
pub mod error;
pub mod security;
pub mod server;
pub mod state;
mod worker;

pub use worker::Worker;

/// Exact production assistance model; provider substitution is never implicit.
pub const MODEL: &str = "gpt-6.1-sol";
/// Maximum complete incoming JSON command; larger requests are refused, not truncated.
pub const MAX_REQUEST_BYTES: usize = 2_097_152;
/// Maximum browser response, including complete retained instance records.
pub const MAX_RESPONSE_BYTES: usize = 16_777_216;
