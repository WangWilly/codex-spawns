use codex_spawns::{scan_sources, FactConfidence, SourceRef, SpawnStatus};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn matches_child_metadata_to_unresolved_call_by_parent_and_task_name() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("parent.jsonl");
    let child = temp.path().join("child.jsonl");
    fs::write(&parent, concat!(
        r#"{"type":"session_meta","payload":{"id":"root-id","timestamp":"2026-08-05T00:00:00Z","cwd":"/repo"}}"#, "\n",
        r#"{"timestamp":"2026-08-05T00:01:00Z","type":"response_item","payload":{"type":"function_call","name":"spawn_agent","call_id":"unresolved","arguments":{"task_name":"name-only-worker"}}}"#, "\n"
    )).unwrap();
    fs::write(&child, concat!(
        r#"{"type":"session_meta","payload":{"id":"child-id","timestamp":"2026-08-05T00:01:02Z","cwd":"/repo","source":{"subagent":{"thread_spawn":{"parent_thread_id":"root-id","agent_path":"/root/name-only-worker"}}}}}"#, "\n"
    )).unwrap();

    let result = scan_sources(&[parent, child], &[]).unwrap();
    assert_eq!(result.spawn_attempts.len(), 1);
    assert_eq!(result.spawn_attempts[0].status, SpawnStatus::Spawned);
    assert_eq!(
        result.spawn_attempts[0].child_thread_id.as_deref(),
        Some("child-id")
    );
}

#[test]
fn merges_parent_call_output_and_child_metadata() {
    let profile = scan_sources(&[fixture("parent.jsonl"), fixture("child.jsonl")], &[])
        .expect("fixtures should parse");

    assert_eq!(profile.root_conversations.len(), 1);
    assert_eq!(profile.agent_sessions.len(), 1);
    assert_eq!(profile.spawn_attempts.len(), 1);
    let attempt = &profile.spawn_attempts[0];
    assert_eq!(attempt.status, SpawnStatus::Spawned);
    assert_eq!(
        attempt.parent_thread_id.as_deref(),
        Some("01900000-0000-7000-8000-000000000001")
    );
    assert_eq!(
        attempt.child_thread_id.as_deref(),
        Some("01900000-0000-7000-8000-000000000002")
    );
    assert_eq!(attempt.task_name.value.as_deref(), Some("repo-scout"));
    assert_eq!(attempt.task_name.confidence, FactConfidence::Observed);
    assert_eq!(
        attempt.requested_model.value.as_deref(),
        Some("gpt-requested")
    );
    assert_eq!(attempt.effective_model.value.as_deref(), Some("gpt-child"));
    assert!(attempt
        .effective_model
        .provenance
        .iter()
        .any(|source| { matches!(source, SourceRef::Rollout { line: Some(2), .. }) }));
}
