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

**Conversation Tokens**:
The sum of the final cumulative token-usage snapshot for the Root Conversation and every descendant Agent Session in its complete agent tree. An Agent Session row reports only that session's own final cumulative usage; child usage is not folded into an agent row. Rollout `total_token_usage` is the primary evidence because it carries the complete breakdown; the Codex App thread catalog's `tokens_used` is a total-only fallback and cross-check. When both totals disagree, the rollout value remains the displayed effective value, both sources remain visible as conflicting evidence, and Profile Quality is at least conflicting. Root Conversation and Agent tables show a compact total such as `12.4K`; Agent Detail shows exact input, cached-input, output, reasoning-output, total, and model-context-window values. If only some sessions have evidence, the conversation displays their known subtotal as a lower bound such as `≥12.4K`, marks the fact partial, and reports covered sessions versus total sessions; only an entirely unevidenced tree displays `unknown`. Missing usage is never zero.
_Avoid_: Root tokens, event token sum

**Project**:
The Codex App project explicitly assigned to a Root Conversation. Its identity and display name come from the app's current project catalog, and its assignment comes from the app's current thread-to-project mapping; a Project may own multiple workspace root paths and worktrees. Refresh re-reads both values, so App renames and reassignment appear in the next applied Browse Snapshot without retaining assignment history. A thread explicitly listed as projectless is displayed as `No Project`; a missing, stale, or unreadable assignment is `unknown` and reduces Profile Quality. Filesystem ancestry or `cwd` basename is not a substitute for an absent app assignment. App metadata is an optional enrichment source: unreadable or changed App storage must not prevent startup, erase the last valid indexed values, or block rollout-based profiling. A successfully read refresh atomically replaces the prior App metadata; failures remain visible in status and Profile Quality.
_Avoid_: Repository name, cwd basename, inferred workspace

**Profile Index**:
A local, incrementally refreshed catalog containing only the metadata and excerpts needed to list Root Conversations and assemble Conversation Profiles. Complete prompts and raw event payloads remain in their rollout sources.
_Avoid_: Cache, transcript database

**Browse Snapshot**:
A stable ordering of Root Conversations used while advancing through cursor-paginated results. Newly indexed conversations appear after an explicit refresh rather than shifting items during the current browse operation.
_Avoid_: Page, result cache

**Viewport**:
The visible window over the currently loaded rows and columns in Interactive Mode. In tables, arrow keys are cursor-first: Up and Down move the row cursor within the visible window, and Left and Right move the focused column; only after the cursor reaches a viewport edge does the viewport reveal one additional row or one complete column at a time. The cursor remains visible. Page Up and Page Down move by a viewport, Home and End move to data boundaries, and mouse-wheel scrolling clamps the cursor to the nearest visible row or column when needed. Detail views have no table cursor, so their arrow keys scroll directly. Viewport movement is distinct from loading another cursor page from the Profile Index.
_Avoid_: Cursor page, browse snapshot

**Conversation Title**:
The human-readable name shown consistently for a Root Conversation in lists, profile headers, and the root row of an Agent Table. The current Codex App thread catalog title is authoritative and refresh reflects App renames in the next applied Browse Snapshot without retaining title history. If that title is unavailable, the deterministic fallback is the first meaningful user-authored text after structured content, injected instructions, plugin catalogs, skill metadata, environment context, and attachment metadata are removed; working directory and start time, then short conversation ID, are the final fallbacks. Markdown input is parsed using CommonMark semantics and projected to single-line display text: formatting syntax and HTML are removed, meaningful text such as link labels, code content, and image alternative text is retained, whitespace is collapsed, and terminal-width truncation is applied. Tables never display the unparsed Markdown source or inline Markdown styling.
_Avoid_: Task name, agent name

**Agent Table**:
The default tabular view of a Root Conversation's complete agent tree. Rows use parent-first preorder, with indentation and tree glyphs in `Title` preserving ancestry; arbitrary column sorting is not provided because it would destroy the tree relationship. Its fixed column order is `Title | Agent Name | Nickname | Model | Effort | Role | Status | Tokens | ID`. `Title` is frozen while the remaining columns share a horizontal viewport. `Title` is the CommonMark-projected spawn message summary, `Agent Name` is the spawn request's task name, and `Nickname` is the runtime-provided agent nickname. The root row uses the Conversation Title and the fixed Agent Name `root conversation`. Every Spawn Attempt remains a row, including requested, failed, orphan, and state-only evidence; a fulfilled attempt and its Agent Session merge into one row. Rows without a session use requested runtime values where available, keep Tokens unknown, and expose missing evidence in Detail. Full agent details are not displayed beside the table by default: Enter or a mouse double-click opens a dedicated full-screen detail view, while a single click only selects a row. Back restores the selected row, vertical and horizontal viewports, and wrap state; the detail view has an independent scroll position.
_Avoid_: Split agent view, selected-agent sidebar

**Root Conversation Table**:
The catalog table for Root Conversations. Its fixed column order is `Title | Project | Tokens | Updated | State | Profile | Agents | Depth | Model | ID`. `Title` is frozen while the remaining columns share a horizontal viewport. Project and Tokens participate in catalog-wide sorting while the default remains Updated descending. Project names sort case-insensitively; Tokens sort numerically by exact total or known subtotal. `No Project` and `unknown` remain after assigned/known values, with `No Project` first, and conversation ID is the stable tie-break. Catalog search includes Project display names. Project filtering uses the stable project ID rather than the possibly duplicated display name, and can explicitly select `No Project` or `unknown`; Tokens are not text-searchable and have no range filter. Root and Agent tables persist independent adjustable Title widths; narrow layouts retain the frozen Title and at least one horizontally scrollable field.
_Avoid_: Conversation list without headers

**Spawn Attempt**:
An observed request or state transition intended to create an Agent Session, whether it succeeded, failed, remains unresolved, exists only in Codex state, or produced an agent whose parent evidence is missing. Every Spawn Attempt appears in the Agent Tree with its evidence completeness made explicit.
_Avoid_: Agent, child session

**Profile Fact**:
A profiling value paired with its provenance and confidence state: observed, derived, unknown, or conflicting. Missing evidence is never represented as zero, and conflicting sources remain visible rather than being silently overwritten.
_Avoid_: Metric, inferred value

**Conversation State**:
The storage lifecycle of a Root Conversation: active, archived, or missing. It does not describe whether agents finished executing or whether profiling evidence is complete.
_Avoid_: Agent status, profile completeness

**Profile Quality**:
The evidence quality of a Conversation Profile: complete, partial, conflicting, updating, or error. Agent execution states such as requested, spawned, complete, and failed remain separate and appear in the Agent Tree.
_Avoid_: Conversation state, completion status

**Maintainer Manual**:
Documentation under `docs/manual/` for workflows that require human judgement or intervention, such as preparing and validating a release. The README links to these procedures but does not duplicate them.
_Avoid_: Runbook scattered across README, release notes
