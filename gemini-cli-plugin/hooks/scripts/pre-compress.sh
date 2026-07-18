#!/usr/bin/env bash
# gemini-cli-plugin/hooks/scripts/pre-compress.sh
# Fires before Gemini compresses conversation history. In the old memory-vault model this
# re-rendered a generated markdown snapshot before compaction. Pebble's knowledge notes are
# already ordinary Markdown under `.pebble/knowledge/` and `~/.pebble/v1/personal/knowledge/`,
# so there is no derived snapshot to materialize. Documented no-op, advisory-only.
set -euo pipefail

# Drain stdin — PreCompress passes {trigger: "auto"|"manual"}.
cat > /dev/null || true

echo '{}'
