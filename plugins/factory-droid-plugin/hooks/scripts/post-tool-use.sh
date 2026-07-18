#!/usr/bin/env bash
# plugins/factory-droid-plugin/hooks/scripts/post-tool-use.sh
# After a Create/Edit/Execute call, kick off a background incremental reindex so Pebble's
# evidence stays current for the next /search or /note call. Fire-and-forget: never blocks
# the tool result on indexing latency.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

if [ ! -f ".pebble/pebble.toml" ]; then
  exit 0
fi

nohup pebble index . >/dev/null 2>&1 &
disown || true
exit 0
