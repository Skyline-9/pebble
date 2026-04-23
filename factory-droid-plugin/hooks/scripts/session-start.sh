#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/session-start.sh
# Emits JSON with additionalContext for Factory Droid to inject into the system prompt.
set -euo pipefail

export PATH="$HOME/.bun/bin:$PATH"

CACHE="$(pebble-mcp hot-cache-for-droid 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
