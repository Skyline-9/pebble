#!/usr/bin/env bash
# gemini-cli-plugin/tests/smoke.sh
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

echo "==> gemini-extension.json structure"
jq -e '.name == "pebble" and .contextFileName == "GEMINI.md" and .mcpServers.pebble.command == "pebble-mcp"' \
  "$PLUGIN_DIR/gemini-extension.json" > /dev/null
echo "    ok"

echo "==> commands/*.toml all parse and have prompt+description"
for f in "$PLUGIN_DIR/commands/"*.toml; do
  grep -qE '^description *= *"' "$f"
  grep -qE '^prompt *= *"""' "$f"
done
echo "    ok"

echo "==> hooks/hooks.json has 4 events under outer 'hooks' wrapper, uses \${extensionPath}"
jq -e '.hooks.SessionStart and .hooks.AfterTool and .hooks.AfterAgent and .hooks.PreCompress' \
  "$PLUGIN_DIR/hooks/hooks.json" > /dev/null
grep -q '${extensionPath}' "$PLUGIN_DIR/hooks/hooks.json"
echo "    ok"

echo "==> SessionStart hook (emits additionalContext with pebble hot-cache)"
out="$(echo '{"hook_event_name":"SessionStart","source":"startup"}' | "$PLUGIN_DIR/hooks/scripts/session-start.sh")"
echo "$out" | jq -e '.hookSpecificOutput.hookEventName == "SessionStart"' > /dev/null
echo "$out" | jq -e '.hookSpecificOutput.additionalContext | test("pebble hot-cache"; "i")' > /dev/null
echo "    ok"

echo "==> AfterTool hook (rounds 1-2; only round 2 emits reviewer hint)"
out1="$(echo '{}' | "$PLUGIN_DIR/hooks/scripts/after-tool.sh")"
out2="$(echo '{}' | "$PLUGIN_DIR/hooks/scripts/after-tool.sh")"
if echo "$out1" | grep -q "pebble-reviewer"; then
  echo "    FAIL: round 1 should not emit reviewer hint"; exit 1
fi
echo "$out2" | jq -e '.hookSpecificOutput.additionalContext | test("pebble-reviewer"; "i")' > /dev/null
echo "    ok"

echo "==> AfterAgent hook (commit-turn as gemini-cli)"
echo '{"prompt":"hi","prompt_response":"hello","stop_hook_active":false}' | "$PLUGIN_DIR/hooks/scripts/after-agent.sh" > /dev/null
latest="$(cd "$ROOT" && git log -1 --format=%B)"
echo "$latest" | grep -qE ":memo: pebble: turn 1 \+[0-9]+ -[0-9]+"
echo "$latest" | grep -q "actor: gemini-cli"
echo "    ok"

echo "==> PreCompress hook (advisory, re-renders vault)"
echo '{"trigger":"auto"}' | "$PLUGIN_DIR/hooks/scripts/pre-compress.sh" > /dev/null
test -f "$ROOT/vault/_index.md"
echo "    ok"

echo "==> pebble-mcp verify"
pebble-mcp verify

echo "==> skills, agents, GEMINI.md present"
test -f "$PLUGIN_DIR/GEMINI.md"
test -f "$PLUGIN_DIR/skills/pebble/SKILL.md"
test -f "$PLUGIN_DIR/skills/pebble-query/SKILL.md"
test -f "$PLUGIN_DIR/skills/pebble-save/SKILL.md"
test -f "$PLUGIN_DIR/agents/pebble-reviewer.md"
echo "    ok"

echo "==> all gemini-cli-plugin wiring OK"
rm -rf "$ROOT"
