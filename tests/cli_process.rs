use assert_cmd::Command;
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
        .stdout(predicate::str::contains("agents: 2"));
    Command::cargo_bin("codex-spawns")
        .unwrap()
        .args(["index", "rebuild"])
        .args(common)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed: 1 conversations"));
}
