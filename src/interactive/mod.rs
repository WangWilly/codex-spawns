//! Pure interactive application state and terminal rendering.
//!
//! I/O adapters translate terminal and repository activity into [`Event`]s and
//! execute returned [`Command`]s. Keeping those effects outside [`App`] makes
//! navigation and refresh behavior deterministic and testable.

mod render;
mod state;

pub use render::render;
pub use state::*;
