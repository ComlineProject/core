#!/usr/bin/env bash
# Vendors the specification's size vectors (vectors/sizes-<format>.json) into
# tests/spec/, at the commit `comline-catalog` is pinned to in core/Cargo.toml:
# the vectors that hold the size rules, so they must be the rules' own.
#
#   vendor.sh <rev>   # vendor <rev> (and pin the catalog to it by hand)
#   vendor.sh --check # fail if tests/spec/ differs from tests/spec/REV's
#
# The spec leads: a size rule changes in specification first, and this
# repository takes it by bumping the rev and re-vendoring. A test fails when
# REV and Cargo.toml disagree.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="https://github.com/ComlineProject/specification"

if [[ "${1:-}" == "--check" ]]; then
  rev="$(cat "$here/REV")"
else
  rev="${1:?usage: vendor.sh <rev> | --check}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git clone --quiet --no-checkout "$repo" "$tmp/specification"
mkdir -p "$tmp/vectors"
for format in packed msgpack json; do
  git -C "$tmp/specification" show "$rev:vectors/sizes-$format.json" > "$tmp/vectors/sizes-$format.json"
done

if [[ "${1:-}" == "--check" ]]; then
  if diff -r "$tmp/vectors" "$here/vectors"; then
    echo "tests/spec/vectors match $repo@$rev"
  else
    echo "tests/spec/vectors differ from $repo@$rev: re-vendor with vendor.sh <rev>" >&2
    exit 1
  fi
  exit 0
fi

rm -rf "$here/vectors"
mv "$tmp/vectors" "$here/vectors"
git -C "$tmp/specification" rev-parse "$rev^{commit}" > "$here/REV"
echo "vendored $repo@$(cat "$here/REV")"
