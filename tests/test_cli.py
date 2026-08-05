from __future__ import annotations

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from codex_spawnlog.cli import main


class CliTests(unittest.TestCase):
    def test_cli_json_and_show(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rollout.jsonl"
            session_id = "01900000-0000-7000-8000-000000000041"
            child_id = "01900000-0000-7000-8000-000000000042"
            events = [
                {
                    "timestamp": "2026-08-05T03:00:00Z",
                    "type": "session_meta",
                    "payload": {
                        "id": session_id,
                        "timestamp": "2026-08-05T03:00:00Z",
                        "cwd": "/repo",
                        "source": "cli",
                    },
                },
                {
                    "timestamp": "2026-08-05T03:01:00Z",
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "collaboration.spawn_agent",
                        "call_id": "call-cli",
                        "arguments": {"task_name": "test-worker", "message": "secret details"},
                    },
                },
                {
                    "timestamp": "2026-08-05T03:01:01Z",
                    "type": "response_item",
                    "payload": {
                        "type": "function_call_output",
                        "call_id": "call-cli",
                        "output": {"agent_id": child_id},
                    },
                },
            ]
            path.write_text(
                "\n".join(json.dumps(event) for event in events) + "\n", encoding="utf-8"
            )

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(
                    main(["--file", str(path), "--no-state-db", "list", "--format", "json"]),
                    0,
                )
            payload = json.loads(stdout.getvalue())
            self.assertEqual(payload["count"], 1)
            self.assertEqual(payload["records"][0]["child_thread_id"], child_id)
            self.assertNotIn('"message":', stdout.getvalue())

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(
                    main(
                        [
                            "--file",
                            str(path),
                            "--no-state-db",
                            "show",
                            "1",
                            "--format",
                            "json",
                            "--include-message",
                        ]
                    ),
                    0,
                )
            detail = json.loads(stdout.getvalue())
            self.assertEqual(detail["message"], "secret details")

    def test_cli_filter_by_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rollout.jsonl"
            session_id = "01900000-0000-7000-8000-000000000051"
            events = [
                {
                    "type": "session_meta",
                    "payload": {"id": session_id, "cwd": "/repo", "source": "cli"},
                },
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "spawn_agent",
                        "call_id": "call-filter",
                        "arguments": {"task_name": "worker"},
                    },
                },
            ]
            path.write_text(
                "\n".join(json.dumps(event) for event in events) + "\n", encoding="utf-8"
            )

            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(
                    main(
                        [
                            "--file",
                            str(path),
                            "--no-state-db",
                            "list",
                            "--parent",
                            session_id,
                            "--format",
                            "json",
                        ]
                    ),
                    0,
                )
            payload = json.loads(stdout.getvalue())
            self.assertEqual(payload["count"], 1)


if __name__ == "__main__":
    unittest.main()
