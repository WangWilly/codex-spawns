use assert_cmd::Command;
use codex_spawns::{
    index::{IndexOptions, ProfileIndex},
    ProjectAssignment,
};
use predicates::prelude::*;
use tempfile::TempDir;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn explicit_list_emits_stable_json_without_messages() {
    let output = Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "list",
            "--file",
            &fixture("parent.jsonl"),
            "--file",
            &fixture("child.jsonl"),
            "--no-state-db",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["count"], 1);
    assert_eq!(value["records"][0]["task_name"], "repo-scout");
    assert!(value["records"][0].get("message").is_none());
    assert_eq!(value["records"][0]["parent_cwd"], "/repo");
    assert_eq!(value["records"][0]["child_cwd"], "/repo");
    assert_eq!(value["records"][0]["call_id"], "call-1");
    assert_eq!(value["records"][0]["parent_line"], 3);
    assert_eq!(value["records"][0]["output_line"], 4);
    assert_eq!(value["records"][0]["multi_agent_version"], "v2");
}

#[test]
fn legacy_time_and_cwd_filters_are_applied() {
    let common = [
        "list",
        "--file",
        &fixture("parent.jsonl"),
        "--file",
        &fixture("child.jsonl"),
        "--no-state-db",
        "--format",
        "json",
    ];
    let included = Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(common)
        .args([
            "--cwd",
            "/repo",
            "--since",
            "2026-08-05T01:02:00Z",
            "--until",
            "2026-08-05T01:02:00Z",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&included).unwrap()["count"],
        1
    );

    let excluded = Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(common)
        .args(["--cwd", "/different", "--since", "2026-08-05T01:02:01Z"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&excluded).unwrap()["count"],
        0
    );
}

#[test]
fn non_tty_no_args_falls_back_to_list() {
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "--codex-home",
            "/definitely/not/a/codex/home",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"records\""));
}

#[test]
fn invalid_limit_is_usage_error() {
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(["list", "--limit", "-1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument '-1'"));
}

#[test]
fn aliases_and_index_commands_are_exposed() {
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("interactive"))
        .stdout(predicate::str::contains("index"));
}

#[test]
fn index_refresh_status_and_rebuild_use_discovered_rollouts() {
    let home = TempDir::new().unwrap();
    let common = [
        "--codex-home",
        home.path().to_str().unwrap(),
        "--file",
        &fixture("parent.jsonl"),
        "--file",
        &fixture("child.jsonl"),
        "--no-state-db",
    ];
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(["index", "refresh"])
        .args(common)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "indexed: 1 conversations, 2 agents, 2 sources",
        ));
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "index",
            "status",
            "--codex-home",
            home.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("conversations: 1"))
        .stdout(predicate::str::contains("agents: 2"))
        .stdout(predicate::str::contains("projection_version: 3"))
        .stdout(predicate::str::contains("required_projection_version: 3"))
        .stdout(predicate::str::contains("needs_reprojection: false"));
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(["index", "rebuild"])
        .args(common)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed: 1 conversations"));
}

#[test]
fn index_refresh_keeps_distinct_rollout_identities_with_inherited_metadata() {
    let home = TempDir::new().unwrap();
    let first = home
        .path()
        .join("rollout-2026-08-13T00-00-00-019eece4-3a1b-7713-8f99-77d716fe2703.jsonl");
    let second = home
        .path()
        .join("rollout-2026-08-13T00-00-01-019eece5-4a1b-7713-8f99-77d716fe2704.jsonl");
    std::fs::write(
        &first,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"019eece4-3a1b-7713-8f99-77d716fe2703","cwd":"/first"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"019eecb6-8910-7603-b48a-998b04738e31"}}"#,
            "\n"
        ),
    )
    .unwrap();
    std::fs::write(
        &second,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"019eece5-4a1b-7713-8f99-77d716fe2704","cwd":"/second"}}"#,
            "\n",
            r#"{"type":"session_meta","payload":{"id":"019eecb6-8910-7603-b48a-998b04738e31"}}"#,
            "\n"
        ),
    )
    .unwrap();

    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "index",
            "refresh",
            "--codex-home",
            home.path().to_str().unwrap(),
            "--file",
            first.to_str().unwrap(),
            "--file",
            second.to_str().unwrap(),
            "--no-state-db",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed: 2 conversations"));
}

#[test]
fn index_refresh_reads_app_metadata_only_from_the_injected_codex_home() {
    let home = TempDir::new().unwrap();
    let catalog = home.path().join("state_5.sqlite");
    let connection = rusqlite::Connection::open(&catalog).unwrap();
    connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,tokens_used INTEGER); INSERT INTO threads VALUES('01900000-0000-7000-8000-000000000001','**App Atlas title**',1234);").unwrap();
    std::fs::write(home.path().join(".codex-global-state.json"), r#"{"local-projects":{"project-1":{"name":"Atlas"}},"thread-project-assignments":{"01900000-0000-7000-8000-000000000001":"project-1"},"projectless-thread-ids":[]}"#).unwrap();

    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "index",
            "refresh",
            "--codex-home",
            home.path().to_str().unwrap(),
            "--file",
            &fixture("parent.jsonl"),
            "--file",
            &fixture("child.jsonl"),
            "--no-state-db",
        ])
        .assert()
        .success();

    let index = ProfileIndex::open(IndexOptions {
        path: home.path().join("cache/codex-spawns/index.sqlite"),
    })
    .unwrap();
    let profile = index
        .profile("01900000-0000-7000-8000-000000000001")
        .unwrap()
        .unwrap();
    assert_eq!(profile.conversation.title, "App Atlas title");
    assert_eq!(
        profile.conversation.project.value,
        Some(ProjectAssignment::Assigned {
            id: "project-1".into(),
            name: "Atlas".into()
        })
    );
    assert_eq!(
        profile
            .conversation
            .tokens
            .usage
            .value
            .unwrap()
            .total_tokens,
        1234
    );
    assert_eq!(
        (
            profile.conversation.tokens.covered_sessions,
            profile.conversation.tokens.total_sessions
        ),
        (1, 2)
    );
}

#[test]
fn index_refresh_falls_back_to_nested_app_catalog_when_primary_is_unreadable() {
    let home = TempDir::new().unwrap();
    std::fs::write(home.path().join("state_5.sqlite"), "not sqlite").unwrap();
    std::fs::create_dir(home.path().join("sqlite")).unwrap();
    let nested = home.path().join("sqlite/state_5.sqlite");
    let connection = rusqlite::Connection::open(&nested).unwrap();
    connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,tokens_used INTEGER); INSERT INTO threads VALUES('01900000-0000-7000-8000-000000000001','Nested title',77);").unwrap();
    std::fs::write(home.path().join(".codex-global-state.json"), r#"{"local-projects":{},"thread-project-assignments":{},"projectless-thread-ids":["01900000-0000-7000-8000-000000000001"]}"#).unwrap();

    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args([
            "index",
            "refresh",
            "--codex-home",
            home.path().to_str().unwrap(),
            "--file",
            &fixture("parent.jsonl"),
            "--no-state-db",
        ])
        .assert()
        .success();
    let index = ProfileIndex::open(IndexOptions {
        path: home.path().join("cache/codex-spawns/index.sqlite"),
    })
    .unwrap();
    let profile = index
        .profile("01900000-0000-7000-8000-000000000001")
        .unwrap()
        .unwrap();
    assert_eq!(profile.conversation.title, "Nested title");
    assert_eq!(
        profile.conversation.project.value,
        Some(ProjectAssignment::Projectless)
    );
    assert!(index.app_metadata_status().unwrap().contains("ready"));
}

#[test]
fn incremental_refresh_retains_unchanged_child_tokens_and_tree_coverage() {
    let home = TempDir::new().unwrap();
    let root = home.path().join("root.jsonl");
    let child = home.path().join("child.jsonl");
    std::fs::write(&root, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"root\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":10}}}}\n").unwrap();
    std::fs::write(&child, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"root\"}}}}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":5}}}}\n").unwrap();
    let args = [
        "index",
        "refresh",
        "--codex-home",
        home.path().to_str().unwrap(),
        "--file",
        root.to_str().unwrap(),
        "--file",
        child.to_str().unwrap(),
        "--no-state-db",
    ];
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(args)
        .assert()
        .success();
    std::fs::write(&root, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"root\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"total_tokens\":12}}}}\n").unwrap();
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(args)
        .assert()
        .success();

    let index = ProfileIndex::open(IndexOptions {
        path: home.path().join("cache/codex-spawns/index.sqlite"),
    })
    .unwrap();
    let profile = index.profile("root").unwrap().unwrap();
    assert_eq!(
        profile
            .conversation
            .tokens
            .usage
            .value
            .unwrap()
            .total_tokens,
        17
    );
    assert_eq!(
        (
            profile.conversation.tokens.covered_sessions,
            profile.conversation.tokens.total_sessions
        ),
        (2, 2)
    );
    assert_eq!(
        profile
            .agents
            .iter()
            .find(|agent| agent.id == "child")
            .unwrap()
            .tokens
            .value
            .as_ref()
            .unwrap()
            .total_tokens,
        5
    );
}

#[test]
fn app_failure_without_rollout_changes_retains_enrichment_and_degrades_profile() {
    let home = TempDir::new().unwrap();
    let catalog = home.path().join("state_5.sqlite");
    let connection = rusqlite::Connection::open(&catalog).unwrap();
    connection.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,tokens_used INTEGER); INSERT INTO threads VALUES('01900000-0000-7000-8000-000000000001','Retained App title',44);").unwrap();
    let global = home.path().join(".codex-global-state.json");
    std::fs::write(&global, r#"{"local-projects":{},"thread-project-assignments":{},"projectless-thread-ids":["01900000-0000-7000-8000-000000000001"]}"#).unwrap();
    let args = [
        "index",
        "refresh",
        "--codex-home",
        home.path().to_str().unwrap(),
        "--file",
        &fixture("parent.jsonl"),
        "--no-state-db",
    ];
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(args)
        .assert()
        .success();
    std::fs::write(&global, "invalid json").unwrap();
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(args)
        .assert()
        .success();

    let index = ProfileIndex::open(IndexOptions {
        path: home.path().join("cache/codex-spawns/index.sqlite"),
    })
    .unwrap();
    let page = index.browse(&Default::default(), None, 25).unwrap();
    assert_eq!(page.conversations[0].title, "Retained App title");
    assert_eq!(
        page.conversations[0].project.value,
        Some(ProjectAssignment::Projectless)
    );
    assert_eq!(
        page.conversations[0]
            .tokens
            .usage
            .value
            .as_ref()
            .unwrap()
            .total_tokens,
        44
    );
    assert_eq!(
        page.semantics["01900000-0000-7000-8000-000000000001"].1,
        codex_spawns::index::ProfileQuality::Partial
    );
    assert!(index
        .app_metadata_status()
        .unwrap()
        .starts_with("App metadata unavailable:"));
}
