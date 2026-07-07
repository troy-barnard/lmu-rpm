#!/usr/bin/env bash
set -euo pipefail

APP_ID="${STEAM_APP_ID:-}"
if [ -z "$APP_ID" ]; then
  echo "Set STEAM_APP_ID to your Le Mans Ultimate Steam app ID first." >&2
  echo "Example: STEAM_APP_ID=1969060 ./scripts/setup-moza-rpm.sh" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEVICE=""

resolve_proton_bin() {
  if [ -n "${PROTON_BIN:-}" ]; then
    echo "$PROTON_BIN"
    return
  fi

  local candidates=(
    "/home/troy/.local/share/Steam/compatibilitytools.d/GE-Proton10-34-LMU-hid_fixes/proton"
    "/home/troy/.local/share/Steam/compatibilitytools.d/GE-Proton10-4-LMU-fixbuild/proton"
    "/home/troy/.local/share/Steam/compatibilitytools.d/GE-Proton10-34/proton"
    "/ssd2/SteamLibrary/steamapps/common/Proton 11.0/proton"
  )

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

PREFIX="/ssd2/SteamLibrary/steamapps/compatdata/${APP_ID}/pfx"
if [ ! -d "$PREFIX" ]; then
  echo "Proton prefix not found at $PREFIX" >&2
  echo "Launch Le Mans Ultimate once in Steam with Proton first, then rerun this script." >&2
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

STEAM_ROOT="/ssd2/SteamLibrary"
STEAM_COMPAT_DATA_PATH="${STEAM_ROOT}/steamapps/compatdata/${APP_ID}"
STEAM_COMPAT_CLIENT_INSTALL_PATH="${STEAM_ROOT}/steamapps"
STEAM_APP_ID="$APP_ID"
export STEAM_ROOT STEAM_COMPAT_DATA_PATH STEAM_COMPAT_CLIENT_INSTALL_PATH STEAM_APP_ID
export WINE="$WINE_BIN" WINESERVER="$WINESERVER_BIN"

WINEPREFIX="$PREFIX" "$WINE_BIN" reg add 'HKEY_LOCAL_MACHINE\\Software\\Wine\\Ports' /v COM1 /t REG_SZ /d "$DEVICE_PATH" /f >/dev/null

cat <<EOF
Moza device mapped to Proton COM1.
Prefix: $PREFIX
Device: /dev/moza-r9
Next step: run ./scripts/run-moza-rpm.sh
EOF
