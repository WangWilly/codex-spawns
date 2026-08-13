#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: $0 TAG INPUT OUTPUT" >&2
  exit 2
fi

tag=$1
input=$2
output=$3

case "$tag" in
  v[0-9]*) ;;
  *) echo "TAG must start with v followed by a version number" >&2; exit 2 ;;
esac
[ -f "$input" ] || { echo "installer not found: $input" >&2; exit 1; }

awk -v tag="$tag" '
  $0 == "release_default_version=\047\047" {
    print "release_default_version=\047" tag "\047"
    replaced = 1
    next
  }
  { print }
  END { if (!replaced) exit 3 }
' "$input" >"$output.tmp"
mv "$output.tmp" "$output"
chmod 0755 "$output"
