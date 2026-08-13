use std::fs;

fn workflow(name: &str) -> String {
    fs::read_to_string(format!(".github/workflows/{name}"))
        .unwrap_or_else(|error| panic!("failed to read {name}: {error}"))
}

#[test]
fn ci_has_least_privilege_checks_and_conditional_release_matrix() {
    let ci = workflow("ci.yml");

    for required in [
        "pull_request:",
        "branches: [main]",
        "contents: read",
        "cargo fmt --all -- --check",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo test --workspace",
        "detect-release-changes",
        "src/**",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "LICENSE",
        ".cargo/config.toml",
        "install.sh",
        "scripts/package-release.sh",
        ".github/actions/**",
        ".github/workflows/**",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "uses: ./.github/actions/build-release",
    ] {
        assert!(ci.contains(required), "ci.yml is missing {required:?}");
    }
    assert!(!ci.contains("contents: write"));
}

#[test]
fn release_is_tag_gated_draft_and_publishes_complete_bundle() {
    let release = workflow("release.yml");

    for required in [
        "tags: ['v*']",
        "contents: write",
        "Cargo.toml",
        "tag/version mismatch",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "build_command: cargo zigbuild",
        "uses: ./.github/actions/build-release",
        "scripts/render-installer.sh",
        ".github/actions/build-release",
        "SHA256SUMS",
        "install.sh",
        "gh release create",
        "--draft",
    ] {
        assert!(
            release.contains(required),
            "release.yml is missing {required:?}"
        );
    }
}

#[test]
fn workflows_share_one_release_build_contract() {
    let ci = workflow("ci.yml");
    let release = workflow("release.yml");
    let action = fs::read_to_string(".github/actions/build-release/action.yml")
        .expect("shared release build action must exist");

    assert!(ci.contains("uses: ./.github/actions/build-release"));
    assert!(release.contains("uses: ./.github/actions/build-release"));
    for required in [
        "cargo-zigbuild --version 0.20.1 --locked",
        "ziglang==0.13.0",
        "scripts/package-release.sh",
        "actual_version=$(",
        "--version)",
        "test \"$actual_version\" = \"$expected_version\"",
        "expected_version",
        "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",
    ] {
        assert!(
            action.contains(required),
            "shared action is missing {required:?}"
        );
    }
    assert!(!action.contains("permissions:"));
}

#[test]
fn release_renders_a_tag_bound_installer() {
    let release = workflow("release.yml");
    assert!(release.contains("scripts/render-installer.sh"));
    assert!(release.contains("github.ref_name"));
    assert!(!release.contains("cp install.sh dist/install.sh"));
}

#[test]
fn all_actions_are_pinned_to_full_commit_shas() {
    for path in [
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/actions/build-release/action.yml",
    ] {
        let source = fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
        for line in source.lines() {
            let Some((_, reference)) = line
                .trim()
                .strip_prefix("uses: ")
                .and_then(|use_| use_.split_once('@'))
            else {
                continue;
            };
            let sha = reference.split_whitespace().next().unwrap();
            assert_eq!(sha.len(), 40, "{path} has an unpinned action: {line}");
            assert!(
                sha.chars().all(|ch| ch.is_ascii_hexdigit()),
                "{path} has an unpinned action: {line}"
            );
        }
    }
}

#[test]
fn rust_toolchain_is_reproducibly_pinned() {
    let toolchain =
        fs::read_to_string("rust-toolchain.toml").expect("rust-toolchain.toml must exist");
    assert!(toolchain.contains("channel = \"1.94.1\""));
    assert!(toolchain.contains("rustfmt"));
    assert!(toolchain.contains("clippy"));
}
