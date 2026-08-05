from __future__ import annotations

import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from codex_spawnlog.parser import discover_sources, scan_sources


def write_jsonl(path: Path, events: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(event) for event in events) + "\n", encoding="utf-8")


def session_meta(
    session_id: str,
    timestamp: str,
    cwd: str,
    *,
    source: object = "cli",
    **extra: object,
) -> dict:
    payload = {
        "id": session_id,
        "timestamp": timestamp,
        "cwd": cwd,
        "source": source,
        "cli_version": "0.1.0-test",
    }
    payload.update(extra)
    return {"timestamp": timestamp, "type": "session_meta", "payload": payload}


class ParserTests(unittest.TestCase):
    def test_matches_child_metadata_to_parent_call_by_task_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tmp_path = Path(directory)
            parent_id = "01900000-0000-7000-8000-000000000061"
            child_id = "01900000-0000-7000-8000-000000000062"
            parent = tmp_path / "parent.jsonl"
            child = tmp_path / "child.jsonl"
            write_jsonl(
                parent,
                [
                    session_meta(parent_id, "2026-08-05T04:00:00Z", "/repo"),
                    {
                        "timestamp": "2026-08-05T04:01:00Z",
                        "type": "response_item",
                        "payload": {
                            "type": "function_call",
                            "name": "spawn_agent",
                            "call_id": "call-name-match",
                            "arguments": {"task_name": "name-only-worker"},
                        },
                    },
                ],
            )
            write_jsonl(
                child,
                [
                    session_meta(
                        child_id,
                        "2026-08-05T04:01:02Z",
                        "/repo",
                        source={
                            "subagent": {
                                "thread_spawn": {
                                    "parent_thread_id": parent_id,
                                    "agent_path": "/root/name-only-worker",
                                }
                            }
                        },
                    )
                ],
            )

            result = scan_sources([parent, child])

            self.assertEqual(len(result.records), 1)
            self.assertEqual(result.records[0].child_thread_id, child_id)
            self.assertEqual(result.records[0].source, "rollout")

    def test_parses_timestamp_from_rollout_filename(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / (
                "rollout-2026-08-05T04-05-06-01900000-0000-7000-8000-000000000063.jsonl"
            )
            write_jsonl(path, [])

            result = scan_sources([path])

            self.assertEqual(result.rollouts[0].created_at, "2026-08-05T04:05:06")

    def test_builds_one_record_from_parent_call_and_child_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tmp_path = Path(directory)
            parent_id = "01900000-0000-7000-8000-000000000001"
            child_id = "01900000-0000-7000-8000-000000000002"
            parent = tmp_path / "sessions/2026/08/05/parent.jsonl"
            child = tmp_path / "sessions/2026/08/05/child.jsonl"

            write_jsonl(
                parent,
                [
                    session_meta(parent_id, "2026-08-05T01:00:00Z", "/repo", source="cli"),
                    {
                        "timestamp": "2026-08-05T01:01:00Z",
                        "type": "turn_context",
                        "payload": {"model": "gpt-parent", "effort": "high"},
                    },
                    {
                        "timestamp": "2026-08-05T01:02:00Z",
                        "type": "response_item",
                        "payload": {
                            "type": "function_call",
                            "name": "spawn_agent",
                            "call_id": "call-1",
                            "arguments": json.dumps(
                                {
                                    "task_name": "repo-scout",
                                    "message": "Inspect the repository and report findings.",
                                    "agent_type": "explorer",
                                    "model": "gpt-requested",
                                    "reasoning_effort": "low",
                                    "fork_turns": "none",
                                }
                            ),
                        },
                    },
                    {
                        "timestamp": "2026-08-05T01:02:01Z",
                        "type": "response_item",
                        "payload": {
                            "type": "function_call_output",
                            "call_id": "call-1",
                            "output": json.dumps({"agent_id": child_id, "nickname": "Scout"}),
                        },
                    },
                ],
            )
            write_jsonl(
                child,
                [
                    session_meta(
                        child_id,
                        "2026-08-05T01:02:02Z",
                        "/repo",
                        source={
                            "subagent": {
                                "thread_spawn": {
                                    "parent_thread_id": parent_id,
                                    "agent_path": "/root/repo-scout",
                                    "agent_nickname": "Scout",
                                    "agent_role": "explorer",
                                    "depth": 1,
                                }
                            }
                        },
                    ),
                    {
                        "timestamp": "2026-08-05T01:02:03Z",
                        "type": "turn_context",
                        "payload": {"model": "gpt-child", "effort": "low", "multi_agent_version": "v2"},
                    },
                ],
            )

            result = scan_sources([parent, child])

            self.assertEqual(len(result.records), 1)
            record = result.records[0]
            self.assertEqual(record.parent_thread_id, parent_id)
            self.assertEqual(record.child_thread_id, child_id)
            self.assertEqual(record.status, "spawned")
            self.assertEqual(record.task_name, "repo-scout")
            self.assertEqual(record.agent_role, "explorer")
            self.assertEqual(record.agent_nickname, "Scout")
            self.assertEqual(record.requested_model, "gpt-requested")
            self.assertEqual(record.effective_model, "gpt-child")
            self.assertEqual(record.multi_agent_version, "v2")
            self.assertEqual(record.source, "rollout")

    def test_child_metadata_is_enough_when_parent_call_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            child = Path(directory) / "child.jsonl"
            parent_id = "01900000-0000-7000-8000-000000000011"
            child_id = "01900000-0000-7000-8000-000000000012"
            write_jsonl(
                child,
                [
                    session_meta(
                        child_id,
                        "2026-08-05T02:00:00Z",
                        "/repo",
                        source={
                            "subagent": {
                                "thread_spawn": {
                                    "parent_thread_id": parent_id,
                                    "agent_role": "worker",
                                }
                            }
                        },
                    ),
                    {"type": "turn_context", "payload": {"model": "gpt-worker", "effort": "medium"}},
                ],
            )

            result = scan_sources([child])

            self.assertEqual(len(result.records), 1)
            self.assertEqual(result.records[0].source, "child-metadata")
            self.assertEqual(result.records[0].parent_thread_id, parent_id)
            self.assertEqual(result.records[0].effective_model, "gpt-worker")

    def test_discovers_archived_rollouts_and_reports_missing_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory) / ".codex"
            archived = home / "archived_sessions"
            file_path = archived / "old.jsonl"
            write_jsonl(
                file_path,
                [session_meta("01900000-0000-7000-8000-000000000021", "2026-08-01T00:00:00Z", "/old")],
            )

            files, dbs, diagnostics = discover_sources(codex_home=home, include_state_databases=False)

            self.assertIn(file_path, files)
            self.assertEqual(dbs, [])
            self.assertFalse(any("archived_sessions" in item and "not found" in item for item in diagnostics))

    def test_reads_thread_spawn_edges_read_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            db = Path(directory) / "state_5.sqlite"
            connection = sqlite3.connect(db)
            connection.execute(
                "CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT, status TEXT)"
            )
            parent_id = "01900000-0000-7000-8000-000000000031"
            child_id = "01900000-0000-7000-8000-000000000032"
            connection.execute(
                "INSERT INTO thread_spawn_edges VALUES (?, ?, ?)",
                (parent_id, child_id, "completed"),
            )
            connection.commit()
            connection.close()

            result = scan_sources([], state_databases=[db])

            self.assertEqual(len(result.records), 1)
            self.assertEqual(result.records[0].source, "state-db")
            self.assertEqual(result.records[0].state_status, "completed")


if __name__ == "__main__":
    unittest.main()
