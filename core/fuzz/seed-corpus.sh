#!/bin/sh
# Writes the seed corpora (corpus/<target>/) from the repo's own schemas and
# package configs: std, the test fixtures and the examples. Not committed.
#
#   core/fuzz/seed-corpus.sh [corpus-dir]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:-$here/corpus}
mkdir -p "$out/ids_parse" "$out/ids_compile" "$out/idp_parse"
find "$root" -path "$here" -prune -o -path "$root/target" -prune -o -name '*.ids' -type f -print |
    while read -r f; do
        name=$(printf '%s' "${f#"$root"/}" | tr '/' '_')
        cp "$f" "$out/ids_parse/$name"
        cp "$f" "$out/ids_compile/$name"
    done
find "$root" -path "$here" -prune -o -path "$root/target" -prune -o -name '*.idp' -type f -print |
    while read -r f; do
        cp "$f" "$out/idp_parse/$(printf '%s' "${f#"$root"/}" | tr '/' '_')"
    done
