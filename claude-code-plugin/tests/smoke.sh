#!/usr/bin/env bash
# claude-code-plugin/tests/smoke.sh
# Smoke test: init, seed, run hooks, verify vault + git.
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

echo "==> PostToolUse hook (rounds 1-2; only round 2 should emit)"
out1="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
out2="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
test -z "$out1"
echo "$out2" | jq -e '.hookSpecificOutput.additionalContext | test("reviewer"; "i")' > /dev/null
echo "    ok"

echo "==> Stop hook (commit-turn)"
"$PLUGIN_DIR/hooks/scripts/stop.sh"
log="$(cd "$ROOT" && git log --oneline)"
echo "$log" | grep -qE ":memo: pebble: turn 1 \+[0-9]+ -[0-9]+"
echo "    ok (${log})"

echo "==> pebble-mcp verify"
pebble-mcp verify

echo "==> all hooks + plugin wiring OK"
rm -rf "$ROOT"
