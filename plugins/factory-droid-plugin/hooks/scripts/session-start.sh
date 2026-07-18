#!/usr/bin/env bash
# plugins/factory-droid-plugin/hooks/scripts/session-start.sh
# Emits JSON with additionalContext for Factory Droid to inject into the system prompt.
# Reports this repository's Pebble index health.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

if [ ! -f ".pebble/pebble.toml" ]; then
  exit 0
fi

REPO_ID="$(sed -n 's/^repository_id *= *"\(.*\)"/\1/p' .pebble/pebble.toml | head -n1)"
if [ -z "$REPO_ID" ]; then
  exit 0
fi

HEALTH="$(pebble health --repository "$REPO_ID" --json 2>/dev/null || echo "")"
if [ -z "$HEALTH" ]; then
  exit 0
fi

HEALTHY="$(echo "$HEALTH" | jq -r '.healthy')"
GEN="$(echo "$HEALTH" | jq -r '.generation // "none"')"
ISSUE="$(echo "$HEALTH" | jq -r '.issue // empty')"

if [ "$HEALTHY" = "true" ]; then
  CTX="Pebble index is healthy for $REPO_ID (generation $GEN). Use /search to retrieve cited code and notes for this repository."
else
  CTX="Pebble index for $REPO_ID is unhealthy${ISSUE:+ ($ISSUE)}. Run \`pebble index .\` or /pebble to rebuild before trusting search results."
fi

jq -n --arg ctx "$CTX" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
