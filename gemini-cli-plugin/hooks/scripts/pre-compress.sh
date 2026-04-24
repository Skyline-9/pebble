#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/pre-compress.sh
# Fires before Gemini compresses conversation history. Advisory-only in Gemini's contract —
# we use this to re-render the pebble hot-cache so the post-compression session picks up a
# fresh snapshot when SessionStart fires next.
set -euo pipefail

export PATH="$HOME/.bun/bin:$PATH"

# Drain stdin — PreCompress passes {trigger: "auto"|"manual"}.
cat > /dev/null || true

# Materialize a fresh vault snapshot (cheap; readonly DB read + markdown write).
pebble-mcp render-vault >/dev/null 2>&1 || true

echo '{}'
