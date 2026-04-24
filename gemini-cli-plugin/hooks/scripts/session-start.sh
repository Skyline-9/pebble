#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/session-start.sh
# Emits JSON with additionalContext so Gemini CLI injects the pebble hot-cache into session context.
# Gemini hook contract: stdin = session_id/transcript_path/cwd/hook_event_name/timestamp/source,
# stdout = {hookSpecificOutput: {additionalContext: "<text>"}}.
set -euo pipefail

# Ensure pebble-mcp resolves even when parent shell doesn't export $HOME/.bun/bin.
export PATH="$HOME/.bun/bin:$PATH"

# Drain stdin (Gemini passes JSON but session-start doesn't need to branch on it).
cat > /dev/null || true

CACHE="$(pebble-mcp hot-cache-for-gemini 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  # Emit a valid empty JSON so the CLI doesn't treat stdout as a systemMessage.
  echo '{}'
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
