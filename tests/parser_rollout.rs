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
fn rollout_identity_is_not_overwritten_by_inherited_session_metadata() {
    let temp = tempdir().unwrap();
    let rollout = temp
        .path()
        .join("rollout-2026-08-13T00-00-00-019eece4-3a1b-7713-8f99-77d716fe2703.jsonl");
    fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"019eece4-3a1b-7713-8f99-77d716fe2703","timestamp":"2026-08-13T00:00:00Z","cwd":"/current"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"019eecb6-8910-7603-b48a-998b04738e31","timestamp":"2026-08-12T00:00:00Z","cwd":"/inherited"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let result = scan_sources(&[rollout], &[]).unwrap();
    assert_eq!(result.root_conversations.len(), 1);
    assert_eq!(
        result.root_conversations[0].id,
        "019eece4-3a1b-7713-8f99-77d716fe2703"
    );
    assert_eq!(
        result.root_conversations[0].cwd.value.as_deref(),
        Some("/current")
    );
}

#[test]
fn rollout_filename_supplies_identity_when_session_metadata_has_no_id() {
    let temp = tempdir().unwrap();
    let rollout = temp
        .path()
        .join("rollout-2026-08-13T00-00-00-019eece4-3a1b-7713-8f99-77d716fe2703.jsonl");
    fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{"timestamp":"2026-08-13T00:00:00Z","cwd":"/current"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"019eecb6-8910-7603-b48a-998b04738e31","timestamp":"2026-08-12T00:00:00Z"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let result = scan_sources(&[rollout], &[]).unwrap();
    assert_eq!(
        result.root_conversations[0].id,
        "019eece4-3a1b-7713-8f99-77d716fe2703"
    );
}

#[test]
fn repeated_matching_session_metadata_keeps_one_identity() {
    let temp = tempdir().unwrap();
    let rollout = temp.path().join("rollout.jsonl");
    fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"root-id","cwd":"/first"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"root-id","model":"gpt-current"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let result = scan_sources(&[rollout], &[]).unwrap();
    assert_eq!(result.root_conversations[0].id, "root-id");
    assert_eq!(
        result.root_conversations[0].model.value.as_deref(),
        Some("gpt-current")
    );
}

#[test]
fn later_session_metadata_supplies_identity_when_filename_and_first_record_do_not() {
    let temp = tempdir().unwrap();
    let rollout = temp.path().join("rollout.jsonl");
    fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{"cwd":"/current"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"root-id"}}"#,
            "\n"
        ),
    )
    .unwrap();

    let result = scan_sources(&[rollout], &[]).unwrap();
    assert_eq!(result.root_conversations[0].id, "root-id");
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
