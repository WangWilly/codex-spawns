//! Public ingestion contract for Codex conversation profiling.
//!
//! [`scan_sources`] combines rollout JSONL and optional Codex state databases.
//! Source files are never modified; SQLite is opened in read-only mode.

mod domain;
pub mod index;
pub mod interactive;
mod parser;
mod projection;

pub use domain::*;
pub use parser::{parse_rollout, scan_sources, ParseError};
pub use projection::{project_user_message, PROJECTION_VERSION, TITLE_PROJECTION_VERSION};
