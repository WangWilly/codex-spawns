use codex_spawns::{
    load_app_metadata, scan_sources_with_app_metadata, AppMetadataPaths, FactConfidence,
    ProjectAssignment,
};
use rusqlite::Connection;
use std::fs;
use tempfile::tempdir;

#[test]
fn fixture_adapter_reads_titles_tokens_and_current_project_states() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("state_5.sqlite");
    let json = dir.path().join(".codex-global-state.json");
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "CREATE TABLE threads (id TEXT, title TEXT, tokens_used INTEGER)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO threads VALUES ('assigned', '**App** [title](https://x)', 12), ('legacy', NULL, 0), ('none', NULL, 0), ('unknown', NULL, NULL)", []).unwrap();
    drop(conn);
    fs::write(&json, r#"{"local-projects":{"p1":{"name":"Project **One**"}},"thread-project-assignments":{"assigned":{"projectKind":"local","projectId":"p1","cwd":"/repo","pendingCoreUpdate":false},"legacy":"p1"},"projectless-thread-ids":["none"]}"#).unwrap();
    let snapshot = load_app_metadata(&AppMetadataPaths::new(&db, &json)).unwrap();
    assert_eq!(
        snapshot.threads["assigned"].title.as_deref(),
        Some("App title")
    );
    assert_eq!(snapshot.threads["none"].tokens_used, Some(0));
    assert_eq!(snapshot.threads["unknown"].tokens_used, None);
    assert_eq!(
        snapshot.projects["assigned"],
        ProjectAssignment::Assigned {
            id: "p1".into(),
            name: "Project One".into()
        }
    );
    assert_eq!(
        snapshot.projects["legacy"],
        ProjectAssignment::Assigned {
            id: "p1".into(),
            name: "Project One".into()
        }
    );
    assert_eq!(snapshot.projects["none"], ProjectAssignment::Projectless);
    assert!(!snapshot.projects.contains_key("unknown"));
}

#[test]
fn rollout_usage_is_effective_while_app_disagreement_and_title_evidence_are_preserved() {
    let dir = tempdir().unwrap();
    let rollout = dir.path().join("root.jsonl");
    let fallback = dir.path().join("fallback.jsonl");
    let db = dir.path().join("state.sqlite");
    let json = dir.path().join("global.json");
    fs::write(&rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"root\",\"title\":\"Rollout title\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":10}}}}\n").unwrap();
    fs::write(
        &fallback,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"fallback\"}}\n",
    )
    .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "CREATE TABLE threads (id TEXT, title TEXT, tokens_used INTEGER)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads VALUES ('root', 'App title', 11), ('fallback', NULL, 0)",
        [],
    )
    .unwrap();
    drop(conn);
    fs::write(
        &json,
        r#"{"local-projects":{},"thread-project-assignments":{},"projectless-thread-ids":[]}"#,
    )
    .unwrap();
    let app = load_app_metadata(&AppMetadataPaths::new(db, json)).unwrap();
    let result = scan_sources_with_app_metadata(&[rollout, fallback], &[], Some(&app)).unwrap();
    let title = &result.app_titles["root"];
    assert_eq!(title.value.as_deref(), Some("App title"));
    assert_eq!(title.conflicting_values, vec!["Rollout title"]);
    let usage = &result.session_tokens["root"];
    assert_eq!(usage.value.as_ref().unwrap().total_tokens, 10);
    assert_eq!(usage.confidence, FactConfidence::Conflicting);
    assert_eq!(usage.conflicting_values[0].total_tokens, 11);
    let aggregate = &result.conversation_tokens["root"].usage;
    assert_eq!(aggregate.value.as_ref().unwrap().total_tokens, 10);
    assert_eq!(aggregate.confidence, FactConfidence::Conflicting);
    assert_eq!(aggregate.conflicting_values[0].total_tokens, 11);
    assert!(aggregate
        .provenance
        .iter()
        .any(|source| matches!(source, codex_spawns::SourceRef::StateDatabase { .. })));
    assert_eq!(
        result.session_tokens["fallback"]
            .value
            .as_ref()
            .unwrap()
            .total_tokens,
        0
    );
}
