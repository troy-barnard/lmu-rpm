#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/launch-lmu-with-rpm.sh"

if ! declare -F wait_for_lmu_process_visible >/dev/null 2>&1; then
  echo "missing wait_for_lmu_process_visible helper" >&2
  exit 1
fi

echo "launcher script sources cleanly"
