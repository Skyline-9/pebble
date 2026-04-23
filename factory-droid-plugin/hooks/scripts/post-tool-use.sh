#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/post-tool-use.sh
# Every matched tool use increments the round counter. At threshold, flag the pebble-reviewer.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
COUNTER_FILE="$ROOT/.droid-rounds"
THRESHOLD="${PEBBLE_REVIEW_EVERY:-8}"

mkdir -p "$ROOT"
[ -f "$COUNTER_FILE" ] || echo "0" > "$COUNTER_FILE"

current="$(cat "$COUNTER_FILE")"
next=$((current + 1))
echo "$next" > "$COUNTER_FILE"

if (( next % THRESHOLD != 0 )); then
  exit 0
fi

payload="$(cat || true)"
transcript="$(echo "$payload" | jq -r '.transcript_path // empty' 2>/dev/null || true)"

if [ -n "$transcript" ] && [ -f "$transcript" ]; then
  MSG="Pebble: time to review. Invoke the pebble-reviewer subagent via the Task tool (subagent_type=pebble-reviewer) with the transcript path \`$transcript\`. The reviewer will call \`pebble-mcp review-turn --transcript $transcript\`."
else
  MSG="Pebble: time to review. Invoke the pebble-reviewer subagent via the Task tool (subagent_type=pebble-reviewer) and pass a summary of the last ${THRESHOLD} user turns."
fi

jq -n --arg ctx "$MSG" \
  '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
