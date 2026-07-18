#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/after-tool.sh
# After an edit-shaped tool call, kick off a background incremental reindex so Pebble's
# evidence stays current for the next /search or /note call. Fire-and-forget: never blocks
# the tool result on indexing latency.
# Gemini AfterTool hook: stdin includes tool_name, tool_input, tool_response, optional
# mcp_context. stdout with hookSpecificOutput.additionalContext gets APPENDED to the tool
# result for the agent; we return {} since there is nothing to append.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

cat > /dev/null || true

if [ -f ".pebble/pebble.toml" ]; then
  nohup pebble index . >/dev/null 2>&1 &
  disown || true
fi

echo '{}'
