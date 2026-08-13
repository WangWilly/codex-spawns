# Use a local SQLite profile index

Interactive Mode needs to show Root Conversations quickly without reparsing every rollout before the first screen. Maintain a separate, rebuildable SQLite Profile Index under the Codex home, incrementally refreshed from file identity and modification metadata; store only profile metadata and excerpts, load full messages and evidence from the source rollout on demand, and keep rollout files and Codex `state_*.sqlite` databases read-only. SQLite was chosen over a JSON cache because it supports indexed filtering, parent-child queries, transactions, schema migration, and stable cursor pagination without loading the complete catalog into memory.

## Consequences

The CLI must support index refresh, rebuild, schema migration, a configurable index path, and a `--no-cache` path. Interactive Mode uses stale-while-refresh: it immediately opens an existing browse snapshot, refreshes the index in the background, and applies newly indexed data only when the user requests it. First-time indexing prioritizes enough recent sources to produce the first result page before continuing in batches. Deleting the index must never delete or modify source data.

Persist a projection schema version independently from the storage schema. When title extraction or another display projection changes, Interactive Mode continues to show the last usable snapshot while affected Root Conversations are reprojected transactionally in the background; users apply the completed snapshot explicitly, and `index status` reports current and required projection versions.
