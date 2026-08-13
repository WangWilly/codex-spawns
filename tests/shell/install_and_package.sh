#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp_root=$(mktemp -d)
trap 'rm -rf "$tmp_root"' EXIT HUP INT TERM

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_file() {
  [ -f "$1" ] || fail "expected file: $1"
}

assert_contains() {
  grep -F "$2" "$1" >/dev/null || fail "expected '$2' in $1"
}

binary="$tmp_root/codex-spawns"
printf '#!/bin/sh\necho codex-spawns 0.3.0\n' >"$binary"
chmod +x "$binary"
out="$tmp_root/release"
mkdir -p "$out"

GIT_COMMIT_SHA=deadbeef "$repo_root/scripts/package-release.sh" \
  0.3.0 x86_64-unknown-linux-musl "$binary" "$out"

archive="$out/codex-spawns-x86_64-unknown-linux-musl.tar.gz"
assert_file "$archive"
assert_file "$out/SHA256SUMS"
checksum_line=$(grep 'codex-spawns-x86_64-unknown-linux-musl.tar.gz$' "$out/SHA256SUMS")
[ -n "$checksum_line" ] || fail "archive checksum is missing"

unpacked="$tmp_root/unpacked"
mkdir -p "$unpacked"
tar -xzf "$archive" -C "$unpacked"
assert_file "$unpacked/codex-spawns/codex-spawns"
assert_file "$unpacked/codex-spawns/LICENSE"
assert_file "$unpacked/codex-spawns/VERSION"
assert_contains "$unpacked/codex-spawns/VERSION" 'version=0.3.0'
assert_contains "$unpacked/codex-spawns/VERSION" 'commit=deadbeef'
assert_contains "$unpacked/codex-spawns/VERSION" 'target=x86_64-unknown-linux-musl'

case "$(tar -tzf "$archive" | sort)" in
  *"codex-spawns/LICENSE"*"codex-spawns/VERSION"*"codex-spawns/codex-spawns"*) ;;
  *) fail "archive contents do not match the release contract" ;;
esac

fake_bin="$tmp_root/fake-bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' "${FAKE_UNAME_S:-Linux}" ;;
  -m) printf '%s\n' "${FAKE_UNAME_M:-x86_64}" ;;
  *) exit 2 ;;
esac
EOF
cat >"$fake_bin/curl" <<'EOF'
#!/bin/sh
set -eu
output=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output=$2; shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
[ -n "$output" ] && [ -n "$url" ]
printf '%s\n' "$url" >>"$FAKE_CURL_LOG"
cp "$FAKE_RELEASE_DIR/${url##*/}" "$output"
EOF
cat >"$fake_bin/sudo" <<'EOF'
#!/bin/sh
echo "sudo was invoked" >>"$FAKE_SUDO_LOG"
exit 99
EOF
chmod +x "$fake_bin/uname" "$fake_bin/curl" "$fake_bin/sudo"

for tool in dirname mktemp rm mkdir cp chmod tar grep awk mv cat; do
  tool_path=$(command -v "$tool")
  ln -s "$tool_path" "$fake_bin/$tool"
done
if command -v sha256sum >/dev/null 2>&1; then
  ln -s "$(command -v sha256sum)" "$fake_bin/sha256sum"
else
  ln -s "$(command -v shasum)" "$fake_bin/shasum"
fi

install_dir="$tmp_root/install"
curl_log="$tmp_root/curl.log"
sudo_log="$tmp_root/sudo.log"
fake_home="$tmp_root/home"
mkdir -p "$fake_home"
printf '%s\n' 'profile sentinel' >"$fake_home/.profile"
: >"$curl_log"
: >"$sudo_log"
PATH="$fake_bin" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
  FAKE_SUDO_LOG="$sudo_log" HOME="$fake_home" \
  CODEX_SPAWNS_VERSION=v0.3.0 CODEX_SPAWNS_REPOSITORY=example/fork \
  CODEX_SPAWNS_INSTALL_DIR="$install_dir" \
  /bin/sh "$repo_root/install.sh"
assert_file "$install_dir/codex-spawns"
assert_contains "$curl_log" 'https://github.com/example/fork/releases/download/v0.3.0/codex-spawns-x86_64-unknown-linux-musl.tar.gz'
assert_contains "$curl_log" 'https://github.com/example/fork/releases/download/v0.3.0/SHA256SUMS'
[ ! -s "$sudo_log" ] || fail "installer invoked sudo"
[ "$(cat "$fake_home/.profile")" = 'profile sentinel' ] || fail "installer modified a shell profile"

# macOS arm64 maps to the release target and latest uses the release-bound URL.
: >"$curl_log"
cp "$archive" "$out/codex-spawns-aarch64-apple-darwin.tar.gz"
checksum_tool=sha256sum
command -v sha256sum >/dev/null 2>&1 || checksum_tool='shasum -a 256'
(cd "$out" && $checksum_tool codex-spawns-aarch64-apple-darwin.tar.gz >SHA256SUMS)
PATH="$fake_bin" FAKE_UNAME_S=Darwin FAKE_UNAME_M=arm64 \
  FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
  CODEX_SPAWNS_INSTALL_DIR="$tmp_root/mac-install" \
  /bin/sh "$repo_root/install.sh"
assert_contains "$curl_log" 'https://github.com/WangWilly/codex-spawns/releases/latest/download/codex-spawns-aarch64-apple-darwin.tar.gz'

# The remaining supported OS/architecture pairs map to their exact target triples.
for mapping in 'Darwin x86_64 x86_64-apple-darwin' 'Linux aarch64 aarch64-unknown-linux-musl'; do
  set -- $mapping
  mapped_os=$1
  mapped_arch=$2
  mapped_target=$3
  mapped_archive="codex-spawns-$mapped_target.tar.gz"
  cp "$archive" "$out/$mapped_archive"
  (cd "$out" && $checksum_tool "$mapped_archive" >SHA256SUMS)
  : >"$curl_log"
  PATH="$fake_bin" FAKE_UNAME_S="$mapped_os" FAKE_UNAME_M="$mapped_arch" \
    FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
    CODEX_SPAWNS_INSTALL_DIR="$tmp_root/$mapped_target-install" \
    /bin/sh "$repo_root/install.sh"
  assert_contains "$curl_log" "https://github.com/WangWilly/codex-spawns/releases/latest/download/$mapped_archive"
done

# A checksum mismatch must not install anything.
printf '%064d  %s\n' 0 codex-spawns-x86_64-unknown-linux-musl.tar.gz >"$out/SHA256SUMS"
if PATH="$fake_bin" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
  CODEX_SPAWNS_INSTALL_DIR="$tmp_root/bad-install" \
  /bin/sh "$repo_root/install.sh" >/dev/null 2>&1; then
  fail "installer accepted a bad checksum"
fi
[ ! -e "$tmp_root/bad-install/codex-spawns" ] || fail "bad artifact was installed"

# Without sha256sum or shasum the installer must fail before downloading.
no_checksum_bin="$tmp_root/no-checksum-bin"
mkdir -p "$no_checksum_bin"
for tool in uname curl dirname mktemp rm mkdir cp chmod tar grep awk mv cat; do
  ln -s "$fake_bin/$tool" "$no_checksum_bin/$tool"
done
: >"$curl_log"
if PATH="$no_checksum_bin" FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
  CODEX_SPAWNS_INSTALL_DIR="$tmp_root/no-checksum-install" \
  /bin/sh "$repo_root/install.sh" >/dev/null 2>&1; then
  fail "installer ran without a checksum tool"
fi
[ ! -s "$curl_log" ] || fail "installer downloaded before checksum capability check"

# Unsupported platforms fail before downloading as well.
: >"$curl_log"
if PATH="$fake_bin" FAKE_UNAME_S=Plan9 FAKE_UNAME_M=mips \
  FAKE_RELEASE_DIR="$out" FAKE_CURL_LOG="$curl_log" \
  /bin/sh "$repo_root/install.sh" >/dev/null 2>&1; then
  fail "installer accepted an unsupported platform"
fi
[ ! -s "$curl_log" ] || fail "unsupported platform triggered a download"

echo "installer and release artifact tests passed"
