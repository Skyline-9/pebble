#!/usr/bin/env bash
# hooks/scripts/post-compact.sh
# After compaction, re-inject profile + top skills + active foresight.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-cc 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "PostCompact", additionalContext: $ctx}}'
