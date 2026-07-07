#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRIDGE_SCRIPT="$ROOT_DIR/scripts/run-moza-rpm.sh"
LOG_FILE="$ROOT_DIR/moza-rpm-launch.log"
APP_ID="${STEAM_APP_ID:-2399420}"
START_DELAY="${MOZA_BRIDGE_START_DELAY:-10}"

if [ $# -eq 0 ]; then
  echo "No LMU launch command was provided." >&2
  echo "Use this script from Steam launch options with %command%." >&2
  exit 1
fi

BRIDGE_PID=""
cleanup() {
  if [ -n "$BRIDGE_PID" ] && kill -0 "$BRIDGE_PID" 2>/dev/null; then
    kill "$BRIDGE_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

"$@" &
GAME_PID=$!

(
  sleep "$START_DELAY"
  STEAM_APP_ID="$APP_ID" "$BRIDGE_SCRIPT" >>"$LOG_FILE" 2>&1
) &
BRIDGE_PID=$!

wait "$GAME_PID"
