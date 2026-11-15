#!/bin/sh
# Launch the portable host with private evidence outside the release directory.
set -eu
umask 077

package=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
state=${SQLolla_STATE:-${XDG_STATE_HOME:-$HOME/.local/state}/sqlolla}
case "$state" in
  /*) ;;
  *) echo 'SQLolla_STATE must be an absolute path' >&2; exit 2 ;;
esac
mkdir -p -- "$state"
state=$(CDPATH='' cd -- "$state" && pwd -P)
case "$state/" in
  "$package/"*) echo 'Choose a private data directory outside the release' >&2; exit 2 ;;
esac

export SQLolla_ROOT="$package"
export SQLolla_PARSER="$package/parser/sqlolla_parser"
export SQLolla_JAVA="$package/runtime/java/bin/java"
export SQLolla_ALLOY_JAR="$package/runtime/alloy/org.alloytools.alloy.dist.jar"
export SQLolla_INSTANCE_STORE="$state/instances"
export TMPDIR="$state/tmp"
mkdir -p -- "$SQLolla_INSTANCE_STORE" "$TMPDIR"
cd "$package"
exec "$package/bin/sqlolla-host"
