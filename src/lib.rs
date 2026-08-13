//! Public ingestion contract for Codex conversation profiling.
//!
//! [`scan_sources`] combines rollout JSONL and optional Codex state databases.
//! Source files are never modified; SQLite is opened in read-only mode.

mod domain;
mod parser;

pub use domain::*;
pub use parser::{parse_rollout, scan_sources, ParseError};
