#!/usr/bin/env bash
# hooks/scripts/post-tool-use.sh
# Every tool use increments a round counter. At threshold, flag the reviewer.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
COUNTER_FILE="$ROOT/.cc-rounds"
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
  MSG="Pebble: time to review. Invoke the @pebble-reviewer subagent with the last ${THRESHOLD} user turns from \`$transcript\`."
else
  MSG="Pebble: time to review. Invoke the @pebble-reviewer subagent to harvest any preferences/projects from recent turns."
fi

jq -n --arg ctx "$MSG" \
  '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
