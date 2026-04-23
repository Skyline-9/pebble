#!/usr/bin/env bash
# hooks/scripts/session-start.sh
# Invoked by CC on SessionStart. Prints JSON with additionalContext for the model.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-cc 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
