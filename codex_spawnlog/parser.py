from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import sqlite3
from typing import Any, Iterable, Iterator

from .models import RolloutMeta, ScanResult, SpawnCall, SpawnRecord, shorten


UUID_RE = re.compile(
    r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
)
ROLLOUT_NAME_RE = re.compile(
    r"^rollout-(?P<timestamp>.+)-(?P<id>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$",
    re.IGNORECASE,
)

SKIP_DEEP_KEYS = {
    "content",
    "message",
    "text",
    "arguments",
    "output",
    "replacement_history",
    "history",
    "base_instructions",
    "user_instructions",
    "encrypted_content",
}


@dataclass
class ParsedRollout:
    meta: RolloutMeta
    calls: list[SpawnCall]


def parse_timestamp(value: str | None) -> datetime | None:
    if not value:
        return None
    value = value.strip()
    if value.endswith("Z"):
        value = value[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=timezone.utc)
    return parsed


def timestamp_sort_key(value: str | None) -> tuple[int, str]:
    parsed = parse_timestamp(value)
    if parsed is None:
        return (0, value or "")
    return (1, parsed.astimezone(timezone.utc).isoformat())


def discover_sources(
    *,
    files: Iterable[str | Path] = (),
    sessions_dirs: Iterable[str | Path] = (),
    codex_home: str | Path | None = None,
    include_archived: bool = True,
    state_databases: Iterable[str | Path] = (),
    include_state_databases: bool = True,
) -> tuple[list[Path], list[Path], list[str]]:
    """Resolve rollout files and optional state databases without touching their contents."""

    diagnostics: list[str] = []
    explicit_files = [_expand_path(p) for p in files]
    explicit_dirs = [_expand_path(p) for p in sessions_dirs]

    rollout_files: list[Path] = []
    if explicit_files:
        for path in explicit_files:
            if path.is_file():
                rollout_files.append(path)
            else:
                diagnostics.append(f"rollout file not found: {path}")
    else:
        roots: list[Path] = []
        if explicit_dirs:
            roots.extend(explicit_dirs)
        else:
            homes = _codex_home_candidates(codex_home)
            for home in _unique_paths(homes):
                roots.append(home / "sessions")
                if include_archived:
                    roots.append(home / "archived_sessions")

        for root in _unique_paths(roots):
            if not root.exists():
                diagnostics.append(f"rollout root not found: {root}")
                continue
            if not root.is_dir():
                diagnostics.append(f"rollout root is not a directory: {root}")
                continue
            try:
                rollout_files.extend(
                    path for path in root.rglob("*.jsonl") if path.is_file()
                )
            except OSError as exc:
                diagnostics.append(f"cannot scan rollout root {root}: {exc}")

    rollout_files = sorted(_unique_paths(rollout_files), key=lambda p: str(p))

    dbs: list[Path] = []
    explicit_dbs = [_expand_path(p) for p in state_databases]
    if explicit_dbs:
        for path in explicit_dbs:
            if path.is_file():
                dbs.append(path)
            else:
                diagnostics.append(f"state database not found: {path}")
    elif include_state_databases:
        homes = _codex_home_candidates(codex_home)
        for home in _unique_paths(homes):
            try:
                dbs.extend(path for path in home.glob("state_*.sqlite") if path.is_file())
            except OSError as exc:
                diagnostics.append(f"cannot scan state databases in {home}: {exc}")

    return rollout_files, sorted(_unique_paths(dbs), key=lambda p: str(p)), diagnostics


def scan_sources(
    rollout_files: Iterable[str | Path],
    *,
    state_databases: Iterable[str | Path] = (),
    initial_diagnostics: Iterable[str] = (),
) -> ScanResult:
    files = [Path(path) for path in rollout_files]
    result = ScanResult(files=[str(path) for path in files], diagnostics=list(initial_diagnostics))
    parsed: list[ParsedRollout] = []

    for path in files:
        try:
            parsed_rollout = parse_rollout(path)
        except OSError as exc:
            result.diagnostics.append(f"cannot read {path}: {exc}")
            continue
        parsed.append(parsed_rollout)
        result.rollouts.append(parsed_rollout.meta)
        if parsed_rollout.meta.parse_errors:
            result.diagnostics.append(
                f"{path}: skipped {parsed_rollout.meta.parse_errors} malformed JSONL line(s)"
            )

    result.state_databases = [str(Path(path)) for path in state_databases]
    records = build_spawn_records(parsed)
    for db_path in state_databases:
        try:
            edges = read_spawn_edges(Path(db_path))
        except (OSError, sqlite3.Error) as exc:
            result.diagnostics.append(f"cannot read state database {db_path}: {exc}")
            continue
        merge_state_edges(records, edges, Path(db_path), result.diagnostics)
    result.records = sorted(records, key=lambda item: timestamp_sort_key(item.created_at))
    return result


def parse_rollout(path: Path) -> ParsedRollout:
    meta = RolloutMeta(path=str(path))
    calls: list[SpawnCall] = []
    pending_by_call_id: dict[str, SpawnCall] = {}
    pending_without_id: list[SpawnCall] = []

    filename_match = ROLLOUT_NAME_RE.match(path.name)
    if filename_match:
        meta.session_id = filename_match.group("id")
        meta.created_at = _filename_timestamp(filename_match.group("timestamp"))

    with path.open("r", encoding="utf-8", errors="replace") as stream:
        for line_number, raw_line in enumerate(stream, start=1):
            if not raw_line.strip():
                continue
            try:
                event = json.loads(raw_line)
            except json.JSONDecodeError:
                meta.parse_errors += 1
                continue
            if not isinstance(event, dict):
                continue

            meta.event_count += 1
            event_timestamp = _event_timestamp(event)
            if event_timestamp:
                if meta.first_event_at is None:
                    meta.first_event_at = event_timestamp
                meta.last_event_at = event_timestamp
                if meta.created_at is None:
                    meta.created_at = event_timestamp

            event_type = event.get("type")
            payload = event.get("payload")
            if not isinstance(payload, dict):
                payload = event

            if event_type == "session_meta":
                _update_session_meta(meta, payload, line_number)
            elif event_type in {"turn_context", "task_started"}:
                _update_runtime_meta(meta, payload)

            call = _extract_spawn_call(event, meta, line_number, event_timestamp)
            if call is not None:
                calls.append(call)
                if call.call_id:
                    pending_by_call_id[call.call_id] = call
                else:
                    pending_without_id.append(call)
                continue

            output = _extract_function_output(event, line_number, event_timestamp)
            if output is not None:
                call = None
                if output[0]:
                    call = pending_by_call_id.get(output[0])
                if call is None and pending_without_id:
                    call = pending_without_id[-1]
                if call is not None:
                    output_value = output[1]
                    call.output = output_value
                    call.output_line = line_number
                    child_ids = _extract_child_ids(output_value)
                    for child_id in child_ids:
                        if child_id not in call.child_thread_ids:
                            call.child_thread_ids.append(child_id)
                    call.output_error = _extract_error(output_value)
                continue

            activity = _extract_subagent_activity(event)
            if activity is not None:
                activity_ids, activity_kind = activity
                target = _latest_unresolved_call(calls)
                if target is not None:
                    target.activity_kind = activity_kind
                    target.activity_line = line_number
                    for child_id in activity_ids:
                        if child_id not in target.child_thread_ids:
                            target.child_thread_ids.append(child_id)

    if meta.session_id is None:
        meta.session_id = _deep_first(meta.__dict__, "id")
    return ParsedRollout(meta=meta, calls=calls)


def build_spawn_records(parsed: list[ParsedRollout]) -> list[SpawnRecord]:
    sessions_by_id: dict[str, RolloutMeta] = {
        rollout.meta.session_id: rollout.meta
        for rollout in parsed
        if rollout.meta.session_id
    }
    calls: list[SpawnCall] = []
    for rollout in parsed:
        for call in rollout.calls:
            if call.parent_session_id is None:
                call.parent_session_id = rollout.meta.session_id
            calls.append(call)

    records: list[SpawnRecord] = []
    unresolved_call_records: list[tuple[SpawnCall, SpawnRecord]] = []
    record_by_child_id: dict[str, SpawnRecord] = {}
    child_sessions = [rollout.meta for rollout in parsed if rollout.meta.is_subagent]

    for call in calls:
        child_ids = call.child_thread_ids or [None]
        for child_id in child_ids:
            child = sessions_by_id.get(child_id) if child_id else None
            record = _record_from_call(call, child, sessions_by_id)
            records.append(record)
            if child_id:
                record_by_child_id[child_id] = record
            else:
                unresolved_call_records.append((call, record))

    for child in child_sessions:
        if not child.session_id:
            continue
        if child.session_id in record_by_child_id:
            continue
        matched = _match_call_to_child(unresolved_call_records, child)
        if matched is not None and matched.child_thread_id is None:
            matched.child_thread_id = child.session_id
            matched.child_path = child.path
            matched.child_cwd = child.cwd
            matched.child_line = child.session_meta_line
            _merge_child_fields(matched, child)
            record_by_child_id[child.session_id] = matched
            continue

        record = _record_from_child(child, sessions_by_id)
        records.append(record)
        record_by_child_id[child.session_id] = record

    return _deduplicate_records(records)


def read_spawn_edges(path: Path) -> list[dict[str, Any]]:
    """Read the optional state DB in read-only mode, tolerating schema changes."""

    uri = f"file:{path.resolve().as_posix()}?mode=ro"
    connection = sqlite3.connect(uri, uri=True)
    connection.row_factory = sqlite3.Row
    try:
        table = connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges'"
        ).fetchone()
        if table is None:
            return []
        columns = [row[1] for row in connection.execute("PRAGMA table_info(thread_spawn_edges)")]
        if not columns:
            return []
        rows = connection.execute("SELECT * FROM thread_spawn_edges").fetchall()
        return [{column: row[column] for column in columns} for row in rows]
    finally:
        connection.close()


def merge_state_edges(
    records: list[SpawnRecord],
    edges: list[dict[str, Any]],
    db_path: Path,
    diagnostics: list[str],
) -> None:
    aliases = {
        "parent": ("parent_thread_id", "parent_id", "source_thread_id"),
        "child": ("child_thread_id", "child_id", "agent_id", "receiver_thread_id"),
        "status": ("status", "state", "terminal_state"),
        "created": ("created_at", "timestamp", "started_at"),
    }

    def field(row: dict[str, Any], names: tuple[str, ...]) -> Any:
        for name in names:
            if name in row and row[name] is not None:
                return row[name]
        return None

    for edge in edges:
        parent_id = _as_text(field(edge, aliases["parent"]))
        child_id = _as_text(field(edge, aliases["child"]))
        if not child_id and not parent_id:
            continue
        record = next(
            (
                item
                for item in records
                if (child_id and item.child_thread_id == child_id)
                or (
                    parent_id
                    and item.parent_thread_id == parent_id
                    and not item.child_thread_id
                )
            ),
            None,
        )
        if record is None:
            record_id = _stable_id("state", parent_id, child_id, str(field(edge, aliases["created"])))
            record = SpawnRecord(
                id=record_id,
                created_at=_as_text(field(edge, aliases["created"])),
                status="state-only",
                parent_thread_id=parent_id,
                child_thread_id=child_id,
                source="state-db",
                state_status=_as_text(field(edge, aliases["status"])),
                state_source=str(db_path),
            )
            records.append(record)
        else:
            record.state_status = _as_text(field(edge, aliases["status"]))
            record.state_source = str(db_path)
            if record.source == "child-metadata":
                record.source = "rollout+state-db"

    if edges and not records:
        diagnostics.append(f"state database {db_path} contained spawn edges but no records were built")


def _record_from_call(
    call: SpawnCall,
    child: RolloutMeta | None,
    sessions_by_id: dict[str, RolloutMeta],
) -> SpawnRecord:
    parent = sessions_by_id.get(call.parent_session_id or "")
    child_id = child.session_id if child else (call.child_thread_ids[0] if call.child_thread_ids else None)
    status = "spawned" if child is not None or call.child_thread_ids else "requested"
    if call.output_error:
        status = "failed" if not child else status
    record_id = _stable_id(
        "call",
        call.parent_session_id,
        call.call_id,
        str(call.line),
        child_id,
    )
    record = SpawnRecord(
        id=record_id,
        created_at=call.timestamp or (child.created_at if child else None),
        status=status,
        parent_thread_id=call.parent_session_id,
        child_thread_id=child_id,
        parent_path=call.parent_path,
        child_path=child.path if child else None,
        parent_cwd=parent.cwd if parent else None,
        child_cwd=child.cwd if child else None,
        task_name=call.task_name,
        message=call.message,
        agent_type=call.agent_type,
        requested_model=call.requested_model,
        requested_effort=call.requested_effort,
        fork_turns=call.fork_turns,
        effective_model=child.model if child else None,
        effective_effort=child.effort if child else None,
        multi_agent_version=child.multi_agent_version if child else None,
        depth=child.depth if child else None,
        source="rollout",
        call_id=call.call_id,
        parent_line=call.line,
        child_line=child.session_meta_line if child else None,
        output_line=call.output_line,
        output_error=call.output_error,
    )
    if child:
        _merge_child_fields(record, child)
        if not record.task_name:
            record.task_name = child.agent_path or child.agent_nickname
    record.evidence.append(
        {
            "kind": "parent_spawn_call",
            "path": call.parent_path,
            "line": call.line,
            "timestamp": call.timestamp,
            "call_id": call.call_id,
            "name": call.name,
            "arguments": _safe_arguments(call.arguments),
        }
    )
    if call.output_line:
        record.evidence.append(
            {
                "kind": "parent_spawn_output",
                "path": call.parent_path,
                "line": call.output_line,
                "output": call.output,
            }
        )
    return record


def _record_from_child(child: RolloutMeta, sessions_by_id: dict[str, RolloutMeta]) -> SpawnRecord:
    parent = sessions_by_id.get(child.parent_thread_id or "")
    record_id = _stable_id("child", child.parent_thread_id, child.session_id, child.path)
    record = SpawnRecord(
        id=record_id,
        created_at=child.created_at,
        status="spawned",
        parent_thread_id=child.parent_thread_id,
        child_thread_id=child.session_id,
        parent_path=parent.path if parent else None,
        child_path=child.path,
        parent_cwd=parent.cwd if parent else None,
        child_cwd=child.cwd,
        task_name=child.agent_path or child.agent_nickname,
        agent_role=child.agent_role,
        agent_nickname=child.agent_nickname,
        agent_path=child.agent_path,
        effective_model=child.model,
        effective_effort=child.effort,
        multi_agent_version=child.multi_agent_version,
        depth=child.depth,
        source="child-metadata",
        child_line=child.session_meta_line,
    )
    record.evidence.append(
        {
            "kind": "child_session_meta",
            "path": child.path,
            "line": child.session_meta_line,
            "timestamp": child.created_at,
        }
    )
    return record


def _merge_child_fields(record: SpawnRecord, child: RolloutMeta) -> None:
    record.agent_role = record.agent_role or child.agent_role
    record.agent_nickname = record.agent_nickname or child.agent_nickname
    record.agent_path = record.agent_path or child.agent_path
    record.effective_model = record.effective_model or child.model
    record.effective_effort = record.effective_effort or child.effort
    record.multi_agent_version = record.multi_agent_version or child.multi_agent_version
    record.depth = record.depth if record.depth is not None else child.depth
    record.child_thread_id = record.child_thread_id or child.session_id
    record.child_path = record.child_path or child.path
    record.child_cwd = record.child_cwd or child.cwd


def _match_call_to_child(
    candidates: list[tuple[SpawnCall, SpawnRecord]], child: RolloutMeta
) -> SpawnRecord | None:
    candidates = [
        (call, record)
        for call, record in candidates
        if call.parent_session_id == child.parent_thread_id
    ]
    if not candidates:
        return None

    child_names = {
        name.lower()
        for value in (child.agent_path, child.agent_nickname, child.agent_role)
        if value
        for name in (value, value.rsplit("/", 1)[-1], value.removeprefix("/root/"))
        if name
    }
    for call, record in candidates:
        call_name = (call.task_name or "").lower()
        call_leaf = call_name.rsplit("/", 1)[-1]
        if call_name and any(
            call_name == name or call_leaf == name or call_name.endswith("/" + name)
            for name in child_names
        ):
            return record

    child_time = parse_timestamp(child.created_at)
    if child_time is None:
        return candidates[0][1] if len(candidates) == 1 else None
    scored: list[tuple[float, SpawnRecord]] = []
    for call, record in candidates:
        call_time = parse_timestamp(call.timestamp)
        if call_time is None:
            continue
        delta = abs((child_time - call_time).total_seconds())
        if delta <= 3600:
            scored.append((delta, record))
    if len(scored) == 1:
        return scored[0][1]
    return min(scored, key=lambda item: item[0])[1] if scored else None


def _deduplicate_records(records: list[SpawnRecord]) -> list[SpawnRecord]:
    result: list[SpawnRecord] = []
    by_id: dict[str, SpawnRecord] = {}
    by_child: dict[str, SpawnRecord] = {}
    for record in records:
        existing = by_id.get(record.id)
        if existing is not None:
            _merge_records(existing, record)
            continue
        if record.child_thread_id and record.child_thread_id in by_child:
            _merge_records(by_child[record.child_thread_id], record)
            continue
        by_id[record.id] = record
        if record.child_thread_id:
            by_child[record.child_thread_id] = record
        result.append(record)
    return result


def _merge_records(target: SpawnRecord, source: SpawnRecord) -> None:
    for field_name in (
        "created_at",
        "parent_thread_id",
        "child_thread_id",
        "parent_path",
        "child_path",
        "parent_cwd",
        "child_cwd",
        "task_name",
        "message",
        "agent_type",
        "agent_role",
        "agent_nickname",
        "agent_path",
        "requested_model",
        "requested_effort",
        "fork_turns",
        "effective_model",
        "effective_effort",
        "multi_agent_version",
        "depth",
        "call_id",
        "parent_line",
        "child_line",
        "output_line",
        "output_error",
        "state_status",
        "state_source",
    ):
        if getattr(target, field_name) is None and getattr(source, field_name) is not None:
            setattr(target, field_name, getattr(source, field_name))
    if target.status in {"requested", "state-only"} and source.status == "spawned":
        target.status = "spawned"
    if target.source == "child-metadata" and source.source == "rollout":
        target.source = "rollout+child-metadata"
    target.evidence.extend(item for item in source.evidence if item not in target.evidence)


def _extract_spawn_call(
    event: dict[str, Any],
    meta: RolloutMeta,
    line_number: int,
    timestamp: str | None,
) -> SpawnCall | None:
    payload = event.get("payload") if isinstance(event.get("payload"), dict) else event
    event_kind = payload.get("type") or event.get("type")
    if event_kind not in {"function_call", "custom_tool_call", "tool_call", "function"}:
        return None
    name = _as_text(
        payload.get("name")
        or event.get("name")
        or _nested_value(payload.get("function"), "name")
        or _nested_value(payload.get("tool"), "name")
    )
    if not _is_spawn_name(name):
        return None
    arguments = _parse_object(
        payload.get("arguments")
        if payload.get("arguments") is not None
        else payload.get("input")
        if payload.get("input") is not None
        else payload.get("args")
    )
    return SpawnCall(
        parent_path=meta.path,
        parent_session_id=meta.session_id,
        timestamp=timestamp,
        line=line_number,
        call_id=_as_text(payload.get("call_id") or event.get("call_id") or payload.get("id")),
        name=name or "spawn_agent",
        arguments=arguments,
    )


def _extract_function_output(
    event: dict[str, Any], line_number: int, timestamp: str | None
) -> tuple[str | None, Any] | None:
    payload = event.get("payload") if isinstance(event.get("payload"), dict) else event
    event_kind = payload.get("type") or event.get("type")
    if event_kind not in {"function_call_output", "custom_tool_call_output", "tool_result", "function_output"}:
        return None
    call_id = _as_text(payload.get("call_id") or event.get("call_id"))
    output = payload.get("output")
    if output is None:
        output = payload.get("result")
    if output is None:
        output = payload.get("content")
    return call_id, _parse_json_if_possible(output)


def _extract_subagent_activity(event: dict[str, Any]) -> tuple[list[str], str] | None:
    payload = event.get("payload") if isinstance(event.get("payload"), dict) else event
    kind = _as_text(payload.get("type") or event.get("type"))
    if kind not in {"sub_agent_activity", "subagent_activity", "agent_activity"}:
        return None
    ids = _extract_child_ids(payload)
    activity_kind = _as_text(payload.get("kind") or payload.get("status") or "unknown") or "unknown"
    return ids, activity_kind


def _latest_unresolved_call(calls: list[SpawnCall]) -> SpawnCall | None:
    for call in reversed(calls):
        if not call.child_thread_ids and not call.output_error:
            return call
    return None


def _update_session_meta(meta: RolloutMeta, payload: dict[str, Any], line_number: int) -> None:
    meta.session_meta_line = line_number
    meta.session_id = _as_text(payload.get("id") or payload.get("session_id")) or meta.session_id
    meta.created_at = _as_text(payload.get("timestamp") or payload.get("created_at")) or meta.created_at
    meta.cwd = _as_text(payload.get("cwd") or payload.get("workdir")) or meta.cwd
    meta.originator = _as_text(payload.get("originator")) or meta.originator
    meta.cli_version = _as_text(payload.get("cli_version") or payload.get("version")) or meta.cli_version
    meta.model = _as_text(payload.get("model")) or meta.model
    source = payload.get("source")
    source_text = source if isinstance(source, str) else None
    meta.source = source_text or meta.source
    meta.thread_source = _as_text(payload.get("thread_source")) or meta.thread_source

    spawn = _find_thread_spawn(payload)
    if spawn:
        meta.source = "subagent"
        meta.thread_source = meta.thread_source or "subagent"
        meta.parent_thread_id = _as_text(
            spawn.get("parent_thread_id") or spawn.get("parent_id")
        ) or meta.parent_thread_id
        meta.forked_from_id = _as_text(
            spawn.get("forked_from_id") or spawn.get("fork_from_id")
        ) or meta.forked_from_id
        meta.agent_path = _as_text(spawn.get("agent_path")) or meta.agent_path
        meta.agent_nickname = _as_text(
            spawn.get("agent_nickname") or spawn.get("nickname")
        ) or meta.agent_nickname
        meta.agent_role = _as_text(
            spawn.get("agent_role") or spawn.get("role") or spawn.get("agent_type")
        ) or meta.agent_role
        meta.depth = _as_int(spawn.get("depth")) if spawn.get("depth") is not None else meta.depth

    meta.parent_thread_id = _as_text(
        payload.get("parent_thread_id") or payload.get("parent_id")
    ) or meta.parent_thread_id
    meta.forked_from_id = _as_text(payload.get("forked_from_id")) or meta.forked_from_id
    meta.agent_path = _as_text(payload.get("agent_path")) or meta.agent_path
    meta.agent_nickname = _as_text(payload.get("agent_nickname")) or meta.agent_nickname
    meta.agent_role = _as_text(payload.get("agent_role")) or meta.agent_role
    if payload.get("depth") is not None:
        meta.depth = _as_int(payload.get("depth"))
    if meta.parent_thread_id:
        meta.thread_source = meta.thread_source or "subagent"


def _update_runtime_meta(meta: RolloutMeta, payload: dict[str, Any]) -> None:
    meta.model = _as_text(payload.get("model")) or meta.model
    meta.effort = _as_text(
        payload.get("effort") or payload.get("reasoning_effort") or payload.get("model_reasoning_effort")
    ) or meta.effort
    meta.multi_agent_version = _as_text(payload.get("multi_agent_version")) or meta.multi_agent_version
    collaboration_mode = payload.get("collaboration_mode")
    if isinstance(collaboration_mode, dict):
        settings = collaboration_mode.get("settings")
        if isinstance(settings, dict):
            meta.model = _as_text(settings.get("model")) or meta.model
            meta.effort = _as_text(settings.get("reasoning_effort")) or meta.effort


def _find_thread_spawn(payload: dict[str, Any]) -> dict[str, Any] | None:
    source = payload.get("source")
    if isinstance(source, dict):
        subagent = source.get("subagent")
        if isinstance(subagent, dict):
            spawn = subagent.get("thread_spawn")
            if isinstance(spawn, dict):
                return spawn
        spawn = source.get("thread_spawn")
        if isinstance(spawn, dict):
            return spawn
    for value in _walk_dicts(payload, max_depth=5):
        if "thread_spawn" in value and isinstance(value["thread_spawn"], dict):
            return value["thread_spawn"]
    return None


def _walk_dicts(value: Any, *, max_depth: int, depth: int = 0) -> Iterator[dict[str, Any]]:
    if depth > max_depth:
        return
    if isinstance(value, dict):
        yield value
        for key, child in value.items():
            if key in SKIP_DEEP_KEYS:
                continue
            yield from _walk_dicts(child, max_depth=max_depth, depth=depth + 1)
    elif isinstance(value, list) and depth < max_depth:
        for child in value[:100]:
            yield from _walk_dicts(child, max_depth=max_depth, depth=depth + 1)


def _deep_first(value: Any, *keys: str) -> Any:
    wanted = set(keys)
    for candidate in _walk_dicts(value, max_depth=5):
        for key in wanted:
            if key in candidate:
                return candidate[key]
    return None


def _extract_child_ids(value: Any) -> list[str]:
    ids: list[str] = []
    id_keys = {
        "agent_id",
        "child_thread_id",
        "receiver_thread_id",
        "thread_id",
        "receiver_thread_ids",
        "child_thread_ids",
        "agent_ids",
    }
    for candidate in _walk_dicts(value, max_depth=6):
        for key in id_keys:
            if key not in candidate:
                continue
            raw = candidate[key]
            values = raw if isinstance(raw, list) else [raw]
            for item in values:
                text = _as_text(item)
                if text and UUID_RE.fullmatch(text) and text not in ids:
                    ids.append(text)
    if not ids and isinstance(value, str):
        for item in UUID_RE.findall(value):
            if item not in ids:
                ids.append(item)
    return ids


def _extract_error(value: Any) -> str | None:
    if isinstance(value, dict):
        for key in ("error", "error_message", "failure", "message"):
            if key in value and value[key]:
                if key == "message" and value.get("status") not in {"error", "failed", "failure"}:
                    continue
                return shorten(value[key], 500)
    if isinstance(value, str) and value.lower().startswith(("error", "failed", "failure")):
        return shorten(value, 500)
    return None


def _safe_arguments(arguments: dict[str, Any]) -> dict[str, Any]:
    result = dict(arguments)
    if "message" in result:
        result["message"] = shorten(result["message"], 1000)
    for key in ("prompt", "instructions"):
        if key in result:
            result[key] = shorten(result[key], 1000)
    return result


def _parse_object(value: Any) -> dict[str, Any]:
    parsed = _parse_json_if_possible(value)
    return parsed if isinstance(parsed, dict) else {}


def _parse_json_if_possible(value: Any) -> Any:
    if not isinstance(value, str):
        return value
    text = value.strip()
    if not text:
        return value
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return value


def _event_timestamp(event: dict[str, Any]) -> str | None:
    timestamp = event.get("timestamp")
    if timestamp is None and isinstance(event.get("payload"), dict):
        timestamp = event["payload"].get("timestamp")
    return _as_text(timestamp)


def _filename_timestamp(value: str) -> str | None:
    if not value:
        return None
    # Rollout filenames commonly replace the time colons with hyphens:
    # 2026-06-06T13-43-41.  Keep already-valid ISO values unchanged.
    match = re.match(r"^(?P<date>\d{4}-\d{2}-\d{2})T(?P<hour>\d{2})[-_](?P<minute>\d{2})[-_](?P<second>\d{2})(?P<suffix>.*)$", value)
    if match:
        return (
            f"{match.group('date')}T{match.group('hour')}:{match.group('minute')}:{match.group('second')}"
            f"{match.group('suffix')}"
        )
    return value


def _is_spawn_name(name: str | None) -> bool:
    if not name:
        return False
    normalized = name.lower().replace("/", ".")
    return normalized == "spawn_agent" or normalized.endswith(".spawn_agent")


def _nested_value(value: Any, key: str) -> Any:
    return value.get(key) if isinstance(value, dict) else None


def _as_text(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, str):
        return value
    return str(value)


def _as_int(value: Any) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _stable_id(*parts: str | None) -> str:
    return ":".join(part if part else "-" for part in parts)


def _expand_path(value: str | Path) -> Path:
    # Keep the caller-visible spelling of macOS' /var -> /private/var alias.
    # Source discovery should make paths absolute without resolving symlinks;
    # the SQLite read-only URI canonicalizes separately when it opens a DB.
    return Path(value).expanduser().absolute()


def _unique_paths(paths: Iterable[Path]) -> list[Path]:
    result: list[Path] = []
    seen: set[str] = set()
    for path in paths:
        key = str(path)
        if key not in seen:
            result.append(path)
            seen.add(key)
    return result


def _codex_home_candidates(codex_home: str | Path | None) -> list[Path]:
    """Use an explicit home, then CODEX_HOME, then the platform user's default."""

    if codex_home:
        return [_expand_path(codex_home)]
    env_home = os.environ.get("CODEX_HOME")
    if env_home:
        return [_expand_path(env_home)]
    return [Path.home() / ".codex"]
