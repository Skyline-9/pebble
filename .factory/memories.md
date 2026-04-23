# Agent Memory — Research Notes

Ecosystem scan for building persistent agent memory. Covers the claude-obsidian
family (markdown-native memory plugins), jodfie/Obsidian-Memory (backend-service
style), iansinnott/aaronsb Obsidian MCP plugins, and bytedance/deer-flow (full
agent harness with memory as one pillar). Includes a synthesis of open-issue
pain points and cross-cutting gaps.

---

## 1. Primary reference: AgriciDaniel/claude-obsidian

Claude Code plugin that turns an Obsidian vault into a self-maintaining,
compounding knowledge base. Based on Karpathy's "LLM Wiki" pattern: the agent
reads sources, extracts entities/concepts, cross-links them, and answers from
the vault (not training data).

### Plugin architecture

```
.claude-plugin/  plugin.json + marketplace.json
skills/          wiki (orchestrator) + wiki-ingest, wiki-query, wiki-lint,
                 save, autoresearch, canvas, obsidian-markdown,
                 obsidian-bases, defuddle
agents/          wiki-ingest.md (parallel ingestion), wiki-lint.md (health check)
commands/        /wiki, /save, /autoresearch, /canvas
hooks/           SessionStart + PostCompact (inject hot cache),
                 PostToolUse (auto-commit wiki/ + .raw/ on Write/Edit),
                 Stop (lazy context refresh, command-type to avoid infinite loops)
_templates/      Obsidian Templater templates
wiki/            concepts/, entities/, sources/, meta/
                 + index.md, log.md, hot.md, overview.md
.raw/            source docs, delta-tracked via .manifest.json hashes
```

### Key patterns worth stealing

- **Hot cache** (`wiki/hot.md`): session-spanning context summary injected via
  SessionStart hook + re-injected after compaction. Solves "agent forgets
  between sessions."
- **Append-only log** (`wiki/log.md`) + **master index** (`wiki/index.md`):
  navigation backbone. Query path is hot → index → domain sub-index →
  specific page.
- **Delta ingestion**: `.raw/.manifest.json` tracks source hashes so re-ingest
  skips unchanged files.
- **Dual MCP options**: Local REST API Obsidian plugin + `mcp-obsidian`, OR
  `@bitbonsai/mcpvault` (pure filesystem, no plugin).
- **Multi-agent bootstraps**: ships `AGENTS.md`, `GEMINI.md`,
  `.cursor/rules/*.mdc`, `.windsurf/rules/*.md`, `.github/copilot-instructions.md`
  + `bin/setup-multi-agent.sh` symlink installer.
- **Native Obsidian Bases dashboard** (v1.9.10+) primary; Dataview optional/legacy.
- **Cross-project reuse**: any other Claude Code project points its
  `CLAUDE.md` at the vault with hot→index→page read order.

---

## 2. Ecosystem map — similar works

| Project | Shape | Distinguishing idea |
|---|---|---|
| **jodfie/Obsidian-Memory** | FastAPI backend + TS MCP server + Next.js UI; SQLite → Supabase/Postgres + ElectricSQL | 13 MCP tools (mem_read/write/search/supersede, graph_traverse/similar, project/session/context); **composite decay scoring** (`decay_class`, `confidence`, `score_breakdown`); Docker + Cloudflare Access OAuth; remote server serves many Claude Code machines; `memory://` URIs |
| **iansinnott/obsidian-claude-code-mcp** | Obsidian plugin with embedded MCP server | **Dual transport**: WebSocket auto-discovery (Claude Code `/ide`) + HTTP/SSE (Claude Desktop via `mcp-remote`); split tool registry (shared / IDE-only / MCP-only); port 22360 per vault |
| **aaronsb/obsidian-mcp-plugin** | Obsidian plugin | Most mature CC-adjacent plugin (288 stars); semantic graph; Dataview integration |
| **omega-memory/omega-obsidian** | MCP server | Persistent semantic memory layer over vaults |
| **AdrianV101/obsidian-pkm-plugin** | Obsidian plugin | Structured per-project memory layer for Claude Code |
| **kengio/onebrain** | Local AI OS | Vault-native markdown KB with persistent memory |
| **AdamTylerLynch/obsidian-agent-memory-skills** | Claude Code skill pack | Skill-only variant of the pattern |
| **jrcruciani/obsidian-memory-for-ai** | Guide + template | Pattern documentation + scaffolding |
| **MCPVault (`@bitbonsai/mcpvault`)** | Filesystem MCP bridge | No Obsidian plugin needed; talks to vault files directly |
| **KIOKU → Claude Desktop (`.mcpb`)** | Packaged desktop memory system | One-drag install, Obsidian-backed memory pipeline |
| **Mem0 Claude Code** | Cloud-backed memory plugin | Non-Obsidian alternative; cloud memory vs. local markdown |

### Convergent patterns across the ecosystem

1. **Markdown is the DB.** Vault files are source-of-truth; any index/SQLite is a
   cache. Gives git versioning, human-editable, portable, searchable with `rg`.
2. **Hot cache + session hooks.** Inject a compact recent-context file at
   SessionStart and PostCompact so the agent wakes up knowing "where we left off."
3. **Auto-commit on write.** PostToolUse hook `git add && git commit` after each
   vault write = free history + undo.
4. **Structured page types.** `concepts/`, `entities/`, `sources/` (plus `meta/`)
   with consistent frontmatter enables graph view, Dataview/Bases dashboards, and
   reliable agent queries.
5. **Lint as a first-class verb.** Orphans, dead wikilinks, stale claims, missing
   cross-refs, undercited entities — the agent gardens the vault.
6. **Ingest → extract → cross-link → log.** Each ingest creates 8–15 pages,
   updates index, appends to log, refreshes hot cache.
7. **MCP over REST vs. filesystem.** Two viable interfaces: Obsidian Local REST
   API plugin (richer, plugin-aware) or pure filesystem bridge (simpler, no
   plugin dependency).
8. **Multi-depth query.** Quick (hot.md only, ~1500 tokens) / Standard (3–5
   pages) / Deep (full wiki + optional web search).
9. **Autonomous research loop.** Configurable `program.md` governs source prefs,
   confidence rules, max rounds/pages; agent runs search → fetch → synthesize → file.
10. **Decay/recency scoring** (Obsidian-Memory) as an alternative to hot-cache
    for relevance — fields like `decay_class`, `confidence`, `score_breakdown`
    exposed via MCP search API.
11. **Per-project vs. shared vault.** Obsidian-Memory isolates by project with
    `project_switch`; claude-obsidian encourages one shared vault that any
    project's `CLAUDE.md` references.
12. **Plugin + skill + hook + command as separate concerns.** Skills describe
    capabilities, commands are entrypoints, agents handle parallelism, hooks
    handle lifecycle.

---

## 3. bytedance/deer-flow — the adjacent full-harness reference

63.3k-star "SuperAgent harness" for long-horizon tasks (minutes to hours).
Built on **LangGraph + LangChain**, Python backend + Next.js frontend.
Not a plugin or memory system alone — it's a full runtime.

### Six pillars

1. **Skills** — Markdown `SKILL.md` capability modules, **loaded progressively**.
   Built-ins: research, report-generation, slide-creation, web-page,
   image-generation. Custom skills via `.skill` archives installed through
   Gateway; optional frontmatter for `version` / `author` / `compatibility`.
   Paths: `/mnt/skills/public/` and `/mnt/skills/custom/`.
2. **Tools** — core toolset (web search/fetch, file ops, bash) + MCP servers +
   Python functions.
3. **Sub-agents** — lead agent decomposes tasks, spawns parallel sub-agents with
   **scoped/isolated contexts**, structured result aggregation.
4. **Sandbox & filesystem** — each thread gets
   `/mnt/user-data/{uploads,workspace,outputs}`. `AioSandboxProvider`
   (Docker/K8s with PVC) or `LocalSandboxProvider` (per-thread host dirs,
   bash disabled by default).
5. **Long-term memory** — local persistent profile/preferences/accumulated
   knowledge across sessions; dedup on apply; "Settings > Memory" in the UI.
6. **Message Gateway** — Telegram/Slack/Discord channels, each chat thread
   mapped to a DeerFlow thread.

### Notable extras

- **Context engineering**: isolated sub-agent contexts, aggressive summarization,
  filesystem offload, strict tool-call recovery for reasoning models.
- **Self-evolving skills**: `skill_manage` flow lets the agent create/modify
  its own skills (PR #1874).
- **`claude-to-deerflow` skill**: `/claude-to-deerflow` command in Claude Code
  to fire DeerFlow tasks (`flash` | `standard` | `pro` | `ultra` modes).
- **Embedded Python client** (`DeerFlowClient`) returning the same schemas
  as the HTTP Gateway API.
- **LangSmith + Langfuse** tracing.
- **Gateway-only mode** (experimental) collapses 4 processes → 3 by embedding
  agent runtime.

### Overlap with claude-obsidian ecosystem

Concepts that converged independently:
- Markdown as the skill/capability format (SKILL.md).
- Progressive context (load only what the task needs).
- Sub-agents with isolated contexts for parallel work.
- Some form of session-spanning memory (hot cache or deduped persistent store).
- MCP + Claude Code as standard integration points.
- Local-first, user-owned data.

The real difference:
- **claude-obsidian** answers: *"How do I give an existing agent persistent,
  human-editable knowledge?"* The vault is the product.
- **Obsidian-Memory** answers: *"How do I give a fleet of Claude Code machines a
  shared, queryable memory backend?"* The server is the product.
- **deer-flow** answers: *"How do I run the whole agent — sandbox, subagents,
  tools, memory, UI, channels — from one harness?"* The runtime is the product.

They are **complementary, not competitive**. deer-flow could use a
claude-obsidian-style vault as one of its memory backends. claude-obsidian users
could offload long-horizon work to a deer-flow instance via the
`claude-to-deerflow` skill.

---

## 4. Open-issue analysis — what each project is missing

### 4.1 deer-flow (479 open issues) — the biggest public backlog

**Memory module (the single largest gap — laid out in `#2450` Memory Module Roadmap):**

| # | Problem | State |
|---|---|---|
| P1 | Write path not multi-worker safe. In-process `list` queue + `threading.Timer`, single JSON file, up to 4 concurrent writers from `ThreadPoolExecutor`, mtime-based cache has TOCTOU window → stale reads silently overwrite newer writes. | RFC #2283 + PR #2403 open, not merged |
| P2 | Retrieval is confidence-first — packs facts into token budget by confidence desc, **ignores the query**. Every prompt gets identical ranking. | Not started |
| P3 | No retrieval observability — no per-injection trace, no replay, no baseline eval. Any ranking change is guesswork. | RFC #1908, PRs #1910/#2196/#2233 all open, none merged |
| P4 | Schema scattered across **7+ files** (storage default, prompt template, updater, API request/response, frontend types, frontend routes). `sourceError` already diverged. | Not started |
| P5 | Facts are a JSON blob, not a table — no per-fact ID, no audit, no indexing beyond linear scan. | Blocked on P1 |
| P6 | Correction/reinforcement signals detected by hardcoded regex (11 correction + 13 reinforcement patterns, EN/ZH only). Drives `confidence ≥ 0.95` — **false positives create high-confidence wrong facts**. No eval. | Not started |
| P7 | Upload-mention scrubbing is a regex baked into `updater.py`, not a pluggable filter. | Blocked on P5 |

Also open: hierarchical memory (Q2 roadmap 🔥🔥🔥🔥🔥, RFCs #1590 #1620), TTL /
importance scoring, duplicate-fact accumulation across sessions.

**Skill self-evolution** (`#2437` RFC): skills are only created manually today.
Proposal adds a `SkillReviewMiddleware` that spawns a background `SkillReviewer`
agent after `creation_nudge_interval` (default 10) tool-call rounds,
auto-extracting reusable skills from conversations. **Not merged.** Also
missing: skill auditing, "Skills Hub" marketplace, `.skill` archive signing /
provenance.

**Context engineering bugs (live right now):**
- Skill files get compressed during context compaction (#2452) — so critical
  instructions vanish.
- `_apply_prompt_caching` exceeds Anthropic's 4-breakpoint cache limit in
  multi-turn (#2448).
- Invalid YAML frontmatter in some SKILL.md files fails silently (#2443).
- `ask_clarification` tool drops prior conversation history (#2425).
- Checkpoint ID reuse collapses thread history (#2392).

**Runtime/infra gaps:**
- Airgapped/internal deployment story missing (#2435).
- Proposed **layered refactor** — runtime kernel / storage package / plugin
  system (#2429) — not done.
- Lead-agent caches `AsyncHttpxClient` across sub-agent event loops →
  `RuntimeError: Event loop is closed` (#2405).
- Docker Sandbox fails on Windows (#2416), mounts permission issues (#2423),
  Docker sandbox file-create failures (#2438).
- No web UI for model management (#2401).
- MCP server calls flaky on long sessions (#2397, #2399).
- Frontend laggy on long conversations (#2396).
- Image-vision pipeline broken (#2439).

**Enterprise (Q2 roadmap):**
- RBAC (#1721), multi-tenant, SSO (#1981 in-flight), sandbox hardening
  (#1808, #1881), MCP tool-call auditing (#1240).
- Channels still missing: Matrix (#1869), DingTalk (#1802), JIRA (#1514),
  WeCom (#1390). Cron scheduler (#1092).
- No stable release yet, docs website and 30-min onboarding goal not met,
  benchmarks missing.

### 4.2 claude-obsidian (10 issues, 9 open)

Early-stage; gaps are structural, not feature-sprawl:

- **Installation is brittle.** `setup-vault.sh` doesn't move `commands/`,
  `skills/`, `hooks/` into `.claude/` so slash commands aren't discovered on a
  fresh `git clone` (#2, #11).
- **Auto-commit is too chatty.** PostToolUse commits per tool call, not per
  user turn (#12) — noisy git history.
- **Hook plumbing brittle.** `SessionStart:resume` and `PostCompact` prompt-type
  hooks fail with "ToolUseContext is required" (#7) — was fixed in v1.4 but
  still being reported; architectural rework needed.
- **`.raw` folder invisible in Obsidian.** Dot-prefix hides it — breaks Web
  Clipper integration since users can't drag files there from Obsidian UI (#5).
- **No scaling guidance.** "How large the wiki is acceptable?" — no answer, no
  benchmarks (#6).
- **No proactive learning trigger.** User has to say "ingest X" — no passive
  background learning (#1).
- **Trust/maintenance concerns.** Rapid-fire v1.4.x releases + the author's
  model-generated commit messages triggered "is this repo legit?" (#16) and
  "maintenance of this repo" (#14).
- Implicit: no multi-vault / multi-project tenancy; no conflict resolution when
  two sessions edit the same page; no API layer that other agents can hit.

### 4.3 jodfie/Obsidian-Memory (1 open issue, 1 star)

Solo project. The one open issue (#37) is a self-reported bug:
`search_by_entity` crashes with `no such column: n.path` when no vault is
configured. Gap: **vault configuration bootstrap is not robust**, error paths
assume vault exists. More broadly: despite heavy infra (FastAPI + MCP +
Next.js + Supabase + ElectricSQL + Docker + Cloudflare Access + K8s OAuth),
zero external adoption signal — the ambition:users ratio is inverted.

### 4.4 iansinnott/obsidian-claude-code-mcp (7 open issues, 258 stars)

- **Security hole (#12):** WebSocket and HTTP servers accept connections from
  **any origin** — anything on localhost (including browsers visiting a
  malicious page) can invoke vault tools. No origin validation.
- **Not in Obsidian community plugins list (#2).** Manual install only.
- **Windows terminal broken (#10, #14):** embedded Claude Code window opens but
  ignores typing/pasting — Windows effectively unsupported.
- **Single-terminal limit (#5):** can't run more than one Claude Code session
  per vault.
- **Hardcoded timeouts (#8)** on `obsidian_api` tool.
- **Vault-locked IDE (#3):** Claude Code can only connect when launched from
  inside the vault dir — no way to use another project's workspace while still
  reading the vault.
- **Last commit: June 2025** — effectively unmaintained.

### 4.5 aaronsb/obsidian-mcp-plugin (288 stars, 10 open issues) — the most mature competitor

- **Silent write races (#139):** parallel `window`/`append`/`patch` calls to the
  same file race without locking or serialization — data loss possible.
- **Stale MCP spec (#134):** doesn't support MCP Streamable HTTP (2025-03-26
  spec); stuck on legacy SSE.
- **Wrong default response format (#133):** returns rendered HTML instead of
  raw markdown — wastes tokens, breaks round-tripping edits.
- **Session lifecycle broken (#128, #125):** MCP session silently dropped after
  ~3h inactivity — every tool call returns "Bad Request: Server not
  initialized"; SSE reconnection loops.
- **Graph API gaps (#132):** `graph.statistics` requires `sourcePath`, can't
  query vault-wide.
- **Dataview integration broken (#123, #115).**
- **Config friction (#135):** API key only readable from `data.json`, no env
  var support.

### 4.6 AdamTylerLynch/obsidian-agent-memory-skills

0 issues, 30 stars. Either well-scoped or no users stressing it.

---

## 5. Cross-cutting gaps — what every project is missing

### The seven holes every team in this space would pay for

1. **A durable, single-writer, crash-safe memory store** with queue + writer
   lease + migration path. deer-flow's `#2283` is the template, not yet shipped
   anywhere. aaronsb races silently on parallel edits (#139). claude-obsidian
   spams git per tool call (#12). Obsidian-Memory crashes when vault missing (#37).

2. **Query-aware retrieval with observability.** Per-injection traces, replayable
   decisions, and a confidence-only baseline to beat. deer-flow is explicitly
   confidence-first and blind to query (P2/P3). claude-obsidian has no scoring
   at all. Obsidian-Memory has decay scoring but no eval harness.

3. **Normalized fact/note table.** Stable IDs, per-fact audit, access metadata,
   access-aware eviction. Only Obsidian-Memory comes close with its SQL model;
   no one has the eval layer.

4. **Declarative shared schema** generating backend types, API contracts, and
   frontend types. deer-flow has 7+ drifted definitions (P4). Every project
   has fragmented schemas.

5. **Compaction-safe skill loading.** Skills must survive context compaction
   intact. deer-flow currently compresses them (#2452). claude-obsidian's
   PostCompact hook is flaky (#7).

6. **Origin/auth layer for MCP servers** safe to expose beyond loopback.
   iansinnott accepts any origin (#12). deer-flow RBAC/SSO/multi-tenant are
   in-flight. aaronsb has only `data.json` API key. No project is safe beyond
   127.0.0.1 today.

7. **Self-evolving skills / auto-extracted capabilities** from conversation
   tool-call patterns. deer-flow RFC `#2437`, nothing in production anywhere.

### Bonus: skill packaging

No unified format yet for skill archives. deer-flow's `.skill` format with
`version`/`author`/`compatibility` frontmatter is the start. Claude Code plugins
are another. Cross-vendor registry is missing.

### Signal detection for memory facts

deer-flow: correction/reinforcement detection is 24 hand-written regex patterns
(EN+ZH only) driving 0.95 confidence on matches (P6). All other projects: no
signal detection at all. NO PROJECT reliably distinguishes "user corrected me"
from "user mentioned X."

---

## 6. Opportunity sizing

- **deer-flow**'s backlog says "our memory module is structurally broken, we
  know it, we're fixing it in 4 waves." That roadmap **is** the spec for what
  production agent memory should look like — open-source, sequenced,
  dependency-mapped.
- **claude-obsidian** is already feature-complete for its scope but brittle at
  the edges (install, hooks, auto-commit cadence, `.raw` visibility). Easy
  wins exist for a fork or replacement.
- **aaronsb's plugin** is the most-used Obsidian MCP today and has the most
  surface-area bugs — a clean rewrite with MCP 2025-03-26 + raw-markdown-default
  + per-file write locks would immediately capture its users.
- **iansinnott's plugin** is unmaintained (last commit Jun 2025) and has a
  security hole — an origin-safe fork would be a lift-and-shift opportunity.
- Nobody has shipped retrieval observability, signal-detection quality, or
  self-evolving skills — these are the frontier.

### Two architectural templates to pick from

| Dimension | Plugin-style (claude-obsidian) | Service-style (Obsidian-Memory / deer-flow) |
|---|---|---|
| Stack weight | Shell + markdown + hooks | Python + FastAPI/LangGraph + Docker/K8s + frontend |
| Memory surface | User's Obsidian vault (human-browsable) | Thread-scoped store or SQL-backed service |
| Who edits it | Human-first (Obsidian IDE) | Mostly agent; UI exposes settings |
| Distribution | CC plugin marketplace + clone-as-vault | Server + UI you host |
| Orchestration | Claude Code native skill/agent runtime | LangGraph state machine w/ explicit subagents |
| Typical task | Seconds to minutes (ingest, query, lint) | Minutes to hours (multi-agent, file-producing) |

### Short version

- **Ship write-path correctness first.** Durable queue, writer lease, crash recovery.
- **Make retrieval measurable before making it smarter.** Traces and replay
  come before query-aware scoring.
- **Bundle schema consolidation with fact normalization.** Do it once, cleanly,
  across backend + API + frontend.
- **Pick one wave and ship it cleanly.** The rest of the ecosystem will migrate
  toward whoever does.

---

## 7. 2026 research landscape (arXiv / alphaXiv scan)

Every ecosystem gap identified in §5 has a 2026 paper attached. The research has
formalized what the production systems are stumbling toward. The fact that the
production systems haven't caught up is the opportunity.

### 7.1 Canonical 2026 surveys (read these first)

1. **Memory in the Age of AI Agents** (`2512.13564`, Dec 2025, NUS + Tongji + Fudan
   + Renmin + Peking + NTU) — three-dimensional taxonomy:
   **Forms** (token / parametric / latent × flat / planar / hierarchical) ×
   **Functions** (factual / experiential / working) ×
   **Dynamics** (formation / evolution / retrieval). Current reference frame.
2. **Memory for Autonomous LLM Agents: Mechanisms, Evaluation, and Emerging
   Frontiers** (`2603.07670`, Mar 2026).
3. **The AI Hippocampus: How Far are We From Human Memory?** (`2601.09113`,
   Jan 2026, Peking + BIGAI) — biological grounding.
4. **Graph-based Agent Memory: Taxonomy, Techniques, and Applications**
   (`2602.05665`, Feb 2026) — graph-memory sub-field survey.
5. **Adaptation of Agentic AI: A Survey of Post-Training, Memory, and Skills**
   (`2512.16301`, Dec 2025).

### 7.2 Gap-to-paper map

**Gap 1 — Durable write path / multi-writer safety.** No paper solves it
directly (engineering problem), but the architectural reframing is here:
- **EverMemOS** (`2601.02163`) — three-phase lifecycle (Episodic Trace Formation
  → Semantic Consolidation → Reconstructive Recollection) makes write-path
  correctness a design choice, not an afterthought.
- **MemFactory** (`2603.29493`) — unified inference + training framework.
- **SSGM / Stability and Safety Governed Memory** (`2603.11768`) — governance
  with rollback and consistency.

**Gap 2 — Query-aware retrieval with observability and eval.** Entire
sub-field pivoted here late 2025 / early 2026:
- **SwiftMem: Fast Agentic Memory via Query-aware Indexing** (`2601.08160`) —
  literally named for the gap.
- **MemR³: Memory Retrieval via Reflective Reasoning** (`2512.20237`) —
  retrieval as reasoning, not cosine similarity.
- **Diagnosing Retrieval vs. Utilization Bottlenecks** (`2603.02473`) —
  separates write-side quality from retrieval quality. Exactly deer-flow P3.
- **Learn to Memorize** (`2508.16629`, Huawei Noah's Ark) — adaptive memory.
- **MemBench / MemoryArena / AMA-Bench / KnowMe-Bench** (all 2026) — four new
  benchmarks that give you the baselines deer-flow's Wave 2 needs.
- **How Memory Management Impacts LLM Agents** (`2505.16067`) — first real
  empirical eval of experience-following behavior.

**Gap 3 — Normalized fact table / schema.** Research has formalized what
production stumbles toward:

| Paper | Schema |
|---|---|
| **EverMemOS** (`2601.02163`) | **MemCell** = `(E: Episode, F: {atomic facts}, P: Foresight with [t_start, t_end] validity, M: Metadata)`. MemScenes cluster MemCells via incremental semantic clustering. *Literally the normalized fact table deer-flow P5 needs.* |
| **Text2Mem** (`2509.11145`) | Unified memory operation language — single declarative schema across layers. Directly addresses P4. |
| **Kumiho / Graph-Native Cognitive Memory** (`2603.17244`) | Formal belief revision semantics + versioned memory architectures. |
| **MemWeaver** (`2601.18204`) | Hybrid memories with traceability for multi-hop reasoning. |
| **A-MEM: Agentic Memory** (`2502.12110`, Ant + Rutgers + Salesforce) | Zettelkasten-inspired, note linking + evolution. |

**Gap 4 — Compaction-safe skills / evolving context.** ByteDance research is
ahead of ByteDance product:
- **Context-Folding** (`2510.11967`, CMU + Stanford + ByteDance) — folds working
  memory on demand; sub-contexts summarized and re-hydrated.
- **ACE: Agentic Context Engineering** (`2510.04618`) — contexts as evolving
  **playbooks** with **delta updates** to prevent collapse; Generator / Reflector
  / Curator roles.
- **COMPASS** (`2510.08790`) — evolving context for long-horizon reasoning.
- **ACON** (`2510.00615`, KAIST + Cambridge + Microsoft) — context compression
  optimized for agents.
- **ContextBudget** (`2604.01664`) — budget-aware management.
- **InfiAgent** (`2601.03204`) — infinite-horizon framework.
- **Context as a Tool** (`2512.22087`) — for SWE-agents specifically.

**Gap 5 — Origin-safe auth / multi-tenant memory.** Mostly engineering, but
starting:
- **Collaborative Memory: Multi-User Memory Sharing with Dynamic Access Control**
  (`2505.18279`, Accenture).
- **MemCollab: Cross-Agent Memory Collaboration** (`2603.23234`) — contrastive
  trajectory distillation.
- **VerificAgent: Domain-Specific Memory Verification for Scalable Oversight**
  (`2506.02539`, Microsoft) — filters **unsafe heuristics** from learned
  memories. Addresses deer-flow P6 (regex-driven 0.95-confidence false positives).

**Gap 6 — Self-evolving skills from conversation traces.** The hottest
sub-area of 2026. deer-flow's `#2437` RFC has at least a dozen competing
academic implementations:

| Paper | Date | Mechanism |
|---|---|---|
| **AutoSkill** (`2603.01145`) | Mar 2026 | Extracts SKILL.md artifacts from **user queries** (not model responses). Judge decides add/merge/discard. Versioned merging. Hybrid BM25 + dense retrieval. Shanghai AI Lab. |
| **Memp: Procedural Memory** (`2508.06433`) | Aug 2025 | Distills trajectories into step-by-step instructions + script abstractions. Validation filtering + in-place adjustment on errors. Alibaba + Zhejiang. |
| **Trace2Skill** (`2603.25158`) | Mar 2026 | Sub-agents analyze trajectory pool in parallel. |
| **CoEvoSkills** (`2604.01687`) | Apr 2026 | References **Anthropic's skills concept**; co-evolutionary verification. |
| **SkillClaw** (`2604.08377`) | Apr 2026 | Skills evolve collectively across agents. |
| **SkillX** (`2604.04804`) | Apr 2026 | Auto-constructs skill knowledge bases. |
| **MemSkill** (`2602.02474`) | Feb 2026 | Learns *memory skills* (meta-skills for what to store). |
| **Memento-Skills** (`2603.18743`) | Mar 2026 | "Agent-designing agent." |
| **SkillRL** (`2602.08234`) | Feb 2026 | RL-based recursive skill-augmented learning. |
| **FLEX** (`2511.06449`) | Nov 2025 | Continuous evolution via forward learning (ByteDance + Tsinghua). |
| **LEGOMem** (`2510.04851`) | Oct 2025 | Microsoft's modular procedural memory for multi-agent workflows. |
| **RL for Self-Improving Agent with Skill Library** (`2512.17102`) | Dec 2025 | RL-based skill library growth. |
| **ReasoningBank** (`2509.25140`) | Sep 2025 | Reasoning-memory for self-evolution. |

**Gap 7 — Meta-architecture / self-evolving memory OS.** Three big swings:
- **MemEvolve** (`2512.18746`, OPPO + LV-NUS) — **bilevel optimization**:
  inner loop evolves experience, outer loop evolves **memory architecture itself**.
  The system designs its own storage.
- **EverMemOS** (`2601.02163`) — self-organizing memory OS.
- **Hyperagents** (`2603.19461`) — self-improving systems that learn to improve
  their own learning.
- **MetaClaw** (`2603.17187`) — meta-learning agent evolving in the wild.
- **Meta-Harness** (`2603.28052`) — end-to-end optimization of the entire model
  harness (code that decides what info goes in context).

### 7.3 Genuinely novel 2026 ideas not in any production system

1. **Foresight as a first-class memory field.** EverMemOS's MemCell includes
   `P` (Foresight) — forward-looking inferences with explicit
   `[t_start, t_end]` validity intervals. Not "what happened" but "what should
   happen next, until when." claude-obsidian / Obsidian-Memory / deer-flow all
   store only past observations.
2. **Bilevel memory evolution.** MemEvolve: inner loop updates contents, outer
   loop evolves the storage/retrieval architecture itself. deer-flow's Memory
   Module Roadmap proposes 4 waves of human-driven refactoring; MemEvolve says
   let the agent do it.
3. **Write quality vs retrieval quality separated.** Most systems only measure
   end-to-end task accuracy — you can't tell if your memory is bad or your
   retriever is bad. `2603.02473` gives the split eval.
4. **Belief revision semantics for memory.** Kumiho applies formal belief
   revision (from knowledge representation) to versioned memory. When a new
   fact contradicts an old one, there's a principled update rule. Closest
   production analog: claude-obsidian's `[!contradiction]` callouts, but those
   are display-only.
5. **Skills extracted from user queries, not model responses.** AutoSkill's
   extraction evidence is user queries because they reveal stable preferences,
   while model responses reveal model behavior. Opposite of what deer-flow's
   proposed `SkillReviewMiddleware` does.
6. **Memory as a computer architecture problem.** `2603.10062` frames memory
   hierarchies (L1/L2/L3 cache analogy), coherence protocols for shared memory
   between agents, cache-line-like eviction. Nobody has implemented this yet.
7. **Recursive Language Models.** `2512.24601`: LLMs recursively invoke
   themselves to process arbitrarily long prompts. Alternative to hot-cache /
   compaction strategies entirely — "context window is irrelevant if you can
   decompose the prompt."

### 7.4 Still missing even from research

1. **Multi-writer TOCTOU race** deer-flow P1 is treated as engineering, not
   research.
2. **Cross-vendor skill archive format / signing / provenance.** Anthropic
   skills, deer-flow `.skill`, Claude Code plugins — all siloed.
3. **Hook-based lifecycle injection theory.** SessionStart / PostCompact
   hot-cache injection is a production pattern without formal grounding.
4. **Human-editable memory** — the "vault is the product" thesis. All papers
   assume memory is agent-private.
5. **Markdown-native agent memory as substrate.** Papers default to vector DB
   + JSON blob + graph DB. Obsidian-flavored markdown + wikilinks as the
   storage layer is a production-only idea.
6. **Git-native versioning as memory substrate** (claude-obsidian's PostToolUse
   auto-commit) has no academic evaluation.

### 7.5 Strategic reading order for someone building in this space

1. **§7.1 surveys** for the taxonomy and language.
2. **EverMemOS** (`2601.02163`) for the schema (MemCell/MemScene).
3. **AutoSkill** (`2603.01145`) for the SKILL.md extraction pattern.
4. **Diagnosing Retrieval vs Utilization** (`2603.02473`) for eval methodology.
5. **ACE** (`2510.04618`) + **Context-Folding** (`2510.11967`) for compaction.
6. **MemEvolve** (`2512.18746`) for the meta-architecture horizon.
7. **VerificAgent** (`2506.02539`) for safety / signal detection.
8. **Collaborative Memory** (`2505.18279`) for multi-user / multi-agent.

### 7.6 The TL;DR shift

The 2026 research has already solved most of the gaps the production ecosystem
flags. The leverage is not in *discovering* what to build — it's in *shipping*
the known-good architectures (MemCell schema, query-aware retrieval with trace
logs, SKILL.md auto-extraction, playbook context engineering) in a
production-ready way with the engineering primitives the papers don't care
about: durable writes, origin-safe auth, schema codegen, compaction survival,
git-native versioning, human-editable storage.
