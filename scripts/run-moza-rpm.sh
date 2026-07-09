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

BINARY="$ROOT_DIR/moza-rpm.exe"
LOCAL_SOURCE_DIR="$ROOT_DIR/moza-rpm-src"
FALLBACK_SOURCE_DIR="$ROOT_DIR/../moza-rpm"

needs_rebuild() {
  local source_dir="$1"
  local binary="$2"

  if [ ! -f "$binary" ]; then
    return 0
  fi

  if [ -f "$source_dir/Cargo.toml" ] && [ "$source_dir/Cargo.toml" -nt "$binary" ]; then
    return 0
  fi

  if [ -d "$source_dir/src" ] && find "$source_dir/src" -type f -newer "$binary" | grep -q .; then
    return 0
  fi

  return 1
}

# Determine prefix path from Steam library paths
PREFIX=""
for steam_lib in "${STEAM_LIBRARY_PATHS_ARRAY[@]}"; do
  candidate="$steam_lib/steamapps/compatdata/${APP_ID}/pfx"
  if [ -d "$candidate" ]; then
    PREFIX="$candidate"
    break
  fi
done

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

if [ -z "$PREFIX" ] || [ ! -d "$PREFIX" ]; then
  echo "Proton prefix not found for app $APP_ID" >&2
  echo "Launch Le Mans Ultimate once in Steam before running this script." >&2
  echo "Searched paths: ${STEAM_LIBRARY_PATHS}" >&2
  exit 2
fi

if [ -d "$LOCAL_SOURCE_DIR" ] && needs_rebuild "$LOCAL_SOURCE_DIR" "$BINARY"; then
  cd "$LOCAL_SOURCE_DIR"
  cargo build --release --target x86_64-pc-windows-gnu
  cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe "$BINARY"
elif [ -d "$FALLBACK_SOURCE_DIR" ] && needs_rebuild "$FALLBACK_SOURCE_DIR" "$BINARY"; then
  cd "$FALLBACK_SOURCE_DIR"
  cargo build --release --target x86_64-pc-windows-gnu
  cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe "$BINARY"
elif [ ! -f "$BINARY" ]; then
  if [ -d "$LOCAL_SOURCE_DIR" ]; then
    cd "$LOCAL_SOURCE_DIR"
    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe "$BINARY"
  elif [ -d "$FALLBACK_SOURCE_DIR" ]; then
    cd "$FALLBACK_SOURCE_DIR"
    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/moza-rpm.exe "$BINARY"
  else
    echo "The compiled moza-rpm.exe was not found." >&2
    echo "Expected bridge source at $LOCAL_SOURCE_DIR (preferred) or $FALLBACK_SOURCE_DIR." >&2
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

STEAM_ROOT="${STEAM_LIBRARY_PATHS_ARRAY[0]}"
STEAM_COMPAT_DATA_PATH="${STEAM_ROOT}/steamapps/compatdata/${APP_ID}"
STEAM_COMPAT_CLIENT_INSTALL_PATH="${STEAM_ROOT}/steamapps"
STEAM_APP_ID="$APP_ID"
export STEAM_ROOT STEAM_COMPAT_DATA_PATH STEAM_COMPAT_CLIENT_INSTALL_PATH STEAM_APP_ID
export WINE="$WINE_BIN" WINESERVER="$WINESERVER_BIN"

cd "$ROOT_DIR"
WINEPREFIX="$PREFIX" "$PROTON_BIN" "$PROTON_CMD" "$BINARY"
