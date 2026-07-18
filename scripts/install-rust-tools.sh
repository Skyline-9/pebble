#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TOOLS_FILE="$ROOT_DIR/config/dev-tools.txt"
EXPECTED_TOOL_COUNT=4

install() {
  local crate="$1"
  local version="$2"
  local installed
  installed="$(cargo install --list)"
  if ! grep -Fqx "${crate} v${version}:" <<<"$installed"; then
    cargo install --locked "${crate}" --version "=${version}"
  fi
}

seen=" "
tool_count=0
while read -r crate version extra; do
  [[ -z "${crate:-}" || "$crate" == \#* ]] && continue
  if [[ -z "${version:-}" || -n "${extra:-}" ]]; then
    echo "invalid development-tool entry: $crate ${version:-} ${extra:-}" >&2
    exit 1
  fi
  case "$crate" in
    cargo-audit|cargo-deny|cargo-geiger|cargo-vet) ;;
    *)
      echo "unexpected development tool: $crate" >&2
      exit 1
      ;;
  esac
  if [[ "$seen" == *" $crate "* ]]; then
    echo "duplicate development tool: $crate" >&2
    exit 1
  fi
  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "development tool must use an exact version: $crate $version" >&2
    exit 1
  fi

  install "$crate" "$version"
  seen+="$crate "
  tool_count=$((tool_count + 1))
done <"$TOOLS_FILE"

if [[ "$tool_count" -ne "$EXPECTED_TOOL_COUNT" ]]; then
  echo "expected $EXPECTED_TOOL_COUNT development tools, found $tool_count" >&2
  exit 1
fi
