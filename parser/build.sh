#!/usr/bin/env sh
# Build with an explicitly selected, already provisioned Lean 4.34.0 toolchain.
set -eu
: "${LEAN_BIN:?Set LEAN_BIN to the native Lean 4.34.0 bin directory}"
parser_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
dependency_dir="$parser_dir/../../dependencies/SQLeanParser"
expected_commit=ad15c585155ff7fc2c5fc4c9746905e4959ff5eb
actual_commit=$(git -C "$dependency_dir" rev-parse HEAD)
[ "$actual_commit" = "$expected_commit" ] || { echo 'SQLeanParser revision mismatch' >&2; exit 1; }
git -C "$dependency_dir" diff --quiet HEAD -- || { echo 'SQLeanParser tracked source changed' >&2; exit 1; }
case "$("$LEAN_BIN/lean" --version)" in
  *'version 4.34.0,'*) ;;
  *) echo 'Lean 4.34.0 required' >&2; exit 1 ;;
esac
cd "$parser_dir"
PATH="$LEAN_BIN:$PATH" "$LEAN_BIN/lake" build
