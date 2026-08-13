# Reimplement codex-spawns as a Rust binary

Reimplement the Python CLI and the new Interactive Mode as a single Rust binary, using a terminal UI stack such as Ratatui and Crossterm. Rust was selected over Go and Python TUI options because minimizing the distributed artifact and runtime resource usage is the primary constraint, while its ownership and type system provide strong guarantees for concurrent background indexing; the team accepts higher implementation and cross-compilation complexity in exchange. Go remained a viable, simpler delivery option, but smallest practical binary size and runtime control took priority.

## Consequences

Release builds should use size-oriented optimization, LTO, symbol stripping, and an aborting panic strategy where compatible with diagnostics. SQLite linkage and cross-platform targets must be validated explicitly in the release pipeline rather than assumed to work from the host build. Migration is compatibility-first: retain the Python implementation as a reference, compare both implementations against shared fixtures and golden outputs, switch the executable only after command compatibility is demonstrated, and remove Python only after Interactive Mode and index performance pass acceptance checks.

The first release supports macOS and Linux on x86_64 and ARM64; Windows-compatible paths and data structures remain a design constraint but Windows artifacts are not a release gate. Bundle SQLite through `rusqlite` so each artifact is self-contained and uses a tested database version, accepting a larger binary rather than depending on a target machine's system SQLite.

Use Ratatui with Crossterm for Interactive Mode. Background indexing uses standard threads and bounded channels rather than an async runtime; SQLite writes belong to a single writer thread, and long work is divided into cancellable batches with progress events. The workload is local filesystem and database I/O, so avoiding Tokio keeps the binary and concurrency model smaller without sacrificing the required responsiveness.
