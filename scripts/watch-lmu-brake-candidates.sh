#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"

source "$SCRIPT_DIR/read-secrets.sh" 2>/dev/null || {
  echo "Error: Could not load secrets.json configuration." >&2
  exit 1
}

APP_ID="${STEAM_APP_ID:-$STEAM_APP_ID}"
BINARY="$ROOT_DIR/lmu-dump.exe"
LOCAL_SOURCE_DIR="$ROOT_DIR"
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
  echo "Launch Le Mans Ultimate once in Steam before running this debug pass." >&2
  exit 2
fi

if [ -d "$LOCAL_SOURCE_DIR" ] && needs_rebuild "$LOCAL_SOURCE_DIR" "$BINARY"; then
  cd "$LOCAL_SOURCE_DIR"
  cargo build --release --target x86_64-pc-windows-gnu --bin lmu_dump
  cp target/x86_64-pc-windows-gnu/release/lmu_dump.exe "$BINARY"
elif [ -d "$FALLBACK_SOURCE_DIR" ] && needs_rebuild "$FALLBACK_SOURCE_DIR" "$BINARY"; then
  cd "$FALLBACK_SOURCE_DIR"
  cargo build --release --target x86_64-pc-windows-gnu --bin lmu_dump
  cp target/x86_64-pc-windows-gnu/release/lmu_dump.exe "$BINARY"
elif [ ! -f "$BINARY" ]; then
  if [ -d "$LOCAL_SOURCE_DIR" ]; then
    cd "$LOCAL_SOURCE_DIR"
    cargo build --release --target x86_64-pc-windows-gnu --bin lmu_dump
    cp target/x86_64-pc-windows-gnu/release/lmu_dump.exe "$BINARY"
  elif [ -d "$FALLBACK_SOURCE_DIR" ]; then
    cd "$FALLBACK_SOURCE_DIR"
    cargo build --release --target x86_64-pc-windows-gnu --bin lmu_dump
    cp target/x86_64-pc-windows-gnu/release/lmu_dump.exe "$BINARY"
  else
    echo "The compiled lmu_dump.exe was not found." >&2
    exit 3
  fi
fi

PROTON_BIN="$(resolve_proton_bin)"
if [ ! -x "$PROTON_BIN" ]; then
  echo "Could not find a Proton runtime binary to use for launching the LMU brake probe." >&2
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

echo "Running LMU live brake watcher..."
echo "Goal: press the brake only and watch for the candidate values that change."
echo "Keep the terminal visible while the race is active."
WINEPREFIX="$PREFIX" "$PROTON_BIN" "$PROTON_CMD" "$BINARY"
