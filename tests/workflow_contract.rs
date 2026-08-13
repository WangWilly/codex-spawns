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
        "install.sh",
        ".github/workflows/**",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "actions/upload-artifact@",
        "retention-days: 7",
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
        "cargo-zigbuild --version 0.20.1 --locked",
        "ziglang==0.13.0",
        "scripts/package-release.sh",
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
fn all_actions_are_pinned_to_full_commit_shas() {
    for name in ["ci.yml", "release.yml"] {
        for line in workflow(name).lines() {
            let Some((_, reference)) = line
                .trim()
                .strip_prefix("uses: ")
                .and_then(|use_| use_.split_once('@'))
            else {
                continue;
            };
            let sha = reference.split_whitespace().next().unwrap();
            assert_eq!(sha.len(), 40, "{name} has an unpinned action: {line}");
            assert!(
                sha.chars().all(|ch| ch.is_ascii_hexdigit()),
                "{name} has an unpinned action: {line}"
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
