use assert_cmd::Command;
use predicates::prelude::*;

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
