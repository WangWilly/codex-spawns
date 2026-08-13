# Codex Spawn Inspection

This context describes how a user explores Codex conversations and the agents spawned from them.

## Language

**Interactive Mode**:
The default terminal interface for navigating conversations and inspecting their spawned agents through incremental, user-driven views.
_Avoid_: Interactive command, wizard

**Command Mode**:
The explicit subcommands retained for scripting, automation, and direct queries, such as `list`, `show`, `sessions`, and `doctor`.
_Avoid_: Legacy mode, batch mode

**Root Conversation**:
A top-level Codex conversation that was not spawned by another agent. It is the primary navigation unit in Interactive Mode and owns the complete descendant agent tree.
_Avoid_: Parent session, main thread

**Agent Session**:
A conversation spawned beneath a Root Conversation to perform delegated work. It appears within its root conversation's agent tree rather than as a separate top-level conversation.
_Avoid_: Child conversation, worker thread

**Conversation Profile**:
A summary of a Root Conversation's complete execution tree, including agent identity, runtime configuration, timing, spawn relationships, status, event diagnostics, and available usage statistics. Full messages and raw evidence are loaded only when explicitly requested.
_Avoid_: Session dump, transcript

**Profile Index**:
A local, incrementally refreshed catalog containing only the metadata and excerpts needed to list Root Conversations and assemble Conversation Profiles. Complete prompts and raw event payloads remain in their rollout sources.
_Avoid_: Cache, transcript database

**Browse Snapshot**:
A stable ordering of Root Conversations used while advancing through cursor-paginated results. Newly indexed conversations appear after an explicit refresh rather than shifting items during the current browse operation.
_Avoid_: Page, result cache

**Conversation Title**:
The human-readable name shown consistently for a Root Conversation in lists and profile headers. It is either supplied by rollout metadata or deterministically derived from the first user message, working directory and start time, or short conversation ID, in that order.
_Avoid_: Task name, agent name

**Spawn Attempt**:
An observed request or state transition intended to create an Agent Session, whether it succeeded, failed, remains unresolved, exists only in Codex state, or produced an agent whose parent evidence is missing. Every Spawn Attempt appears in the Agent Tree with its evidence completeness made explicit.
_Avoid_: Agent, child session

**Profile Fact**:
A profiling value paired with its provenance and confidence state: observed, derived, unknown, or conflicting. Missing evidence is never represented as zero, and conflicting sources remain visible rather than being silently overwritten.
_Avoid_: Metric, inferred value
