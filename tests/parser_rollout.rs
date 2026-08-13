use codex_spawns::{scan_sources, FactConfidence, SourceRef, SpawnStatus};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
