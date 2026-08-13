use codex_spawns::{
    parse_rollout, project_user_message, PROJECTION_VERSION, TITLE_PROJECTION_VERSION,
};
use serde_json::json;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/title")
        .join(name)
}

#[test]
fn resumes_at_explicit_user_request_after_attachment_metadata() {
    let parsed = parse_rollout(fixture("attachment_then_request.jsonl")).unwrap();
    let root = parsed.root.unwrap();

    assert_eq!(
        root.first_user_message.value.as_deref(),
        Some("讓頁面支援上下左右捲動，並加入清楚的欄位標題")
    );
}

#[test]
fn injected_only_content_is_not_a_title_candidate() {
    let parsed = parse_rollout(fixture("injected_only.jsonl")).unwrap();
    assert_eq!(parsed.root.unwrap().first_user_message.value, None);
}

#[test]
fn preserves_official_title_and_projects_plain_object_text() {
    let parsed = parse_rollout(fixture("official_and_object.jsonl")).unwrap();
    let root = parsed.root.unwrap();

    assert_eq!(
        root.title.value.as_deref(),
        Some("Official conversation title")
    );
    assert_eq!(
        root.first_user_message.value.as_deref(),
        Some("Review the profiling interface")
    );
}

#[test]
fn projection_collapses_whitespace_and_caps_display_width() {
    let projected = project_user_message(&json!({
        "type": "input_text",
        "text": "  這是一段需要壓縮空白的標題    and a deliberately long suffix that exceeds eighty terminal columns  "
    }))
    .unwrap();

    assert_eq!(
        projected,
        "這是一段需要壓縮空白的標題 and a deliberately long suffix that exceeds eighty t…"
    );
}

#[test]
fn extracts_intent_from_structured_content_without_serializing_injected_plugins() {
    let parsed = parse_rollout(fixture("plugins_then_intent.jsonl")).unwrap();
    let root = parsed.root.unwrap();

    assert_eq!(TITLE_PROJECTION_VERSION, 2);
    assert_eq!(PROJECTION_VERSION, TITLE_PROJECTION_VERSION);
    assert_eq!(
        root.first_user_message.value.as_deref(),
        Some("目前的交互是 command 模式，我想改成 Interactive 模式")
    );
    assert!(!root
        .first_user_message
        .value
        .unwrap()
        .contains("[{\"text\""));
}

#[test]
fn skips_environment_and_app_context_candidates() {
    let projected = project_user_message(&json!([
        {"text": "<app-context>private application metadata</app-context>"},
        {"text": "<environment_context>private environment metadata</environment_context>"},
        {"text": "Open the conversation profiler"}
    ]));

    assert_eq!(projected.as_deref(), Some("Open the conversation profiler"));
}
