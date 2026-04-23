#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/post-compact.sh
# After compaction, re-inject profile + top skills + active foresight.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-droid 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "PostCompact", additionalContext: $ctx}}'
