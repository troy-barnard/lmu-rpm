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
STATUS_GUI_PY="$ROOT_DIR/scripts/status-gui.py"

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
SETUP_RETRY_COUNT="${MOZA_SETUP_RETRY_COUNT:-8}"
SETUP_RETRY_DELAY="${MOZA_SETUP_RETRY_DELAY:-2}"
BRIDGE_CONNECT_TIMEOUT="${MOZA_BRIDGE_CONNECT_TIMEOUT:-45}"
BRIDGE_RESTART_ON_CONNECT_FAIL="${MOZA_BRIDGE_RESTART_ON_CONNECT_FAIL:-1}"
REQUIRE_RPM_SAMPLE="${MOZA_REQUIRE_RPM_SAMPLE:-1}"
FORCE_RPM_COLORS_ON_BOOT="${MOZA_FORCE_RPM_COLORS:-1}"
FORCE_BUTTON_COLORS_ON_BOOT="${MOZA_FORCE_BUTTON_COLORS:-0}"
MENU_FORCE_BUTTON_COLORS_ON_IDLE="${MOZA_MENU_FORCE_BUTTON_COLORS:-0}"
IDLE_LED_WRITES="${MOZA_IDLE_LED_WRITES:-0}"
ENABLE_BUTTON_COLOR_OVERRIDES="${MOZA_ENABLE_BUTTON_COLOR_OVERRIDES:-0}"

if [ $# -eq 0 ]; then
  echo "No LMU launch command was provided." >&2
  echo "Use this script from Steam launch options with %command%." >&2
  exit 1
fi

BRIDGE_PID=""
STATUS_GUI_PID=""
BRIDGE_LOG_OFFSET=0
BRIDGE_STATUS_OFFSET=0

status_line() {
  if [ "$#" -eq 0 ]; then
    return
  fi

  local level="INFO"
  local message
  if [ "$#" -ge 2 ]; then
    level="$1"
    shift
    message="$*"
  else
    message="$1"
  fi

  printf '[%s] [%s] %s\n' "$(date '+%H:%M:%S')" "$level" "$message" >>"$STATUS_LOG_FILE"
}

start_status_gui() {
  if [ "$STATUS_GUI_ENABLED" != "1" ]; then
    status_line "WARN" "Status window disabled (MOZA_STATUS_GUI=$STATUS_GUI_ENABLED)."
    return
  fi

  if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    status_line "WARN" "Status window disabled: no GUI display detected."
    return
  fi

  : >"$STATUS_GUI_ERROR_LOG"

  if command -v python3 >/dev/null 2>&1 && [ -f "$STATUS_GUI_PY" ]; then
    setsid python3 "$STATUS_GUI_PY" "$STATUS_LOG_FILE" >>"$STATUS_GUI_ERROR_LOG" 2>&1 &
  elif command -v zenity >/dev/null 2>&1; then
    # Fallback path if Python/Tk is not available.
    setsid bash -c "tail -n +1 -f '$STATUS_LOG_FILE' | zenity --title='LMU RPM Bridge Status' --text-info" >>"$STATUS_GUI_ERROR_LOG" 2>&1 &
  else
    status_line "WARN" "Status window disabled: python3/Tk and zenity unavailable."
    return
  fi

  STATUS_GUI_PID=$!

  sleep 1
  if [ -n "$STATUS_GUI_PID" ] && kill -0 "$STATUS_GUI_PID" 2>/dev/null; then
    status_line "SUCCESS" "Status window started (PID $STATUS_GUI_PID)."
  else
    status_line "ERROR" "Status window failed to start. Check moza-rpm-status-gui.log."
  fi
}

cleanup() {
  status_line "WARN" "Cleanup triggered. Stopping helper processes."

  if [ -n "$BRIDGE_PID" ] && kill -0 "$BRIDGE_PID" 2>/dev/null; then
    # Terminate the full bridge process group so proton/wine children do not linger.
    kill -TERM -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    status_line "INFO" "Bridge process group stopped."
  fi

  if [ -n "$STATUS_GUI_PID" ] && kill -0 "$STATUS_GUI_PID" 2>/dev/null; then
    kill -TERM -- "-$STATUS_GUI_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$STATUS_GUI_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

stop_bridge_process_group() {
  if [ -n "$BRIDGE_PID" ] && kill -0 "$BRIDGE_PID" 2>/dev/null; then
    kill -TERM -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
    sleep 1
    kill -KILL -- "-$BRIDGE_PID" >/dev/null 2>&1 || true
  fi
}

is_lmu_process_visible() {
  pgrep -f "Le Mans Ultimate\\.exe|LeMansUltimate\\.exe" >/dev/null 2>&1
}

start_bridge_process() {
  BRIDGE_LOG_OFFSET=$(wc -c <"$LOG_FILE" 2>/dev/null || echo 0)
  BRIDGE_STATUS_OFFSET=$BRIDGE_LOG_OFFSET
  setsid bash -c "sleep '$START_DELAY'; STEAM_APP_ID='$APP_ID' MOZA_FORCE_RPM_COLORS='$FORCE_RPM_COLORS_ON_BOOT' MOZA_FORCE_BUTTON_COLORS='$FORCE_BUTTON_COLORS_ON_BOOT' MOZA_MENU_FORCE_BUTTON_COLORS='$MENU_FORCE_BUTTON_COLORS_ON_IDLE' MOZA_IDLE_LED_WRITES='$IDLE_LED_WRITES' MOZA_ENABLE_BUTTON_COLOR_OVERRIDES='$ENABLE_BUTTON_COLOR_OVERRIDES' '$BRIDGE_SCRIPT' >>'$LOG_FILE' 2>&1" &
  BRIDGE_PID=$!
  status_line "INFO" "Bridge process group started (PID $BRIDGE_PID)."
}

bridge_connected_since_start() {
  local start_byte=$((BRIDGE_LOG_OFFSET + 1))
  if [ ! -f "$LOG_FILE" ]; then
    return 1
  fi

  if [ "$REQUIRE_RPM_SAMPLE" = "1" ]; then
    tail -c +"$start_byte" "$LOG_FILE" 2>/dev/null | grep -Eq "RPM telemetry active\."
  else
    tail -c +"$start_byte" "$LOG_FILE" 2>/dev/null | grep -Eq "Connected!|Connected to LMU native shared memory"
  fi
}

wait_for_bridge_connection() {
  local elapsed=0
  while [ "$elapsed" -lt "$BRIDGE_CONNECT_TIMEOUT" ]; do
    if [ -n "$BRIDGE_PID" ] && ! kill -0 "$BRIDGE_PID" 2>/dev/null; then
      status_line "ERROR" "Bridge process exited before telemetry connection."
      return 1
    fi

    if bridge_connected_since_start; then
      status_line "SUCCESS" "Bridge telemetry connection established."
      return 0
    fi

    sleep 1
    elapsed=$((elapsed + 1))
  done

  status_line "ERROR" "Bridge telemetry connection timed out after ${BRIDGE_CONNECT_TIMEOUT}s."
  return 1
}

pump_bridge_status_from_log() {
  if [ ! -f "$LOG_FILE" ]; then
    return
  fi

  local current_size
  current_size=$(wc -c <"$LOG_FILE" 2>/dev/null || echo 0)
  if [ "$current_size" -lt "$BRIDGE_STATUS_OFFSET" ]; then
    BRIDGE_STATUS_OFFSET=0
  fi
  if [ "$current_size" -eq "$BRIDGE_STATUS_OFFSET" ]; then
    return
  fi

  local start_byte=$((BRIDGE_STATUS_OFFSET + 1))
  while IFS= read -r line; do
    case "$line" in
      "RPM telemetry sample:"*)
        status_line "INFO" "$line"
        ;;
      "RPM telemetry active."*)
        status_line "SUCCESS" "$line"
        ;;
    esac
  done < <(tail -c +"$start_byte" "$LOG_FILE" 2>/dev/null || true)

  BRIDGE_STATUS_OFFSET=$current_size
}

: >"$STATUS_LOG_FILE"
status_line "INFO" "Launcher started (APP_ID=$APP_ID, bridge delay=${START_DELAY}s)."
status_line "INFO" "LMU launch command: $*"
status_line "INFO" "LED config: force_rpm_colors=$FORCE_RPM_COLORS_ON_BOOT force_button_colors=$FORCE_BUTTON_COLORS_ON_BOOT menu_force_button_colors=$MENU_FORCE_BUTTON_COLORS_ON_IDLE idle_led_writes=$IDLE_LED_WRITES enable_button_color_overrides=$ENABLE_BUTTON_COLOR_OVERRIDES"

if [ "$AUTO_RESET_ON_LAUNCH" = "1" ]; then
  status_line "INFO" "Running runtime reset preflight."
  if "$RESET_SCRIPT" >>"$LOG_FILE" 2>&1; then
    status_line "SUCCESS" "Runtime reset preflight completed."
  else
    status_line "ERROR" "Runtime reset preflight failed. Continuing launch; see moza-rpm-launch.log."
  fi
else
  status_line "WARN" "Runtime reset preflight disabled (MOZA_AUTO_RESET_ON_LAUNCH=$AUTO_RESET_ON_LAUNCH)."
fi

if [ "$AUTO_SETUP_ON_LAUNCH" = "1" ]; then
  status_line "INFO" "Running COM1 preflight setup."
  setup_ok=0
  attempt=1
  while [ "$attempt" -le "$SETUP_RETRY_COUNT" ]; do
    if STEAM_APP_ID="$APP_ID" "$SETUP_SCRIPT" >>"$LOG_FILE" 2>&1; then
      setup_ok=1
      status_line "SUCCESS" "COM1 preflight setup completed (attempt $attempt/$SETUP_RETRY_COUNT)."
      break
    fi

    status_line "WARN" "COM1 preflight setup attempt $attempt/$SETUP_RETRY_COUNT failed; retrying in ${SETUP_RETRY_DELAY}s."
    attempt=$((attempt + 1))
    sleep "$SETUP_RETRY_DELAY"
  done

  if [ "$setup_ok" -ne 1 ]; then
    status_line "ERROR" "COM1 preflight setup failed after $SETUP_RETRY_COUNT attempts. Continuing launch; see moza-rpm-launch.log."
  fi
else
  status_line "WARN" "COM1 preflight setup disabled (MOZA_AUTO_SETUP=$AUTO_SETUP_ON_LAUNCH)."
fi

# Start the GUI after preflight steps so reset/setup activity cannot interfere with it.
start_status_gui

"$@" &
GAME_PID=$!
status_line "INFO" "LMU process started (PID $GAME_PID)."

status_line "INFO" "Bridge launch scheduled after ${START_DELAY}s."
start_bridge_process

if ! wait_for_bridge_connection && [ "$BRIDGE_RESTART_ON_CONNECT_FAIL" = "1" ]; then
  status_line "WARN" "Restarting bridge once after connection failure."
  stop_bridge_process_group
  start_bridge_process
  if ! wait_for_bridge_connection; then
    status_line "ERROR" "Bridge did not establish telemetry connection after restart."
  fi
fi

seen_lmu_process=0
missing_count=0
reported_waiting_for_lmu=0

while kill -0 "$GAME_PID" 2>/dev/null; do
  pump_bridge_status_from_log

  if is_lmu_process_visible; then
    if [ "$seen_lmu_process" -eq 0 ]; then
      status_line "SUCCESS" "LMU Windows process detected in Proton."
    fi
    seen_lmu_process=1
    missing_count=0
  elif [ "$seen_lmu_process" -eq 1 ]; then
    missing_count=$((missing_count + 1))
    status_line "WARN" "LMU process temporarily missing (${missing_count}/2)."
    if [ "$missing_count" -ge 2 ]; then
      status_line "ERROR" "LMU process disappeared; stopping bridge."
      cleanup
      break
    fi
  elif [ "$reported_waiting_for_lmu" -eq 0 ]; then
    status_line "INFO" "Waiting for LMU Windows process to appear..."
    reported_waiting_for_lmu=1
  fi
  sleep 2
done

wait "$GAME_PID"
status_line "INFO" "LMU process exited."
