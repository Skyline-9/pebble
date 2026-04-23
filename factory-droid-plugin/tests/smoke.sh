#!/usr/bin/env bash
# factory-droid-plugin/tests/smoke.sh
# Smoke test: init, seed, run hooks, verify vault + git + commit actor.
set -euo pipefail

ROOT="$(mktemp -d)"
export PEBBLE_ROOT="$ROOT"
export PEBBLE_REVIEW_EVERY=2
PLUGIN_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> pebble-mcp init"
pebble-mcp init

echo "==> pebble-mcp seed-test-fixture"
pebble-mcp seed-test-fixture

echo "==> SessionStart hook"
out="$("$PLUGIN_DIR/hooks/scripts/session-start.sh")"
echo "$out" | jq -e '.hookSpecificOutput.hookEventName == "SessionStart"' > /dev/null
echo "    ok"

echo "==> PostCompact hook"
out="$("$PLUGIN_DIR/hooks/scripts/post-compact.sh")"
echo "$out" | jq -e '.hookSpecificOutput.hookEventName == "PostCompact"' > /dev/null
echo "    ok"

echo "==> PostToolUse hook (rounds 1-2; only round 2 emits)"
out1="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
out2="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
test -z "$out1"
echo "$out2" | jq -e '.hookSpecificOutput.additionalContext | test("pebble-reviewer"; "i")' > /dev/null
echo "    ok"

echo "==> Stop hook (commit-turn as factory-droid)"
"$PLUGIN_DIR/hooks/scripts/stop.sh"
latest="$(cd "$ROOT" && git log -1 --format=%B)"
echo "$latest" | grep -qE ":memo: pebble: turn 1 \+[0-9]+ -[0-9]+"
echo "$latest" | grep -q "actor: factory-droid"
echo "    ok"

echo "==> pebble-mcp verify"
pebble-mcp verify

echo "==> review-turn CLI end-to-end"
transcript="$(mktemp)"
cat > "$transcript" <<'JSONL'
{"role":"user","content":"I prefer TypeScript for backend services."}
{"role":"assistant","content":"Noted."}
{"role":"user","content":"My primary language is Python actually."}
JSONL
pebble-mcp review-turn --transcript "$transcript" | grep -qE "asserted: [1-9]"
rm -f "$transcript"
echo "    ok"

echo "==> all hooks + plugin wiring OK"
rm -rf "$ROOT"
