#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"

# Load configuration from secrets.json
source "$SCRIPT_DIR/read-secrets.sh" 2>/dev/null || {
  echo "Error: Could not load secrets.json configuration." >&2
  exit 1
}

# Allow override via environment variable
APP_ID="${STEAM_APP_ID:-$STEAM_APP_ID}"

DEVICE=""

resolve_proton_bin() {
  if [ -n "${PROTON_BIN:-}" ]; then
    echo "$PROTON_BIN"
    return
  fi

  local candidates=("${PROTON_INSTALL_PATHS_ARRAY[@]}")

  local candidate
  for candidate in "${candidates[@]}"; do
    if [ -x "$candidate" ]; then
      echo "$candidate"
      return
    fi
  done

  echo ""
}

resolve_wine_tools() {
  local proton_bin="$1"
  local proton_dir
  proton_dir="$(dirname "$proton_bin")"

  local wine_bin="${proton_dir}/../files/bin/wine"
  local wineserver_bin="${proton_dir}/../files/bin/wineserver"

  if [ ! -x "$wine_bin" ] || [ ! -x "$wineserver_bin" ]; then
    wine_bin="${proton_dir}/files/bin/wine"
    wineserver_bin="${proton_dir}/files/bin/wineserver"
  fi

  if [ ! -x "$wine_bin" ] || [ ! -x "$wineserver_bin" ]; then
    return 1
  fi

  printf '%s\n%s\n' "$wine_bin" "$wineserver_bin"
}
for candidate in /dev/serial/by-id/usb-Gudsen_MOZA_R9_Base_* /dev/ttyACM*; do
  if [ -e "$candidate" ]; then
    DEVICE="$(readlink -f "$candidate")"
    break
  fi
done

if [ -z "$DEVICE" ]; then
  echo "No Moza serial device was detected. Connect the wheel base and try again." >&2
  exit 2
fi

echo "Using device: $DEVICE"

if ln -sfn "$DEVICE" /dev/moza-r9 2>/dev/null; then
  echo "Created /dev/moza-r9 symlink."
else
  echo "Could not create /dev/moza-r9 without elevated privileges; using $DEVICE directly."
fi

DEVICE_PATH="/dev/moza-r9"
if [ ! -e "$DEVICE_PATH" ]; then
  DEVICE_PATH="$DEVICE"
fi

# Determine prefix path from Steam library paths
PREFIX=""
for steam_lib in "${STEAM_LIBRARY_PATHS_ARRAY[@]}"; do
  candidate="$steam_lib/steamapps/compatdata/${APP_ID}/pfx"
  if [ -d "$candidate" ]; then
    PREFIX="$candidate"
    break
  fi
done
if [ -z "$PREFIX" ] || [ ! -d "$PREFIX" ]; then
  echo "Proton prefix not found for app $APP_ID" >&2
  echo "Launch Le Mans Ultimate once in Steam with Proton first, then rerun this script." >&2
  echo "Searched paths: ${STEAM_LIBRARY_PATHS}" >&2
  exit 3
fi

PROTON_BIN="$(resolve_proton_bin)"
if [ ! -x "$PROTON_BIN" ]; then
  echo "Could not find a Proton runtime binary to use for Wine registry changes." >&2
  echo "Set PROTON_BIN to the runtime LMU uses in Steam launch options." >&2
  exit 4
fi

if ! mapfile -t WINE_TOOLS < <(resolve_wine_tools "$PROTON_BIN"); then
  echo "Could not find matching wine/wineserver binaries for: $PROTON_BIN" >&2
  exit 5
fi
WINE_BIN="${WINE_TOOLS[0]}"
WINESERVER_BIN="${WINE_TOOLS[1]}"

STEAM_ROOT="${STEAM_LIBRARY_PATHS_ARRAY[0]}"
STEAM_COMPAT_DATA_PATH="${STEAM_ROOT}/steamapps/compatdata/${APP_ID}"
STEAM_COMPAT_CLIENT_INSTALL_PATH="${STEAM_ROOT}/steamapps"
STEAM_APP_ID="$APP_ID"
export STEAM_ROOT STEAM_COMPAT_DATA_PATH STEAM_COMPAT_CLIENT_INSTALL_PATH STEAM_APP_ID
export WINE="$WINE_BIN" WINESERVER="$WINESERVER_BIN"

WINEPREFIX="$PREFIX" "$WINE_BIN" reg add 'HKEY_LOCAL_MACHINE\Software\Wine\Ports' /v COM1 /t REG_SZ /d "$DEVICE_PATH" /f >/dev/null

cat <<EOF
Moza device mapped to Proton COM1.
Prefix: $PREFIX
Device: /dev/moza-r9
Next step: run ./scripts/run-moza-rpm.sh
EOF
