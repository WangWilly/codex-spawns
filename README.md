# codex-spawns

Explore a Codex root conversation and every agent it spawned from one fast,
local terminal interface.

```text
Root conversations                 Agent profile
────────────────────────────────   ─────────────────────────────
▶ Improve spawn profiling          model      gpt-5.6-sol
  Release portable binaries        status     completed
  Diagnose index latency           children   3
                                    duration   2m 18s
```

`codex-spawns` combines rollout events, child-session metadata, and optional
read-only Codex state databases. It keeps the root conversation title visible,
shows the complete agent tree, and loads detailed evidence only when requested.

## Features

- Interactive root-conversation browser with cursor pagination, App project assignments, and exact or lower-bound token usage.
- Complete nested agent trees, including requested, failed, state-only, and
  unresolved spawn attempts.
- Requested and effective model, reasoning effort, role, status, timing, tool,
  token, and provenance details when the source records them.
- A local incremental SQLite Profile Index with stale-while-refresh behavior.
- Scriptable table, JSON, JSONL, and CSV output.
- One self-contained Rust binary with bundled SQLite.

## Install

Install the latest release to `~/.local/bin`:

```sh
curl -fsSL https://github.com/WangWilly/codex-spawns/releases/latest/download/install.sh | sh
```

The installer selects the macOS or Linux binary for your architecture and
refuses to install unless its SHA-256 checksum verifies. It never uses `sudo`
or changes your shell profile. Ensure `~/.local/bin` is on your `PATH`.

To install a pinned release or choose another destination:

```sh
curl -fsSL https://github.com/WangWilly/codex-spawns/releases/download/v0.2.0/install.sh \
  | CODEX_SPAWNS_VERSION=v0.2.0 CODEX_SPAWNS_INSTALL_DIR="$HOME/bin" sh
```

See the [installation manual](docs/manual/installing.md) for manual downloads,
script inspection, supported targets, and macOS Gatekeeper notes.

## Quick start

Run with no arguments in a terminal to open Interactive Mode:

```sh
codex-spawns
```

The initial screen lists Root Conversations by recent activity, newest first.
Its fixed headers are `Title | Project | Tokens | Updated | State | Profile |
Agents | Depth | Model | ID`. Open a row to inspect its full-width Agent Table;
open an agent to enter its separate full-screen detail view. Existing index data appears
immediately while changed rollouts refresh in the background.

The default data source is `$CODEX_HOME`, or `~/.codex` when that variable is
unset. Active and archived sessions are included unless disabled.

## Interactive controls

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | Move selection |
| `PageUp` / `PageDown`, `Ctrl+U` / `Ctrl+D` | Move one visible page |
| `Home` / `End`, `g` / `G` | Move to the first or last loaded row |
| `←` / `→`, `H` / `L` | Scroll horizontally |
| `Shift+←` / `Shift+→` | Scroll horizontally by one viewport |
| `Enter` | Open a conversation or agent; apply an available snapshot |
| `Esc`, `Backspace`, `h` | Return to the previous view and position |
| `s` | Choose a sort column; choosing it again reverses direction |
| `/` | Search indexed metadata |
| `f` | Cycle active, archived, and all-conversation filters |
| `p` | Cycle App project filters by stable project ID, then No Project and unknown |
| `r` | Incrementally refresh the index |
| `R` | Confirm and rebuild the index |
| `e` | Open raw evidence on demand |
| `m` | Show the complete task message after the privacy prompt |
| `?` | Show help |
| `q` | Quit |

Mouse input is optional. Status is never communicated by color alone, and
`NO_COLOR` is respected. Clicking a sortable column header selects it; clicking
the active header reverses its direction. A single row click selects; a bounded
double-click opens a conversation or agent. Root and Agent title widths are
stored independently. See the
[Interactive Mode manual](docs/manual/interactive-mode.md) for table semantics,
navigation behavior, sorting, and conversation-title migration.

## Command Mode

Explicit commands remain available for scripts and direct inspection. With no
TTY, running `codex-spawns` without a command falls back to `list`.

```sh
codex-spawns list --format json --limit 25
codex-spawns list --cwd ~/src/my-repo --model gpt-5.6-sol
codex-spawns show 1 --evidence
codex-spawns show 019f --format json --include-message
codex-spawns sessions
codex-spawns doctor
codex-spawns index status
codex-spawns index refresh
codex-spawns index rebuild
codex-spawns index prune --before 1723420800
```

Point the scanner at another Codex home, one or more session trees, or explicit
rollout files:

```sh
codex-spawns --codex-home /path/to/.codex list
codex-spawns --sessions-dir /path/to/sessions list
codex-spawns --file /path/to/rollout.jsonl list
```

Run `codex-spawns --help` or `codex-spawns <command> --help` for the complete
option reference.

## Privacy and local data

All inspection and indexing happens locally. `codex-spawns` does not send
telemetry or upload rollout data. The Profile Index stores display metadata and
message excerpts, not complete prompts, task messages, raw evidence, or
transcripts. Complete content is read from its source only when requested.

Rollout JSONL and Codex state databases are always read-only. `index rebuild`
and `index prune` modify only the disposable Profile Index. Codex App enrichment
is read from the selected `CODEX_HOME`; if either App store is unavailable, the
last valid indexed enrichment is retained. Machine-readable
exports omit complete messages unless `--include-message` is explicit; review
exports before sharing because rollout metadata can still contain private paths
and project details.

## Build from source

Install the pinned Rust toolchain described by `rust-toolchain.toml`, then run:

```sh
cargo build --release
./target/release/codex-spawns --version
```

SQLite is bundled, so the resulting binary does not require a system
`libsqlite3`.

Development checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo bench --bench index_query
```

Maintainers should use the [release manual](docs/manual/releasing.md) and the
[toolchain upgrade manual](docs/manual/upgrading-toolchain.md). Architecture
decisions are recorded in [`docs/adr`](docs/adr/).
