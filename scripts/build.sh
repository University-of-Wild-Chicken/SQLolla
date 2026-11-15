#!/usr/bin/env sh
# Native Unix build entry point; platform execution is still a release gate.
set -eu
: "${LEAN_BIN:?Set LEAN_BIN to the native Lean 4.34.0 bin directory}"
: "${SQLolla_ALLOY_JAR:?Set SQLolla_ALLOY_JAR to the pinned Alloy 6.2 jar}"
task_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$task_root"
python3 -c 'import hashlib,os; p=os.environ["SQLolla_ALLOY_JAR"]; assert hashlib.sha256(open(p,"rb").read()).hexdigest()=="6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d", "Alloy artifact mismatch"'
sh parser/build.sh
mkdir -p bridge/classes
"${SQLolla_JAVAC:-javac}" --release 17 -Xlint:all -cp "$SQLolla_ALLOY_JAR" -d bridge/classes bridge/AlloyBridge.java
cargo build --workspace --locked
