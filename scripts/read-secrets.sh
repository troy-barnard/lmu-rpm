#!/usr/bin/env bash
# Helper script to read configuration from secrets.json
# Usage: source scripts/read-secrets.sh
# Then access: STEAM_APP_ID, STEAM_LIBRARY_PATHS, PROTON_INSTALL_PATHS, etc.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
SECRETS_FILE="$ROOT_DIR/secrets.json"

if [ ! -f "$SECRETS_FILE" ]; then
  echo "Error: secrets.json not found at $SECRETS_FILE" >&2
  echo "Please copy example.secrets.json to secrets.json and customize it." >&2
  exit 1
fi

# Check if jq is available
if ! command -v jq &> /dev/null; then
  echo "Error: jq is required to parse secrets.json" >&2
  exit 1
fi

# Export configuration as environment variables
export STEAM_APP_ID=$(jq -r '.steam.app_id' "$SECRETS_FILE")
export STEAM_LIBRARY_PATHS=$(jq -r '.steam.library_paths[]' "$SECRETS_FILE")
export PROTON_INSTALL_PATHS=$(jq -r '.proton.install_paths[]' "$SECRETS_FILE")
export WHEEL_SERIAL_DEVICE=$(jq -r '.wheel.serial_device' "$SECRETS_FILE")
export BRIDGE_START_DELAY=$(jq -r '.bridge.start_delay_seconds' "$SECRETS_FILE")

# Convert array paths into an indexed array for easier access
PROTON_INSTALL_PATHS_ARRAY=()
while IFS= read -r line; do
  PROTON_INSTALL_PATHS_ARRAY+=("$line")
done < <(jq -r '.proton.install_paths[]' "$SECRETS_FILE")

STEAM_LIBRARY_PATHS_ARRAY=()
while IFS= read -r line; do
  STEAM_LIBRARY_PATHS_ARRAY+=("$line")
done < <(jq -r '.steam.library_paths[]' "$SECRETS_FILE")
