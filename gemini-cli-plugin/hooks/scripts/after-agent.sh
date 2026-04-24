#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/after-agent.sh
# Fires once per turn after the model responds. Equivalent to CC's Stop hook for Pebble's purposes:
# compute turn + adds/retracts delta and call pebble-mcp commit-turn, then reset round counter.
# We never deny/retry here — this is advisory commit logging, not response validation.
set -euo pipefail

export PATH="$HOME/.bun/bin:$PATH"

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
TURN_FILE="$ROOT/.gemini-turn"
LAST_EVT_FILE="$ROOT/.gemini-last-event-count"

mkdir -p "$ROOT"

# Drain stdin — AfterAgent passes prompt + prompt_response + stop_hook_active.
cat > /dev/null || true

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

pebble-mcp commit-turn --turn "$turn" --adds "$adds" --retracts "$retracts" --actor gemini-cli || true

echo "0" > "$ROOT/.gemini-rounds"

# Return empty JSON — advisory only, no retry/block.
echo '{}'
