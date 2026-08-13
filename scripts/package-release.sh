#!/bin/sh
set -eu

usage() {
  echo "usage: $0 VERSION TARGET BINARY OUTPUT_DIR" >&2
  exit 2
}

[ "$#" -eq 4 ] || usage
version=$1
target=$2
binary=$3
output_dir=$4

[ -f "$binary" ] || { echo "binary not found: $binary" >&2; exit 1; }
[ -f LICENSE ] || { echo "LICENSE must exist in the repository root" >&2; exit 1; }

case "$version" in
  ''|v*) echo "VERSION must be an unprefixed version number" >&2; exit 2 ;;
esac
case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin|aarch64-unknown-linux-musl|x86_64-unknown-linux-musl) ;;
  *) echo "unsupported release target: $target" >&2; exit 2 ;;
esac

if command -v sha256sum >/dev/null 2>&1; then
  checksum=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  checksum='shasum -a 256'
else
  echo "sha256sum or shasum is required" >&2
  exit 1
fi

mkdir -p "$output_dir"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/codex-spawns"
cp "$binary" "$stage/codex-spawns/codex-spawns"
chmod 0755 "$stage/codex-spawns/codex-spawns"
cp LICENSE "$stage/codex-spawns/LICENSE"
{
  echo "version=$version"
  echo "commit=${GIT_COMMIT_SHA:-unknown}"
  echo "target=$target"
} >"$stage/codex-spawns/VERSION"

archive_name="codex-spawns-$target.tar.gz"
archive="$output_dir/$archive_name"
tar -czf "$archive" -C "$stage" codex-spawns

checksum_file="$output_dir/SHA256SUMS"
checksum_line=$(cd "$output_dir" && $checksum "$archive_name")
if [ -f "$checksum_file" ]; then
  awk -v name="$archive_name" '$2 != name { print }' "$checksum_file" >"$checksum_file.tmp"
  printf '%s\n' "$checksum_line" >>"$checksum_file.tmp"
  mv "$checksum_file.tmp" "$checksum_file"
else
  printf '%s\n' "$checksum_line" >"$checksum_file"
fi

echo "created $archive"
