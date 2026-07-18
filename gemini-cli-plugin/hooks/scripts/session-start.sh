#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/session-start.sh
# Emits JSON with additionalContext so Gemini CLI injects Pebble's index health into session
# context. Gemini hook contract: stdin = session_id/transcript_path/cwd/hook_event_name/
# timestamp/source, stdout = {hookSpecificOutput: {additionalContext: "<text>"}}.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

# Drain stdin (Gemini passes JSON but session-start doesn't need to branch on it).
cat > /dev/null || true

if [ ! -f ".pebble/pebble.toml" ]; then
  # Emit a valid empty JSON so the CLI doesn't treat stdout as a systemMessage.
  echo '{}'
  exit 0
fi

REPO_ID="$(sed -n 's/^repository_id *= *"\(.*\)"/\1/p' .pebble/pebble.toml | head -n1)"
if [ -z "$REPO_ID" ]; then
  echo '{}'
  exit 0
fi

HEALTH="$(pebble health --repository "$REPO_ID" --json 2>/dev/null || echo "")"
if [ -z "$HEALTH" ]; then
  echo '{}'
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
