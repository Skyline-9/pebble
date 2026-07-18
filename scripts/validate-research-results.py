#!/usr/bin/env python3
"""Validate checked-in local macOS dependency-spike evidence."""

import json
import math
import pathlib
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parent.parent
RESULTS = ROOT / "research" / "results"
EXPECTED_RUSTC = "rustc 1.96.1 (31fca3adb 2026-06-26) (Homebrew)"
COMMON_KEYS = ["schema", "spike", "fixture_hash", "dependencies", "targets"]
TARGET_KEYS = [
    "target",
    "rustc",
    "ci_run_id",
    "measurements",
    "checks",
    "failures",
]
MEASUREMENT_KEYS = ["name", "value", "unit"]
EXPECTED = {
    "foundation": {
        "fixture_hash": (
            "blake3:"
            "d08a49024e613471b3bca3dd712c8f402038bea1197cddcd63787b1c023d6087"
        ),
        "dependencies": [
            "clap=4.6.1[default=false,help,std,usage]",
            "serde=1.0.228[default=false,derive,std]",
            "serde_json=1.0.150[default=false,std]",
            "thiserror=2.0.18[default=false,std]",
            "tokio=1.52.3[default=false,macros,rt-multi-thread,sync,time]",
        ],
        "measurements": [
            ("noop_clean_build", 1.234882708, "seconds"),
            ("noop_binary", 338880.0, "bytes"),
            ("noop_startup", 217204375.0, "nanoseconds"),
            ("noop_idle_rss", 1536.0, "kibibytes"),
            ("noop_clean_shutdown", 1.0, "boolean"),
            ("foundation_clean_build", 7.390443083, "seconds"),
            ("foundation_binary", 780816.0, "bytes"),
            ("foundation_startup", 14896167.0, "nanoseconds"),
            ("foundation_idle_rss", 2240.0, "kibibytes"),
            ("foundation_clean_shutdown", 1.0, "boolean"),
        ],
        "checks": [
            "exact_request_query_filter_and_limit_bounds",
            "invalid_field_bound_rejection",
            "malformed_unknown_duplicate_trailing_and_oversized_rejection",
            "clap_help_snapshot_and_argument_contracts",
            "cli_encoded_request_bound",
            "tokio_cancellation_under_100ms",
            "noop_protocol_clean_bounded_eof",
            "foundation_protocol_clean_bounded_eof",
        ],
    },
    "mcp-runtime": {
        "fixture_hash": (
            "blake3:"
            "ce64721270c6e3a61c27685e7394c6b7b4dcbb1df4e894d909634d5fcb002264"
        ),
        "dependencies": [
            "rmcp=1.8.0[default=false,server,transport-io]",
            "serde_json=1.0.150[default=false,std]",
            "tokio-util=0.7.18[default=false,rt]",
            "tokio=1.52.3[default=false,io-std,macros,rt-multi-thread]",
        ],
        "measurements": [
            ("noop_clean_build", 1.213470208, "seconds"),
            ("noop_binary", 338880.0, "bytes"),
            ("noop_startup", 241695583.0, "nanoseconds"),
            ("noop_idle_rss", 1536.0, "kibibytes"),
            ("noop_clean_shutdown", 1.0, "boolean"),
            ("mcp_runtime_clean_build", 21.92398225, "seconds"),
            ("mcp_runtime_binary", 1784896.0, "bytes"),
            ("mcp_runtime_startup", 9777458.0, "nanoseconds"),
            ("mcp_runtime_idle_rss", 2992.0, "kibibytes"),
            ("mcp_runtime_clean_shutdown", 1.0, "boolean"),
            ("maximum_inbound_frame", 1048576.0, "bytes"),
        ],
        "checks": [
            "noop_protocol_clean_bounded_eof",
            "pre_initialize_eof_prompt_success",
            "claude_initialize_initialized_tools_list_and_ping",
            "factory_initialize_initialized_tools_list_and_ping",
            "gemini_initialize_initialized_tools_list_and_ping",
            "malformed_frame_protocol_error_and_recovery",
            "oversized_frame_preparse_rejection",
            "request_cancellation_and_recovery",
            "in_flight_eof_cancellation",
            "protocol_only_stdout",
            "clean_bounded_eof_shutdown",
        ],
    },
    "storage-search": {
        "fixture_hash": (
            "blake3:"
            "ccde1abaa8026348a6852f4300630905a155db528d03bf694549302f8ccafe11"
        ),
        "dependencies": [
            "rusqlite=0.40.1[default=false,bundled]",
            "tantivy=0.26.1[default=false,mmap]",
            "thiserror=2.0.18[default=false,std]",
        ],
        "measurements": [
            ("storage_search_clean_build", 39.669418584, "seconds"),
            ("storage_search_commit_child_binary", 2165920.0, "bytes"),
            ("contract_index_bytes", 7609.0, "bytes"),
            ("contract_commit_latency", 61849500.0, "nanoseconds"),
            ("contract_cold_query_latency", 121417.0, "nanoseconds"),
            ("contract_warm_query_latency", 106750.0, "nanoseconds"),
            ("contract_peak_rss", 21728.0, "kibibytes"),
            ("documents_1000_index_bytes", 130573.0, "bytes"),
            ("documents_1000_commit_latency", 90181625.0, "nanoseconds"),
            ("documents_1000_cold_query_latency", 267167.0, "nanoseconds"),
            ("documents_1000_warm_query_latency", 336375.0, "nanoseconds"),
            ("documents_1000_peak_rss", 24144.0, "kibibytes"),
            ("documents_10000_index_bytes", 1258814.0, "bytes"),
            ("documents_10000_commit_latency", 261674000.0, "nanoseconds"),
            ("documents_10000_cold_query_latency", 1332625.0, "nanoseconds"),
            ("documents_10000_warm_query_latency", 1333167.0, "nanoseconds"),
            ("documents_10000_peak_rss", 34736.0, "kibibytes"),
            ("documents_100000_index_bytes", 12637084.0, "bytes"),
            ("documents_100000_commit_latency", 260272208.0, "nanoseconds"),
            ("documents_100000_cold_query_latency", 11624750.0, "nanoseconds"),
            ("documents_100000_warm_query_latency", 11652334.0, "nanoseconds"),
            ("documents_100000_peak_rss", 46304.0, "kibibytes"),
        ],
        "checks": [
            "sqlite_strict_schema_transaction_rollback_and_integrity",
            "sqlite_generation_scoped_parameterized_ordered_limit_ten_reads",
            "tantivy_held_generation_one_searcher_is_immutable_after_generation_two_opens",
            "tantivy_generation_two_reader_is_isolated",
            "tantivy_exact_language_filter",
            "tantivy_corrupted_copied_segment_is_rejected",
            "tantivy_memory_mapped_segment_cache",
            "killed_child_inside_staging_commit_preserves_active_generation_one",
            "deterministic_document_count_fixtures_1000_10000_100000",
        ],
    },
    "ingestion": {
        "fixture_hash": (
            "blake3:"
            "b306658d419e429691e31c4969366f6eb1025b7d424a0456713cf511078ca637"
        ),
        "dependencies": [
            "protobuf=3.7.2[default=false]",
            "scip=0.6.1[default=false]",
            "tree-sitter=0.26.8[default=false,std]",
            "tree-sitter-c=0.24.2[default=false]",
            "tree-sitter-c-sharp=0.23.5[default=false]",
            "tree-sitter-cpp=0.23.4[default=false]",
            "tree-sitter-go=0.25.0[default=false]",
            "tree-sitter-java=0.23.5[default=false]",
            "tree-sitter-javascript=0.25.0[default=false]",
            "tree-sitter-kotlin-ng=1.1.0[default=false]",
            "tree-sitter-python=0.25.0[default=false]",
            "tree-sitter-ruby=0.23.1[default=false]",
            "tree-sitter-rust=0.24.2[default=false]",
            "tree-sitter-swift=0.7.3[default=false]",
            "tree-sitter-typescript=0.23.2[default=false]",
        ],
        "measurements": [
            ("grammar_count", 12.0, "grammars"),
            ("fixture_named_nodes", 1153.0, "nodes"),
            ("malformed_error_nodes", 36.0, "nodes"),
            ("fixture_parse_latency", 2868.0, "microseconds"),
            ("peak_rss", 36768.0, "kibibytes"),
        ],
        "checks": [
            "twelve_checked_in_grammar_fixtures_parse",
            "all_grammar_abis_are_compatible",
            "one_parser_is_reused",
            "jsx_fixture_parses",
            "tsx_fixture_parses",
            "nonrecursive_named_node_walk",
            "malformed_input_isolated",
            "truncated_input_isolated",
            "five_mebibyte_pathological_parse_is_cancelled",
            "scip_pre_and_post_decode_bounds",
            "scip_unknown_fields_preserve_decoded_counts",
            "scip_invalid_occurrence_ranges_are_rejected",
        ],
    },
    "documents-git-watch": {
        "fixture_hash": (
            "blake3:"
            "a679d7edc0319cd6ccf5fdd413f7a8f45c793d52fe852d2d29f34bffd5748a0c"
        ),
        "dependencies": [
            "blake3=1.8.5[default=true]",
            "ignore=0.4.28[default=true]",
            "notify=8.2.0[default=true]",
            "pulldown-cmark=0.13.4[default=false]",
            "toml=1.1.2+spec-1.1.0[default=false,parse,serde]",
            "ulid=1.2.1[default=true]",
            "yaml-rust2=0.11.0[default=false]",
        ],
        "measurements": [
            ("document_parse_latency", 1099459.0, "nanoseconds"),
            ("documents_git_watch_clean_build", 7.63625475, "seconds"),
            ("documents_git_watch_probe_binary", 338560.0, "bytes"),
            ("documents_git_watch_idle_rss", 13632.0, "kibibytes"),
            ("documents_git_watch_peak_rss", 13632.0, "kibibytes"),
            ("watcher_coalescing_latency", 12431888167.0, "nanoseconds"),
            ("watcher_reconciled_paths", 4.0, "paths"),
            ("documents_git_watch_disk", 338560.0, "bytes"),
        ],
        "checks": [
            "byte_preserving_markdown_offsets_and_single_level_managed_regions",
            "bounded_frontmatter_yaml_alias_depth_size_and_claim_validation",
            "toml_duplicate_key_rejection",
            "system_git_subprocess_boundary_local_state_worktrees_alternates_and_remotes",
            "system_git_subprocess_isolation_blocks_config_hooks_credentials_and_network",
            "nested_gitignore_and_symlink_parity_with_system_git",
            "blake3_known_vector_and_streaming_stability",
            "ulid_fixed_part_lexical_order_and_deterministic_wrapper",
            "notify_atomic_save_rename_and_ten_thousand_write_storm_coalescing",
            "notify_os_error_and_intentionally_dropped_event_recovered_by_blake3_scan",
        ],
        "failures": [
            "gix_0_83_0_rejected_malformed_index_panics_in_isolated_process",
        ],
    },
    "embeddings-vectors": {
        "fixture_hash": (
            "blake3:"
            "2eaf5df1aa17178bded19ec9f7b7de72ef2b5537cb11edb3567cbeac80c1ecca"
        ),
        "dependencies": [
            "candle-core=0.11.0[default=true(empty),transitive=candle-nn,candle-transformers]",
            "candle-nn=0.11.0[default=true(empty),transitive=candle-transformers]",
            "candle-transformers=0.11.0[default=false]",
            "hnsw=0.11.0[default=false]",
            "instant-distance=0.6.1[default=false]",
            "memmap2=0.9.11[default=false,boundary-only]",
            "safetensors=0.7.0[default=false]",
            "sha2=0.10.9[default=false]",
            "tokenizers=0.23.1[default=false,onig]",
            "ureq=3.1.2[default=false,rustls]",
            "usearch=2.24.0[default=false]",
        ],
        "measurements": [
            ("cold_cpu_model_load", 53125.0, "nanoseconds"),
            ("warm_cpu_embedding_batch", 255375.0, "nanoseconds"),
            ("golden_cosine", 0.9999999403953552, "cosine"),
            ("sealed_vector_file", 1288.0, "bytes"),
            ("mapped_vector_rss", 3604480.0, "bytes"),
            ("flat_10000_query", 4557708.0, "nanoseconds"),
            ("flat_10000_recall_at_10", 1.0, "ratio"),
            ("flat_10000_vectors", 120000.0, "bytes"),
            ("flat_100000_query", 107905875.0, "nanoseconds"),
            ("flat_100000_recall_at_10", 1.0, "ratio"),
            ("flat_100000_vectors", 1200000.0, "bytes"),
            ("flat_1000000_query", 1231613042.0, "nanoseconds"),
            ("flat_1000000_recall_at_10", 1.0, "ratio"),
            ("flat_1000000_vectors", 12000000.0, "bytes"),
        ],
        "checks": [
            "explicit_consent_precedes_loopback_socket_open",
            "parsed_scheme_host_port_origin_checksum_byte_limit_resume_and_atomic_install",
            "prefix_subdomain_redirect_filename_traversal_duplicate_and_partial_cleanup_rejected",
            "cpu_only_deterministic_fixture_embedding_offline_second_run_and_idle_unload",
            "malicious_safetensors_dtype_offsets_overflow_shape_and_token_limit_rejected_before_inference",
            "sealed_private_copy_mmap_boundary_validates_magic_schema_fingerprint_dimensions_rows_and_digest",
            "sealed_vector_source_replacement_mutation_truncation_and_current_generation_changes_are_isolated",
            "flat_top_k_is_bounded_deterministic_and_exact_recall_at_ten_at_10000_100000_1000000",
            "ann_candidates_compile_only_and_remain_deferred_without_flat_contract_failure",
            "model_free_default_build_reports_all_optional_capabilities_unavailable",
        ],
        "failures": [
            "external_model_profile_remains_deferred_pending_reviewed_file_sha256_and_two_target_golden_artifacts",
        ],
    },
}


def reject_constant(value):
    raise ValueError(f"invalid JSON constant: {value}")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_result_path(path):
    raw = path.read_bytes()
    if not raw.endswith(b"\n"):
        raise ValueError(f"{path}: missing final newline")
    return json.loads(
        raw,
        object_pairs_hook=unique_object,
        parse_constant=reject_constant,
    )


def read_result(name):
    return read_result_path(RESULTS / f"{name}.json")


def require_keys(value, keys, location):
    if not isinstance(value, dict) or list(value) != keys:
        raise ValueError(f"{location}: expected ordered keys {keys}")


def validate_measurements(actual, expected, location):
    if not isinstance(actual, list) or len(actual) != len(expected):
        raise ValueError(f"{location}: unexpected measurement count")
    observed_metadata = []
    for index, measurement in enumerate(actual):
        item = f"{location}[{index}]"
        require_keys(measurement, MEASUREMENT_KEYS, item)
        value = measurement["value"]
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise ValueError(f"{item}: measurement must be numeric")
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"{item}: measurement must be finite and positive")
        observed_metadata.append((measurement["name"], measurement["unit"]))
    expected_metadata = [(name, unit) for name, _value, unit in expected]
    if observed_metadata != expected_metadata:
        raise ValueError(f"{location}: checked-in measurement metadata changed")


def validate_result(name, expected):
    result = read_result(name)
    location = f"research/results/{name}.json"
    require_keys(result, COMMON_KEYS, location)
    if (
        type(result["schema"]) is not int
        or result["schema"] != 1
        or result["spike"] != name
    ):
        raise ValueError(f"{location}: unexpected schema or spike")
    if result["fixture_hash"] != expected["fixture_hash"]:
        raise ValueError(f"{location}: fixture hash changed")
    if result["dependencies"] != expected["dependencies"]:
        raise ValueError(f"{location}: dependency evidence changed")
    targets = result["targets"]
    if not isinstance(targets, list) or len(targets) != 1:
        raise ValueError(f"{location}: expected one provisional target")
    target = targets[0]
    require_keys(target, TARGET_KEYS, f"{location}.targets[0]")
    if target["target"] != "aarch64-apple-darwin":
        raise ValueError(f"{location}: unexpected provisional target")
    if target["rustc"] != EXPECTED_RUSTC:
        raise ValueError(f"{location}: unexpected compiler")
    if target["ci_run_id"] != "local":
        raise ValueError(f"{location}: provisional evidence must be local")
    validate_measurements(
        target["measurements"],
        expected["measurements"],
        f"{location}.targets[0].measurements",
    )
    if target["checks"] != expected["checks"]:
        raise ValueError(f"{location}: correctness checks changed")
    if target["failures"] != expected.get("failures", []):
        raise ValueError(f"{location}: unexpected correctness failures")


def main():
    for name, expected in EXPECTED.items():
        validate_result(name, expected)
    if len(sys.argv) == 2:
        validate_regenerated_embeddings(pathlib.Path(sys.argv[1]))
    elif len(sys.argv) != 1:
        raise ValueError("expected at most one regenerated embeddings result path")
    print("verified checked-in local macOS research evidence")


def validate_regenerated_embeddings(path):
    checked = read_result("embeddings-vectors")
    generated = read_result_path(path)
    require_keys(generated, COMMON_KEYS, str(path))
    checked_target = checked["targets"][0]
    generated_targets = generated.get("targets")
    if not isinstance(generated_targets, list) or len(generated_targets) != 1:
        raise ValueError(f"{path}: expected one regenerated target")
    generated_target = generated_targets[0]
    require_keys(generated_target, TARGET_KEYS, f"{path}.targets[0]")
    for field in ["schema", "spike", "fixture_hash", "dependencies"]:
        if generated.get(field) != checked[field]:
            raise ValueError(f"{path}: regenerated {field} diverged from source contract")
    for field in ["checks", "failures"]:
        if generated_target.get(field) != checked_target[field]:
            raise ValueError(f"{path}: regenerated {field} diverged from source contract")
    checked_metadata = [
        (item["name"], item["unit"]) for item in checked_target["measurements"]
    ]
    generated_metadata = []
    for index, item in enumerate(generated_target["measurements"]):
        require_keys(item, MEASUREMENT_KEYS, f"{path}.targets[0].measurements[{index}]")
        value = item["value"]
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise ValueError(f"{path}: regenerated measurement must be numeric")
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"{path}: regenerated measurement must be finite and positive")
        generated_metadata.append((item["name"], item["unit"]))
    if generated_metadata != checked_metadata:
        raise ValueError(f"{path}: regenerated measurement metadata diverged")


if __name__ == "__main__":
    try:
        main()
    except (
        OSError,
        TypeError,
        ValueError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"research result validation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
