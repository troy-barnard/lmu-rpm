#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(dirname "${BASH_SOURCE[0]}")"

# Load configuration when available.
if source "$SCRIPT_DIR/read-secrets.sh" 2>/dev/null; then
  APP_ID="${STEAM_APP_ID:-$STEAM_APP_ID}"
else
  APP_ID="${STEAM_APP_ID:-2399420}"
fi

resolve_prefix() {
  if [ -n "${WINEPREFIX:-}" ] && [ -d "$WINEPREFIX" ]; then
    echo "$WINEPREFIX"
    return
  fi

  if [ -n "${STEAM_LIBRARY_PATHS_ARRAY+x}" ] && [ "${#STEAM_LIBRARY_PATHS_ARRAY[@]}" -gt 0 ]; then
    local steam_lib
    for steam_lib in "${STEAM_LIBRARY_PATHS_ARRAY[@]}"; do
      local candidate="$steam_lib/steamapps/compatdata/${APP_ID}/pfx"
      if [ -d "$candidate" ]; then
        echo "$candidate"
        return
      fi
    done
  fi

  local fallback="/ssd2/SteamLibrary/steamapps/compatdata/${APP_ID}/pfx"
  if [ -d "$fallback" ]; then
    echo "$fallback"
    return
  fi

  echo ""
}

resolve_wineserver() {
  if [ -n "${WINESERVER:-}" ] && [ -x "$WINESERVER" ]; then
    echo "$WINESERVER"
    return
  fi

  if [ -n "${PROTON_BIN:-}" ] && [ -x "$PROTON_BIN" ]; then
    local proton_dir
    proton_dir="$(dirname "$PROTON_BIN")"
    local c1="$proton_dir/files/bin/wineserver"
    local c2="$proton_dir/../files/bin/wineserver"
    if [ -x "$c1" ]; then
      echo "$c1"
      return
    fi
    if [ -x "$c2" ]; then
      echo "$c2"
      return
    fi
  fi

  if [ -n "${PROTON_INSTALL_PATHS_ARRAY+x}" ] && [ "${#PROTON_INSTALL_PATHS_ARRAY[@]}" -gt 0 ]; then
    local proton_bin
    for proton_bin in "${PROTON_INSTALL_PATHS_ARRAY[@]}"; do
      if [ ! -x "$proton_bin" ]; then
        continue
      fi
      local proton_dir
      proton_dir="$(dirname "$proton_bin")"
      local c1="$proton_dir/files/bin/wineserver"
      local c2="$proton_dir/../files/bin/wineserver"
      if [ -x "$c1" ]; then
        echo "$c1"
        return
      fi
      if [ -x "$c2" ]; then
        echo "$c2"
        return
      fi
    done
  fi

  if command -v wineserver >/dev/null 2>&1; then
    command -v wineserver
    return
  fi

  echo ""
}

echo "[reset-runtime] Stopping stale LMU and bridge processes for app ${APP_ID}..."

# Stop known bridge and game-side processes. Avoid broad SteamLaunch matches that can hit the current wrapper.
pkill -f "run-moza-rpm\.sh|moza-rpm\.exe|proton.*moza-rpm|Le Mans Ultimate\.exe|LeMansUltimate\.exe|start_protected_game\.exe" >/dev/null 2>&1 || true

# Kill wineserver for the LMU prefix to fully release locks.
PREFIX="$(resolve_prefix)"
WINESERVER_BIN="$(resolve_wineserver)"
if [ -n "$PREFIX" ] && [ -n "$WINESERVER_BIN" ] && [ -x "$WINESERVER_BIN" ]; then
  WINEPREFIX="$PREFIX" "$WINESERVER_BIN" -k >/dev/null 2>&1 || true
fi

echo "[reset-runtime] Done."
