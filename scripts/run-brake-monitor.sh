#!/usr/bin/env bash
# Helper script to run brake_monitor.exe through Proton

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"

# Load configuration from secrets.json
source "$SCRIPT_DIR/read-secrets.sh" || {
  echo "Error: Could not load secrets.json configuration." >&2
  exit 1
}

# Find first valid Proton installation
PROTON_BIN=""
for candidate in "${PROTON_INSTALL_PATHS_ARRAY[@]}"; do
  if [ -x "$candidate" ]; then
    PROTON_BIN="$candidate"
    break
  fi
done

if [ -z "$PROTON_BIN" ]; then
  echo "Error: Could not find Proton binary in configured paths:" >&2
  printf '  %s\n' "${PROTON_INSTALL_PATHS_ARRAY[@]}" >&2
  exit 1
fi

echo "Using Proton: $PROTON_BIN"
echo "Running brake monitor..."
echo ""

BRAKE_MONITOR_EXE="$ROOT_DIR/moza-rpm-src/target/x86_64-pc-windows-gnu/release/brake_monitor.exe"

if [ ! -f "$BRAKE_MONITOR_EXE" ]; then
  echo "Error: brake_monitor.exe not found at $BRAKE_MONITOR_EXE" >&2
  echo "Run: cd $ROOT_DIR/moza-rpm-src && cargo build --release --bin brake_monitor" >&2
  exit 1
fi

# Determine prefix path from Steam library paths
PREFIX=""
for steam_lib in "${STEAM_LIBRARY_PATHS_ARRAY[@]}"; do
  candidate="$steam_lib/steamapps/compatdata/2399420/pfx"
  if [ -d "$candidate" ]; then
    PREFIX="$candidate"
    break
  fi
done

if [ -z "$PREFIX" ]; then
  echo "Error: Could not find Proton prefix" >&2
  exit 1
else
  echo "Using Wine prefix: $PREFIX"
  export WINEPREFIX="$PREFIX"
fi

# Resolve wine binary from Proton
PROTON_DIR="$(dirname "$PROTON_BIN")"
WINE_BIN="${PROTON_DIR}/../files/bin/wine"

if [ ! -x "$WINE_BIN" ]; then
  WINE_BIN="${PROTON_DIR}/files/bin/wine"
fi

if [ ! -x "$WINE_BIN" ]; then
  echo "Error: Could not find wine binary at $WINE_BIN" >&2
  exit 1
fi

echo "Using wine: $WINE_BIN"

# Run directly with wine
exec "$WINE_BIN" "$BRAKE_MONITOR_EXE"
