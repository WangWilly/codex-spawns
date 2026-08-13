#!/bin/sh
set -eu

repository=${CODEX_SPAWNS_REPOSITORY:-WangWilly/codex-spawns}
install_dir=${CODEX_SPAWNS_INSTALL_DIR:-"$HOME/.local/bin"}
# Release automation replaces this exact assignment in the uploaded copy.
# The tracked installer intentionally remains suitable for the latest URL.
release_default_version=''
version=${CODEX_SPAWNS_VERSION:-$release_default_version}

case "$repository" in
  */*) ;;
  *) echo "CODEX_SPAWNS_REPOSITORY must be OWNER/REPOSITORY" >&2; exit 2 ;;
esac
case "$version" in
  '') release_path=latest/download ;;
  v*) release_path="download/$version" ;;
  *) echo "CODEX_SPAWNS_VERSION must include the v prefix (for example v0.3.0)" >&2; exit 2 ;;
esac

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  Darwin:x86_64|Darwin:amd64) target=x86_64-apple-darwin ;;
  Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-musl ;;
  Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-musl ;;
  *) echo "unsupported platform: $os $arch" >&2; exit 1 ;;
esac

if command -v sha256sum >/dev/null 2>&1; then
  checksum_kind=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  checksum_kind=shasum
else
  echo "sha256sum or shasum is required; refusing to install unverified software" >&2
  exit 1
fi

archive_name="codex-spawns-$target.tar.gz"
base_url="https://github.com/$repository/releases/$release_path"
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM

curl -fsSL -o "$work_dir/$archive_name" "$base_url/$archive_name"
curl -fsSL -o "$work_dir/SHA256SUMS" "$base_url/SHA256SUMS"
awk -v name="$archive_name" '$2 == name { print }' "$work_dir/SHA256SUMS" >"$work_dir/EXPECTED_SHA256"
[ -s "$work_dir/EXPECTED_SHA256" ] || { echo "checksum entry missing for $archive_name" >&2; exit 1; }

if [ "$checksum_kind" = sha256sum ]; then
  (cd "$work_dir" && sha256sum -c EXPECTED_SHA256)
else
  (cd "$work_dir" && shasum -a 256 -c EXPECTED_SHA256)
fi

mkdir -p "$work_dir/unpacked"
tar -xzf "$work_dir/$archive_name" -C "$work_dir/unpacked"
[ -f "$work_dir/unpacked/codex-spawns/codex-spawns" ] || { echo "release archive does not contain codex-spawns" >&2; exit 1; }

mkdir -p "$install_dir"
destination_tmp="$install_dir/.codex-spawns.tmp.$$"
cp "$work_dir/unpacked/codex-spawns/codex-spawns" "$destination_tmp"
chmod 0755 "$destination_tmp"
mv "$destination_tmp" "$install_dir/codex-spawns"

echo "installed codex-spawns to $install_dir/codex-spawns"
