#!/usr/bin/env bash
# hooks/scripts/stop.sh
# On Stop, compute turn number + adds/retracts delta, then commit.
set -euo pipefail

export PATH="$HOME/.bun/bin:$PATH"

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
TURN_FILE="$ROOT/.cc-turn"
LAST_EVT_FILE="$ROOT/.cc-last-event-count"

mkdir -p "$ROOT"

[ -f "$TURN_FILE" ] || echo "0" > "$TURN_FILE"
turn=$(( $(cat "$TURN_FILE") + 1 ))
echo "$turn" > "$TURN_FILE"

current_events=0
if [ -f "$ROOT/log.jsonl" ]; then
  current_events="$(wc -l < "$ROOT/log.jsonl" | tr -d ' ')"
fi
[ -f "$LAST_EVT_FILE" ] || echo "0" > "$LAST_EVT_FILE"
previous_events="$(cat "$LAST_EVT_FILE")"
echo "$current_events" > "$LAST_EVT_FILE"

adds=0
retracts=0
if (( current_events > previous_events )) && [ -f "$ROOT/log.jsonl" ]; then
  delta=$(( current_events - previous_events ))
  delta_events="$(tail -n "$delta" "$ROOT/log.jsonl" 2>/dev/null || true)"
  if [ -n "$delta_events" ]; then
    adds="$(echo "$delta_events" | jq -s '[.[] | select(.ev == "assert")] | length')"
    retracts="$(echo "$delta_events" | jq -s '[.[] | select(.ev == "retract")] | length')"
  fi
fi

pebble-mcp commit-turn --turn "$turn" --adds "$adds" --retracts "$retracts" --actor claude-code || true

echo "0" > "$ROOT/.cc-rounds"
