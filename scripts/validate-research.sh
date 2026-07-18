#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT_DIR/research/Cargo.toml"
VET_STORE="$ROOT_DIR/research/supply-chain"

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cd "$ROOT_DIR"

workspace_package_manifests() {
  cargo metadata \
    --manifest-path "$MANIFEST" \
    --no-deps \
    --format-version 1 \
    --locked |
    python3 -c '
import hashlib
import json
import os
import sys

metadata = json.load(sys.stdin)
for package in sorted(metadata["packages"], key=lambda item: item["manifest_path"]):
    print(package["manifest_path"])
'
}

check_vet_pruned() {
  local temporary_root
  temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/pebble-vet-prune.XXXXXX")"
  cp -R "$VET_STORE" "$temporary_root/supply-chain"

  if ! cargo vet prune \
    --manifest-path "$MANIFEST" \
    --store-path "$temporary_root/supply-chain" \
    --locked \
    --no-minimize-exemptions; then
    rm -R "$temporary_root"
    return 1
  fi
  if ! diff -ru "$VET_STORE" "$temporary_root/supply-chain"; then
    echo "cargo-vet evidence requires pruning: $VET_STORE" >&2
    rm -R "$temporary_root"
    return 1
  fi
  rm -R "$temporary_root"
}

validate_geiger_stderr() {
  python3 - "$1" "$2" "$MANIFEST" "$3" <<'PY'
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib

path = pathlib.Path(sys.argv[1])
package_manifest = pathlib.Path(sys.argv[2]).resolve(strict=True)
workspace_manifest = pathlib.Path(sys.argv[3]).resolve(strict=True)
geiger_output = pathlib.Path(sys.argv[4]).read_text(encoding="utf-8")
metadata = json.loads(
    subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(workspace_manifest),
            "--format-version",
            "1",
            "--locked",
        ],
        text=True,
    )
)
packages = {package["id"]: package for package in metadata["packages"]}
roots = [
    package["id"]
    for package in metadata["packages"]
    if pathlib.Path(package["manifest_path"]).resolve(strict=True) == package_manifest
]
if len(roots) != 1:
    raise SystemExit(f"cannot identify cargo-geiger package: {package_manifest}")
root_name = packages[roots[0]]["name"]
activated_tree = subprocess.check_output(
    [
        "cargo",
        "tree",
        "--manifest-path",
        str(workspace_manifest),
        "--package",
        root_name,
        "--all-features",
        "--edges",
        "normal,build,dev",
        "--prefix",
        "none",
        "--format",
        "{p}",
        "--locked",
    ],
    text=True,
)
activated_packages = {
    match.groups()
    for line in activated_tree.splitlines()
    if (match := re.match(r"^(\S+) v(\S+)", line)) is not None
}
lockfile = tomllib.loads(
    workspace_manifest.with_name("Cargo.lock").read_text(encoding="utf-8")
)
locked_packages = {
    (package["name"], package["version"])
    for package in lockfile["package"]
    if str(package.get("source", "")).startswith("registry+")
}
registry_source = (
    pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    / "registry"
    / "src"
).resolve(strict=True)
expected_parse_failures = {
    ("bon-macros-3.9.3", "tests/snapshots/bon_incomplete_if.rs"):
        "f6bf01d59b4345c0322a8f7dffb12410590da682560aca911c5fc1b6116dd3f6",
    ("cxx-1.0.197", "tests/ui/ptr_no_const_mut.rs"):
        "402cb4e26413953386975da3a3a3b7d4e35987a5297ec41713d39ae9ecdc2a0a",
    ("cxx-1.0.197", "tests/ui/root_namespace.rs"):
        "32fbf454c246d6ac32c92d6735ea1ed62ad92dea5066b81e670cf7e49d8ebbeb",
    ("erased-serde-0.4.10", "src/features_check/error.rs"):
        "1f2b5d89f9d59ad84a9b77566f9446d36ce430e45eaffdcc30701e8e4de289b9",
}
observed_parse_failures = set()
allowed_unscanned_active = {
    "storage-search-spike": {("cc", "1.2.67")},
}.get(root_name, set())
scanned_packages = {
    package
    for package in locked_packages
    if re.search(
        rf"(?m)\s{re.escape(package[0])} {re.escape(package[1])}$",
        geiger_output,
    )
}
progress = (
    "Blocking ",
    "Checking ",
    "Compiling ",
    "Finished ",
    "Removed ",
    "Scanning done",
)
for line in path.read_text(encoding="utf-8").splitlines():
    stripped = line.lstrip()
    if (
        not stripped
        or stripped.startswith(progress)
        or line.startswith("WARNING: Dependency file was never scanned: ")
        or line.startswith("error: Found ")
    ):
        continue
    if line.startswith("Failed to parse file: "):
        path_text, separator, _error = line.removeprefix(
            "Failed to parse file: "
        ).partition(", Syn(Error(")
        if not separator:
            raise SystemExit(f"malformed cargo-geiger parse failure: {line}")
        path = pathlib.Path(path_text).resolve(strict=True)
        if registry_source not in path.parents:
            raise SystemExit(f"unexpected cargo-geiger parse failure: {line}")
        relative = path.relative_to(registry_source)
        key = (relative.parts[1], pathlib.Path(*relative.parts[2:]).as_posix())
        if (
            key not in expected_parse_failures
            or key in observed_parse_failures
            or hashlib.sha256(path.read_bytes()).hexdigest()
            != expected_parse_failures[key]
        ):
            raise SystemExit(f"unreviewed cargo-geiger parse failure: {line}")
        observed_parse_failures.add(key)
        continue
    unmatched = re.fullmatch(
        r"Failed to match \(ignoring source\) package: "
        r"registry\+https://github\.com/rust-lang/crates\.io-index"
        r"#([A-Za-z0-9_-]+)@([^ ]+) *",
        line,
    )
    if unmatched is not None:
        package = unmatched.groups()
        if package not in locked_packages:
            raise SystemExit(f"unsafe cargo-geiger package mismatch: {line}")
        continue
    if line.startswith('{"$message_type":"artifact"'):
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise SystemExit(f"malformed cargo-geiger artifact: {error}") from error
        if message.get("$message_type") == "artifact":
            continue
    raise SystemExit(f"unexpected cargo-geiger stderr: {line}")
if observed_parse_failures != set(expected_parse_failures):
    raise SystemExit(
        f"unexpected cargo-geiger parse failures: "
        f"{sorted(observed_parse_failures)}"
    )
PY
}

run_geiger() {
  local package_manifest="$1"
  local target_directory
  local geiger_output
  local geiger_errors
  local geiger_status
  target_directory="$(mktemp -d "${TMPDIR:-/tmp}/pebble-geiger-target.XXXXXX")"
  geiger_output="$(mktemp "${TMPDIR:-/tmp}/pebble-geiger-output.XXXXXX")"
  geiger_errors="$(mktemp "${TMPDIR:-/tmp}/pebble-geiger.XXXXXX")"

  if CARGO_TARGET_DIR="$target_directory" cargo geiger \
    --manifest-path "$package_manifest" \
    --all-features \
    --all-targets \
    --locked \
    --color never \
    >"$geiger_output" \
    2>"$geiger_errors"; then
    geiger_status=0
  else
    geiger_status=$?
  fi
  if grep -Eiq '^(warning|error):' "$geiger_output"; then
    cat "$geiger_output" >&2
    cat "$geiger_errors" >&2
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    echo "cargo-geiger emitted diagnostics on stdout" >&2
    return 1
  fi
  cat "$geiger_output"
  cat "$geiger_errors" >&2
  if ! validate_geiger_stderr \
    "$geiger_errors" "$package_manifest" "$geiger_output"; then
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return 1
  fi
  if (( geiger_status == 0 )); then
    if grep -Eiq '^(warning|error):' "$geiger_errors"; then
      rm "$geiger_output" "$geiger_errors"
      rm -R "$target_directory"
      echo "cargo-geiger succeeded with unexpected diagnostics" >&2
      return 1
    fi
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return 0
  fi
  if (( geiger_status != 1 )); then
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return "$geiger_status"
  fi

  if ! python3 - "$geiger_errors" "$target_directory" "$package_manifest" <<'PY'
import pathlib
import hashlib
import os
import re
import sys

error_path = pathlib.Path(sys.argv[1])
target_directory = pathlib.Path(sys.argv[2]).resolve(strict=True)
package = pathlib.Path(sys.argv[3]).parent.name
root_package = package
registry_source = (
    pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    / "registry"
    / "src"
).resolve(strict=True)
lines = error_path.read_text(encoding="utf-8").splitlines()
prefix = "WARNING: Dependency file was never scanned: "
warnings = [line for line in lines if line.lower().startswith("warning:")]
warning_paths = [
    pathlib.Path(line.removeprefix(prefix)).resolve(strict=True)
    for line in lines
    if line.startswith(prefix)
]
errors = [line for line in lines if line.startswith("error:")]
if len(warnings) != len(warning_paths):
    raise SystemExit(f"unexpected cargo-geiger warnings: {warnings}")

expected_generated = {
    "ref-cast": """#[doc(hidden)]
pub mod __private25 {
    #[doc(hidden)]
    pub use crate::private::*;
}
""",
    "serde": """#[doc(hidden)]
pub mod __private228 {
    #[doc(hidden)]
    pub use crate::private::*;
}
use serde_core::__private228 as serde_core_private;
""",
    "serde_core": """#[doc(hidden)]
pub mod __private228 {
    #[doc(hidden)]
    pub use crate::private::*;
}
""",
    "thiserror": """#[doc(hidden)]
pub mod __private18 {
    #[doc(hidden)]
    pub use crate::private::*;
}
""",
    "typetag": """#[doc(hidden)]
pub mod __private22 {
    #[doc(hidden)]
    pub use crate::private::*;
}
""",
}
expected_generated_hashes = {
    ("crunchy", "lib.rs"):
        "8a34c08e38e337c79f1ed30061082b2c9d78f07ec4d9a7b1129b3d7ed5a0a9cb",
    ("libsqlite3-sys", "bindgen.rs"):
        "5b5a37a74dc728fb0bcd041867eb577c03c95e2fe46285262c589a3b1cd4bbac",
    ("protobuf", "version.rs"):
        "cd75b44de5972630354402e9f2537729c86ba37a9ac0fc56d998bb3b3e841b16",
    ("rustversion", "version.expr"):
        "abee5452b2ddf17b998eaa82bb92b227c9d3e28f3ad2ab1642b26b3616a1d388",
    ("tree-sitter", "stdlib-symbols.txt"):
        "a9396e1f1a0369090ff8a548b38fb7dcd4472d7318f75c937d0e007cc511dc16",
}
expected_documents = {
    ("bon-3.9.3", "README.md"):
        "ef314f06acee5216ff92113ce46f2e5bceb0d2f93c55560d96d021f0e8269c8d",
    ("bon-macros-3.9.3", "README.md"):
        "1c3eb2e154d9e3adc313a1ef133b86d71ae1bbac186b3bfefc68c160a5b2f74f",
    ("clap-4.6.1", "examples/demo.md"):
        "6c00568794ff99007ab0f08bf7e06fe521cb1f2f0fab30c219f16c6f54bf638d",
    ("clap_builder-4.6.0", "README.md"):
        "f42fbb1b8c923e94e17446f00e820da68ff3932d008610298e91ab68558326e7",
    ("pastey-0.2.3", "README.md"):
        "0255b6ac2213c61e86ebe182d4876b45094992612dac146c75b06838a37f54ef",
    ("rmcp-1.8.0", "README.md"):
        "bacf698b6278204e8f8f5dc0f8be13a9ea321ef925f430ee75c65d12926d570c",
    ("schemars-1.2.1", "README.md"):
        "190f330e6f50c71acd60502d5fb37643d9720e878677c24d1b5d2cb6c7412efd",
    ("schemars_derive-1.2.1", "attributes.md"):
        "dbb7a074c517235f2afc9b95993f3983a297e48e78443cb8e68eb558e498eb01",
    ("schemars_derive-1.2.1", "deriving.md"):
        "258b4d418949107901eaca35c9dc071c4537f027b5dfc34c63d660b1ddd33277",
    ("getrandom-0.4.3", "README.md"):
        "700180aadb3c22383920a2c9dd2fa7fedaf2e5992a0924a061ae09aae19cc69e",
}
expected_source_files = {
    ("cc-1.2.67", "src/detect_compiler_family.c"):
        "97ca4b021495611e828becea6187add37414186a16dfedd26c2947cbce6e8b2f",
}
expected_documents_git_watch_registry_files = {
    ("cc-1.2.67", "src/detect_compiler_family.c"):
        "97ca4b021495611e828becea6187add37414186a16dfedd26c2947cbce6e8b2f",
    ("cpufeatures-0.3.0", "README.md"):
        "1dd921163014e927e76c62fb21951e4d280c6dd5f5ed3a8b8eabfec8eb1ce48d",
    ("getrandom-0.3.4", "README.md"):
        "39a9371cee9b72f7aff7fcbe0018e1a90c1a0692266cacdcaac25166080b9cb0",
}
expected_ingestion_registry_files = {
    ("cc-1.2.67", "src/detect_compiler_family.c"):
        "97ca4b021495611e828becea6187add37414186a16dfedd26c2947cbce6e8b2f",
    ("tree-sitter-0.26.8", "binding_rust/README.md"):
        "0b7123387b8b32759c4f35cceec9ac0cbb0a9d0ab9146d1e62dc1f0ef671f557",
    ("tree-sitter-0.26.8", "src/parser.h"):
        "180b893c8734778fd32f372dfbc27bd6ad1cd2221f26150b31256ff6716320d2",
    ("tree-sitter-c-0.24.2", "queries/highlights.scm"):
        "3378d854dda695b2b282b9468247524ed4271ef74af691033d2e36883379409c",
    ("tree-sitter-c-0.24.2", "queries/tags.scm"):
        "774ca67cfe23b0e0d1d3a4f01788049a822beb1311feee72b20a595a37be1e28",
    ("tree-sitter-c-0.24.2", "src/node-types.json"):
        "23e819ef1eefd357bb6eba844f47f8492f6a88da3e5046b06ec4acd4da8a9fd6",
    ("tree-sitter-c-sharp-0.23.5", "queries/highlights.scm"):
        "ab8a9930aeeee70fa2dbfde82e4763170b7e826bc642338ad0683772c20c060f",
    ("tree-sitter-c-sharp-0.23.5", "queries/tags.scm"):
        "4ed08da0162ecd48206ac34bebe7ea9757a8c7b617f6ad8f70c168d685d514fe",
    ("tree-sitter-c-sharp-0.23.5", "src/node-types.json"):
        "91d69f7e0d97c002e738a09caadd8ddfa36b1552f7ae94303cc53b03b28551c0",
    ("tree-sitter-cpp-0.23.4", "queries/highlights.scm"):
        "52136576a9a9dacd9e95a8de0f351689bf46140738572ab4e9f24c9278e6b458",
    ("tree-sitter-cpp-0.23.4", "queries/tags.scm"):
        "029731ca946e32f919491d9f76c85a50e5deb5ad1934e37fd6849312c0d4a705",
    ("tree-sitter-cpp-0.23.4", "src/node-types.json"):
        "fdfd4b1f3dca1516616a1eb615bb6c1ad3082b8937ad21070d0def5dcfe7e535",
    ("tree-sitter-go-0.25.0", "queries/highlights.scm"):
        "81182c986547eba7fa6316e82dfd621fb13b8fc89efac85432aee51a48ed0896",
    ("tree-sitter-go-0.25.0", "queries/tags.scm"):
        "d1a9b1f678fe0278b85054e2dc56a28ef26aa478b8c88fb2b0dd83cdcdb9db35",
    ("tree-sitter-go-0.25.0", "src/node-types.json"):
        "8d77e723df0f0dfccb66d4571a5c3c17ecfb6c90959044c0992146b21f383bff",
    ("tree-sitter-java-0.23.5", "queries/highlights.scm"):
        "576c0df8df0b116cd642140ddc508c01f9d3283582afd8581c1f35caf4d71386",
    ("tree-sitter-java-0.23.5", "queries/tags.scm"):
        "bcb22147b8582d92743fc973864cefb894a4c12b3957f16f3d472b2ec7cd4c49",
    ("tree-sitter-java-0.23.5", "src/node-types.json"):
        "19c46facc653381c337ff6cad75dd8b052524179a366c80825d6d0010520eef2",
    ("tree-sitter-javascript-0.25.0", "queries/highlights-jsx.scm"):
        "0cad1712acaf40804bc5dc7c8476858752c1f549cdf7d8c63c747317be0f8fa6",
    ("tree-sitter-javascript-0.25.0", "queries/highlights.scm"):
        "d3630ae6dc9b2b27b230b5f8bb92b05cd491fb12bff353dae62a0a6d780461ee",
    ("tree-sitter-javascript-0.25.0", "queries/injections.scm"):
        "bea345407ce1f24bccf2b3c1a128acc14e19301327074285368586e793dcb86d",
    ("tree-sitter-javascript-0.25.0", "queries/locals.scm"):
        "c1dd2cfa1400d397fa974208fc9edfe817be4a3c8b42bb99af5c8d193b1bb753",
    ("tree-sitter-javascript-0.25.0", "queries/tags.scm"):
        "6ef988cce5a428a15b5460e3ee96f1478705f965fc6e848a91a18bfa6c6f0212",
    ("tree-sitter-javascript-0.25.0", "src/node-types.json"):
        "188a8baa97018edaf63bf7ece372c07bc0d74d1301004bc8f211eb7dee5a6e70",
    ("tree-sitter-kotlin-ng-1.1.0", "src/node-types.json"):
        "0f21509031ad37902dc05d5828f178fed3545c466ffcf38ef30d969cb64e8e9d",
    ("tree-sitter-python-0.25.0", "queries/highlights.scm"):
        "a6708f209381618e2b398972c8f1ccd892f0c064eab35a2a3f911c3e22e79a7e",
    ("tree-sitter-python-0.25.0", "queries/tags.scm"):
        "d0f3e577878167bfabc30e526e497bce58d1699b9bcabf8ab3a50698efb5ca3e",
    ("tree-sitter-python-0.25.0", "src/node-types.json"):
        "a2456847bea3adff5b2222b2f7b03a870159470d8908622204e6eb29ee2fe45e",
    ("tree-sitter-ruby-0.23.1", "queries/highlights.scm"):
        "0858de9feece6dcd3408a541f995a34918d587ac2c552096026b0e47e2b332e6",
    ("tree-sitter-ruby-0.23.1", "queries/locals.scm"):
        "f00178e94b9c2ea672fd4ee2e60ac5d7d90be59c72e86292e22620358ac9dbb0",
    ("tree-sitter-ruby-0.23.1", "queries/tags.scm"):
        "ed1e7dc07a95b87aef267e7f9715c6717a70f09efbd7b6f48bbd51c7b7510cf3",
    ("tree-sitter-ruby-0.23.1", "src/node-types.json"):
        "9683f80e7be3dd93f1d6f575800de3f3445cda8af1f313f1f80003229c328e8b",
    ("tree-sitter-rust-0.24.2", "queries/highlights.scm"):
        "0f0343107f14a7690157f51090a979eb8f8bfe4eada7c61763ddb4c54b1311d1",
    ("tree-sitter-rust-0.24.2", "queries/injections.scm"):
        "723146f179bc0edfba2f51731c99afce8bed3b677dfb4435c303859fae299ba5",
    ("tree-sitter-rust-0.24.2", "queries/tags.scm"):
        "f22867fdebde5cb091861c08d34690dc2540f4318068bf81be9f6b0d348ab8c1",
    ("tree-sitter-rust-0.24.2", "src/node-types.json"):
        "4b73a1248978340336100db455bf0731c23f9190568c9ae62265fa4a80a327d5",
    ("tree-sitter-swift-0.7.3", "queries/highlights.scm"):
        "b0bbeb918904b252e8bd361f4afb4756647232391110a67deecbf7227464aff8",
    ("tree-sitter-swift-0.7.3", "queries/injections.scm"):
        "fd541e467fd2667ed7cfdf604b0f3b3d1ac2632791972840eaedd0b99a581e58",
    ("tree-sitter-swift-0.7.3", "queries/locals.scm"):
        "430a7d71c30212e1e0e21eb1b3ed71959fb71bf74a6fea27a0efbf491ebda1c7",
    ("tree-sitter-swift-0.7.3", "queries/tags.scm"):
        "6d3884e3f785d621128356a23ac81dfc438d9a853b8ee24b8070dc2caaf631ca",
    ("tree-sitter-swift-0.7.3", "src/node-types.json"):
        "62e0d3bf83969c7c3e783bb377117b370749584fbfe7e1f4926ce4cda629c5d4",
    ("tree-sitter-typescript-0.23.2", "queries/highlights.scm"):
        "e0c35adb819127bfd4f853fac5419e7d8ba44760246201d04a4a5ce0228a10c5",
    ("tree-sitter-typescript-0.23.2", "queries/locals.scm"):
        "c3680f9b56276fb2ccc9f1d5f4d03dbca5d64bdf3e6d52ce5a4ad342cec162d5",
    ("tree-sitter-typescript-0.23.2", "queries/tags.scm"):
        "b391288bcc71b513a5df7c9bb232d8bc7418d7e274b125aa0aa4bdd6121a0338",
    ("tree-sitter-typescript-0.23.2", "tsx/src/node-types.json"):
        "78b5789145286799a27a0a7ecc36cc1bcb151f94ec7fa631b248459867010c8c",
    ("tree-sitter-typescript-0.23.2", "typescript/src/node-types.json"):
        "c790a733fc756b54d4e54dceeb7d2d51e40d8b57136e70277753a75804cce3e3",
}
expected_ingestion_fixture_files = {
    "sample.c": "fefcb5d72c62f15cee4f537ae58e69c21d5e6caa66b24ec0eeb1982a9b9f7dcc",
    "sample.cpp": "1adf7f2fa24438a3e99c0f464eed99d75ad814ab0931e5ecd80d460200c1b36c",
    "sample.cs": "07bfbdfd5adb2e3f28df544a2f16300ca06f1efca2b3fac820d6a31fed449797",
    "sample.go": "4a2641ee9b29fcd521398d372d0e1c38ac6bf290cb386def398fa2993efa178a",
    "sample.java": "dcbdeddf22a5aa26c419c21c0c8b4cc90c63c4818ae89ac35fba1108b46855aa",
    "sample.js": "a0d143cec8600d2df8566dab6781a69490925b9a15209064f020e2776cb44c6e",
    "sample.kt": "fe38a0f37be056fd4e9999c5a6579e9b4ff5e4547dedc829a961d0cca728b882",
    "sample.py": "fc5776d06035e1d662c8e0a4cf54de6ce4f39433364a8f83c592d0ce76546dab",
    "sample.rb": "79609d9be479609397a21a86ff35214548bdece77dae2b0e29d171f7851906e6",
    "sample.swift": "58ab551dc18de2f426f0671393d50117c4bb4175244650feef6e0921c16991fb",
    "sample.ts": "4eac75f24cd061159a1d12d2b2b47e366ff867071c99d4fe461c3bd1278f5589",
    "sample.tsx": "59faee99ca99905bd6d2d8780bba6f40b197c89f19eec633e9b352c706e0222f",
}
expected_by_package = {
    "spike-support": (
        {"serde", "serde_core"},
        set(),
    ),
    "foundation": (
        {"serde", "serde_core", "thiserror"},
        {
            ("clap-4.6.1", "examples/demo.md"),
            ("clap_builder-4.6.0", "README.md"),
        },
    ),
    "mcp-runtime": (
        {"ref-cast", "serde", "serde_core", "thiserror"},
        {
            ("pastey-0.2.3", "README.md"),
            ("rmcp-1.8.0", "README.md"),
            ("schemars-1.2.1", "README.md"),
            ("schemars_derive-1.2.1", "attributes.md"),
            ("schemars_derive-1.2.1", "deriving.md"),
        },
    ),
    "storage-search": (
        {
            "crunchy",
            "libsqlite3-sys",
            "rustversion",
            "serde",
            "serde_core",
            "thiserror",
            "typetag",
        },
        {
            ("bon-3.9.3", "README.md"),
            ("bon-macros-3.9.3", "README.md"),
            ("getrandom-0.4.3", "README.md"),
        },
        {
            ("cc-1.2.67", "src/detect_compiler_family.c"),
        },
    ),
    "ingestion": (
        {"protobuf", "serde", "serde_core", "tree-sitter"},
        set(),
    ),
    "documents-git-watch": (
        {"serde", "serde_core"},
        set(),
    ),
}
if package not in expected_by_package:
    raise SystemExit(f"unexpected research package: {package}")
expected_package_generated, expected_package_documents, *expected_package_sources = (
    expected_by_package[package]
)
expected_package_sources = (
    expected_package_sources[0] if expected_package_sources else set()
)
observed_generated = {}
observed_private_counts = {}
observed_documents = set()
observed_sources = set()
observed_ingestion_registry_files = set()
observed_ingestion_fixture_files = set()
observed_documents_git_watch_registry_files = set()
ingestion_fixture_directory = (
    pathlib.Path(sys.argv[3]).resolve(strict=True).parent / "fixtures"
)
for path in warning_paths:
    if target_directory in path.parents:
        if path.parent.name != "out" or path.parent.parent.parent.name != "build":
            raise SystemExit(f"unexpected generated dependency path: {path}")
        package_directory = path.parent.parent.name
        private_matches = [
            package
            for package in expected_generated
            if package_directory.startswith(f"{package}-")
        ]
        if path.name == "private.rs":
            if len(private_matches) != 1:
                raise SystemExit(f"unexpected generated dependency package: {path}")
            package = private_matches[0]
            expected_count = 2 if (
                root_package == "ingestion" and package == "serde_core"
            ) else 1
            observed_private_counts[package] = (
                observed_private_counts.get(package, 0) + 1
            )
            if observed_private_counts[package] > expected_count:
                raise SystemExit(f"unexpected generated dependency package: {path}")
            contents = path.read_text(encoding="utf-8")
            if (
                contents != expected_generated[package]
                or re.search(r"\bunsafe\b", contents)
            ):
                raise SystemExit(f"unreviewed generated dependency contents: {path}")
            observed_generated[package] = path
            continue
        generated_matches = [
            key
            for key in expected_generated_hashes
            if package_directory.startswith(f"{key[0]}-") and path.name == key[1]
        ]
        if len(generated_matches) != 1 or generated_matches[0] in observed_generated:
            raise SystemExit(f"unexpected generated dependency file: {path}")
        key = generated_matches[0]
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected_generated_hashes[key]:
            raise SystemExit(f"changed generated dependency file: {path}")
        observed_generated[key] = path
        continue

    if root_package == "ingestion" and ingestion_fixture_directory in path.parents:
        relative = path.relative_to(ingestion_fixture_directory).as_posix()
        if (
            relative not in expected_ingestion_fixture_files
            or relative in observed_ingestion_fixture_files
            or hashlib.sha256(path.read_bytes()).hexdigest()
            != expected_ingestion_fixture_files[relative]
        ):
            raise SystemExit(f"unexpected ingestion fixture warning: {path}")
        observed_ingestion_fixture_files.add(relative)
        continue

    if registry_source not in path.parents:
        raise SystemExit(f"unexpected unscanned dependency file: {path}")
    relative = path.relative_to(registry_source)
    if len(relative.parts) < 3:
        raise SystemExit(f"unexpected registry dependency path: {path}")
    key = (relative.parts[1], pathlib.Path(*relative.parts[2:]).as_posix())
    if root_package == "documents-git-watch":
        if (
            key not in expected_documents_git_watch_registry_files
            or key in observed_documents_git_watch_registry_files
            or hashlib.sha256(path.read_bytes()).hexdigest()
            != expected_documents_git_watch_registry_files[key]
        ):
            raise SystemExit(f"unexpected documents/Git dependency warning: {path}")
        observed_documents_git_watch_registry_files.add(key)
        continue
    if root_package == "ingestion":
        if (
            key not in expected_ingestion_registry_files
            or key in observed_ingestion_registry_files
            or hashlib.sha256(path.read_bytes()).hexdigest()
            != expected_ingestion_registry_files[key]
        ):
            raise SystemExit(f"unexpected ingestion dependency warning: {path}")
        observed_ingestion_registry_files.add(key)
        continue
    if key in expected_source_files:
        if key in observed_sources:
            raise SystemExit(f"duplicate dependency source file: {path}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != expected_source_files[key]:
            raise SystemExit(f"changed dependency source file: {path}")
        observed_sources.add(key)
        continue
    if key not in expected_documents or key in observed_documents:
        raise SystemExit(f"unexpected dependency document: {path}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != expected_documents[key]:
        raise SystemExit(f"changed dependency document: {path}")
    observed_documents.add(key)

observed_private_generated = {
    package for package in observed_generated if isinstance(package, str)
}
observed_hashed_generated = {
    package for package in observed_generated if isinstance(package, tuple)
}
expected_hashed_generated = {
    package
    for package in expected_generated_hashes
    if package[0] in expected_package_generated
}
if observed_private_generated != (
    expected_package_generated - {package[0] for package in expected_hashed_generated}
):
    raise SystemExit(
        f"unexpected generated shims for {package}: {sorted(observed_private_generated)}"
    )
expected_private_counts = {
    package: 1
    for package in expected_package_generated
    if package not in {key[0] for key in expected_hashed_generated}
}
if root_package == "ingestion":
    expected_private_counts["serde_core"] = 2
if observed_private_counts != expected_private_counts:
    raise SystemExit(
        f"unexpected generated shim counts for {package}: {observed_private_counts}"
    )
if observed_hashed_generated != expected_hashed_generated:
    raise SystemExit(
        f"unexpected generated files for {package}: {sorted(observed_hashed_generated)}"
    )
if observed_documents != expected_package_documents:
    raise SystemExit(
        f"unexpected dependency documents for {package}: "
        f"{sorted(observed_documents)}"
    )
if observed_sources != expected_package_sources:
    raise SystemExit(
        f"unexpected dependency source files for {package}: {sorted(observed_sources)}"
    )
if root_package == "ingestion":
    if observed_ingestion_registry_files != set(expected_ingestion_registry_files):
        raise SystemExit(
            "unexpected ingestion dependency warnings: "
            f"{sorted(observed_ingestion_registry_files)}"
        )
    if observed_ingestion_fixture_files != set(expected_ingestion_fixture_files):
        raise SystemExit(
            "unexpected ingestion fixture warnings: "
            f"{sorted(observed_ingestion_fixture_files)}"
        )
if root_package == "documents-git-watch":
    if (
        observed_documents_git_watch_registry_files
        != set(expected_documents_git_watch_registry_files)
    ):
        raise SystemExit(
            "unexpected documents/Git dependency warnings: "
            f"{sorted(observed_documents_git_watch_registry_files)}"
        )
if errors != [f"error: Found {len(warning_paths)} warnings"]:
    raise SystemExit(f"unexpected cargo-geiger errors: {errors}")
PY
  then
    cat "$geiger_output" >&2
    cat "$geiger_errors" >&2
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return 1
  fi

  rm "$geiger_output" "$geiger_errors"
  rm -R "$target_directory"
  echo "verified exact audited generated shims and dependency documents" >&2
}

run_optional_geiger() {
  local package_manifest="$1"
  local expected_root="$2"
  local expected_warnings="$3"
  local target_directory
  local geiger_output
  local geiger_errors
  local status
  target_directory="$(mktemp -d "${TMPDIR:-/tmp}/pebble-optional-geiger.XXXXXX")"
  geiger_output="$(mktemp "${TMPDIR:-/tmp}/pebble-optional-geiger-output.XXXXXX")"
  geiger_errors="$(mktemp "${TMPDIR:-/tmp}/pebble-optional-geiger.XXXXXX")"
  if CARGO_TARGET_DIR="$target_directory" cargo geiger \
    --manifest-path "$package_manifest" --all-features --all-targets --locked \
    --color never >"$geiger_output" 2>"$geiger_errors"; then
    status=0
  else
    status=$?
  fi
  if (( status != 1 )); then
    cat "$geiger_output" >&2
    cat "$geiger_errors" >&2
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return "$status"
  fi
  if ! python3 - "$geiger_output" "$geiger_errors" "$target_directory" \
    "$expected_root" "$expected_warnings" "$MANIFEST" <<'PY'
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib

output = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
errors = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
target = pathlib.Path(sys.argv[3]).resolve(strict=True)
root = sys.argv[4]
expected_warnings = int(sys.argv[5])
manifest = pathlib.Path(sys.argv[6])

metadata = json.loads(
    subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(manifest),
            "--format-version",
            "1",
            "--locked",
        ],
        text=True,
    )
)
package_manifest = pathlib.Path(
    next(
        package["manifest_path"]
        for package in metadata["packages"]
        if package["name"] == root
    )
).resolve(strict=True)
tree = subprocess.check_output(
    [
        "cargo",
        "tree",
        "--manifest-path",
        str(manifest),
        "--package",
        root,
        "--all-features",
        "--edges",
        "normal",
        "--prefix",
        "none",
        "--format",
        "{p}",
        "--locked",
    ],
    text=True,
)
package_pattern = re.compile(r"^([A-Za-z0-9_-]+) v([0-9][A-Za-z0-9.+-]*)")
activated = {
    match.groups()
    for line in tree.splitlines()
    if (match := package_pattern.match(line)) is not None
}
if not activated:
    raise SystemExit("cannot determine the activated optional dependency closure")
scanned = {
    match.groups()
    for line in output.splitlines()
    if (
        match := re.search(
            r"([A-Za-z0-9_-]+) ([0-9][A-Za-z0-9.+-]*)\s*$", line
        )
    )
    is not None
}
unscanned = activated - scanned
if unscanned:
    raise SystemExit(
        f"activated optional dependencies were not scanned: {sorted(unscanned)}"
    )
if not re.search(rf"(?m)^.* {re.escape(root)} 0\.0\.0$", output):
    raise SystemExit(f"missing expected cargo-geiger root row: {root}")

warning_prefix = "WARNING: Dependency file was never scanned: "
warnings = [line for line in errors if line.startswith(warning_prefix)]
parse_prefix = "Failed to parse file: "
parse_failures = [line for line in errors if line.startswith(parse_prefix)]
aggregate = [line for line in errors if line.startswith("error:")]
progress = ("Blocking ", "Checking ", "Compiling ", "Finished ", "Removed ", "Scanning done")
known_prefixes = (warning_prefix, parse_prefix, "error: Found ")
unmatched_prefix = "Failed to match (ignoring source) package: "
known_prefixes += (unmatched_prefix,)
unknown = [
    line
    for line in errors
    if line
    and not line.lstrip().startswith(progress)
    and not line.startswith('{"$message_type":"artifact",')
    and not line.startswith(known_prefixes)
]
if unknown:
    raise SystemExit(f"unexpected cargo-geiger diagnostics: {unknown}")
if len(warnings) != expected_warnings or aggregate != [f"error: Found {expected_warnings} warnings"]:
    raise SystemExit("unexpected cargo-geiger warning count")

registry = (
    pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    / "registry"
    / "src"
).resolve(strict=True)
target_warning_hashes = {
    ("pulp", "x86_64_asm.rs"):
        "fa4e0b773e1cbfa0cb319809b6327aaaf913cf5b12c2a00d0a902f53f2767cfa",
    ("rustversion", "version.expr"):
        "abee5452b2ddf17b998eaa82bb92b227c9d3e28f3ad2ab1642b26b3616a1d388",
    ("serde", "private.rs"):
        "f8e9470772811a1bdcd201fb21e14cbf37a57d192b38cd98f66bd4a83442c26b",
    ("serde_core", "private.rs"):
        "27ad1b5fb4eebeb1da6419cbfad3ac1922ed1d817fcbab0a1dae1bdfd38d8307",
    ("thiserror", "private.rs"):
        "5ecbfb43b967bb5711c3bb87517713f04b59d7673d7a98f6358303c28ff6dee4",
}
expected_warning_manifests = {
    "embeddings-vectors-spike":
        "49bb4b0cf6bc0ed446245128b46737d5522c2adc869a60f32da24b8c4024ef94",
    "mmap-boundary":
        "00a0a42d1f48e0e7aee5fe0be7611edd16cf7b23b99126106cd8dcb14b9cac7a",
}
expected_warning_manifest_sha256 = expected_warning_manifests.get(root)
if expected_warning_manifest_sha256 is None:
    raise SystemExit(f"missing optional cargo-geiger warning allowlist: {root}")
canonical_warnings = []
for line in warnings:
    path = pathlib.Path(line.removeprefix(warning_prefix)).resolve(strict=True)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if registry in path.parents:
        relative = path.relative_to(registry)
        if len(relative.parts) < 3:
            raise SystemExit(f"unexpected registry warning path: {path}")
        package = relative.parts[1]
        package_name, separator, package_version = package.rpartition("-")
        if not separator:
            raise SystemExit(f"warning from unrecognized package path: {path}")
        canonical_warnings.append(
            (
                f"registry/{package}/{pathlib.Path(*relative.parts[2:]).as_posix()}",
                digest,
            )
        )
        continue
    if target in path.parents:
        relative = path.relative_to(target)
        match = re.fullmatch(
            r"debug/build/([A-Za-z0-9_-]+)-[0-9a-f]+/out/(.+)",
            relative.as_posix(),
        )
        if match is None:
            raise SystemExit(f"unexpected generated warning path: {path}")
        package_name, generated_path = match.groups()
        expected_digest = target_warning_hashes.get((package_name, generated_path))
        if expected_digest != digest:
            raise SystemExit(f"unexpected generated warning content: {path}")
        canonical_warnings.append((f"target/{package_name}/{generated_path}", digest))
        continue
    else:
        raise SystemExit(f"unexpected unscanned cargo-geiger path: {path}")

canonical_manifest = "\n".join(
    f"{path}\t{digest}" for path, digest in sorted(canonical_warnings)
)
if hashlib.sha256(canonical_manifest.encode()).hexdigest() != expected_warning_manifest_sha256:
    raise SystemExit("unexpected cargo-geiger warning path or content allowlist")

expected_parse_failures = {
    ("bon-macros-3.9.3", "tests/snapshots/bon_incomplete_if.rs"):
        "f6bf01d59b4345c0322a8f7dffb12410590da682560aca911c5fc1b6116dd3f6",
    ("cxx-1.0.197", "tests/ui/ptr_no_const_mut.rs"):
        "402cb4e26413953386975da3a3a3b7d4e35987a5297ec41713d39ae9ecdc2a0a",
    ("cxx-1.0.197", "tests/ui/root_namespace.rs"):
        "32fbf454c246d6ac32c92d6735ea1ed62ad92dea5066b81e670cf7e49d8ebbeb",
    ("erased-serde-0.4.10", "src/features_check/error.rs"):
        "1f2b5d89f9d59ad84a9b77566f9446d36ce430e45eaffdcc30701e8e4de289b9",
}
observed_parse_failures = set()
for line in parse_failures:
    path_text, separator, _error = line.removeprefix(parse_prefix).partition(", Syn(Error(")
    if not separator:
        raise SystemExit(f"malformed cargo-geiger parse failure: {line}")
    path = pathlib.Path(path_text).resolve(strict=True)
    if registry not in path.parents:
        raise SystemExit(f"unexpected cargo-geiger parse failure: {path}")
    relative = path.relative_to(registry)
    if len(relative.parts) < 3:
        raise SystemExit(f"unexpected cargo-geiger parse path: {path}")
    key = (relative.parts[1], pathlib.Path(*relative.parts[2:]).as_posix())
    package_name, separator, package_version = relative.parts[1].rpartition("-")
    if (
        not separator
        or key not in expected_parse_failures
        or key in observed_parse_failures
        or hashlib.sha256(path.read_bytes()).hexdigest() != expected_parse_failures[key]
    ):
        raise SystemExit(f"unreviewed cargo-geiger parse failure: {line}")
    observed_parse_failures.add(key)
if observed_parse_failures != set(expected_parse_failures):
    raise SystemExit(
        f"unexpected cargo-geiger parse failures: {sorted(observed_parse_failures)}"
    )
locked = {
    (package["name"], package["version"])
    for package in tomllib.loads(
        manifest.with_name("Cargo.lock").read_text(encoding="utf-8")
    )["package"]
    if str(package.get("source", "")).startswith("registry+")
}
for line in errors:
    if not line.startswith(unmatched_prefix):
        continue
    matched = re.fullmatch(
        r"Failed to match \(ignoring source\) package: "
        r"registry\+https://github\.com/rust-lang/crates\.io-index"
        r"#([A-Za-z0-9_-]+)@([^ ]+) *",
        line,
    )
    if (
        matched is None
        or matched.groups() not in locked
    ):
        raise SystemExit(f"unexpected cargo-geiger package mismatch: {line}")
PY
  then
    cat "$geiger_output" >&2
    cat "$geiger_errors" >&2
    rm "$geiger_output" "$geiger_errors"
    rm -R "$target_directory"
    return 1
  fi
  rm "$geiger_output" "$geiger_errors"
  rm -R "$target_directory"
}

regenerate_embeddings_result() {
  local result_directory
  result_directory="$(mktemp -d "${TMPDIR:-/tmp}/pebble-embeddings-result.XXXXXX")"
  if ! PEBBLE_SPIKE_RESULT_DIR="$result_directory" PEBBLE_MODEL_FREE_VALIDATED=1 cargo test \
    --manifest-path "$MANIFEST" \
    --package embeddings-vectors-spike \
    --test result \
    --all-features \
    --locked; then
    rm -R "$result_directory"
    return 1
  fi
  if ! python3 "$ROOT_DIR/scripts/validate-research-results.py" \
    "$result_directory/embeddings-vectors.json"; then
    rm -R "$result_directory"
    return 1
  fi
  rm -R "$result_directory"
}

cargo fmt --manifest-path "$MANIFEST" --all -- --check
cargo test --manifest-path "$MANIFEST" \
  --package embeddings-vectors-spike \
  --test no_features \
  --no-default-features \
  --locked
cargo check --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked
cargo clippy --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked -- -D warnings
cargo test --manifest-path "$MANIFEST" \
  --workspace --all-targets --all-features --locked
regenerate_embeddings_result
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path "$MANIFEST" \
  --workspace --all-features --no-deps --locked
cargo audit --file "$ROOT_DIR/research/Cargo.lock"
cargo deny --manifest-path "$MANIFEST" check \
  --config "$ROOT_DIR/deny.toml" \
  advisories bans licenses sources
cargo vet \
  --manifest-path "$MANIFEST" \
  --store-path "$VET_STORE" \
  --locked \
  --no-minimize-exemptions
check_vet_pruned
bash "$ROOT_DIR/scripts/check-vet-exemptions.sh"
package_manifests="$(workspace_package_manifests)"
while IFS= read -r package_manifest; do
  case "$package_manifest" in
    */spikes/embeddings-vectors/Cargo.toml)
      run_optional_geiger "$package_manifest" "embeddings-vectors-spike" 69
      ;;
    */spikes/mmap-boundary/Cargo.toml)
      run_optional_geiger "$package_manifest" "mmap-boundary" 2
      ;;
    *)
      run_geiger "$package_manifest"
      ;;
  esac
done <<< "$package_manifests"

for result in research/results/*.json; do
  python3 -m json.tool "$result" >/dev/null
done

python3 "$ROOT_DIR/scripts/validate-research-results.py"
