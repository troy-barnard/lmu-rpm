#!/usr/bin/env bash
set -euo pipefail

APP_ID="${STEAM_APP_ID:-}"
if [ -z "$APP_ID" ]; then
  echo "Set STEAM_APP_ID to your Le Mans Ultimate Steam app ID first." >&2
  echo "Example: STEAM_APP_ID=1969060 ./scripts/run-moza-rpm.sh" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="$ROOT_DIR/moza-rpm.exe"
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

if [ ! -d "$PREFIX" ]; then
  echo "Proton prefix not found at $PREFIX" >&2
  echo "Launch Le Mans Ultimate once in Steam before running this script." >&2
  exit 2
fi

if [ ! -f "$BINARY" ]; then
  if [ -d "$ROOT_DIR/../moza-rpm" ]; then
    cd "$ROOT_DIR/../moza-rpm"
    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe "$BINARY"
  else
    echo "The compiled moza-rpm.exe was not found." >&2
    echo "Clone the moza-rpm repository next to this folder, or build it manually first." >&2
    exit 3
  fi
fi

PROTON_BIN="$(resolve_proton_bin)"
if [ ! -x "$PROTON_BIN" ]; then
  echo "Could not find a Proton runtime binary to use for launching the bridge." >&2
  echo "Set PROTON_BIN to the runtime LMU uses in Steam launch options." >&2
  exit 4
fi

if ! mapfile -t WINE_TOOLS < <(resolve_wine_tools "$PROTON_BIN"); then
  echo "Could not find matching wine/wineserver binaries for: $PROTON_BIN" >&2
  exit 5
fi
WINE_BIN="${WINE_TOOLS[0]}"
WINESERVER_BIN="${WINE_TOOLS[1]}"

PROTON_CMD="${PROTON_CMD:-run}"
if [ -n "${PROTON_BIN:-}" ] && [ "$PROTON_CMD" = run ]; then
  PROTON_CMD=runinprefix
fi

STEAM_ROOT="/ssd2/SteamLibrary"
STEAM_COMPAT_DATA_PATH="${STEAM_ROOT}/steamapps/compatdata/${APP_ID}"
STEAM_COMPAT_CLIENT_INSTALL_PATH="${STEAM_ROOT}/steamapps"
STEAM_APP_ID="$APP_ID"
export STEAM_ROOT STEAM_COMPAT_DATA_PATH STEAM_COMPAT_CLIENT_INSTALL_PATH STEAM_APP_ID
export WINE="$WINE_BIN" WINESERVER="$WINESERVER_BIN"

cd "$ROOT_DIR"
WINEPREFIX="$PREFIX" "$PROTON_BIN" "$PROTON_CMD" "$BINARY"
