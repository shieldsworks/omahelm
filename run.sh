#!/usr/bin/env bash
# Opens the chartplotter from this checkout, in its own Quickshell process.
# The window starts the engine (OMAHELM_BIN, default this checkout's
# release build) when it isn't already running.
set -euo pipefail
cd "$(dirname "$0")"
export OMAHELM_BIN=${OMAHELM_BIN:-$PWD/target/release/omahelm}
if [[ ! -x $OMAHELM_BIN ]]; then
  printf 'No engine at %s. Build it with: cargo build --release\n' "$OMAHELM_BIN" >&2
  exit 1
fi
exec quickshell -p ui/shell.qml "$@"
