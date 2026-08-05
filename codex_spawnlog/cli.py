from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import sys
from typing import Any, Iterable

from . import __version__
from .models import RolloutMeta, ScanResult, SpawnRecord, short_id, shorten
from .parser import (
    discover_sources,
    parse_timestamp,
    scan_sources,
    timestamp_sort_key,
)


COMMON_OPTIONS = {
    "sessions_dirs": "sessions_dirs",
    "files": "files",
    "state_databases": "state_databases",
    "codex_home": "codex_home",
    "include_archived": "include_archived",
    "include_state_databases": "include_state_databases",
    "output_format": "output_format",
    "include_message": "include_message",
    "limit": "limit",
    "session_id": "session_id",
    "parent_thread_id": "parent_thread_id",
    "child_thread_id": "child_thread_id",
    "cwd": "cwd",
    "model": "model",
    "role": "role",
    "status": "status",
    "since": "since",
    "until": "until",
    "reverse": "reverse",
}


def build_parser() -> argparse.ArgumentParser:
    common = argparse.ArgumentParser(add_help=False)
    _add_common_options(common)

    parser = argparse.ArgumentParser(
        prog="codex-spawns",
        description="Inspect Codex session subagent spawn records from local rollout JSONL files.",
        parents=[common],
    )
    parser.add_argument("--version", action="version", version=f"codex-spawns {__version__}")
    subparsers = parser.add_subparsers(dest="command")

    list_parser = subparsers.add_parser(
        "list", aliases=["ls"], parents=[common], help="list subagent spawn records (default)"
    )
    list_parser.set_defaults(command="list")

    show_parser = subparsers.add_parser(
        "show", aliases=["inspect"], parents=[common], help="show one spawn record in detail"
    )
    show_parser.add_argument("identifier", help="spawn id, child thread id, or 1-based table index")
    show_parser.add_argument(
        "--evidence", action="store_true", help="include source event locations and payload evidence"
    )
    show_parser.set_defaults(command="show")

    sessions_parser = subparsers.add_parser(
        "sessions", aliases=["session"], parents=[common], help="list discovered rollout sessions"
    )
    sessions_parser.set_defaults(command="sessions")

    doctor_parser = subparsers.add_parser(
        "doctor", parents=[common], help="show discovered sources and parser diagnostics"
    )
    doctor_parser.set_defaults(command="doctor")

    return parser


def _add_common_options(parser: argparse.ArgumentParser) -> None:
    suppress = argparse.SUPPRESS
    parser.add_argument(
        "--codex-home",
        dest="codex_home",
        default=suppress,
        help="Codex home directory; defaults to $CODEX_HOME or ~/.codex",
    )
    parser.add_argument(
        "--sessions-dir",
        dest="sessions_dirs",
        action="append",
        default=suppress,
        metavar="DIR",
        help="rollout root to scan; repeat for multiple roots",
    )
    parser.add_argument(
        "--file",
        dest="files",
        action="append",
        default=suppress,
        metavar="JSONL",
        help="scan one rollout JSONL file; repeat for multiple files",
    )
    parser.add_argument(
        "--state-db",
        dest="state_databases",
        action="append",
        default=suppress,
        metavar="SQLITE",
        help="read a state_*.sqlite file read-only; repeat for multiple databases",
    )
    parser.add_argument(
        "--no-archived",
        dest="include_archived",
        action="store_false",
        default=suppress,
        help="do not scan archived_sessions",
    )
    parser.add_argument(
        "--no-state-db",
        dest="include_state_databases",
        action="store_false",
        default=suppress,
        help="do not auto-discover state_*.sqlite",
    )
    parser.add_argument(
        "--format",
        dest="output_format",
        choices=("table", "json", "jsonl", "csv"),
        default=suppress,
        help="output format (default: table; JSON is best for scripting)",
    )
    parser.add_argument(
        "--include-message",
        dest="include_message",
        action="store_true",
        default=suppress,
        help="include full task message; messages may contain private prompts",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=suppress,
        help="maximum records to print (0 means no limit)",
    )
    parser.add_argument(
        "--session",
        "--session-id",
        dest="session_id",
        default=suppress,
        metavar="ID",
        help="filter records touching this parent or child session id",
    )
    parser.add_argument(
        "--parent",
        "--parent-thread-id",
        dest="parent_thread_id",
        default=suppress,
        metavar="ID",
        help="filter by parent thread id",
    )
    parser.add_argument(
        "--child",
        "--child-thread-id",
        dest="child_thread_id",
        default=suppress,
        metavar="ID",
        help="filter by child thread id",
    )
    parser.add_argument(
        "--cwd",
        "--workdir",
        dest="cwd",
        default=suppress,
        metavar="PATH",
        help="filter by parent or child working directory",
    )
    parser.add_argument(
        "--model",
        default=suppress,
        help="case-insensitive substring filter across requested/effective model",
    )
    parser.add_argument(
        "--role",
        default=suppress,
        help="case-insensitive substring filter across role, nickname, and task name",
    )
    parser.add_argument(
        "--status",
        default=suppress,
        help="filter by status: spawned, requested, failed, or state-only",
    )
    parser.add_argument("--since", default=suppress, help="only records at/after ISO timestamp")
    parser.add_argument("--until", default=suppress, help="only records at/before ISO timestamp")
    parser.add_argument("--reverse", action="store_true", default=suppress, help="newest first")


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    command = getattr(args, "command", None) or "list"

    try:
        scan = _scan_from_args(args)
        if command in {"list", "ls"}:
            records = _filter_records(scan.records, args)
            _print_records(records, scan, args)
        elif command in {"show", "inspect"}:
            _show_record(scan, args)
        elif command in {"sessions", "session"}:
            _print_sessions(scan.rollouts, scan, args)
        elif command == "doctor":
            _print_doctor(scan, args)
        else:
            parser.error(f"unknown command: {command}")
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except BrokenPipeError:
        return 0
    return 0


def _scan_from_args(args: argparse.Namespace) -> ScanResult:
    sessions_dirs = getattr(args, "sessions_dirs", [])
    files = getattr(args, "files", [])
    state_databases = getattr(args, "state_databases", [])
    include_archived = getattr(args, "include_archived", True)
    include_state_databases = getattr(args, "include_state_databases", True)
    rollout_files, dbs, diagnostics = discover_sources(
        files=files,
        sessions_dirs=sessions_dirs,
        codex_home=getattr(args, "codex_home", None),
        include_archived=include_archived,
        state_databases=state_databases,
        include_state_databases=include_state_databases,
    )
    return scan_sources(rollout_files, state_databases=dbs, initial_diagnostics=diagnostics)


def _filter_records(records: Iterable[SpawnRecord], args: argparse.Namespace) -> list[SpawnRecord]:
    session_id = getattr(args, "session_id", None)
    parent_id = getattr(args, "parent_thread_id", None)
    child_id = getattr(args, "child_thread_id", None)
    cwd = _normalized_path(getattr(args, "cwd", None))
    model = (getattr(args, "model", None) or "").lower()
    role = (getattr(args, "role", None) or "").lower()
    status = (getattr(args, "status", None) or "").lower()
    since = _parse_filter_date(getattr(args, "since", None), "--since")
    until = _parse_filter_date(getattr(args, "until", None), "--until")

    filtered: list[SpawnRecord] = []
    for record in records:
        if session_id and session_id not in {record.parent_thread_id, record.child_thread_id}:
            continue
        if parent_id and record.parent_thread_id != parent_id:
            continue
        if child_id and record.child_thread_id != child_id:
            continue
        if cwd and not any(_normalized_path(value) == cwd for value in (record.parent_cwd, record.child_cwd)):
            continue
        if model:
            model_values = [record.requested_model, record.effective_model]
            if not any(model in (value or "").lower() for value in model_values):
                continue
        if role:
            role_values = [record.agent_type, record.agent_role, record.agent_nickname, record.task_name]
            if not any(role in (value or "").lower() for value in role_values):
                continue
        if status and status not in {record.status.lower(), (record.state_status or "").lower()}:
            continue
        timestamp = parse_timestamp(record.created_at)
        if since and (timestamp is None or timestamp < since):
            continue
        if until and (timestamp is None or timestamp > until):
            continue
        filtered.append(record)

    filtered.sort(key=lambda item: timestamp_sort_key(item.created_at), reverse=getattr(args, "reverse", False))
    limit = getattr(args, "limit", 0) or 0
    if limit < 0:
        raise ValueError("--limit must be zero or a positive integer")
    return filtered if limit == 0 else filtered[:limit]


def _show_record(scan: ScanResult, args: argparse.Namespace) -> None:
    records = _filter_records(scan.records, args)
    identifier = args.identifier
    matches = _resolve_records(records, identifier)
    if not matches:
        raise ValueError(f"no spawn record matches {identifier!r}")
    include_message = getattr(args, "include_message", False)
    include_evidence = getattr(args, "evidence", False)
    output_format = getattr(args, "output_format", "table")
    if len(matches) > 1 and output_format == "table":
        print(f"{len(matches)} records match {identifier!r}:")
        _render_record_table(matches)
        return
    objects = [
        item.to_dict(include_message=include_message, include_evidence=include_evidence)
        for item in matches
    ]
    if output_format == "jsonl":
        for obj in objects:
            print(json.dumps(obj, ensure_ascii=False))
    elif output_format == "json":
        print(json.dumps(objects[0] if len(objects) == 1 else objects, ensure_ascii=False, indent=2))
    elif output_format == "csv":
        _render_csv(matches, include_message=include_message)
    else:
        for index, record in enumerate(matches, start=1):
            if index > 1:
                print()
            _render_record_detail(record, include_message=include_message, include_evidence=include_evidence)


def _resolve_records(records: list[SpawnRecord], identifier: str) -> list[SpawnRecord]:
    if identifier.isdigit():
        index = int(identifier)
        if 1 <= index <= len(records):
            return [records[index - 1]]
    exact = [
        record
        for record in records
        if identifier in {
            record.id,
            record.parent_thread_id,
            record.child_thread_id,
            record.call_id,
        }
    ]
    if exact:
        return exact
    prefix = [
        record
        for record in records
        if any((value or "").startswith(identifier) for value in (record.id, record.parent_thread_id, record.child_thread_id, record.call_id))
    ]
    return prefix


def _print_records(records: list[SpawnRecord], scan: ScanResult, args: argparse.Namespace) -> None:
    output_format = getattr(args, "output_format", "table")
    include_message = getattr(args, "include_message", False)
    if output_format == "json":
        payload = {
            "records": [record.to_dict(include_message=include_message) for record in records],
            "count": len(records),
            "scanned_rollout_files": len(scan.files),
            "diagnostics": scan.diagnostics,
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    elif output_format == "jsonl":
        for record in records:
            print(json.dumps(record.to_dict(include_message=include_message), ensure_ascii=False))
    elif output_format == "csv":
        _render_csv(records, include_message=include_message)
    else:
        _render_record_table(records)
    _print_diagnostics(scan, output_format)


def _print_sessions(rollouts: list[RolloutMeta], scan: ScanResult, args: argparse.Namespace) -> None:
    output_format = getattr(args, "output_format", "table")
    rollouts = sorted(rollouts, key=lambda item: timestamp_sort_key(item.created_at), reverse=getattr(args, "reverse", False))
    if output_format == "json":
        print(json.dumps({"sessions": [item.to_dict() for item in rollouts], "count": len(rollouts)}, ensure_ascii=False, indent=2))
    elif output_format == "jsonl":
        for item in rollouts:
            print(json.dumps(item.to_dict(), ensure_ascii=False))
    elif output_format == "csv":
        _render_csv_dicts([item.to_dict() for item in rollouts])
    else:
        rows = []
        for item in rollouts:
            rows.append(
                [
                    short_id(item.session_id),
                    (item.created_at or "-")[:19],
                    "subagent" if item.is_subagent else (item.source or "root"),
                    short_id(item.parent_thread_id),
                    item.model or "-",
                    shorten(item.cwd, 36) or "-",
                    str(item.event_count),
                    shorten(item.path, 46) or "-",
                ]
            )
        _render_table(["Session", "Created", "Type", "Parent", "Model", "CWD", "Events", "Rollout"], rows)
    _print_diagnostics(scan, output_format)


def _print_doctor(scan: ScanResult, args: argparse.Namespace) -> None:
    payload = {
        "rollout_files": scan.files,
        "rollout_file_count": len(scan.files),
        "state_databases": scan.state_databases,
        "state_database_count": len(scan.state_databases),
        "session_count": len(scan.rollouts),
        "spawn_record_count": len(scan.records),
        "subagent_session_count": sum(1 for item in scan.rollouts if item.is_subagent),
        "malformed_jsonl_lines": sum(item.parse_errors for item in scan.rollouts),
        "diagnostics": scan.diagnostics,
    }
    if getattr(args, "output_format", "table") == "table":
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        print(json.dumps(payload, ensure_ascii=False, indent=2))


def _render_record_table(records: list[SpawnRecord]) -> None:
    rows: list[list[str]] = []
    for index, record in enumerate(records, start=1):
        task = record.task_name or record.agent_role or record.agent_nickname or "-"
        model = record.effective_model or record.requested_model or "-"
        if record.requested_model and record.effective_model and record.requested_model != record.effective_model:
            model = f"{record.requested_model} → {record.effective_model}"
        rows.append(
            [
                str(index),
                (record.created_at or "-")[:19],
                short_id(record.parent_thread_id),
                short_id(record.child_thread_id),
                shorten(task, 28) or "-",
                shorten(model, 28) or "-",
                record.status,
            ]
        )
    _render_table(["#", "Created", "Parent", "Child", "Task / role", "Model", "Status"], rows)


def _render_record_detail(record: SpawnRecord, *, include_message: bool, include_evidence: bool) -> None:
    data = record.to_dict(include_message=include_message, include_evidence=include_evidence)
    for key, value in data.items():
        if key == "evidence" and not include_evidence:
            continue
        if value is None or value == []:
            continue
        if isinstance(value, (dict, list)):
            rendered = json.dumps(value, ensure_ascii=False, indent=2)
            print(f"{key}:\n{rendered}")
        else:
            print(f"{key}: {value}")


def _render_csv(records: list[SpawnRecord], *, include_message: bool) -> None:
    objects = [record.to_dict(include_message=include_message) for record in records]
    _render_csv_dicts(objects)


def _render_csv_dicts(objects: list[dict[str, Any]]) -> None:
    if not objects:
        return
    keys: list[str] = []
    for obj in objects:
        for key in obj:
            if key not in keys:
                keys.append(key)
    writer = csv.DictWriter(sys.stdout, fieldnames=keys, extrasaction="ignore")
    writer.writeheader()
    for obj in objects:
        writer.writerow({key: _csv_value(obj.get(key)) for key in keys})


def _csv_value(value: Any) -> Any:
    if isinstance(value, (dict, list)):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    return value


def _render_table(headers: list[str], rows: list[list[str]]) -> None:
    if not rows:
        print("No records found.")
        return
    widths = [len(header) for header in headers]
    for row in rows:
        for index, value in enumerate(row):
            widths[index] = max(widths[index], len(value))
    print("  ".join(header.ljust(widths[index]) for index, header in enumerate(headers)))
    print("  ".join("-" * width for width in widths))
    for row in rows:
        print("  ".join(value.ljust(widths[index]) for index, value in enumerate(row)))


def _print_diagnostics(scan: ScanResult, output_format: str) -> None:
    if not scan.diagnostics:
        return
    for diagnostic in scan.diagnostics:
        print(f"warning: {diagnostic}", file=sys.stderr)


def _parse_filter_date(value: str | None, option: str) -> datetime | None:
    if not value:
        return None
    parsed = parse_timestamp(value)
    if parsed is None:
        raise ValueError(f"{option} must be an ISO-8601 timestamp: {value}")
    return parsed


def _normalized_path(value: str | None) -> str | None:
    if not value:
        return None
    return str(Path(value).expanduser().resolve(strict=False))
