# Remove the Python reference implementation

Rust is now the sole production implementation, build system, and test entry point for `codex-spawns`. Remove the Python package, packaging metadata, and Python-only tests and ignore rules; migrate any useful rollout or SQLite fixtures to Rust ownership. The Python implementation was valuable during compatibility-first migration, but retaining two implementations now creates ambiguous ownership, duplicated maintenance, and a false promise that both command paths remain supported.

## Consequences

Command compatibility must be protected by Rust process tests and fixtures rather than differential execution against Python. Cargo becomes the only documented build, development, and verification interface.
