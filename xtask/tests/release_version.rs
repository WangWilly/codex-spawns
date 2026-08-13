#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    cargo_log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "codex-spawns-xtask-{}-{unique}-{counter}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"codex-spawns\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.lock"),
            "# fixture lock\nname = \"codex-spawns\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();

        let bin = dir.join("bin");
        fs::create_dir(&bin).unwrap();
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_CARGO_LOG"
case "$1" in
  check)
    version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
    printf '# fixture lock\nname = "codex-spawns"\nversion = "%s"\n' "$version" > Cargo.lock
    ;;
  run)
    version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
    printf 'codex-spawns %s\n' "$version"
    ;;
  *) exit 91 ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&cargo).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&cargo, permissions).unwrap();

        Self {
            cargo_log: dir.join("cargo.log"),
            dir,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(self.dir.join("bin")).chain(std::env::split_paths(&old_path)),
        )
        .unwrap();
        command
            .current_dir(&self.dir)
            .env("PATH", path)
            .env("FAKE_CARGO_LOG", &self.cargo_log);
        command
    }

    fn read(&self, path: impl AsRef<Path>) -> String {
        fs::read_to_string(self.dir.join(path)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn output(command: &mut Command) -> (bool, String, String) {
    let output = command.output().unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn updates_manifest_lockfile_and_verifies_binary_version() {
    let fixture = Fixture::new();

    let (success, stdout, stderr) = output(fixture.command().args(["release-version", "0.3.0"]));
    assert!(success, "{stderr}");
    assert!(stdout.contains("Updated codex-spawns to 0.3.0"));

    assert!(fixture.read("Cargo.toml").contains("version = \"0.3.0\""));
    assert!(fixture.read("Cargo.lock").contains("version = \"0.3.0\""));
    assert_eq!(
        fixture.read("cargo.log"),
        "check\nrun --quiet -- --version\n"
    );
}

#[test]
fn dry_run_validates_without_writing_or_running_cargo() {
    let fixture = Fixture::new();
    let manifest_before = fixture.read("Cargo.toml");
    let lock_before = fixture.read("Cargo.lock");

    let (success, stdout, stderr) =
        output(
            fixture
                .command()
                .args(["release-version", "0.3.0", "--dry-run"]),
        );
    assert!(success, "{stderr}");
    assert!(stdout.contains("Would update codex-spawns from 0.2.0 to 0.3.0"));

    assert_eq!(fixture.read("Cargo.toml"), manifest_before);
    assert_eq!(fixture.read("Cargo.lock"), lock_before);
    assert!(!fixture.cargo_log.exists());
}

#[test]
fn accepts_semver_prerelease_and_build_identifiers() {
    let fixture = Fixture::new();
    let (success, stdout, stderr) = output(fixture.command().args([
        "release-version",
        "1.0.0-alpha-beta.1+build.42",
        "--dry-run",
    ]));

    assert!(success, "{stderr}");
    assert!(stdout.contains("1.0.0-alpha-beta.1+build.42"));
}

#[test]
fn rejects_prefixed_and_invalid_versions_without_writing() {
    for version in ["v0.3.0", "next", "1.2"] {
        let fixture = Fixture::new();
        let manifest_before = fixture.read("Cargo.toml");

        let (success, _, stderr) = output(fixture.command().args(["release-version", version]));
        assert!(!success);
        assert!(stderr.contains("valid SemVer"), "{stderr}");

        assert_eq!(fixture.read("Cargo.toml"), manifest_before);
        assert!(!fixture.cargo_log.exists());
    }
}

#[test]
fn rolls_back_files_when_binary_version_does_not_match() {
    let fixture = Fixture::new();
    let manifest_before = fixture.read("Cargo.toml");
    let lock_before = fixture.read("Cargo.lock");
    let cargo = fixture.dir.join("bin/cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$FAKE_CARGO_LOG"
if [ "$1" = check ]; then
  printf '# fixture lock\nname = "codex-spawns"\nversion = "0.3.0"\n' > Cargo.lock
else
  printf 'codex-spawns 9.9.9\n'
fi
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&cargo).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cargo, permissions).unwrap();

    let (success, _, stderr) = output(fixture.command().args(["release-version", "0.3.0"]));
    assert!(!success);
    assert!(stderr.contains("version verification failed"), "{stderr}");

    assert_eq!(fixture.read("Cargo.toml"), manifest_before);
    assert_eq!(fixture.read("Cargo.lock"), lock_before);
}
