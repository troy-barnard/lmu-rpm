#!/usr/bin/env bash
set -euo pipefail

APP_ID="${STEAM_APP_ID:-2399420}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="/ssd2/SteamLibrary/steamapps/compatdata/${APP_ID}/pfx"

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

echo "Checking LMU bridge environment"

echo "- App ID: $APP_ID"
if [ ! -d "$PREFIX" ]; then
  echo "ERROR: Proton prefix missing: $PREFIX" >&2
  exit 2
fi

echo "- Prefix: OK ($PREFIX)"

PROTON_BIN="$(resolve_proton_bin)"
if [ ! -x "$PROTON_BIN" ]; then
  echo "ERROR: Proton runtime not found. Set PROTON_BIN." >&2
  exit 3
fi

echo "- Proton: OK ($PROTON_BIN)"

if ! mapfile -t WINE_TOOLS < <(resolve_wine_tools "$PROTON_BIN"); then
  echo "ERROR: matching wine/wineserver not found for runtime" >&2
  exit 4
fi

WINE_BIN="${WINE_TOOLS[0]}"
WINESERVER_BIN="${WINE_TOOLS[1]}"

echo "- Wine: OK ($WINE_BIN)"
echo "- Wineserver: OK ($WINESERVER_BIN)"

if [ -e /dev/moza-r9 ]; then
  echo "- Device: OK (/dev/moza-r9 -> $(readlink -f /dev/moza-r9 || true))"
else
  echo "- Device: /dev/moza-r9 missing; setup script should map current ttyACM device"
fi

STEAM_ROOT="/ssd2/SteamLibrary"
export STEAM_ROOT
export STEAM_COMPAT_DATA_PATH="${STEAM_ROOT}/steamapps/compatdata/${APP_ID}"
export STEAM_COMPAT_CLIENT_INSTALL_PATH="${STEAM_ROOT}/steamapps"
export STEAM_APP_ID="$APP_ID"
export WINE="$WINE_BIN"
export WINESERVER="$WINESERVER_BIN"

if WINEPREFIX="$PREFIX" "$WINE_BIN" reg query 'HKEY_LOCAL_MACHINE\\Software\\Wine\\Ports' /v COM1 >/tmp/moza-com1-check.txt 2>/dev/null; then
  echo "- COM1 mapping:"
  cat /tmp/moza-com1-check.txt
else
  echo "- COM1 mapping: missing (run setup-moza-rpm.sh)"
fi

echo "Check complete"
