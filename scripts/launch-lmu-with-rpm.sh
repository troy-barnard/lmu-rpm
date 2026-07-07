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
    # Terminate the full bridge process group so proton/wine children do not linger.
    kill -TERM -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

is_lmu_process_visible() {
  pgrep -f "Le Mans Ultimate\\.exe|LeMansUltimate\\.exe" >/dev/null 2>&1
}

"$@" &
GAME_PID=$!

setsid bash -c "sleep '$START_DELAY'; STEAM_APP_ID='$APP_ID' '$BRIDGE_SCRIPT' >>'$LOG_FILE' 2>&1" &
BRIDGE_PID=$!

seen_lmu_process=0
missing_count=0

while kill -0 "$GAME_PID" 2>/dev/null; do
  if is_lmu_process_visible; then
    seen_lmu_process=1
    missing_count=0
  elif [ "$seen_lmu_process" -eq 1 ]; then
    missing_count=$((missing_count + 1))
    if [ "$missing_count" -ge 2 ]; then
      cleanup
      break
    fi
  fi
  sleep 2
done

wait "$GAME_PID"
