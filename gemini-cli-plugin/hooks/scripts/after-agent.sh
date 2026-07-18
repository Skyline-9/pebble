#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/after-agent.sh
# Fires once per turn after the model responds. Equivalent to CC's Stop hook for Pebble's
# purposes: run one final synchronous reindex so the next session starts from a current
# generation. Pebble never stages or commits `.pebble/knowledge/`; any note edits remain an
# ordinary working-tree diff for the user to review and commit. Advisory only — we never
# deny/retry the model's response here.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

# Drain stdin — AfterAgent passes prompt + prompt_response + stop_hook_active.
cat > /dev/null || true

if [ -f ".pebble/pebble.toml" ]; then
  pebble index . >/dev/null 2>&1 || true
fi

# Return empty JSON — advisory only, no retry/block.
echo '{}'
