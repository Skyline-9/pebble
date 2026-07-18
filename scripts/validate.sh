#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT_DIR/Cargo.toml"
VET_STORE="$ROOT_DIR/supply-chain"

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cd "$ROOT_DIR"

workspace_package_manifests() {
  local manifest="$1"
  cargo metadata \
    --manifest-path "$manifest" \
    --no-deps \
    --format-version 1 \
    --locked |
    python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
for package in sorted(metadata["packages"], key=lambda item: item["manifest_path"]):
    print(package["manifest_path"])
'
}

check_vet_pruned() {
  local manifest="$1"
  local store="$2"
  local temporary_root
  temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/pebble-vet-prune.XXXXXX")"
  cp -R "$store" "$temporary_root/supply-chain"

  if ! cargo vet prune \
    --manifest-path "$manifest" \
    --store-path "$temporary_root/supply-chain" \
    --locked \
    --no-minimize-exemptions; then
    rm -R "$temporary_root"
    return 1
  fi
  if ! diff -ru "$store" "$temporary_root/supply-chain"; then
    echo "cargo-vet evidence requires pruning: $store" >&2
    rm -R "$temporary_root"
    return 1
  fi
  rm -R "$temporary_root"
}

run_geiger() {
  local package_manifest="$1"
  local expected_warnings="$2"
  local expected_manifest="${3:-}"
  local geiger_output
  local geiger_errors
  local geiger_status
  geiger_output="$(mktemp "${TMPDIR:-/tmp}/pebble-geiger-out.XXXXXX")"
  geiger_errors="$(mktemp "${TMPDIR:-/tmp}/pebble-geiger-err.XXXXXX")"

  set +e
  cargo geiger \
    --manifest-path "$package_manifest" \
    --all-features \
    --all-targets \
    --locked >"$geiger_output" 2>"$geiger_errors"
  geiger_status=$?
  set -e
  cat "$geiger_output"
  cat "$geiger_errors" >&2

  GEIGER_STATUS="$geiger_status" \
  GEIGER_EXPECTED_WARNINGS="$expected_warnings" \
  GEIGER_EXPECTED_MANIFEST="$expected_manifest" \
  GEIGER_ERRORS="$geiger_errors" \
  ROOT_DIR="$ROOT_DIR" \
    python3 - <<'PY'
import hashlib
import os
import pathlib
import re

status = int(os.environ["GEIGER_STATUS"])
expected_count = int(os.environ["GEIGER_EXPECTED_WARNINGS"])
errors = pathlib.Path(os.environ["GEIGER_ERRORS"]).read_text(
    encoding="utf-8"
).splitlines()
warning_prefix = "WARNING: Dependency file was never scanned: "
warnings = [line for line in errors if line.startswith(warning_prefix)]
other_warnings = [
    line
    for line in errors
    if line.lower().startswith("warning:") and not line.startswith(warning_prefix)
]
aggregate = [line for line in errors if line.startswith("error:")]

if expected_count == 0:
    if status != 0 or warnings or other_warnings or aggregate:
        raise SystemExit("cargo-geiger failed without an approved warning manifest")
    raise SystemExit(0)
if status != 1:
    raise SystemExit(f"cargo-geiger returned unexpected status {status}")
if len(warnings) != expected_count:
    raise SystemExit(
        f"cargo-geiger warning count changed: {len(warnings)} != {expected_count}"
    )
if other_warnings:
    raise SystemExit(f"unexpected cargo-geiger warning class: {other_warnings}")
if aggregate != [f"error: Found {expected_count} warnings"]:
    raise SystemExit(f"unexpected cargo-geiger errors: {aggregate}")

root = pathlib.Path(os.environ["ROOT_DIR"]).resolve(strict=True)
registry = (
    pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    / "registry"
    / "src"
).resolve(strict=True)
canonical = []
generated = re.compile(
    r"(?:^|/)target/debug/build/"
    r"(crunchy|libsqlite3-sys|protobuf|ref-cast|rustversion|serde|serde_core|"
    r"thiserror|tree-sitter|typetag)-[^/]+/out/"
    r"(bindgen\.rs|lib\.rs|private\.rs|stdlib-symbols\.txt|version\.expr|version\.rs)$"
)
for line in warnings:
    path = pathlib.Path(line.removeprefix(warning_prefix)).resolve(strict=True)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if registry in path.parents:
        relative = path.relative_to(registry)
        if len(relative.parts) < 3:
            raise SystemExit(f"unexpected registry warning path: {path}")
        canonical_path = pathlib.Path(*relative.parts[1:]).as_posix()
        canonical.append((f"registry/{canonical_path}", digest))
        continue
    if root in path.parents:
        match = generated.search(path.as_posix())
        if match is None:
            raise SystemExit(f"unexpected generated warning path: {path}")
        canonical.append((f"target/{match.group(1)}/{match.group(2)}", digest))
        continue
    raise SystemExit(f"unexpected cargo-geiger path: {path}")

manifest = "\n".join(
    f"{path}\t{digest}" for path, digest in sorted(canonical)
)
observed = hashlib.sha256(manifest.encode()).hexdigest()
expected = os.environ["GEIGER_EXPECTED_MANIFEST"]
if observed != expected:
    raise SystemExit("cargo-geiger warning path or content changed")
PY
  rm "$geiger_output" "$geiger_errors"
}

cargo fmt --manifest-path "$MANIFEST" --all -- --check
cargo check --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked
cargo clippy --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked -- -D warnings
cargo test --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path "$MANIFEST" \
  --workspace --all-features --no-deps --locked
cargo size-check
cargo dependency-check
cargo audit --file "$ROOT_DIR/Cargo.lock"
cargo deny --manifest-path "$MANIFEST" check \
  --config "$ROOT_DIR/deny.toml" \
  advisories bans licenses sources
cargo vet \
  --manifest-path "$MANIFEST" \
  --store-path "$VET_STORE" \
  --locked \
  --no-minimize-exemptions
check_vet_pruned "$MANIFEST" "$VET_STORE"
bash "$ROOT_DIR/scripts/check-vet-exemptions.sh"
package_manifests="$(workspace_package_manifests "$MANIFEST")"
while IFS= read -r package_manifest; do
  case "$package_manifest" in
    */pebble-cli/Cargo.toml)
      run_geiger "$package_manifest" 69 \
        4d7cbc5757f5c02c3bf13a199f3691744f1977c4412ba48f9e60b2d41ab2f4f3
      ;;
    */pebble-core/Cargo.toml)
      run_geiger "$package_manifest" 61 \
        90e796b1c3914a143e977c1882a7fb8b36fd99b5933f0d7304ba637c6ddc47a8
      ;;
    *)
      run_geiger "$package_manifest" 0
      ;;
  esac
done <<< "$package_manifests"
git -C "$ROOT_DIR" diff --check
bash "$ROOT_DIR/scripts/validate-research.sh"
