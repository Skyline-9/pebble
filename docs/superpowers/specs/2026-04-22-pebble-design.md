# Pebble — Design Spec

> Cross-platform agent memory that makes Claude Code and Factory Droid feel
> super personal. Event-sourced substrate, three surfaces (profile + skills +
> knowledge wiki), shared between both platforms, human-editable in Obsidian.

**Status:** Approved (awaiting final spec review before implementation plan)
**Date:** 2026-04-22
**Author:** design session driven by research in `.factory/memories.md`
**Name origin:** small, persistent, accumulates — event log = pebbles stacking up

---

## 1. Problem statement

Agents today are stateless between sessions. The ecosystem (claude-obsidian,
Obsidian-Memory, deer-flow, iansinnott/aaronsb plugins) converges on markdown
vaults + MCP + hooks, but every production system is missing at least one of:

- durable, multi-writer-safe write path
- query-aware retrieval with traces / replay / offline eval
- normalized fact schema with per-fact audit
- compaction-safe skill loading
- origin-safe auth for MCP
- auto-extracted skills from conversation traces
- belief revision / principled invalidation

The 2026 research has answers (EverMemOS MemCell schema, AutoSkill extraction
judge, SwiftMem query-aware indexing, Kumiho belief revision, ACE playbook
context, VerificAgent safety checks, MemEvolve bilevel architecture
evolution). No production system has shipped any of them cleanly.

**Pebble ships the known-good research architecture for Claude Code and
Factory Droid on a single shared substrate.**

## 2. Goals and non-goals

**Goals:**

- One memory, three surfaces: style profile, skill library, knowledge wiki.
- Works in Claude Code AND Factory Droid, using each platform's native features.
- Event-sourced: JSONL log source of truth, SQLite projection, markdown view.
- Human-editable vault in Obsidian; user edits flow back as events.
- Query-aware retrieval with per-turn trace logs (observability from day 1).
- Auto-extract skills from user queries (AutoSkill pattern); user can also save
  explicitly.
- Principled invalidation: foresight expiry, judge supersession, user retract,
  access-aware eviction, type-aware TTL, contradiction surfacing, stale
  sweeper.
- Git-native history: one commit per user turn.

**Non-goals (v1):**

- Cloud sync / multi-device CRDT sync. Git baseline only.
- Team/multi-user memory. Single user per vault.
- Obsidian plugin. We talk to the vault as files; Obsidian loads them as
  normal markdown.
- Own ML model training. We use whatever LLM the user has configured.
- Marketplace distribution for Factory Droid before Factory ships a
  marketplace.

## 3. Architecture overview

```
                    ┌──────────────────────────────┐
                    │        persona-mcp           │
                    │    (single shared binary)    │
                    │                              │
                    │  MCP tools:                  │
                    │    memory_* skill_*          │
                    │    profile_* trace_*         │
                    │    verify_fact               │
                    │                              │
                    │  File watchers (user edits)  │
                    │  Cron (foresight/stale)      │
                    └──────┬────────────┬──────────┘
                           │            │
              ┌────────────▼─┐       ┌──▼───────────┐
              │  Claude Code │       │ Factory Droid│
              │    plugin    │       │    plugin    │
              │ skills/      │       │ droids/      │
              │ agents/      │       │ skills/      │
              │ commands/    │       │ commands/    │
              │ hooks/       │       │ hooks/       │
              └──────────────┘       └──────────────┘

                           │            │
                           └────┬───────┘
                                ▼
                        ~/.pebble/
                        ├── log.jsonl          ← source of truth (append-only)
                        ├── projection.db      ← SQLite: cells, facts, scenes,
                        │                        skills, events, FTS5, vec
                        ├── checkpoints/
                        │   └── NNNNNN.db      ← projection snapshot every 500ev
                        ├── vault/             ← human view (Obsidian-openable)
                        │   ├── profile.md
                        │   ├── scenes/*.md
                        │   ├── skills/*.md
                        │   ├── _contradictions.md
                        │   ├── _foresight.md
                        │   └── _index.md
                        ├── trace.jsonl        ← per-turn retrieval traces
                        └── .git/              ← turn-batched commits
```

## 4. Data model

### 4.1 MemCell (atomic unit)

EverMemOS-inspired (`2601.02163`), extended with confidence, evidence, access,
supersession fields.

```typescript
type MemCell = {
  id: string;           // "mc_<ulid>"
  type: "profile" | "preference" | "project" | "episodic"
      | "skill" | "transient";

  E: string;            // Episode: third-person narrative, semantic anchor
  F: AtomicFact[];      // Atomic facts: verifiable statements
  P?: Foresight;        // Forward-looking inference with validity
  M: Metadata;          // created_at, source, actor, thread_id, project_id

  confidence: number;              // 0.0 - 1.0
  evidence: Evidence[];            // refs to source events
  scene_ids: string[];             // which MemScenes this belongs to
  access: AccessStats;             // { count, last_at, last_query_hash }
  supersedes?: string[];           // parent cell ids
  superseded_by?: string;          // child cell id
  retracted_at?: string;           // null if active
};

type AtomicFact = {
  subject: string;      // "user.stack.frontend"
  predicate: string;    // "is"
  object: string;       // "Solid"
  confidence: number;
};

type Foresight = {
  inference: string;
  t_start: string;      // ISO8601
  t_end?: string;
  status: "active" | "expired" | "fulfilled" | "invalidated";
};

type Evidence = {
  event_id: string;
  kind: "user_query" | "tool_result" | "file_read"
      | "url_fetch" | "user_edit";
  excerpt?: string;
};
```

### 4.2 MemScene (cluster)

```typescript
type MemScene = {
  id: string;           // "ms_<slug>"
  label: string;
  description: string;
  cell_ids: string[];
  centroid?: number[];  // embedding for incremental clustering (V1+)
  created_at: string;
  updated_at: string;
};
```

### 4.3 Skill (MemCell of type="skill")

```typescript
type Skill = MemCell & {
  type: "skill";
  skill: {
    name: string;                    // slug
    description: string;             // trigger-matching one-liner
    trigger_phrases: string[];
    body: string;                    // SKILL.md body
    allowed_tools?: string[];        // CC frontmatter compat
    version: string;                 // semver
    compatibility: string;           // "claude-code>=1.0 OR factory-droid>=0.5"
    source_events: string[];         // conversation events that extracted this
  };
};
```

### 4.4 Profile (singleton, derived from profile-type cells)

```typescript
type Profile = {
  voice: {
    tone: string;
    vocabulary_dos: string[];
    vocabulary_donts: string[];
    examples: string[];
  };
  stack: {
    primary_langs: string[];
    frameworks: string[];
    tools: string[];
    never_use: string[];
  };
  conventions: {
    commit_style: string;
    code_style: string;
    test_style: string;
    doc_style: string;
  };
  goals: Foresight[];  // active foresight items
  updated_at: string;
};
```

### 4.5 Event types (log.jsonl)

Every event: `{ v, ev, id: ULID, actor, ts, ...payload }`.

| type | payload | actor | emitted by |
|---|---|---|---|
| `assert` | `cell_id`, `cell` | `reviewer` \| `user` \| `system` | reviewer or /remember |
| `supersede` | `target`, `by`, `reason` | `judge` \| `user` | judge on contradiction |
| `retract` | `target`, `reason` | `user` \| `system` | /forget or stale sweep |
| `expire` | `target`, `reason` | `system` | foresight sweeper |
| `contradict` | `a`, `b`, `resolution` | `judge` | judge when flag_both |
| `touch` | `target`, `query` | `system` | retrieval hit |
| `correct` | `target`, `diff` | `user` | user edit in vault |
| `user_edit` | `cell_id`, `diff` | `user` | file watcher |
| `checkpoint` | `at_seq`, `db_hash` | `system` | every 500 events |

### 4.6 SQLite projection schema

```sql
CREATE TABLE cells (
  id TEXT PRIMARY KEY, type TEXT, json BLOB,
  confidence REAL, created_at INTEGER, updated_at INTEGER,
  retracted_at INTEGER, access_count INTEGER, last_accessed_at INTEGER
);
CREATE TABLE facts (cell_id TEXT, subject TEXT, predicate TEXT, object TEXT, confidence REAL);
CREATE TABLE scenes (id TEXT PRIMARY KEY, label TEXT, description TEXT, updated_at INTEGER);
CREATE TABLE scene_members (scene_id TEXT, cell_id TEXT);
CREATE TABLE foresight (cell_id TEXT, t_start INTEGER, t_end INTEGER, status TEXT);
CREATE TABLE events (id TEXT PRIMARY KEY, seq INTEGER, type TEXT, json BLOB, ts INTEGER);
CREATE VIRTUAL TABLE cells_fts USING fts5(cell_id, content);
CREATE VIRTUAL TABLE cells_vec USING vec0(cell_id TEXT PRIMARY KEY, embedding FLOAT[384]); -- V1+
```

Projector is idempotent on `event.id`. Crash-safe: replay resumes from last
checkpoint.

### 4.7 Vault render layout

```
~/.pebble/vault/
├── profile.md               # rendered from profile-type cells
├── scenes/                  # one .md per MemScene
│   ├── auth-refactor.md
│   └── climbing.md
├── skills/                  # one SKILL.md per skill-type cell (CC-compatible)
│   ├── commit-style.md
│   └── deploy-flow.md
├── _contradictions.md       # aggregated [!contradiction] callouts
├── _foresight.md            # active foresight items timeline
└── _index.md                # auto-generated dashboard (Obsidian Bases in V2)
```

Write path: projection → render. One-way at write time.
Read path: user edits watched → emit `user_edit` events back to log.

## 5. Data flow

### 5.1 Write path

```
User turn → agent tool calls → PostToolUse hook counts rounds →
  (rounds % N) ? spawn background reviewer subagent :
  Background reviewer (anti-recursion: no middlewares, memory_* + skill_* tools only):
    1. Read conversation history
    2. Extract candidates (AutoSkill: from user queries, not model responses)
    3. Read existing cells for dedup (hybrid retrieval)
    4. Emit candidate events
  Judge:
    MVP: rule-based — type-aware confidence threshold + hybrid BM25 dedup.
    V1+: small LLM call:
      For each candidate:
        1. Hybrid retrieve similar cells (BM25 + vec)
        2. Decide: assert | merge | supersede | flag_both | discard
        3. Apply type-aware confidence threshold
        4. (V2) VerificAgent safety check on identity/preference mutations
  Event writer (O_APPEND, flock):
    log.jsonl ← batch of events
  Projector (idempotent by event.id):
    Replay new events → update SQLite + FTS5 + vec indices
    Every 500 events: checkpoint snapshot
  Vault renderer (affected files only):
    Write scenes/*.md, skills/*.md
    Regenerate _contradictions.md if changed
    Regenerate _index.md (cheap)
  Stop hook (end of turn):
    git add -A && git commit -m "pebble: turn N +<asserts> -<retracts>"
```

**Invariants:**

- `O_APPEND` + `flock` on log.jsonl = multi-writer safe (deer-flow P1 fix).
- Projector idempotent on event.id = crash-safe.
- Checkpoints bound replay cost to ≤500 events (~ms).
- Git commit per user turn, not per tool call (fixes claude-obsidian #12).
- Reviewer anti-recursion: no middlewares on reviewer subagent.

### 5.2 Read path

```
SessionStart hook:
  Load hot cache from projection:
    1. Profile (merged from profile-type cells)
    2. Top-K skills (by access_count * confidence)
    3. Active foresight items (t_end not past)
    4. Last N touched MemScenes
  Inject via:
    CC: additionalSystemPromptParts output
    Droid: system prompt injection via hook

Per user turn:
  Query-aware retrieval (SwiftMem-style):
    1. Embed user message (V1+) or BM25 only (MVP)
    2. Hybrid score per cell:
         α·BM25(query, E+F) + β·cos(q_vec, cell_vec)
       + γ·recency_boost + δ·confidence
    3. Top-K (default 5), inject as [!memory] blocks
    4. Emit touch events for retrieved cells

  Trace log (observability from day 1):
    { turn, query_hash, candidates: [{id, scores}], selected, injected_tokens }

PostCompact hook:
  Re-inject profile + top skills + active foresight (not per-turn cells)
```

**Observability:** every retrieval writes to `trace.jsonl`. Retrieval can be
replayed offline, A/B tested against different scoring weights, compared to
the confidence-only baseline (deer-flow Wave 2 / `2603.02473`).

## 6. Invalidation model

Seven mechanisms on one event log:

1. **Foresight-based expiration.** Every MemCell.P has `[t_start, t_end]`.
   Hourly cron emits `expire` events for anything past `t_end`.
2. **Judge-driven supersession.** Reviewer sees near-duplicate → judge decides
   `keep_old | keep_new | merge | flag_both`. `flag_both` surfaces as
   `[!contradiction]` callout in vault.
3. **Explicit user retraction.** `/forget <query>` → `retract` events. User
   edit/delete in Obsidian → file watcher emits `retract` or `correct`.
4. **Access-aware eviction.** Compound score:
   `score = confidence * exp(-λ * age_days) * log(1 + access_count)`.
   When active cells exceed cap (1000), bottom 5% retracted (still in log).
5. **Type-aware TTL defaults:**
   - `profile`: no auto-expire, manual supersede only
   - `preference`: no auto-expire, supersede on contradiction
   - `project`: `t_end = now + 30d` default, refresh on touch
   - `episodic`: never expire, pure access-decay
   - `skill`: versioned, superseded via skill_save replacement
   - `transient`: `t_end = now + 7d`
6. **Contradiction surfacing.** `flag_both` renders `[!contradiction]` in scene
   markdown with sources, dates, `/resolve mc_a mc_b` hint.
7. **Stale-fact sweeper (V2).** Weekly: `project|preference` cells untouched
   >60d flagged for one-time user review at SessionStart. Capped at 3/session.

**Principle:** nothing is deleted. The log is append-only. The projection
filters. You can `git blame` why your agent forgot something.

**Anti-pattern avoided:** no regex-based signal detection at write time
(deer-flow P6). Assert liberally, invalidate lazily via judge + user review.

## 7. Platform surfaces

### 7.1 Shared core

`pebble-mcp` (TypeScript, single binary via `bun build --compile` or `pkg`):

- MCP tools: `memory_assert`, `memory_supersede`, `memory_retract`,
  `memory_touch`, `memory_query`, `memory_read_cell`, `profile_read`,
  `profile_update`, `skill_save`, `skill_list`, `skill_read`, `trace_read`,
  `verify_fact`.
- File watchers: fsnotify on `vault/*.md` → emit `user_edit` events.
- Cron workers: foresight sweep (hourly), stale sweep (weekly, V2).
- Storage: `~/.pebble/` (single user, single machine).

One MCP server per machine. Both CC and Droid connect to the same instance.

### 7.2 Claude Code plugin

```
.claude-plugin/
  plugin.json, marketplace.json

skills/
  pebble/SKILL.md              # orchestrator
  pebble-query/SKILL.md        # multi-depth query
  pebble-save/SKILL.md         # explicit save
  pebble-review/SKILL.md       # staging area (V1+)
  pebble-lint/SKILL.md         # vault health

agents/
  pebble-reviewer.md           # background reviewer (anti-recursion)
  pebble-judge.md              # judge subagent
  pebble-linter.md             # vault health

commands/
  pebble.md       # /pebble status
  remember.md     # /remember
  forget.md       # /forget
  recall.md       # /recall
  review.md       # /review (V1+)
  profile.md      # /profile
  resolve.md      # /resolve mc_a mc_b
  skill.md        # /skill save|list|show

hooks/hooks.json
  SessionStart(startup|resume) → hot-cache-for-cc
  PostCompact                   → hot-cache-for-cc (profile+skills+foresight)
  PostToolUse(Write|Edit|MultiEdit) → round-tick (spawn reviewer at threshold)
  Stop                          → commit-turn (git batched)
```

### 7.3 Factory Droid plugin

```
.factory/
  droids/
    pebble.droid.md              # primary droid; system prompt has hot cache
    pebble-reviewer.droid.md     # reviewer subagent via Task
    pebble-judge.droid.md        # judge subagent
  skills/pebble/
    SKILL.md (symlinked with CC)
    ... (same tree)
  commands/
    pebble, remember, forget, recall, review, profile, resolve, skill
  hooks/
    session-start.sh, post-compact.sh, post-tool-use.sh, stop.sh
  AGENTS.md                      # bootstrap

~/.factory/
  droids/pebble.droid.md         # personal-level droid
  AGENTS.md                      # personal bootstrap
```

**Factory-native leverage:** custom droids for reviewer/judge (more first-class
than CC subagents), Task tool for delegation, AGENTS.md at project + personal
levels.

### 7.4 Cross-platform behavior

- Both talk to the same `pebble-mcp` instance.
- Both read/write `~/.pebble/`.
- Skills extracted in CC show up in Droid (shared skill store).
- Git commits carry `actor: claude-code | factory-droid` metadata.
- `log.jsonl` single-writer via `flock`; batch writes milliseconds.
- Multi-device: manual `git pull` on another machine; projection rebuild from
  log. (V2 adds git-remote sync daemon.)

## 8. MVP scope

Both platforms in parallel (proves cross-platform thesis day 1).

**In scope for MVP:**

- `pebble-mcp` server with log + SQLite projection + FTS5
- Core MCP tools: `memory_assert`, `memory_query`, `memory_touch`,
  `skill_save`, `profile_read`, `profile_update`, `trace_read`
- Vault render: profile.md, scenes/*.md, skills/*.md
- CC plugin: skills, `/pebble`, `/remember`, `/forget`, `/recall`, `/profile`,
  SessionStart + PostCompact + PostToolUse + Stop hooks
- Droid plugin: droid + same commands + equivalent hooks
- Background reviewer (simple: confidence threshold, no LLM judge call yet)
- Foresight fields in schema (no expiry sweep yet)
- MemScenes in schema (flat, no auto-clustering yet)
- Git commit per turn
- Trace log for retrieval observability

**Out of MVP (V1):**

- `sqlite-vec` + embedding pipeline
- LLM-judge with hybrid retrieval
- MemScenes incremental semantic clustering
- Foresight expiry cron
- Contradiction resolution workflow
- Access-aware eviction
- `/review` staging area

**Out of V1 (V2):**

- Obsidian Bases dashboard, canvas demos, CSS snippets
- Stale-fact sweeper
- VerificAgent-style safety checks
- Marketplace distribution
- Multi-device sync daemon

## 9. Testing strategy

- **Unit tests** per module: log writer, projector, renderer, retrieval.
- **Property tests** for idempotency: `replay(events) == replay(events + events)`.
- **Integration tests:** full turn loop (mock agent → write events → render →
  SessionStart → hot cache). Both platforms.
- **Trace replay tests:** save golden traces, replay with new scoring weights,
  assert top-K overlap ≥ threshold.
- **Concurrency tests:** two simulated agents writing to log simultaneously;
  projection state must match either serialization.
- **Schema migration tests:** old `v:1` events projected by new projector.
- **Benchmark harness** using MemBench / MemoryArena / AMA-Bench as external
  eval baselines (V1+).

## 10. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Projector bugs silently drop events | Checksum per event; replay assertion on startup; `pebble verify` command |
| Reviewer extracts junk with high confidence | Type-aware thresholds (profile 0.9, project 0.7, transient 0.5); VerificAgent checks in V2; `/review` staging in V1 |
| Vault edits create infinite loop | File-watcher debounce; `user_edit` events marked to bypass re-render of the edited file |
| Cross-platform drift (CC and Droid disagree) | Contract tests against shared MCP server; shared `pebble-mcp` means one source of truth |
| Log grows unbounded | Checkpoint + snapshot every 500 events; V2 adds log compaction (old events collapsed into synthetic `assert` + pointer to git history) |
| Git commits on every turn noise | Turn-batched commits only (not per tool call); squash history with `git rebase` opt-in command |
| Slow SessionStart with large vault | Projection is SQLite (ms queries); hot cache is bounded-size; checkpoint snapshot loaded, not replayed |
| User hates the name | Rename before marketplace submission (V2); internal name change is a `sed` |

## 11. Open questions

1. **Embedding model for V1.** Local (Ollama / transformers.js) vs API
   (OpenAI / Anthropic)? Default to local for privacy?
2. **Judge LLM for V1.** Reuse the user's configured agent model, or a
   smaller/cheaper model via configuration?
3. **Skill ownership in multi-platform vault.** If a skill is `version: 1.2.3`
   in CC and `1.2.4` in Droid from concurrent edits, do we auto-merge or
   flag_both?
4. **Foresight expiry policy.** Default `t_end` if the reviewer doesn't
   specify one? Currently: type-aware defaults (see §6.5). Is that right?
5. **Vault-less mode?** Some users may want memory without a visible vault.
   Should `vault/` rendering be optional?
6. **Telemetry.** Any opt-in telemetry to improve defaults (e.g. retrieval
   weights)? Off by default, opt-in via `/pebble telemetry on`?

## 12. References

From `.factory/memories.md` §7:

- EverMemOS `2601.02163` — MemCell schema
- AutoSkill `2603.01145` — SKILL.md extraction from user queries
- SwiftMem `2601.08160` — query-aware indexing
- ACE `2510.04618` — playbook context engineering
- Kumiho `2603.17244` — belief revision semantics
- VerificAgent `2506.02539` — memory verification
- `2603.02473` — retrieval vs utilization eval methodology
- MemEvolve `2512.18746` — architecture evolution (inspiration for projector-swappability)
- deer-flow `#2450` — Memory Module Roadmap (waves 1-4 dependency)
- deer-flow `#2437` — self-evolving skill reviewer RFC (anti-recursion pattern)
- claude-obsidian — hot cache + PostCompact injection + auto-commit pattern
- Obsidian-Memory — MCP server pattern + Cloudflare Access learnings

## 13. Approval

Section-level approvals captured during the design session on 2026-04-22:

1. Scope — three surfaces on shared schema — approved
2. Storage — event-sourced (JSONL log + SQLite projection + markdown view) — approved
3. Write trigger — background reviewer + /remember — approved
4. Read path — hot cache at SessionStart + per-turn retrieval + PostCompact — approved
5. Invalidation — all seven mechanisms — approved
6. Platform surfaces — full CC + Droid with native features — approved
7. MVP — both platforms in parallel — approved
8. Name — Pebble — approved

Final written-spec approval: **pending user review of this document.**

**Next step (once spec approved):** invoke `writing-plans` skill to produce
the implementation plan.
