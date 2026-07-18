#!/usr/bin/env bash
# gemini-cli-plugin/tests/smoke.sh
# Smoke test: extension config references the new `pebble` binary, command files are
# well-formed TOML, and hooks no-op cleanly outside a registered Pebble repository.
set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> gemini-extension.json references the pebble binary"
jq -e '.name == "pebble" and .contextFileName == "GEMINI.md" and .mcpServers.pebble.command == "pebble" and .mcpServers.pebble.args == ["serve"]' \
  "$PLUGIN_DIR/gemini-extension.json" > /dev/null
if grep -q "pebble-mcp" "$PLUGIN_DIR/gemini-extension.json"; then
  echo "    FAIL: gemini-extension.json still references pebble-mcp"; exit 1
fi
echo "    ok"

echo "==> commands/*.toml all parse and have prompt+description, no legacy tool names"
for f in "$PLUGIN_DIR/commands/"*.toml; do
  grep -qE '^description *= *"' "$f"
  grep -qE '^prompt *= *"""' "$f"
  if grep -qE 'memory_(assert|query|read_cell|retract|touch)|skill_save|profile_read' "$f"; then
    echo "    FAIL: $f still references legacy memory tools"; exit 1
  fi
done
echo "    ok ($(ls "$PLUGIN_DIR/commands/"*.toml | wc -l | tr -d ' ') command files)"

echo "==> hooks/hooks.json has 4 events under outer 'hooks' wrapper, uses \${extensionPath}"
jq -e '.hooks.SessionStart and .hooks.AfterTool and .hooks.AfterAgent and .hooks.PreCompress' \
  "$PLUGIN_DIR/hooks/hooks.json" > /dev/null
grep -q '${extensionPath}' "$PLUGIN_DIR/hooks/hooks.json"
echo "    ok"

echo "==> hooks/scripts no longer reference pebble-mcp"
if grep -rl "pebble-mcp" "$PLUGIN_DIR/hooks/scripts/" > /dev/null; then
  echo "    FAIL: a hook script still references pebble-mcp"; exit 1
fi
echo "    ok"

echo "==> hooks no-op cleanly outside a registered Pebble repository"
WORKDIR="$(mktemp -d)"
(
  cd "$WORKDIR"
  out="$(echo '{"hook_event_name":"SessionStart","source":"startup"}' | "$PLUGIN_DIR/hooks/scripts/session-start.sh")"
  echo "$out" | jq -e '. == {}' > /dev/null
  out="$(echo '{}' | "$PLUGIN_DIR/hooks/scripts/after-tool.sh")"
  echo "$out" | jq -e '. == {}' > /dev/null
  out="$(echo '{"prompt":"hi","prompt_response":"hello","stop_hook_active":false}' | "$PLUGIN_DIR/hooks/scripts/after-agent.sh")"
  echo "$out" | jq -e '. == {}' > /dev/null
  out="$(echo '{"trigger":"auto"}' | "$PLUGIN_DIR/hooks/scripts/pre-compress.sh")"
  echo "$out" | jq -e '. == {}' > /dev/null
)
rm -rf "$WORKDIR"
echo "    ok"

echo "==> all extension config + command wiring OK"
