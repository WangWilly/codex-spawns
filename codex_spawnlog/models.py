from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any
import json


def shorten(value: Any, limit: int = 180) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str):
        try:
            value = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        except (TypeError, ValueError):
            value = str(value)
    value = " ".join(value.split())
    if len(value) <= limit:
        return value
    return value[: max(0, limit - 1)] + "…"


def short_id(value: str | None, length: int = 8) -> str:
    if not value:
        return "-"
    return value[:length]


@dataclass
class RolloutMeta:
    path: str
    session_id: str | None = None
    created_at: str | None = None
    cwd: str | None = None
    originator: str | None = None
    cli_version: str | None = None
    source: str | None = None
    thread_source: str | None = None
    parent_thread_id: str | None = None
    forked_from_id: str | None = None
    agent_path: str | None = None
    agent_nickname: str | None = None
    agent_role: str | None = None
    depth: int | None = None
    model: str | None = None
    effort: str | None = None
    multi_agent_version: str | None = None
    event_count: int = 0
    parse_errors: int = 0
    first_event_at: str | None = None
    last_event_at: str | None = None
    session_meta_line: int | None = None

    @property
    def is_subagent(self) -> bool:
        return bool(
            self.parent_thread_id
            or self.thread_source == "subagent"
            or self.source == "subagent"
            or self.agent_role
            or self.agent_path
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "path": self.path,
            "session_id": self.session_id,
            "created_at": self.created_at,
            "cwd": self.cwd,
            "originator": self.originator,
            "cli_version": self.cli_version,
            "source": self.source,
            "thread_source": self.thread_source,
            "parent_thread_id": self.parent_thread_id,
            "forked_from_id": self.forked_from_id,
            "agent_path": self.agent_path,
            "agent_nickname": self.agent_nickname,
            "agent_role": self.agent_role,
            "depth": self.depth,
            "model": self.model,
            "effort": self.effort,
            "multi_agent_version": self.multi_agent_version,
            "event_count": self.event_count,
            "parse_errors": self.parse_errors,
            "first_event_at": self.first_event_at,
            "last_event_at": self.last_event_at,
            "session_meta_line": self.session_meta_line,
            "is_subagent": self.is_subagent,
        }


@dataclass
class SpawnCall:
    parent_path: str
    parent_session_id: str | None
    timestamp: str | None
    line: int
    call_id: str | None
    name: str
    arguments: dict[str, Any] = field(default_factory=dict)
    child_thread_ids: list[str] = field(default_factory=list)
    output: Any = None
    output_line: int | None = None
    output_error: str | None = None
    activity_kind: str | None = None
    activity_line: int | None = None

    @property
    def task_name(self) -> str | None:
        return _first_string(self.arguments, "task_name", "task", "name")

    @property
    def message(self) -> str | None:
        value = _first_value(self.arguments, "message", "prompt", "instructions")
        if value is None:
            return None
        if isinstance(value, str):
            return value
        return shorten(value, 100_000)

    @property
    def agent_type(self) -> str | None:
        return _first_string(self.arguments, "agent_type", "agent_role", "role")

    @property
    def requested_model(self) -> str | None:
        return _first_string(self.arguments, "model", "effective_model")

    @property
    def requested_effort(self) -> str | None:
        return _first_string(
            self.arguments, "reasoning_effort", "effort", "model_reasoning_effort"
        )

    @property
    def fork_turns(self) -> str | None:
        value = _first_value(self.arguments, "fork_turns")
        if value is not None:
            return str(value)
        value = _first_value(self.arguments, "fork_context")
        if value is None:
            return None
        if isinstance(value, bool):
            return "all" if value else "none"
        return str(value)


@dataclass
class SpawnRecord:
    id: str
    created_at: str | None
    status: str
    parent_thread_id: str | None = None
    child_thread_id: str | None = None
    parent_path: str | None = None
    child_path: str | None = None
    parent_cwd: str | None = None
    child_cwd: str | None = None
    task_name: str | None = None
    message: str | None = None
    agent_type: str | None = None
    agent_role: str | None = None
    agent_nickname: str | None = None
    agent_path: str | None = None
    requested_model: str | None = None
    requested_effort: str | None = None
    fork_turns: str | None = None
    effective_model: str | None = None
    effective_effort: str | None = None
    multi_agent_version: str | None = None
    depth: int | None = None
    source: str = "rollout"
    call_id: str | None = None
    parent_line: int | None = None
    child_line: int | None = None
    output_line: int | None = None
    output_error: str | None = None
    state_status: str | None = None
    state_source: str | None = None
    evidence: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self, include_message: bool = False, include_evidence: bool = False) -> dict[str, Any]:
        result: dict[str, Any] = {
            "id": self.id,
            "created_at": self.created_at,
            "status": self.status,
            "parent_thread_id": self.parent_thread_id,
            "child_thread_id": self.child_thread_id,
            "parent_path": self.parent_path,
            "child_path": self.child_path,
            "parent_cwd": self.parent_cwd,
            "child_cwd": self.child_cwd,
            "task_name": self.task_name,
            "message_excerpt": shorten(self.message),
            "agent_type": self.agent_type,
            "agent_role": self.agent_role,
            "agent_nickname": self.agent_nickname,
            "agent_path": self.agent_path,
            "requested_model": self.requested_model,
            "requested_effort": self.requested_effort,
            "fork_turns": self.fork_turns,
            "effective_model": self.effective_model,
            "effective_effort": self.effective_effort,
            "multi_agent_version": self.multi_agent_version,
            "depth": self.depth,
            "source": self.source,
            "call_id": self.call_id,
            "parent_line": self.parent_line,
            "child_line": self.child_line,
            "output_line": self.output_line,
            "output_error": self.output_error,
            "state_status": self.state_status,
            "state_source": self.state_source,
        }
        if include_message:
            result["message"] = self.message
        if include_evidence:
            result["evidence"] = self.evidence
        return result


@dataclass
class ScanResult:
    rollouts: list[RolloutMeta] = field(default_factory=list)
    records: list[SpawnRecord] = field(default_factory=list)
    files: list[str] = field(default_factory=list)
    state_databases: list[str] = field(default_factory=list)
    diagnostics: list[str] = field(default_factory=list)

    def to_dict(self, include_message: bool = False, include_evidence: bool = False) -> dict[str, Any]:
        return {
            "records": [
                r.to_dict(include_message=include_message, include_evidence=include_evidence)
                for r in self.records
            ],
            "count": len(self.records),
            "rollout_files": len(self.files),
            "state_databases": self.state_databases,
            "diagnostics": self.diagnostics,
        }


def _first_value(data: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in data:
            return data[key]
    return None


def _first_string(data: dict[str, Any], *keys: str) -> str | None:
    value = _first_value(data, *keys)
    if value is None:
        return None
    if isinstance(value, str):
        return value
    return str(value)
