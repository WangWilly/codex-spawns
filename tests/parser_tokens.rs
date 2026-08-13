use codex_spawns::{scan_sources, FactConfidence};
use std::fs;
use tempfile::tempdir;

#[test]
fn final_cumulative_usage_is_not_summed_and_conversation_includes_descendants() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root.jsonl");
    let child = dir.path().join("child.jsonl");
    fs::write(&root, concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"root\"}}\n",
        "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":4,\"cached_input_tokens\":1,\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":6},\"model_context_window\":100}}}\n",
        "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":7,\"cached_input_tokens\":2,\"output_tokens\":3,\"reasoning_output_tokens\":1,\"total_tokens\":10},\"model_context_window\":100}}}\n"
    )).unwrap();
    fs::write(&child, concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"root\"}}}}}\n",
        "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":5}}}}\n"
    )).unwrap();
    let result = scan_sources(&[root, child], &[]).unwrap();
    assert_eq!(
        result.session_tokens["root"]
            .value
            .as_ref()
            .unwrap()
            .total_tokens,
        10
    );
    assert_eq!(
        result.session_tokens["root"]
            .value
            .as_ref()
            .unwrap()
            .cached_input_tokens,
        Some(2)
    );
    let summary = &result.conversation_tokens["root"];
    assert_eq!(summary.usage.value.as_ref().unwrap().total_tokens, 15);
    assert_eq!((summary.covered_sessions, summary.total_sessions), (2, 2));
    assert_eq!(summary.usage.confidence, FactConfidence::Derived);
}

#[test]
fn missing_session_usage_makes_known_total_partial() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("root.jsonl");
    let child = dir.path().join("child.jsonl");
    fs::write(&root, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"root\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":8}}}}\n").unwrap();
    fs::write(&child, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"root\"}}}}}\n").unwrap();
    let result = scan_sources(&[root, child], &[]).unwrap();
    let summary = &result.conversation_tokens["root"];
    assert_eq!(summary.usage.value.as_ref().unwrap().total_tokens, 8);
    assert_eq!((summary.covered_sessions, summary.total_sessions), (1, 2));
}
