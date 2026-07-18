#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="$ROOT_DIR/research/results/mvp-baseline.json"
OUTPUT_TMP="$OUTPUT.tmp.$$"
TMP_ROOT="$(mktemp -d)"
SERVER_PID=""
SERVER_INPUT_OPEN=0

close_server_input() {
  if (( SERVER_INPUT_OPEN == 1 )); then
    exec 9>&-
    SERVER_INPUT_OPEN=0
  fi
}

stop_server() {
  close_server_input
  if [[ -z "$SERVER_PID" ]]; then
    return
  fi

  kill "$SERVER_PID" 2>/dev/null || true
  for _ in {1..100}; do
    local state
    state="$(ps -o stat= -p "$SERVER_PID" 2>/dev/null | tr -d ' ' || true)"
    if [[ -z "$state" || "$state" == Z* ]]; then
      break
    fi
    sleep 0.02
  done
  if kill -0 "$SERVER_PID" 2>/dev/null; then
    local state
    state="$(ps -o stat= -p "$SERVER_PID" 2>/dev/null | tr -d ' ' || true)"
    if [[ "$state" != Z* ]]; then
      kill -KILL "$SERVER_PID" 2>/dev/null || true
    fi
  fi
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
}

cleanup() {
  stop_server
  rm -rf "$TMP_ROOT"
  rm -f "$OUTPUT_TMP"
}
trap cleanup EXIT

now_ns() {
  python3 -c 'import time; print(time.time_ns())'
}

measure_rss_kib() {
  local pid="$1"
  ps -o rss= -p "$pid" | tr -d '[:space:]'
}

response_status() {
  local output="$1"
  local id="$2"
  local mode="$3"
  local expected_cell="${4:-}"
  python3 - "$output" "$id" "$mode" "$expected_cell" <<'PY'
import json
import pathlib
import sys

path, expected_id, mode, expected_cell = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        continue
    if message.get("id") != expected_id:
        continue
    if "error" in message or "result" not in message:
        raise SystemExit(2)
    result = message["result"]
    if mode == "initialize":
        if not isinstance(result, dict) or not result.get("protocolVersion"):
            raise SystemExit(2)
    elif mode == "query":
        try:
            content = result["content"]
            text_items = [
                item["text"]
                for item in content
                if isinstance(item, dict)
                and item.get("type") == "text"
                and isinstance(item.get("text"), str)
            ]
            payloads = [json.loads(text) for text in text_items]
            hits = [
                hit
                for payload in payloads
                if isinstance(payload, dict)
                for hit in payload.get("hits", [])
                if isinstance(hit, dict)
            ]
        except (KeyError, TypeError, json.JSONDecodeError):
            raise SystemExit(2)
        if not hits or not any(hit.get("cell_id") == expected_cell for hit in hits):
            raise SystemExit(2)
    else:
        raise SystemExit(2)
    raise SystemExit(0)
raise SystemExit(1)
PY
}

wait_for_response() {
  local output="$1"
  local id="$2"
  local mode="$3"
  local expected_cell="${4:-}"
  local deadline=$((SECONDS + 10))

  while true; do
    local status
    if response_status "$output" "$id" "$mode" "$expected_cell"; then
      return
    else
      status=$?
    fi
    if (( status == 2 )); then
      echo "MCP response id $id failed validation" >&2
      return 1
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      echo "MCP server exited while waiting for response id $id" >&2
      return 1
    fi
    if (( SECONDS >= deadline )); then
      echo "timed out waiting for MCP response id $id" >&2
      return 1
    fi
    sleep 0.02
  done
}

file_size_bytes() {
  local path="$1"
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f %z "$path"
  else
    stat -c %s "$path"
  fi
}

cpu_model() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    sysctl -n machdep.cpu.brand_string
  else
    awk -F: '/Model name/{sub(/^[[:space:]]+/, "", $2); print $2; exit}' /proc/cpuinfo
  fi
}

logical_cpu_count() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    sysctl -n hw.logicalcpu
  else
    getconf _NPROCESSORS_ONLN
  fi
}

memory_bytes() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    sysctl -n hw.memsize
  else
    awk '/MemTotal/{printf "%.0f\n", $2 * 1024; exit}' /proc/meminfo
  fi
}

run_fixture() {
  local count="$1"
  local root="$TMP_ROOT/$count"
  local fifo="$root/stdin"
  local output="$root/server.out"
  local error_output="$root/server.err"
  mkdir -p "$root"
  PEBBLE_ROOT="$root" bun run "$ROOT_DIR/pebble-mcp/src/index.ts" \
    init >/dev/null
  PEBBLE_ROOT="$root" bun run "$ROOT_DIR/pebble-mcp/src/index.ts" \
    seed-benchmark --cells "$count"

  mkfifo "$fifo"
  exec 9<>"$fifo"
  SERVER_INPUT_OPEN=1
  : >"$output"
  : >"$error_output"
  local started
  started="$(now_ns)"
  PEBBLE_ROOT="$root" bun run "$ROOT_DIR/pebble-mcp/src/index.ts" serve \
    <&9 >"$output" 2>"$error_output" &
  SERVER_PID="$!"

  printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"baseline","version":"1"}}}' \
    >&9
  wait_for_response "$output" 1 initialize
  local ready
  ready="$(now_ns)"
  local rss
  rss="$(measure_rss_kib "$SERVER_PID")"

  printf '%s\n' \
    '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' \
    >&9
  local query_started
  query_started="$(now_ns)"
  printf '%s\n' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_query","arguments":{"query":"benchmark symbol 000042","top_k":5}}}' \
    >&9
  wait_for_response "$output" 2 query "mc_benchmark_000042"
  local query_finished
  query_finished="$(now_ns)"

  stop_server

  [[ "$rss" =~ ^[1-9][0-9]*$ ]]
  [[ "$(wc -l <"$error_output" | tr -d '[:space:]')" == "1" ]]
  grep -Fxq "pebble-mcp listening on stdio" "$error_output"
  response_status "$output" 1 initialize
  response_status "$output" 2 query "mc_benchmark_000042"
  local startup_ns=$((ready - started))
  local query_ns=$((query_finished - query_started))
  (( startup_ns > 0 ))
  (( query_ns > 0 ))

  printf '{"cells":%s,"idle_rss_kib":%s,"startup_ns":%s,"query_ns":%s,"db_bytes":%s}' \
    "$count" "$rss" "$startup_ns" "$query_ns" \
    "$(file_size_bytes "$root/projection.db")"
}

mkdir -p "$(dirname "$OUTPUT")"
{
  printf '{"schema":1,"commit":"%s","bun":"%s","os":"%s","arch":"%s","cpu":"%s","logical_cpus":%s,"memory_bytes":%s,"fixtures":[' \
    "$(git -C "$ROOT_DIR" rev-parse HEAD)" \
    "$(bun --version)" \
    "$(uname -s)" \
    "$(uname -m)" \
    "$(cpu_model)" \
    "$(logical_cpu_count)" \
    "$(memory_bytes)"
  run_fixture 100
  printf ','
  run_fixture 1000
  printf ','
  run_fixture 10000
  printf ']}\n'
} >"$OUTPUT_TMP"
mv "$OUTPUT_TMP" "$OUTPUT"
