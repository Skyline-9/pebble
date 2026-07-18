#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

for config in \
  "$ROOT_DIR/supply-chain/config.toml" \
  "$ROOT_DIR/research/supply-chain/config.toml"; do
  if [[ -f "$config" ]] \
    && grep -Eq '^[[:space:]]*\[\[exemptions\.' "$config"; then
    echo "cargo-vet exemptions are forbidden: $config" >&2
    exit 1
  fi
done
