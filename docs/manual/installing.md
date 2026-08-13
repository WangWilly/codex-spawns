# Installing codex-spawns

Release binaries support Apple Silicon and Intel macOS, plus ARM64 and x86_64
Linux using musl. Windows is not a first-release target.

## Quick installation

The convenient command executes a release-hosted script directly:

```sh
curl -fsSL https://github.com/WangWilly/codex-spawns/releases/latest/download/install.sh | sh
```

Piping a remote response to a shell trusts both the release and the content
served at that URL. For a reviewable installation, download first:

```sh
curl -fsSLO https://github.com/WangWilly/codex-spawns/releases/latest/download/install.sh
less install.sh
sh install.sh
```

The installer detects the target, downloads its archive and `SHA256SUMS`, and
requires `sha256sum` or `shasum -a 256` to verify it. A missing verifier,
missing checksum entry, or mismatch stops installation. It installs to
`~/.local/bin` without `sudo` and does not edit shell configuration.

Available overrides:

```sh
CODEX_SPAWNS_VERSION=v0.2.0          # pin a release; the v prefix is required
CODEX_SPAWNS_INSTALL_DIR="$HOME/bin" # choose the destination
CODEX_SPAWNS_REPOSITORY=owner/repo    # test a fork's releases
```

Pass overrides to the receiving shell when using a pipe:

```sh
curl -fsSL https://github.com/WangWilly/codex-spawns/releases/download/v0.2.0/install.sh \
  | CODEX_SPAWNS_VERSION=v0.2.0 CODEX_SPAWNS_INSTALL_DIR="$HOME/bin" sh
```

## Manual installation

1. Open the desired GitHub Release and download the archive matching your
   platform and `SHA256SUMS`.
2. Verify the archive with `sha256sum -c SHA256SUMS` on Linux or
   `shasum -a 256 -c SHA256SUMS` on macOS.
3. Extract the archive and inspect its `VERSION` file.
4. Move `codex-spawns/codex-spawns` to a directory on your `PATH` and make it
   executable.
5. Run `codex-spawns --version`.

Artifact names use these target triples:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-musl`
- `x86_64-unknown-linux-musl`

## macOS Gatekeeper

Initial release binaries are not Apple-notarized. Gatekeeper may therefore
block the first launch even when the checksum is valid. Review the GitHub
Release and checksum before approving the binary through macOS Privacy &
Security settings. Do not disable Gatekeeper globally.
