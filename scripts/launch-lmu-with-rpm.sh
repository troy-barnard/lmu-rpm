#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"
BRIDGE_SCRIPT="$ROOT_DIR/scripts/run-moza-rpm.sh"
SETUP_SCRIPT="$ROOT_DIR/scripts/setup-moza-rpm.sh"
RESET_SCRIPT="$ROOT_DIR/scripts/reset-runtime.sh"
LOG_FILE="$ROOT_DIR/moza-rpm-launch.log"
STATUS_LOG_FILE="$ROOT_DIR/moza-rpm-status.log"
STATUS_GUI_ERROR_LOG="$ROOT_DIR/moza-rpm-status-gui.log"

# Load configuration from secrets.json
source "$SCRIPT_DIR/read-secrets.sh" 2>/dev/null || {
  echo "Error: Could not load secrets.json configuration." >&2
  echo "Please copy example.secrets.json to secrets.json and customize it." >&2
  exit 1
}

# Allow overrides via environment variables
APP_ID="${STEAM_APP_ID:-$STEAM_APP_ID}"
START_DELAY="${MOZA_BRIDGE_START_DELAY:-$BRIDGE_START_DELAY}"
STATUS_GUI_ENABLED="${MOZA_STATUS_GUI:-1}"
AUTO_SETUP_ON_LAUNCH="${MOZA_AUTO_SETUP:-1}"
AUTO_RESET_ON_LAUNCH="${MOZA_AUTO_RESET_ON_LAUNCH:-1}"

if [ $# -eq 0 ]; then
  echo "No LMU launch command was provided." >&2
  echo "Use this script from Steam launch options with %command%." >&2
  exit 1
fi

BRIDGE_PID=""
STATUS_GUI_PID=""

status_line() {
  local message="$1"
  printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$message" >>"$STATUS_LOG_FILE"
}

start_status_gui() {
  if [ "$STATUS_GUI_ENABLED" != "1" ]; then
    status_line "Status window disabled (MOZA_STATUS_GUI=$STATUS_GUI_ENABLED)."
    return
  fi

  if ! command -v zenity >/dev/null 2>&1; then
    status_line "Status window disabled: zenity not found."
    return
  fi

  if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    status_line "Status window disabled: no GUI display detected."
    return
  fi

  : >"$STATUS_GUI_ERROR_LOG"
  # Keep the zenity command minimal for compatibility with Steam runtime variants.
  setsid bash -c "tail -n +1 -f '$STATUS_LOG_FILE' | zenity --title='LMU RPM Bridge Status' --text-info" >>"$STATUS_GUI_ERROR_LOG" 2>&1 &
  STATUS_GUI_PID=$!

  sleep 1
  if [ -n "$STATUS_GUI_PID" ] && kill -0 "$STATUS_GUI_PID" 2>/dev/null; then
    status_line "Status window started (PID $STATUS_GUI_PID)."
  else
    status_line "Status window failed to start. Check moza-rpm-status-gui.log."
  fi
}

cleanup() {
  status_line "Cleanup triggered. Stopping helper processes."

  if [ -n "$BRIDGE_PID" ] && kill -0 "$BRIDGE_PID" 2>/dev/null; then
    # Terminate the full bridge process group so proton/wine children do not linger.
    kill -TERM -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    status_line "Bridge process group stopped."
  fi

  if [ -n "$STATUS_GUI_PID" ] && kill -0 "$STATUS_GUI_PID" 2>/dev/null; then
    kill -TERM -- "-$STATUS_GUI_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$STATUS_GUI_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

is_lmu_process_visible() {
  pgrep -f "Le Mans Ultimate\\.exe|LeMansUltimate\\.exe" >/dev/null 2>&1
}

: >"$STATUS_LOG_FILE"
status_line "Launcher started (APP_ID=$APP_ID, bridge delay=${START_DELAY}s)."
status_line "LMU launch command: $*"

if [ "$AUTO_RESET_ON_LAUNCH" = "1" ]; then
  status_line "Running runtime reset preflight."
  if "$RESET_SCRIPT" >>"$LOG_FILE" 2>&1; then
    status_line "Runtime reset preflight completed."
  else
    status_line "Runtime reset preflight failed. Continuing launch; see moza-rpm-launch.log."
  fi
else
  status_line "Runtime reset preflight disabled (MOZA_AUTO_RESET_ON_LAUNCH=$AUTO_RESET_ON_LAUNCH)."
fi

if [ "$AUTO_SETUP_ON_LAUNCH" = "1" ]; then
  status_line "Running COM1 preflight setup."
  if STEAM_APP_ID="$APP_ID" "$SETUP_SCRIPT" >>"$LOG_FILE" 2>&1; then
    status_line "COM1 preflight setup completed."
  else
    status_line "COM1 preflight setup failed. Continuing launch; see moza-rpm-launch.log."
  fi
else
  status_line "COM1 preflight setup disabled (MOZA_AUTO_SETUP=$AUTO_SETUP_ON_LAUNCH)."
fi

# Start the GUI after preflight steps so reset/setup activity cannot interfere with it.
start_status_gui

"$@" &
GAME_PID=$!
status_line "LMU process started (PID $GAME_PID)."

status_line "Bridge launch scheduled after ${START_DELAY}s."
setsid bash -c "sleep '$START_DELAY'; STEAM_APP_ID='$APP_ID' '$BRIDGE_SCRIPT' >>'$LOG_FILE' 2>&1" &
BRIDGE_PID=$!
status_line "Bridge process group started (PID $BRIDGE_PID)."

seen_lmu_process=0
missing_count=0
reported_waiting_for_lmu=0

while kill -0 "$GAME_PID" 2>/dev/null; do
  if is_lmu_process_visible; then
    if [ "$seen_lmu_process" -eq 0 ]; then
      status_line "LMU Windows process detected in Proton."
    fi
    seen_lmu_process=1
    missing_count=0
  elif [ "$seen_lmu_process" -eq 1 ]; then
    missing_count=$((missing_count + 1))
    status_line "LMU process temporarily missing (${missing_count}/2)."
    if [ "$missing_count" -ge 2 ]; then
      status_line "LMU process disappeared; stopping bridge."
      cleanup
      break
    fi
  elif [ "$reported_waiting_for_lmu" -eq 0 ]; then
    status_line "Waiting for LMU Windows process to appear..."
    reported_waiting_for_lmu=1
  fi
  sleep 2
done

wait "$GAME_PID"
status_line "LMU process exited."
