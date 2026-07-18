# 🧠 Pebble

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.96.1%2B-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![MCP](https://img.shields.io/badge/MCP-Model%20Context%20Protocol-blue)](https://modelcontextprotocol.io)

**Pebble is a local-first, model-free repository evidence and personal knowledge base for AI coding agents.**

Pebble provides a durable, syntax-aware memory layer that indexes your codebase structure (using tree-sitter AST chunking) and manages living-knowledge claims across your repositories. It exposes a stdio Model Context Protocol (MCP) server that lets agents search, read, and maintain context efficiently within strict token budgets.

### 🔬 Architecture & Features

Pebble's core is written in **Rust** and consists of two main crates:
*   [`pebble-core`](./crates/pebble-core) — The core library handling SQLite-backed metadata/graph storage, tree-sitter chunking, Tantivy search indexing, and living-knowledge engines.
*   [`pebble-cli`](./crates/pebble-cli) — The `pebble` command-line executable, which includes a stdio MCP server adapter.

Three plugins integrate Pebble with AI coding agents:
*   [`claude-code-plugin/`](./claude-code-plugin) — Claude Code plugin adding slash commands, hooks, and a reviewer subagent.
*   [`factory-droid-plugin/`](./factory-droid-plugin) — Factory Droid plugin providing commands, hooks, and a reviewer droid.
*   [`gemini-cli-plugin/`](./gemini-cli-plugin) — Antigravity (AGY) plugin defining skills, hooks, and a reviewer subagent.

### 🔬 Research Basis

Pebble's core concepts (specifically its living-knowledge claims, symbol anchoring, and update queues) are conceptually derived from research in agentic memory and context management:

*   **[EverMemOS](https://arxiv.org/abs/2601.02163)** (`2601.02163`) — Schema design for structured memory consolidation (MemCells and MemScenes).
*   **[A-MEM: Agentic Memory](https://arxiv.org/abs/2502.12110)** (`2502.12110`) — Zettelkasten-inspired note linking and evolution over time.
*   **[ACE: Agentic Context Engineering](https://arxiv.org/abs/2510.04618)** (`2510.04618`) — Context playbooks with delta updates, which inspired Pebble's queued update packets.
*   **[AutoSkill](https://arxiv.org/abs/2603.01145)** (`2603.01145`) — Automated skill extraction from user queries and logs.
*   **[SwiftMem](https://arxiv.org/abs/2601.08160)** (`2601.08160`) — Fast agentic memory indexing and query-aware cache management.

---

## 🚀 Quickstart

Pebble requires **Rust** (v1.96.1+ or toolchain configured via `rust-toolchain.toml`).

### 1) Core setup
You can install the `pebble` binary directly from GitHub:
```bash
cargo install --git https://github.com/skyline-9/pebble pebble-cli
```

Or, if you are developing or setting up plugins, clone the repository and install it locally:
```bash
git clone https://github.com/skyline-9/pebble.git
cd pebble
cargo install --path crates/pebble-cli
```

Verify the installation:
```bash
pebble --help
```

### 2) Repository Initialization
Initialize and register a local git repository to index:
```bash
cd /path/to/your/git-repo
pebble init                     # Creates .pebble/pebble.toml settings
pebble register                 # Registers the checkout globally in registry.json
pebble index                    # Compiles AST chunks and builds the search index
```

### 3) Plugin Integration

#### Claude Code
Run these inside the Claude Code prompt — no local clone needed:
```
/plugin marketplace add skyline-9/pebble
/plugin install pebble@pebble
```

#### Factory Droid
```bash
droid plugin marketplace add https://github.com/skyline-9/pebble.git
droid plugin install pebble@pebble
```

#### Antigravity (AGY)
```bash
agy plugin install https://github.com/skyline-9/pebble/gemini-cli-plugin
```


---

## 🛠️ CLI Reference

The `pebble` executable provides the following subcommands:

```
pebble init [PATH]                # Initialize Pebble configuration (default: .)
pebble register [PATH]            # Register a local checkout (default: .)
pebble index [PATH]               # Compile and activate an immutable index
pebble watch [PATH]               # Watch and reconcile a checkout (optional: --once)
pebble search [ARGS]              # Search model-free repository evidence
pebble read [ARGS]                # Resolve an exact citation
pebble health --repository <ID>   # Validate the current index health
pebble traces --repository <ID>   # List local retrieval traces (optional: --limit <N>)
pebble rebuild [PATH]             # Build a fresh disposable projection
pebble model <install|list|select|remove> [ARGS]   # Manage local embedding models
pebble note <list|read> [ARGS]    # Manage living-knowledge claims
pebble update <list|apply> [ARGS] # Manage living-note update packets
pebble workspace <create|add|list|search> [ARGS]   # Manage multi-repository workspaces
pebble personal <create|list|promote> [ARGS]       # Manage personal knowledge notes
pebble serve                      # Start the stdio MCP server
```

---

## 🧩 MCP Tools (exposed via `serve`)

| Tool | Purpose |
| --- | --- |
| `repository_init` | Initialize portable Pebble configuration for a repository |
| `repository_register` | Register one initialized local checkout |
| `repository_index` | Compile and atomically activate an immutable repository index |
| `search` | Search model-free repository evidence within a strict token budget |
| `evidence_read` | Resolve an exact citation against its indexed worktree revision |
| `index_health` | Validate the current immutable index generation |
| `trace_list` | List a bounded tail of local retrieval traces |
| `projection_rebuild` | Build and atomically activate a fresh disposable projection |
| `model_install` | Show consent disclosure for, or install, a local embedding model |
| `model_list` | List every installed local embedding model |
| `model_select` | Select the active local embedding model for search |
| `model_remove` | Remove one installed local embedding model |
| `note_list` | List managed living-knowledge claims for a repository |
| `note_read` | Read one managed living-knowledge claim's status and prose |
| `update_list` | List queued living-note update packets awaiting a patch |
| `update_apply` | Validate and apply one queued replacement patch |
| `workspace_create` | Create a new empty multi-repository workspace |
| `workspace_add_repository` | Add one registered repository to a workspace |
| `workspace_list` | List every workspace's name |
| `workspace_search` | Search every repository in a workspace and merge results |
| `personal_note_create` | Create a new personal knowledge note |
| `personal_note_list` | List every personal knowledge note |
| `personal_note_promote` | Preview or apply promoting one personal note into a repository |

---

## 🧪 Tests

Validate your changes by running the test suite:
```bash
cargo test --workspace --all-targets --all-features --locked
```

---

## 📄 License

MIT License
