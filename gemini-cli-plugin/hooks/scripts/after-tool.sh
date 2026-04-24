#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/after-tool.sh
# Round counter — every matched tool use increments; at threshold, flag the pebble-reviewer agent.
# Gemini AfterTool hook: stdin includes tool_name, tool_input, tool_response, optional mcp_context.
# stdout with hookSpecificOutput.additionalContext gets APPENDED to the tool result for the agent.
set -euo pipefail

export PATH="$HOME/.bun/bin:$PATH"

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
COUNTER_FILE="$ROOT/.gemini-rounds"
THRESHOLD="${PEBBLE_REVIEW_EVERY:-8}"

mkdir -p "$ROOT"
[ -f "$COUNTER_FILE" ] || echo "0" > "$COUNTER_FILE"

current="$(cat "$COUNTER_FILE")"
next=$((current + 1))
echo "$next" > "$COUNTER_FILE"

if (( next % THRESHOLD != 0 )); then
  echo '{}'
  exit 0
fi

payload="$(cat || true)"
transcript="$(echo "$payload" | jq -r '.transcript_path // empty' 2>/dev/null || true)"

if [ -n "$transcript" ] && [ -f "$transcript" ]; then
  MSG="Pebble: time to review. Delegate to the pebble-reviewer sub-agent with the transcript path \`$transcript\`. The reviewer will call \`pebble-mcp review-turn --transcript $transcript\`."
else
  MSG="Pebble: time to review. Delegate to the pebble-reviewer sub-agent and pass a summary of the last ${THRESHOLD} user turns."
fi

jq -n --arg ctx "$MSG" \
  '{hookSpecificOutput: {hookEventName: "AfterTool", additionalContext: $ctx}}'
