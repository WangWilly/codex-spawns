//! Public ingestion contract for Codex conversation profiling.
//!
//! [`scan_sources`] combines rollout JSONL and optional Codex state databases.
//! Source files are never modified; SQLite is opened in read-only mode.

mod app_metadata;
mod domain;
pub mod index;
pub mod interactive;
mod parser;
mod projection;

pub use app_metadata::{
    load_app_metadata, AppMetadataError, AppMetadataPaths, AppMetadataSnapshot, AppThreadMetadata,
};
pub use domain::*;
pub use parser::{parse_rollout, scan_sources, scan_sources_with_app_metadata, ParseError};
pub use projection::{
    project_plain_text, project_user_message, PROJECTION_VERSION, TITLE_PROJECTION_VERSION,
};
