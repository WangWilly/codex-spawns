# Using Interactive Mode

Run `codex-spawns` without arguments in a terminal. The first screen is a table
of Root Conversations, ordered by `Updated` from newest to oldest by default.
The highlighted row and the visible window move together, so `Enter` always
opens the row shown as selected.

## Read the conversation table

The header stays visible while rows scroll:

| Column | Meaning |
| --- | --- |
| `Title` | A human-readable conversation title |
| `Updated` | Most recent recorded activity, shown in local time |
| `Agents` | Number of agents or observable spawn attempts |
| `Depth` | Deepest known level in the agent tree |
| `State` | Storage lifecycle: `active`, `archived`, or `missing` |
| `Profile` | Evidence quality: `complete`, `partial`, `conflict`, `updating`, or `error` |
| `ID` | Short Root Conversation ID; the preview shows the full ID |

`State` does not mean that an agent finished. Agent execution states such as
`requested`, `spawned`, `complete`, and `failed` appear in the Agent Tree.

The selected-row preview shows the longer title, working directory, full ID,
and title source. The `Title` column remains visible when the remaining columns
are scrolled horizontally.

## Move and return

- Use `↑`/`↓` or `j`/`k` to move one row.
- Use `PageUp`/`PageDown` or `Ctrl+U`/`Ctrl+D` to move one visible page.
- Use `Home`/`End` or `g`/`G` to reach the first or last loaded row.
- Use `←`/`→` or `H`/`L` to scroll columns; `Shift+←` and
  `Shift+→` move by one horizontal viewport.
- Use `Esc`, `Backspace`, or `h` to return. The previous selection, vertical
  and horizontal positions, sorting, and filter are restored.

During search entry, `Backspace` edits the query. Press `Esc` to leave search
before using a return key. Each Agent Tree, detail pane, and help screen keeps
its own viewport. In detail views, `w` switches between wrapped and unwrapped
content.

## Sort the complete catalog

Press `s` to open the sort menu, select `Updated`, `Title`, `Agents`, `Depth`,
`State`, or `Profile`, and press `Enter`. Selecting the active field reverses
the direction. The active header displays `↑` or `↓`.

You can also click a sortable header and click it again to reverse direction.
Sorting applies to the full indexed catalog, not only the rows currently
loaded. A new sorted browse snapshot starts at its first row; equal values use
the full conversation ID as a stable tie-breaker.

## Understand conversation titles

When rollout metadata has no official title, `codex-spawns` derives one from
the first meaningful user-authored text. It removes structured wrappers and
injected plugin, skill, environment, instruction, and attachment metadata.
When no meaningful text remains, it falls back to the working directory and
start time, then to a short conversation ID.

Title extraction is a versioned index projection. After an upgrade changes
the rule, Interactive Mode keeps the last usable snapshot visible while the
index is reprojected transactionally. Apply the refreshed snapshot when the
status bar reports that it is ready. To diagnose migration state, run:

```sh
codex-spawns index status
```

The output reports `projection_version`, `required_projection_version`, and
`needs_reprojection`. A failed reprojection leaves the previous snapshot
available; source rollouts and Codex state databases remain read-only.
