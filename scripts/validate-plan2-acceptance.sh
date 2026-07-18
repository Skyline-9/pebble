#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cd "$ROOT_DIR"

cargo test -p pebble-cli --test plan2_e2e --locked
cargo build -p pebble-cli --release --locked

PEBBLE_BIN="${PEBBLE_BIN:-$ROOT_DIR/target/release/pebble}"
FIXTURE="$ROOT_DIR/tests/fixtures/plan2"
RETRIEVAL_RESULT="$ROOT_DIR/research/results/plan2-retrieval-acceptance.json"
RESOURCE_RESULT="$ROOT_DIR/research/results/plan2-resource-acceptance.json"

test -x "$PEBBLE_BIN"
test -f "$FIXTURE/acceptance.json"
mkdir -p "$(dirname "$RETRIEVAL_RESULT")"

ROOT_DIR="$ROOT_DIR" \
PEBBLE_BIN="$PEBBLE_BIN" \
FIXTURE="$FIXTURE" \
RETRIEVAL_RESULT="$RETRIEVAL_RESULT" \
RESOURCE_RESULT="$RESOURCE_RESULT" \
python3 - <<'PY'
import hashlib
import json
import math
import os
import pathlib
import platform
import resource
import shutil
import statistics
import subprocess
import tempfile
import time

root = pathlib.Path(os.environ["ROOT_DIR"])
binary = pathlib.Path(os.environ["PEBBLE_BIN"])
fixture = pathlib.Path(os.environ["FIXTURE"])
retrieval_path = pathlib.Path(os.environ["RETRIEVAL_RESULT"])
resource_path = pathlib.Path(os.environ["RESOURCE_RESULT"])
spec = json.loads((fixture / "acceptance.json").read_text(encoding="utf-8"))
thresholds = {
    "recall_at_10_min": 0.80,
    "ndcg_at_10_min": 0.70,
    "citation_precision_min": 0.98,
    "hosted_model_network_attempts_max": 0,
}
resource_limits = spec["resource_contract"]


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def fixture_digest():
    digest = hashlib.sha256()
    for path in sorted(item for item in fixture.rglob("*") if item.is_file()):
        digest.update(path.relative_to(fixture).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def run(command, *, cwd, home, check=True):
    environment = os.environ.copy()
    environment.update({
        "HOME": str(home),
        "USERPROFILE": str(home),
        "HTTP_PROXY": "http://127.0.0.1:9",
        "HTTPS_PROXY": "http://127.0.0.1:9",
        "ALL_PROXY": "http://127.0.0.1:9",
        "NO_PROXY": "",
    })
    started = time.perf_counter_ns()
    result = subprocess.run(
        [str(part) for part in command],
        cwd=cwd,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    if check and result.returncode != 0:
        raise RuntimeError(
            f"{command!r} failed ({result.returncode}): "
            f"{result.stderr.decode(errors='replace')}"
        )
    return result, elapsed_ms


def run_json(arguments, *, cwd, home):
    result, elapsed_ms = run([binary, "--json", *arguments], cwd=cwd, home=home)
    if result.stderr:
        raise RuntimeError(f"unexpected stderr: {result.stderr.decode(errors='replace')}")
    return json.loads(result.stdout), elapsed_ms


def git(arguments, *, cwd, home):
    result, _ = run(["git", "-C", cwd, *arguments], cwd=cwd, home=home)
    return result.stdout.decode().strip()


def packet_id(packet):
    canonical = json.dumps(packet, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def revision_argument(citation):
    revision = citation["revision"]
    suffix = revision.get("dirty_digest")
    return revision["base_oid"] if suffix is None else f"{revision['base_oid']}+dirty.{suffix}"


def ranked_paths(packet):
    paths = []
    for item in packet["items"]:
        path = item["citation"]["path"]
        if path not in paths:
            paths.append(path)
    return paths[:10]


def query_metrics(ranked, relevance):
    relevant = set(relevance)
    recall = len(relevant.intersection(ranked)) / len(relevant)
    gains = [relevance.get(path, 0) for path in ranked[:10]]
    ideal = sorted(relevance.values(), reverse=True)[:10]

    def dcg(values):
        return sum((2**grade - 1) / math.log2(rank + 2) for rank, grade in enumerate(values))

    ideal_dcg = dcg(ideal)
    ndcg = dcg(gains) / ideal_dcg if ideal_dcg else 0.0
    return recall, ndcg


def directory_bytes(path):
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def cpu_model():
    cpuinfo = pathlib.Path("/proc/cpuinfo")
    if not cpuinfo.is_file():
        return platform.processor() or "unknown"
    for line in cpuinfo.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("model name"):
            return line.split(":", 1)[1].strip()
    return platform.processor() or "unknown"


def memory_bytes():
    meminfo = pathlib.Path("/proc/meminfo")
    if not meminfo.is_file():
        return None
    for line in meminfo.read_text(encoding="utf-8").splitlines():
        if line.startswith("MemTotal:"):
            return int(line.split()[1]) * 1024
    return None


def static_network_proof():
    tree = subprocess.run(
        ["cargo", "tree", "-p", "pebble-cli", "-e", "features", "--prefix", "none", "--locked"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        text=True,
    ).stdout
    prohibited_packages = [
        "reqwest", "hyper ", "curl", "ureq", "openssl", "rustls",
        "tonic", "hf-hub", "aws-sdk",
    ]
    package_hits = [name for name in prohibited_packages if name in tree.lower()]
    symbols = subprocess.run(
        ["nm", "-D", "--undefined-only", str(binary)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        text=True,
    ).stdout
    imported = {
        line.split()[-1].split("@", 1)[0]
        for line in symbols.splitlines()
        if line.split()
    }
    prohibited_symbols = {
        "socket", "connect", "sendto", "recvfrom",
        "getaddrinfo", "gethostbyname", "curl_easy_perform",
    }
    symbol_hits = sorted(imported.intersection(prohibited_symbols))
    passed = not package_hits and not symbol_hits
    return {
        "method": "linux_release_binary_dynamic-symbol-and-cargo-feature-boundary",
        "platform": platform.system().lower(),
        "prohibited_network_packages": prohibited_packages,
        "observed_prohibited_packages": package_hits,
        "prohibited_network_symbols": sorted(prohibited_symbols),
        "observed_prohibited_symbols": symbol_hits,
        "hosted_model_network_attempts": 0 if passed else 1,
        "passed": passed,
    }


retrieval = {
    "schema_version": 1,
    "fixture": spec["fixture"],
    "fixture_sha256": fixture_digest(),
    "thresholds": thresholds,
    "query_count": len(spec["queries"]),
    "queries": [],
}
resources = {
    "schema_version": 1,
    "fixture": spec["fixture"],
    "fixture_description": resource_limits["fixture_description"],
    "fixture_sha256": retrieval["fixture_sha256"],
    "binary_sha256": sha256_file(binary),
    "hardware": {
        "os": platform.platform(),
        "architecture": platform.machine(),
        "cpu": cpu_model(),
        "logical_cpus": os.cpu_count(),
        "memory_bytes": memory_bytes(),
        "python": platform.python_version(),
    },
    "limits": {
        key: value for key, value in resource_limits.items() if key.endswith("_max")
    },
}

try:
    if len(spec["languages"]) != 14 or len(spec["queries"]) < 30:
        raise RuntimeError("fixture coverage or independently adjudicated query count is incomplete")
    with tempfile.TemporaryDirectory(prefix="pebble-plan2-acceptance-") as temporary:
        temporary = pathlib.Path(temporary)
        repository = temporary / "repository"
        home = temporary / "home"
        shutil.copytree(fixture, repository)
        home.mkdir()
        git(["init", "-q"], cwd=repository, home=home)
        git(["config", "user.email", "plan2@example.invalid"], cwd=repository, home=home)
        git(["config", "user.name", "Plan 2 Acceptance"], cwd=repository, home=home)
        git(["add", "."], cwd=repository, home=home)
        git(["commit", "-qm", "plan2 fixture"], cwd=repository, home=home)

        initialized, _ = run_json(["init", repository], cwd=repository, home=home)
        repository_id = initialized["repository_id"]
        git(["add", ".pebble/pebble.toml"], cwd=repository, home=home)
        git(["commit", "-qm", "configure pebble"], cwd=repository, home=home)
        run_json(["register", repository], cwd=repository, home=home)
        indexed, initial_index_ms = run_json(["index", repository], cwd=repository, home=home)

        citation_total = 0
        citation_resolved = 0
        recalls = []
        ndcgs = []
        first_packet = None
        cold_query_ms = None
        for number, adjudication in enumerate(spec["queries"]):
            packet, elapsed_ms = run_json([
                "search", adjudication["query"], "--repository", repository_id,
                "--limit", "10", "--budget", "6000",
            ], cwd=repository, home=home)
            if number == 0:
                first_packet = packet
                cold_query_ms = elapsed_ms
            ranked = ranked_paths(packet)
            recall, ndcg = query_metrics(ranked, adjudication["relevance"])
            recalls.append(recall)
            ndcgs.append(ndcg)
            for item in packet["items"]:
                citation_total += 1
                citation = item["citation"]
                resolved, _ = run_json([
                    "read", "--repository", citation["repository"],
                    "--revision", revision_argument(citation),
                    f"--path={citation['path']}",
                    "--start-line", citation["start_line"],
                    "--end-line", citation["end_line"],
                ], cwd=repository, home=home)
                if resolved["content"].rstrip() == item["content"].rstrip():
                    citation_resolved += 1
            retrieval["queries"].append({
                "id": adjudication["id"],
                "query": adjudication["query"],
                "relevance": adjudication["relevance"],
                "ranked_paths_at_10": ranked,
                "recall_at_10": recall,
                "ndcg_at_10": ndcg,
                "packet_id": packet_id(packet),
            })

        replay, _ = run_json([
            "search", spec["queries"][0]["query"], "--repository", repository_id,
            "--limit", "10", "--budget", "6000",
        ], cwd=repository, home=home)
        deterministic_replay = first_packet == replay
        deterministic_packet_id = packet_id(first_packet) == packet_id(replay)

        warm_samples = []
        for _ in range(5):
            _, elapsed_ms = run_json([
                "search", spec["queries"][0]["query"], "--repository", repository_id,
                "--limit", "10", "--budget", "6000",
            ], cwd=repository, home=home)
            warm_samples.append(elapsed_ms)

        with (repository / "distractors.txt").open("a", encoding="utf-8") as handle:
            handle.write("IncrementalIndexSentinel is a measured dirty update.\n")
        incremental, incremental_index_ms = run_json(
            ["index", repository], cwd=repository, home=home
        )
        dirty_revision = "+dirty." in incremental["revision"]
        git(["checkout", "--", "distractors.txt"], cwd=repository, home=home)
        run_json(["rebuild", repository], cwd=repository, home=home)

        generations = (
            home / ".pebble" / "v1" / "repos" / repository_id / "generations"
        )
        (generations / "CRASH-POINT.building").mkdir()
        (generations / "CRASH-POINT.building" / "partial").write_text(
            "incomplete", encoding="utf-8"
        )
        (generations / "CURRENT.tmp").write_text("CRASH-POINT\n", encoding="utf-8")
        crash_packet, _ = run_json([
            "search", spec["queries"][0]["query"], "--repository", repository_id,
            "--limit", "10", "--budget", "6000",
        ], cwd=repository, home=home)
        crash_isolation = packet_id(crash_packet) == packet_id(first_packet)

        (generations / "CURRENT").write_text("../invalid\n", encoding="utf-8")
        unavailable, _ = run(
            [binary, "--json", "search", spec["queries"][0]["query"],
             "--repository", repository_id],
            cwd=repository, home=home, check=False,
        )
        rejected_corrupt_current = (
            unavailable.returncode == 1
            and not unavailable.stdout
            and b"unavailable" in unavailable.stderr
        )
        run_json(["rebuild", repository], cwd=repository, home=home)
        recovered_packet, _ = run_json([
            "search", spec["queries"][0]["query"], "--repository", repository_id,
            "--limit", "10", "--budget", "6000",
        ], cwd=repository, home=home)
        recovery = (
            rejected_corrupt_current
            and packet_id(recovered_packet) == packet_id(first_packet)
        )

        network_proof = static_network_proof()
        recall_at_10 = statistics.fmean(recalls)
        ndcg_at_10 = statistics.fmean(ndcgs)
        citation_precision = citation_resolved / citation_total if citation_total else 0.0
        retrieval.update({
            "metrics": {
                "recall_at_10": recall_at_10,
                "ndcg_at_10": ndcg_at_10,
                "citation_precision": citation_precision,
                "citations_resolved": citation_resolved,
                "citations_total": citation_total,
                "deterministic_replay": deterministic_replay,
                "deterministic_packet_ids": deterministic_packet_id,
                "crash_isolation": crash_isolation,
                "recovery": recovery,
                "hosted_model_network_attempts": network_proof[
                    "hosted_model_network_attempts"
                ],
            },
            "network_proof": network_proof,
            "indexed_revision": indexed["revision"],
        })

        peak_rss_bytes = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss * 1024
        index_bytes = directory_bytes(
            home / ".pebble" / "v1" / "repos" / repository_id
        )
        resources["measurements"] = {
            "peak_rss_bytes": peak_rss_bytes,
            "index_bytes": index_bytes,
            "initial_index_ms": initial_index_ms,
            "cold_query_ms": cold_query_ms,
            "warm_query_ms_median": statistics.median(warm_samples),
            "warm_query_ms_samples": warm_samples,
            "incremental_index_ms": incremental_index_ms,
            "incremental_revision_was_dirty": dirty_revision,
        }

        retrieval_passed = (
            recall_at_10 >= thresholds["recall_at_10_min"]
            and ndcg_at_10 >= thresholds["ndcg_at_10_min"]
            and citation_precision >= thresholds["citation_precision_min"]
            and deterministic_replay
            and deterministic_packet_id
            and crash_isolation
            and recovery
            and network_proof["passed"]
        )
        measurements = resources["measurements"]
        resource_passed = (
            0 < peak_rss_bytes <= resource_limits["peak_rss_bytes_max"]
            and 0 < index_bytes <= resource_limits["index_bytes_max"]
            and 0 < cold_query_ms <= resource_limits["cold_query_ms_max"]
            and 0 < measurements["warm_query_ms_median"]
            <= resource_limits["warm_query_ms_max"]
            and 0 < incremental_index_ms
            <= resource_limits["incremental_index_ms_max"]
            and dirty_revision
        )
        retrieval["status"] = "passed" if retrieval_passed else "failed"
        resources["status"] = "passed" if resource_passed else "failed"
except Exception as error:
    retrieval["status"] = "failed"
    retrieval["error"] = str(error)
    resources["status"] = "failed"
    resources["error"] = str(error)

retrieval_path.write_text(
    json.dumps(retrieval, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
resource_path.write_text(
    json.dumps(resources, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
print(json.dumps({
    "retrieval": retrieval["status"],
    "resources": resources["status"],
    "retrieval_result": str(retrieval_path),
    "resource_result": str(resource_path),
}, sort_keys=True))
if retrieval["status"] != "passed" or resources["status"] != "passed":
    raise SystemExit(1)
PY
