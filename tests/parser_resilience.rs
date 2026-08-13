use codex_spawns::{scan_sources, FactConfidence, ProfileFact, SourceRef, SpawnStatus};
use rusqlite::Connection;
use std::{fs, path::PathBuf};
use tempfile::tempdir;

fn write(path: &std::path::Path, lines: &[&str]) {
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

#[test]
fn preserves_requested_failed_orphan_and_malformed_evidence() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("parent.jsonl");
    let orphan = temp.path().join("orphan.jsonl");
    write(
        &parent,
        &[
            r#"{"type":"session_meta","payload":{"id":"root","timestamp":"2026-08-05T00:00:00Z","cwd":"/repo"}}"#,
            "not-json",
            r#"{"timestamp":"2026-08-05T00:01:00Z","type":"response_item","payload":{"type":"function_call","name":"spawn_agent","call_id":"pending","arguments":{"task_name":"pending"}}}"#,
            r#"{"timestamp":"2026-08-05T00:02:00Z","type":"response_item","payload":{"type":"function_call","name":"spawn_agent","call_id":"failed","arguments":{"task_name":"failed"}}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"failed","output":{"error":"capacity exhausted"}}}"#,
        ],
    );
    write(
        &orphan,
        &[
            r#"{"type":"session_meta","payload":{"id":"orphan-agent","timestamp":"2026-08-05T00:03:00Z","cwd":"/repo","source":{"subagent":{"thread_spawn":{"parent_thread_id":"missing-root","agent_role":"worker"}}}}}"#,
        ],
    );

    let result = scan_sources(&[parent, orphan], &[]).unwrap();
    assert_eq!(result.root_conversations[0].parse_errors, 1);
    assert!(result
        .spawn_attempts
        .iter()
        .any(|a| a.status == SpawnStatus::Requested));
    let failed = result
        .spawn_attempts
        .iter()
        .find(|a| a.status == SpawnStatus::Failed)
        .unwrap();
    assert_eq!(
        failed.output_error.value.as_deref(),
        Some("capacity exhausted")
    );
    let orphan = result
        .spawn_attempts
        .iter()
        .find(|a| a.status == SpawnStatus::Orphan)
        .unwrap();
    assert_eq!(orphan.child_thread_id.as_deref(), Some("orphan-agent"));
}

#[test]
fn reads_state_edges_without_writing_and_builds_state_only_attempt() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("state_5.sqlite");
    let connection = Connection::open(&db).unwrap();
    connection.execute("CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT, status TEXT)", []).unwrap();
    connection
        .execute(
            "INSERT INTO thread_spawn_edges VALUES ('root', 'state-child', 'completed')",
            [],
        )
        .unwrap();
    drop(connection);

    let before = fs::metadata(&db).unwrap().modified().unwrap();
    let result = scan_sources(&[], std::slice::from_ref(&db)).unwrap();
    let after = fs::metadata(&db).unwrap().modified().unwrap();
    assert_eq!(before, after);
    assert_eq!(result.spawn_attempts[0].status, SpawnStatus::StateOnly);
    assert_eq!(
        result.spawn_attempts[0].state_status.value.as_deref(),
        Some("completed")
    );
    assert!(matches!(
        result.spawn_attempts[0].evidence[0],
        SourceRef::StateDatabase { .. }
    ));
}

#[test]
fn profile_fact_retains_conflicting_observations() {
    let one = SourceRef::Rollout {
        path: PathBuf::from("one"),
        line: Some(1),
    };
    let two = SourceRef::Rollout {
        path: PathBuf::from("two"),
        line: Some(2),
    };
    let mut fact = ProfileFact::observed("gpt-a".to_string(), one);
    fact.observe("gpt-b".to_string(), two);

    assert_eq!(fact.value.as_deref(), Some("gpt-a"));
    assert_eq!(fact.confidence, FactConfidence::Conflicting);
    assert_eq!(fact.conflicting_values, vec!["gpt-b"]);
    assert_eq!(fact.provenance.len(), 2);
}
