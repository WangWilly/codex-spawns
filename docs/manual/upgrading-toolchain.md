# Upgrading the pinned toolchain

Rust, Zig, `cargo-zigbuild`, and third-party GitHub Actions are pinned so local
and CI behavior changes intentionally. Upgrade them in a dedicated pull request.

1. Read upstream release notes and security advisories for the old and new
   versions.
2. Update `rust-toolchain.toml`, including the `rustfmt` and `clippy`
   components, and update any matching CI references.
3. Update the pinned Zig and `cargo-zigbuild` versions in both CI and release
   workflows.
4. Update third-party actions to reviewed full commit SHAs. Keep a comment with
   the corresponding release version beside each SHA.
5. Regenerate `Cargo.lock` when required and review dependency changes.
6. Run the complete local suite:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test --workspace
   cargo bench --bench index_query
   ```

7. Run or dispatch both macOS builds and both Linux musl builds. Inspect their
   archive layout, checksums, `VERSION` metadata, and binary architecture.
8. Smoke-test the binaries on native macOS Intel, macOS Apple Silicon, Linux
   x86_64, and Linux ARM64 hosts where available. The Linux ARM64 runtime test
   remains a manual requirement when CI cross-compiles it.
9. Confirm the installer tests and a clean installation still pass before
   merging.

Do not combine a toolchain upgrade with a feature release unless an urgent
security fix makes that necessary; separating the changes keeps regressions
attributable and rollback straightforward.
