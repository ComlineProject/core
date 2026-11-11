#!/usr/bin/env bash
# Vendors the specification's size vectors (vectors/sizes-<format>.json) and
# compiler conformance cases (compiler/cases/<group>.json) into tests/spec/,
# at the commit `comline-catalog` is pinned to in core/Cargo.toml: the
# vectors that hold the size rules, and the cases the compiler's rules about
# the guarantees the catalog names, so they must be the specification's own.
#
#   vendor.sh <rev>   # vendor <rev> (and pin the catalog to it by hand)
#   vendor.sh --check # fail if tests/spec/ differs from tests/spec/REV's
#
# The spec leads: a size rule or a case changes in specification first, and this
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
mkdir -p "$tmp/vectors" "$tmp/cases"
for format in packed msgpack json; do
  git -C "$tmp/specification" show "$rev:vectors/sizes-$format.json" > "$tmp/vectors/sizes-$format.json"
done
for case in $(git -C "$tmp/specification" ls-tree --name-only "$rev" compiler/cases/); do
  git -C "$tmp/specification" show "$rev:$case" > "$tmp/cases/$(basename "$case")"
done

if [[ "${1:-}" == "--check" ]]; then
  if diff -r "$tmp/vectors" "$here/vectors" && diff -r "$tmp/cases" "$here/cases"; then
    echo "tests/spec/vectors and tests/spec/cases match $repo@$rev"
  else
    echo "tests/spec/vectors or tests/spec/cases differ from $repo@$rev: re-vendor with vendor.sh <rev>" >&2
    exit 1
  fi
  exit 0
fi

rm -rf "$here/vectors" "$here/cases"
mv "$tmp/vectors" "$here/vectors"
mv "$tmp/cases" "$here/cases"
git -C "$tmp/specification" rev-parse "$rev^{commit}" > "$here/REV"
echo "vendored $repo@$(cat "$here/REV")"
