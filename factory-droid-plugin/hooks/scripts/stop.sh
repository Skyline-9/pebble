#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/stop.sh
# On Stop, run one final synchronous reindex so the next session starts from a current
# generation. Pebble never stages or commits `.pebble/knowledge/` on the user's behalf; any
# note edits remain an ordinary working-tree diff for the user to review and commit.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

if [ ! -f ".pebble/pebble.toml" ]; then
  exit 0
fi

pebble index . >/dev/null 2>&1 || true
exit 0
