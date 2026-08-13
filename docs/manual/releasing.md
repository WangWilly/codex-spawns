# Releasing codex-spawns

Publishing a release is deliberately split between automated builds and human
approval. A `v*` tag creates a draft release; a maintainer publishes it only
after completing this checklist.

## Prepare the version

Start from a clean, current `main` branch. Choose a SemVer version without a
leading `v`, then run:

```sh
cargo xtask release-version 0.3.0 --dry-run
cargo xtask release-version 0.3.0
```

The command validates SemVer, updates the root Cargo package version and lock
file, runs its checks, and verifies the binary version. It does not commit,
tag, push, or write release notes. Review the diff and prepare human-written
release notes before committing the version change.

Run the local gates:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
cargo bench --bench index_query
```

Push the version commit and wait for `.github/workflows/ci.yml` to succeed.

## Create the draft

Create a signed or annotated `v0.3.0` tag whose numeric part exactly matches
the root `Cargo.toml` version, then push it. `.github/workflows/release.yml`
must reject a mismatch and otherwise create a draft GitHub Release.

Do not publish until the workflow completes and the draft contains:

- Four `codex-spawns-<target>.tar.gz` archives for both macOS architectures
  and both Linux musl architectures.
- `SHA256SUMS` covering every archive.
- The version-bound `install.sh`.
- Release notes that describe user-visible changes and known limitations.

For each archive, confirm it contains only the `codex-spawns/` directory with
the executable, `LICENSE`, and `VERSION`. Confirm `VERSION` names the release
version, tagged commit, and target triple.

## Validate artifacts

Verify all archive checksums on a clean machine. Smoke-test `--version` and
`--help` on every executable platform available to the maintainer. In
particular, run the ARM64 Linux musl artifact on an ARM64 Linux machine; its CI
build is cross-compiled and this manual runtime check is a release gate.

Test the release-bound installer without relying on local build output:

```sh
curl -fsSL https://github.com/WangWilly/codex-spawns/releases/download/v0.3.0/install.sh \
  | CODEX_SPAWNS_VERSION=v0.3.0 CODEX_SPAWNS_INSTALL_DIR="$(mktemp -d)" sh
```

Verify the installed binary reports the expected version. Review the draft's
target commit and notes one final time, then publish it in GitHub. If any gate
fails, leave the release in draft while fixing the issue; do not replace files
in an already published release silently.
