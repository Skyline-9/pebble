#!/usr/bin/env bash
# factory-droid-plugin/tests/smoke.sh
# Smoke test: plugin config references the new `pebble` binary, command files are
# well-formed, and hooks no-op cleanly outside a registered Pebble repository.
set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> plugin.json references the pebble binary"
jq -e '.mcpServers.pebble.command == "pebble" and .mcpServers.pebble.args == ["serve"]' \
  "$PLUGIN_DIR/.factory-plugin/plugin.json" > /dev/null
if grep -q "pebble-mcp" "$PLUGIN_DIR/.factory-plugin/plugin.json"; then
  echo "    FAIL: plugin.json still references pebble-mcp"; exit 1
fi
echo "    ok"

echo "==> commands/*.md have valid frontmatter and no legacy tool names"
for f in "$PLUGIN_DIR/commands/"*.md; do
  head -n1 "$f" | grep -qx -- "---"
  grep -qE '^description: ' "$f"
  awk '/^---$/{c++} END{exit c==2?0:1}' "$f"
  if grep -qE 'memory_(assert|query|read_cell|retract|touch)|skill_save|profile_read' "$f"; then
    echo "    FAIL: $f still references legacy memory tools"; exit 1
  fi
done
echo "    ok ($(ls "$PLUGIN_DIR/commands/"*.md | wc -l | tr -d ' ') command files)"

echo "==> hooks/scripts no longer reference pebble-mcp"
if grep -rl "pebble-mcp" "$PLUGIN_DIR/hooks/scripts/" > /dev/null; then
  echo "    FAIL: a hook script still references pebble-mcp"; exit 1
fi
echo "    ok"

echo "==> hooks no-op cleanly outside a registered Pebble repository"
WORKDIR="$(mktemp -d)"
(
  cd "$WORKDIR"
  out="$("$PLUGIN_DIR/hooks/scripts/session-start.sh")"
  test -z "$out"
  out="$("$PLUGIN_DIR/hooks/scripts/post-compact.sh")"
  test -z "$out"
  "$PLUGIN_DIR/hooks/scripts/post-tool-use.sh"
  "$PLUGIN_DIR/hooks/scripts/stop.sh"
)
rm -rf "$WORKDIR"
echo "    ok"

echo "==> all plugin config + command wiring OK"
