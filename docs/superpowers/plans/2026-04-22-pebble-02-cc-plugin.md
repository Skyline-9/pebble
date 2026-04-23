# Pebble — Plan 2: Claude Code Plugin (`claude-code-plugin`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Claude Code plugin shell that wraps `pebble-mcp` into a first-class CC experience — skills, slash commands, subagents, and hooks for hot-cache injection, background review, and per-turn git commits.

**Architecture:** Thin plugin over `pebble-mcp`. The MCP server is declared in `plugin.json`; skills instruct the model on when and how to call `memory_*`, `skill_*`, `profile_*` tools; the background reviewer is a subagent invoked by the main model when the PostToolUse hook flags "time to review"; hooks inject the hot-cache on SessionStart and PostCompact, and commit per turn on Stop.

**Tech Stack:** Claude Code plugin manifest format, bash (POSIX) for hook scripts, `jq` for JSON shaping, `pebble-mcp` binary (Plan 1) on `PATH`.

**Spec reference:** `docs/superpowers/specs/2026-04-22-pebble-design.md` (§7.2, §8).

**Prerequisites:** Plan 1 (`pebble-mcp`) complete. `bun`, `jq`, and `git` available on `PATH`. `pebble-mcp init` has been run at least once so that `~/.pebble/` exists.

---

## File structure

```
claude-code-plugin/
├── .claude-plugin/
│   ├── plugin.json                  # plugin manifest + MCP server registration
│   └── marketplace.json             # marketplace listing (single-plugin marketplace)
├── skills/
│   ├── pebble/SKILL.md              # orchestrator: when to use memory/skill/profile
│   ├── pebble-query/SKILL.md        # how to call memory_query well
│   └── pebble-save/SKILL.md         # when to call memory_assert / skill_save
├── agents/
│   └── pebble-reviewer.md           # background reviewer subagent (anti-recursion)
├── commands/
│   ├── pebble.md                    # /pebble — status
│   ├── remember.md                  # /remember <text>
│   ├── forget.md                    # /forget <query|cell_id>
│   ├── recall.md                    # /recall <query>
│   └── profile.md                   # /profile — show current profile
├── hooks/
│   ├── hooks.json                   # hook configuration
│   └── scripts/
│       ├── session-start.sh         # inject hot-cache-for-cc
│       ├── post-compact.sh          # re-inject hot-cache-for-cc
│       ├── post-tool-use.sh         # round-tick; flag reviewer at threshold
│       └── stop.sh                  # commit-turn on git
└── tests/
    └── smoke.sh                     # manual install + tool-listing smoke test

# In addition, Plan 2 extends pebble-mcp (owned by Plan 1 at src/, modified here):
pebble-mcp/src/cli/commit-turn.ts    # new CLI — wraps commitTurn() from src/git/
pebble-mcp/src/cli/review-turn.ts    # new CLI — reads transcript, extracts, asserts
```

Each file has one responsibility. Hook scripts are small (≤40 lines). Skills are in the model's voice.

---

## Task 1: pebble-mcp CLI extensions — `commit-turn` and `review-turn`

Plan 2 and Plan 3 both need these to glue platform hooks to the event log. They sit in `pebble-mcp/src/cli/` because they share the `~/.pebble/` substrate.

**Files:**
- Create: `pebble-mcp/src/cli/commit-turn.ts`
- Create: `pebble-mcp/src/cli/review-turn.ts`
- Modify: `pebble-mcp/src/index.ts` (add `commit-turn` and `review-turn` subcommands)
- Create: `pebble-mcp/tests/cli-commit-turn.test.ts`
- Create: `pebble-mcp/tests/cli-review-turn.test.ts`

- [ ] **Step 1: Write failing test for commit-turn CLI**

```typescript
// pebble-mcp/tests/cli-commit-turn.test.ts
import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

let tmp: string;
const BIN = `bun run ${join(import.meta.dir, "..", "src", "index.ts")}`;

beforeEach(() => { tmp = mkdtempSync(join(tmpdir(), "pebble-cti-")); process.env.PEBBLE_ROOT = tmp; });
afterEach(() => { delete process.env.PEBBLE_ROOT; rmSync(tmp, { recursive: true, force: true }); });

describe("commit-turn CLI", () => {
  test("commits all changes with gitmoji subject", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    writeFileSync(join(tmp, "trigger.txt"), "change");
    execSync(`${BIN} commit-turn --turn 7 --adds 2 --retracts 1 --actor claude-code`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    });
    const log = execSync("git log --oneline", { cwd: tmp }).toString();
    expect(log).toMatch(/:memo: pebble: turn 7 \+2 -1/);
  });

  test("no-op when nothing changed", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const out = execSync(`${BIN} commit-turn --turn 1 --adds 0 --retracts 0`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out.toLowerCase()).toMatch(/nothing|no changes/);
  });
});
```

- [ ] **Step 2: Implement `src/cli/commit-turn.ts`**

```typescript
// src/cli/commit-turn.ts
import { commitTurn } from "../git/turn-commit";

interface Args {
  turn?: number;
  adds?: number;
  retracts?: number;
  actor?: "claude-code" | "factory-droid" | "cli";
}

function parseArgs(argv: string[]): Args {
  const out: Args = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const v = argv[i + 1];
    if (k === "--turn" && v !== undefined)     { out.turn = Number(v); i++; }
    else if (k === "--adds" && v !== undefined)    { out.adds = Number(v); i++; }
    else if (k === "--retracts" && v !== undefined){ out.retracts = Number(v); i++; }
    else if (k === "--actor" && v !== undefined)   { out.actor = v as Args["actor"]; i++; }
  }
  return out;
}

export function cliCommitTurn(argv: string[]): void {
  const args = parseArgs(argv);
  const result = commitTurn({
    turn: args.turn ?? 0,
    adds: args.adds ?? 0,
    retracts: args.retracts ?? 0,
    actor: args.actor,
  });
  if (!result.committed) {
    console.log("nothing to commit");
    return;
  }
  console.log(`committed turn ${args.turn ?? 0}`);
}
```

- [ ] **Step 3: Write failing test for review-turn CLI**

```typescript
// pebble-mcp/tests/cli-review-turn.test.ts
import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Database } from "bun:sqlite";

let tmp: string;
const BIN = `bun run ${join(import.meta.dir, "..", "src", "index.ts")}`;

beforeEach(() => { tmp = mkdtempSync(join(tmpdir(), "pebble-rt-")); process.env.PEBBLE_ROOT = tmp; });
afterEach(() => { delete process.env.PEBBLE_ROOT; rmSync(tmp, { recursive: true, force: true }); });

describe("review-turn CLI", () => {
  test("extracts candidates from JSONL transcript and asserts them", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const transcriptPath = join(tmp, "transcript.jsonl");
    const lines = [
      JSON.stringify({ role: "user", content: "I prefer TypeScript for backend work." }),
      JSON.stringify({ role: "assistant", content: "Got it." }),
      JSON.stringify({ role: "user", content: "My primary language is Haskell actually." }),
    ];
    writeFileSync(transcriptPath, lines.join("\n") + "\n");
    const out = execSync(`${BIN} review-turn --transcript ${transcriptPath}`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out).toMatch(/asserted:\s*[1-9]/i);

    const db = new Database(join(tmp, "projection.db"), { readonly: true });
    const count = (db.query("SELECT COUNT(*) AS n FROM cells WHERE retracted_at IS NULL").get() as any).n;
    db.close();
    expect(count).toBeGreaterThan(0);
  });

  test("skips empty transcripts gracefully", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const transcriptPath = join(tmp, "empty.jsonl");
    writeFileSync(transcriptPath, "");
    const out = execSync(`${BIN} review-turn --transcript ${transcriptPath}`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out).toMatch(/asserted:\s*0/i);
  });
});
```

- [ ] **Step 4: Implement `src/cli/review-turn.ts`**

```typescript
// src/cli/review-turn.ts
import { Database } from "bun:sqlite";
import { readFileSync, existsSync } from "node:fs";
import { dbPath } from "../paths";
import { initSchema } from "../projection/schema";
import { extractCandidates, type Transcript } from "../reviewer/extractor";
import { judgeCandidate } from "../reviewer/judge";
import { appendEvents } from "../log/writer";
import { projectEvent } from "../projection/projector";
import { newEventId } from "../ids";
import type { AssertEvent } from "../types";

interface Args { transcript?: string; }

function parseArgs(argv: string[]): Args {
  const out: Args = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--transcript" && argv[i + 1] !== undefined) {
      out.transcript = argv[i + 1]; i++;
    }
  }
  return out;
}

function readTranscript(path: string): Transcript {
  if (!existsSync(path)) return [];
  const raw = readFileSync(path, "utf8");
  const lines = raw.split("\n").filter(l => l.trim());
  const turns: Transcript = [];
  for (const line of lines) {
    try {
      const t = JSON.parse(line);
      if (t && typeof t.role === "string" && typeof t.content === "string") {
        turns.push({ role: t.role, content: t.content });
      }
    } catch { /* skip malformed */ }
  }
  return turns;
}

export async function cliReviewTurn(argv: string[]): Promise<void> {
  const args = parseArgs(argv);
  if (!args.transcript) {
    console.error("usage: pebble-mcp review-turn --transcript <path>");
    process.exit(1);
  }

  const db = new Database(dbPath());
  initSchema(db);

  try {
    const transcript = readTranscript(args.transcript);
    const candidates = extractCandidates(transcript);
    let asserted = 0, discarded = 0;
    const events: AssertEvent[] = [];
    const now = new Date().toISOString();

    for (const cand of candidates) {
      const decision = judgeCandidate(db, cand);
      if (decision.action !== "assert") { discarded++; continue; }
      const ev: AssertEvent = {
        v: 1, ev: "assert", id: newEventId(), actor: "reviewer", ts: now,
        cell_id: cand.id, cell: cand,
      };
      events.push(ev);
      asserted++;
    }

    if (events.length > 0) {
      await appendEvents(events);
      for (const ev of events) projectEvent(db, ev);
    }
    console.log(`asserted: ${asserted}, discarded: ${discarded}, seen: ${candidates.length}`);
  } finally {
    db.close();
  }
}
```

- [ ] **Step 5: Update `src/index.ts` dispatch**

Modify the `switch (cmd)` block in `src/index.ts` to add the two new commands:

```typescript
// src/index.ts
#!/usr/bin/env bun
import { startServer } from "./mcp/server";
import { cliInit } from "./cli/init";
import { cliVerify } from "./cli/verify";
import { cliStatus } from "./cli/status";
import { cliHotCache } from "./cli/hot-cache";
import { cliSeedTestFixture } from "./cli/seed-test-fixture";
import { cliCommitTurn } from "./cli/commit-turn";
import { cliReviewTurn } from "./cli/review-turn";

const cmd = process.argv[2] ?? "serve";
const rest = process.argv.slice(3);

async function main() {
  switch (cmd) {
    case "serve":               await startServer(); break;
    case "init":                cliInit(); break;
    case "verify":              await cliVerify(); break;
    case "status":              cliStatus(); break;
    case "hot-cache-for-cc":    await cliHotCache({ target: "cc" }); break;
    case "hot-cache-for-droid": await cliHotCache({ target: "droid" }); break;
    case "seed-test-fixture":   await cliSeedTestFixture(); break;
    case "commit-turn":         cliCommitTurn(rest); break;
    case "review-turn":         await cliReviewTurn(rest); break;
    default:
      console.error(`unknown command: ${cmd}`);
      console.error("usage: pebble-mcp [serve|init|verify|status|hot-cache-for-cc|hot-cache-for-droid|commit-turn|review-turn]");
      process.exit(1);
  }
}

main().catch(err => { console.error(err); process.exit(1); });
```

- [ ] **Step 6: Run the new tests**

Run: `cd pebble-mcp && bun test tests/cli-commit-turn.test.ts tests/cli-review-turn.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 7: Run the full pebble-mcp suite to guard against regressions**

Run: `cd pebble-mcp && bun test`
Expected: all tests pass (Plan 1 total + 4 new).

- [ ] **Step 8: Commit**

```bash
git add pebble-mcp/src/cli/commit-turn.ts pebble-mcp/src/cli/review-turn.ts pebble-mcp/src/index.ts pebble-mcp/tests/cli-commit-turn.test.ts pebble-mcp/tests/cli-review-turn.test.ts
git commit -m ":sparkles: pebble-mcp: add commit-turn + review-turn CLI (Plan 2 Task 1)"
```

---

## Task 2: CC plugin scaffolding + manifest

**Files:**
- Create: `claude-code-plugin/.claude-plugin/plugin.json`
- Create: `claude-code-plugin/.claude-plugin/marketplace.json`
- Create: `claude-code-plugin/.gitignore`

- [ ] **Step 1: Create `.claude-plugin/plugin.json`**

```json
{
  "name": "pebble",
  "version": "0.0.1",
  "description": "Super-personal memory for Claude Code — profile, skills, and knowledge that survive compaction and sessions.",
  "authors": [{ "name": "pebble" }],
  "homepage": "https://github.com/pebble/pebble",
  "license": "MIT",
  "mcpServers": {
    "pebble": {
      "command": "pebble-mcp",
      "args": ["serve"],
      "env": { "PEBBLE_ROOT": "${HOME}/.pebble" }
    }
  }
}
```

- [ ] **Step 2: Create `.claude-plugin/marketplace.json`**

```json
{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "pebble-marketplace",
  "plugins": [
    {
      "name": "pebble",
      "source": "."
    }
  ]
}
```

- [ ] **Step 3: Create `.gitignore`**

```
node_modules/
.DS_Store
*.log
```

- [ ] **Step 4: Verify manifest parses**

Run: `cd claude-code-plugin && jq . .claude-plugin/plugin.json && jq . .claude-plugin/marketplace.json`
Expected: both print successfully (valid JSON).

- [ ] **Step 5: Commit**

```bash
git add claude-code-plugin/.claude-plugin/ claude-code-plugin/.gitignore
git commit -m ":seedling: pebble-cc: scaffold plugin manifest (Task 2)"
```

---

## Task 3: `skills/pebble/SKILL.md` — orchestrator

The orchestrator instructs the model on when to use the memory system at all. Claude Code triggers skills by matching the `description` against the user's request.

**Files:**
- Create: `claude-code-plugin/skills/pebble/SKILL.md`

- [ ] **Step 1: Create the orchestrator skill**

```markdown
---
name: pebble
description: Use the Pebble memory system — store and recall user preferences, active projects, skills, and foresight. Activate when the user mentions preferences, projects, past conversations, "remember", "forget", or when context continuity across sessions matters.
allowed-tools: [memory_assert, memory_query, memory_touch, memory_retract, memory_read_cell, profile_read, skill_list, skill_read, trace_read]
---

# Pebble — memory orchestrator

You have access to Pebble, a persistent memory system backed by an append-only event log and a queryable SQLite projection. Use it to make the user feel like they have a continuous working relationship with you.

## When to CALL `memory_query`

- The user asks about a past conversation ("did I say...", "what did we decide...")
- The user's request would benefit from knowing their preferences or active projects
- You are about to make a decision that depends on style conventions (commit style, code style)
- The user's request is ambiguous and their prior context could disambiguate it

Default `top_k: 5`. Pass a concise query that captures the information need, not the full user turn.

## When to CALL `memory_touch`

Whenever you USE a cell that `memory_query` returned. This keeps the recency/access signal fresh.

## When to CALL `profile_read`

- Before generating code (to honor stack preferences)
- Before writing commits or PRs (to honor commit style)
- At the start of a new plan

## When to CALL `skill_list` and `skill_read`

- When you see a user request that a saved skill might cover
- `skill_list` returns `[{name, description, cell_id}]`. Scan descriptions for trigger matches.
- If matched, `skill_read {name}` to load the body, then follow it.

## Do NOT

- Do not call `memory_assert` yourself. Saving is done by the `pebble-save` skill, the `/remember` command, and the background reviewer.
- Do not bulk-retract cells. Use `/forget` for explicit user intent only.

## Observability

Every `memory_query` writes to `trace.jsonl`. The user can inspect via `trace_read` for debugging retrieval quality.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/skills/pebble/
git commit -m ":sparkles: pebble-cc: add orchestrator skill (Task 3)"
```

---

## Task 4: `skills/pebble-query/SKILL.md`

A focused skill for composing good retrieval queries.

**Files:**
- Create: `claude-code-plugin/skills/pebble-query/SKILL.md`

- [ ] **Step 1: Create the query skill**

```markdown
---
name: pebble-query
description: Compose a high-quality memory query. Activate when the user's turn would benefit from recalling stored context and you need to call memory_query with a well-formed query string.
allowed-tools: [memory_query, memory_touch]
---

# Pebble — query composer

A good retrieval query is:

1. **Short** — 3 to 8 words. FTS5 BM25 favors high-signal tokens.
2. **Concrete** — include the entity, topic, or artifact name, not verbs like "tell me about".
3. **Domain-shaped** — if the user is talking about code, include language/library. If about a project, include the project slug.

## Examples

| User turn | Good query | Bad query |
| --- | --- | --- |
| "Let's pick up where we left off on auth." | `auth refactor` | `where we left off` |
| "What's my commit style?" | `commit style gitmoji` | `my commit style` |
| "Deploy the backend." | `backend deploy frontend` | `deploy` |

## Procedure

1. Construct the query string.
2. Call `memory_query` with `{ query, top_k: 5, turn: <current turn number if known> }`.
3. For each hit you actually use in your response, call `memory_touch {cell_id, query: <same query>}`.
4. If no hits or low-scoring hits, say so once, then proceed.

## Anti-pattern

Do not call `memory_query` more than twice per user turn. If two queries return nothing relevant, continue without memory and let the reviewer pick up any new context from this turn.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/skills/pebble-query/
git commit -m ":sparkles: pebble-cc: add query composer skill (Task 4)"
```

---

## Task 5: `skills/pebble-save/SKILL.md`

Explicit-save path. Distinct from the background reviewer which runs without user intent.

**Files:**
- Create: `claude-code-plugin/skills/pebble-save/SKILL.md`

- [ ] **Step 1: Create the save skill**

```markdown
---
name: pebble-save
description: Save an explicit memory. Activate when the user says "remember", "save", "note that", "going forward", or when they've explicitly articulated a preference that should persist across sessions.
allowed-tools: [memory_assert, skill_save]
---

# Pebble — explicit save

Distinguish TWO save paths:

## Path A — a fact, preference, or project update → `memory_assert`

Args:
- `type`: `"profile" | "preference" | "project" | "episodic"`
  - `profile`: enduring identity — primary language, tone, conventions. Threshold 0.9.
  - `preference`: general preference — dark mode, prefers pytest. Threshold 0.75.
  - `project`: current work — "auth refactor in Q2". Threshold 0.7.
  - `episodic`: one-time fact worth recording — a decision, a constraint. Threshold 0.5.
- `episode`: third-person narrative (1 sentence) that captures the fact.
- `facts`: atomic fact list, `{ subject, predicate, object, confidence }`. `subject` uses dotted notation: `user.stack.lang`, `user.voice.tone`, `project.current`.
- `confidence`: your calibrated confidence. Be conservative.
- `actor`: `"user"` for explicit saves.

## Path B — a reusable pattern or procedure → `skill_save`

Args:
- `name`: slug, e.g. `commit-style`, `deploy-flow`.
- `description`: one-line match-the-trigger. E.g. "Use gitmoji in commit subjects."
- `body`: the instructions you would follow if this skill activated.
- `trigger_phrases`: 3–6 natural phrases that should load this skill.
- `confidence`: your confidence in the skill's correctness.

## When to choose which

| User intent | Path |
| --- | --- |
| "Remember I prefer vitest." | A (`preference`) |
| "Remember my commit style is gitmoji." | B (reusable pattern) |
| "I'm working on pebble through May." | A (`project` with foresight `t_end`) |
| "Always run lint before commit." | B |

## After saving

Tell the user exactly what was saved, the `cell_id`, and where it rendered in the vault (`profile.md`, `skills/<name>.md`, or a scene).
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/skills/pebble-save/
git commit -m ":sparkles: pebble-cc: add explicit-save skill (Task 5)"
```

---

## Task 6: `agents/pebble-reviewer.md` — background reviewer subagent

The reviewer is anti-recursive: restricted tool access, no hooks, no access to other skills.

**Files:**
- Create: `claude-code-plugin/agents/pebble-reviewer.md`

- [ ] **Step 1: Create the reviewer agent**

```markdown
---
name: pebble-reviewer
description: Background memory reviewer. Extracts user preferences, profile updates, and project context from the conversation and calls memory_assert. Invoked by the main session when the PostToolUse hook indicates it's time.
tools: [memory_query, memory_assert, memory_read_cell]
---

# Pebble — background reviewer

You are invoked in the background to harvest long-lived facts from the current conversation. You are anti-recursive: do not call other skills, do not run commands, do not do anything beyond extracting and asserting.

## Input

A transcript slice of the last N user turns (provided by the main session via the Task tool's prompt).

## Procedure

For each user turn (IGNORE assistant/tool turns — AutoSkill principle):

1. **Scan for profile signals.** Patterns like "my primary language is X", "I prefer X for Y", "always use X". If matched, prepare a candidate of type `profile` or `preference`.

2. **Scan for project signals.** Patterns like "I'm working on X", "the X project", "through next week". If matched, prepare a candidate of type `project` with a foresight `t_end` set to a reasonable default (30 days from now for `project` per spec §6.5).

3. **Dedup via memory_query.** For each candidate, call `memory_query { query: <candidate.episode>, top_k: 3 }`. If a near-duplicate exists (same subject, same object, high score), DROP the candidate.

4. **Assert.** Call `memory_assert { type, episode, facts, confidence, actor: "reviewer" }` for each surviving candidate.

## Confidence calibration

- `profile`: ≥0.9 only — you must be confident this is enduring identity.
- `preference`: 0.75–0.9.
- `project`: 0.7–0.85.
- Unsure? Lower the confidence — the rule-based judge will filter below-threshold candidates.

## Output

Plain text summary: `{asserted: N, dropped: M, notes: [...]}`. The main session will display a condensed version to the user if relevant.

## Hard rules

- **Never** call `memory_retract`. Retraction requires explicit user intent.
- **Never** call `skill_save`. Skills are authored by the user via `/remember` or `pebble-save`.
- **Never** call any tool outside the three in your allowed list.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/agents/pebble-reviewer.md
git commit -m ":sparkles: pebble-cc: add background reviewer agent (Task 6)"
```

---

## Task 7: `/pebble` status command

**Files:**
- Create: `claude-code-plugin/commands/pebble.md`

- [ ] **Step 1: Create /pebble command**

```markdown
---
description: Show Pebble memory status — cell count, event count, top skills, active foresight.
allowed-tools: [Bash]
---

# /pebble

Run the `pebble-mcp status` CLI and render the output as a compact dashboard.

Procedure:

1. Run: `pebble-mcp status`
2. Run: `pebble-mcp hot-cache-for-cc | head -40`
3. Format the output as a short table or bulleted block.
4. If `status` exits non-zero, tell the user "Pebble is not initialized — run `pebble-mcp init` to begin" and stop.

Example response shape:

```
**Pebble status**

| Item | Count |
| --- | --- |
| Cells | 128 |
| Events | 412 |
| Skills | 6 |

**Top skills**: commit-style, deploy-flow, test-conventions.

**Active foresight**:
- Ship auth refactor by 2026-06-30
- Finish pebble v1 by 2026-05-15
```
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/commands/pebble.md
git commit -m ":sparkles: pebble-cc: add /pebble status command (Task 7)"
```

---

## Task 8: `/remember` command

**Files:**
- Create: `claude-code-plugin/commands/remember.md`

- [ ] **Step 1: Create /remember command**

```markdown
---
description: Explicitly remember something. Use the pebble-save skill to store a fact or skill.
argument-hint: <what to remember>
allowed-tools: [memory_assert, skill_save]
---

# /remember

The user wants to save something explicitly. Arguments: `$ARGUMENTS`.

1. Load the `pebble-save` skill.
2. Parse the user's intent:
   - If `$ARGUMENTS` describes a fact or preference → `memory_assert` (Path A).
   - If `$ARGUMENTS` describes a reusable procedure or pattern → `skill_save` (Path B).
3. Confirm to the user what was saved, including the resulting `cell_id`.

If `$ARGUMENTS` is empty, ask: "What should I remember?" and stop.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/commands/remember.md
git commit -m ":sparkles: pebble-cc: add /remember command (Task 8)"
```

---

## Task 9: `/forget` command

**Files:**
- Create: `claude-code-plugin/commands/forget.md`

- [ ] **Step 1: Create /forget command**

```markdown
---
description: Forget a stored fact. Retracts the matching cell; the event stays in the log (append-only).
argument-hint: <query or mc_... cell id>
allowed-tools: [memory_query, memory_retract, memory_read_cell]
---

# /forget

The user wants to retract a memory. Arguments: `$ARGUMENTS`.

Procedure:

1. If `$ARGUMENTS` starts with `mc_`, treat it as a direct cell_id:
   - Call `memory_read_cell { cell_id }` to read it back.
   - Show the user the cell's episode and ask: "Retract this? (yes/no)"
   - On yes, call `memory_retract { cell_id, reason: "user:/forget" }`.
2. Otherwise, treat `$ARGUMENTS` as a query:
   - Call `memory_query { query: $ARGUMENTS, top_k: 3 }`.
   - Show the top 3 candidates.
   - Ask: "Which should I retract? (1/2/3 or none)"
   - On 1/2/3, call `memory_retract { cell_id: hits[i].cell_id, reason: "user:/forget" }`.
3. Confirm the retraction.

Important: retractions are reversible by finding the retract event in `log.jsonl` and not replaying it — but for the user, the cell is gone from queries.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/commands/forget.md
git commit -m ":sparkles: pebble-cc: add /forget command (Task 9)"
```

---

## Task 10: `/recall` command

**Files:**
- Create: `claude-code-plugin/commands/recall.md`

- [ ] **Step 1: Create /recall command**

```markdown
---
description: Search memory for a topic. Returns matching cells with their scores.
argument-hint: <query>
allowed-tools: [memory_query, memory_read_cell, memory_touch]
---

# /recall

The user wants to search their memory. Arguments: `$ARGUMENTS`.

Procedure:

1. Load the `pebble-query` skill for query composition tips.
2. Call `memory_query { query: $ARGUMENTS, top_k: 5 }`.
3. For each hit, optionally call `memory_read_cell` to show the episode and facts.
4. Render as a compact list:

   ```
   1. [mc_...] (score 0.82) — <episode>
   2. [mc_...] (score 0.67) — <episode>
   ...
   ```

5. If the user references one of the results in their next turn, call `memory_touch` to record the hit.

If no results: "No matches for '$ARGUMENTS'. Try a shorter, more specific query."
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/commands/recall.md
git commit -m ":sparkles: pebble-cc: add /recall command (Task 10)"
```

---

## Task 11: `/profile` command

**Files:**
- Create: `claude-code-plugin/commands/profile.md`

- [ ] **Step 1: Create /profile command**

```markdown
---
description: Show the current user profile — voice, stack, conventions.
allowed-tools: [profile_read]
---

# /profile

Call `profile_read {}` and render the result as a compact markdown view:

```
# Your Pebble profile

**Voice**
- Tone: <profile.voice.tone>
- Dos: <comma-sep list>
- Don'ts: <comma-sep list>

**Stack**
- Primary langs: <comma-sep>
- Frameworks: <comma-sep>
- Tools: <comma-sep>
- Never use: <comma-sep>

**Conventions**
- Commit: <profile.conventions.commit_style>
- Code: <profile.conventions.code_style>
- Test: <profile.conventions.test_style>
- Doc: <profile.conventions.doc_style>

_Last updated: <profile.updated_at>_
```

If any field is empty, write `_not set_`.
```

- [ ] **Step 2: Commit**

```bash
git add claude-code-plugin/commands/profile.md
git commit -m ":sparkles: pebble-cc: add /profile command (Task 11)"
```

---

## Task 12: `hooks/hooks.json` + SessionStart script

The hooks.json wires up the four lifecycle events. Each calls a small shell script in `hooks/scripts/`.

**Files:**
- Create: `claude-code-plugin/hooks/hooks.json`
- Create: `claude-code-plugin/hooks/scripts/session-start.sh`

- [ ] **Step 1: Create hooks.json**

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/session-start.sh"
          }
        ]
      }
    ],
    "PostCompact": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/post-compact.sh"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/post-tool-use.sh"
          }
        ]
      }
    ],
    "Stop": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/stop.sh"
          }
        ]
      }
    ]
  }
}
```

- [ ] **Step 2: Create session-start.sh**

```bash
#!/usr/bin/env bash
# hooks/scripts/session-start.sh
# Invoked by CC on SessionStart. Prints JSON with additionalContext for the model.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-cc 2>/dev/null || echo "")"

# If pebble-mcp isn't available or returned nothing, exit silently (non-blocking).
if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
```

- [ ] **Step 3: Make scripts executable and verify JSON shape**

```bash
chmod +x claude-code-plugin/hooks/scripts/session-start.sh

# Verify: with a seeded fixture, the script outputs valid JSON with additionalContext.
(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  output="$(./claude-code-plugin/hooks/scripts/session-start.sh)"
  echo "$output" | jq -e '.hookSpecificOutput.additionalContext | test("Profile"; "i")'
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: the `jq -e` assertion exits 0 (additionalContext contains "Profile").

- [ ] **Step 4: Commit**

```bash
git add claude-code-plugin/hooks/hooks.json claude-code-plugin/hooks/scripts/session-start.sh
git commit -m ":sparkles: pebble-cc: add hooks.json + SessionStart script (Task 12)"
```

---

## Task 13: PostCompact script

**Files:**
- Create: `claude-code-plugin/hooks/scripts/post-compact.sh`

- [ ] **Step 1: Create post-compact.sh**

```bash
#!/usr/bin/env bash
# hooks/scripts/post-compact.sh
# After compaction, re-inject profile + top skills + active foresight.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-cc 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "PostCompact", additionalContext: $ctx}}'
```

- [ ] **Step 2: Make executable + verify**

```bash
chmod +x claude-code-plugin/hooks/scripts/post-compact.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  ./claude-code-plugin/hooks/scripts/post-compact.sh | jq -e '.hookSpecificOutput.hookEventName == "PostCompact"'
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add claude-code-plugin/hooks/scripts/post-compact.sh
git commit -m ":sparkles: pebble-cc: add PostCompact script (Task 13)"
```

---

## Task 14: PostToolUse script — round-tick and reviewer flag

**Files:**
- Create: `claude-code-plugin/hooks/scripts/post-tool-use.sh`

The round counter lives in `~/.pebble/.cc-rounds`. When the counter hits a multiple of `PEBBLE_REVIEW_EVERY` (default 8), the script emits `additionalContext` that instructs the main model to invoke `@pebble-reviewer` via the Task tool.

- [ ] **Step 1: Create post-tool-use.sh**

```bash
#!/usr/bin/env bash
# hooks/scripts/post-tool-use.sh
# Every tool use increments a round counter. At threshold, flag the reviewer.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
COUNTER_FILE="$ROOT/.cc-rounds"
THRESHOLD="${PEBBLE_REVIEW_EVERY:-8}"

mkdir -p "$ROOT"
[ -f "$COUNTER_FILE" ] || echo "0" > "$COUNTER_FILE"

current="$(cat "$COUNTER_FILE")"
next=$((current + 1))
echo "$next" > "$COUNTER_FILE"

# Only flag every $THRESHOLD rounds.
if (( next % THRESHOLD != 0 )); then
  exit 0
fi

# Read the hook payload (JSON on stdin) to get transcript_path.
payload="$(cat || true)"
transcript="$(echo "$payload" | jq -r '.transcript_path // empty' 2>/dev/null || true)"

if [ -n "$transcript" ] && [ -f "$transcript" ]; then
  MSG="Pebble: time to review. Invoke the @pebble-reviewer subagent with the last ${THRESHOLD} user turns from \`$transcript\`."
else
  MSG="Pebble: time to review. Invoke the @pebble-reviewer subagent to harvest any preferences/projects from recent turns."
fi

jq -n --arg ctx "$MSG" \
  '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
```

- [ ] **Step 2: Make executable + verify round behavior**

```bash
chmod +x claude-code-plugin/hooks/scripts/post-tool-use.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  export PEBBLE_REVIEW_EVERY=3
  mkdir -p "$PEBBLE_ROOT"
  # First two invocations: no output
  out1="$(./claude-code-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"
  out2="$(./claude-code-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"
  # Third invocation: should emit JSON with additionalContext
  out3="$(./claude-code-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"

  test -z "$out1" || (echo "expected empty on round 1"; exit 1)
  test -z "$out2" || (echo "expected empty on round 2"; exit 1)
  echo "$out3" | jq -e '.hookSpecificOutput.additionalContext | test("reviewer"; "i")'

  rm -rf "$PEBBLE_ROOT"
)
```

Expected: only the 3rd invocation emits a JSON block containing "reviewer". `jq -e` exits 0.

- [ ] **Step 3: Commit**

```bash
git add claude-code-plugin/hooks/scripts/post-tool-use.sh
git commit -m ":sparkles: pebble-cc: add PostToolUse round-tick script (Task 14)"
```

---

## Task 15: Stop script — commit turn

**Files:**
- Create: `claude-code-plugin/hooks/scripts/stop.sh`

On Stop, we count events appended during this session (via a delta counter), then call `pebble-mcp commit-turn`.

- [ ] **Step 1: Create stop.sh**

```bash
#!/usr/bin/env bash
# hooks/scripts/stop.sh
# On Stop, compute turn number + adds/retracts delta, then commit.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
TURN_FILE="$ROOT/.cc-turn"
LAST_EVT_FILE="$ROOT/.cc-last-event-count"

mkdir -p "$ROOT"

# Turn number: monotonic per session; increments each Stop.
[ -f "$TURN_FILE" ] || echo "0" > "$TURN_FILE"
turn=$(( $(cat "$TURN_FILE") + 1 ))
echo "$turn" > "$TURN_FILE"

# Event-count delta to compute adds/retracts.
current_events=0
if [ -f "$ROOT/log.jsonl" ]; then
  current_events="$(wc -l < "$ROOT/log.jsonl" | tr -d ' ')"
fi
[ -f "$LAST_EVT_FILE" ] || echo "0" > "$LAST_EVT_FILE"
previous_events="$(cat "$LAST_EVT_FILE")"
echo "$current_events" > "$LAST_EVT_FILE"

# Count this session's adds/retracts by scanning the delta tail.
adds=0
retracts=0
if (( current_events > previous_events )) && [ -f "$ROOT/log.jsonl" ]; then
  delta=$(( current_events - previous_events ))
  delta_events="$(tail -n "$delta" "$ROOT/log.jsonl" 2>/dev/null || true)"
  if [ -n "$delta_events" ]; then
    adds="$(echo "$delta_events" | jq -s '[.[] | select(.ev == "assert")] | length')"
    retracts="$(echo "$delta_events" | jq -s '[.[] | select(.ev == "retract")] | length')"
  fi
fi

pebble-mcp commit-turn --turn "$turn" --adds "$adds" --retracts "$retracts" --actor claude-code || true

# Reset the round counter at turn boundary.
echo "0" > "$ROOT/.cc-rounds"
```

- [ ] **Step 2: Make executable + verify end-to-end**

```bash
chmod +x claude-code-plugin/hooks/scripts/stop.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  # seed-test-fixture asserted 2 cells. On Stop, we expect 2 adds, 0 retracts.
  ./claude-code-plugin/hooks/scripts/stop.sh
  log="$(cd "$PEBBLE_ROOT" && git log --oneline)"
  echo "$log" | grep -E ":memo: pebble: turn 1 \+2 -0"
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: the `grep` succeeds (commit subject matches).

- [ ] **Step 3: Commit**

```bash
git add claude-code-plugin/hooks/scripts/stop.sh
git commit -m ":sparkles: pebble-cc: add Stop commit-turn script (Task 15)"
```

---

## Task 16: Local install smoke test

**Files:**
- Create: `claude-code-plugin/tests/smoke.sh`

A scripted manual smoke test that exercises the plugin end-to-end without needing CC to be running interactively.

- [ ] **Step 1: Create smoke.sh**

```bash
#!/usr/bin/env bash
# claude-code-plugin/tests/smoke.sh
# Smoke test: init, seed, run hooks, verify vault + git.
set -euo pipefail

ROOT="$(mktemp -d)"
export PEBBLE_ROOT="$ROOT"
export PEBBLE_REVIEW_EVERY=2
PLUGIN_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> pebble-mcp init"
pebble-mcp init

echo "==> pebble-mcp seed-test-fixture"
pebble-mcp seed-test-fixture

echo "==> SessionStart hook"
out="$("$PLUGIN_DIR/hooks/scripts/session-start.sh")"
echo "$out" | jq -e '.hookSpecificOutput.hookEventName == "SessionStart"' > /dev/null
echo "    ok"

echo "==> PostCompact hook"
out="$("$PLUGIN_DIR/hooks/scripts/post-compact.sh")"
echo "$out" | jq -e '.hookSpecificOutput.hookEventName == "PostCompact"' > /dev/null
echo "    ok"

echo "==> PostToolUse hook (rounds 1-2; only round 2 should emit)"
out1="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
out2="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
test -z "$out1"
echo "$out2" | jq -e '.hookSpecificOutput.additionalContext | test("reviewer"; "i")' > /dev/null
echo "    ok"

echo "==> Stop hook (commit-turn)"
"$PLUGIN_DIR/hooks/scripts/stop.sh"
log="$(cd "$ROOT" && git log --oneline)"
echo "$log" | grep -qE ":memo: pebble: turn 1 \+[0-9]+ -[0-9]+"
echo "    ok (${log})"

echo "==> pebble-mcp verify"
pebble-mcp verify

echo "==> all hooks + plugin wiring OK"
rm -rf "$ROOT"
```

- [ ] **Step 2: Run smoke test**

```bash
chmod +x claude-code-plugin/tests/smoke.sh
bash claude-code-plugin/tests/smoke.sh
```

Expected: prints `all hooks + plugin wiring OK` and exits 0.

- [ ] **Step 3: Commit**

```bash
git add claude-code-plugin/tests/smoke.sh
git commit -m ":white_check_mark: pebble-cc: add plugin smoke test (Task 16)"
```

---

## Task 17: Register plugin locally (interactive verification)

This task is human-verified. It confirms the plugin loads inside Claude Code.

**Files:** none (documentation-in-steps).

- [ ] **Step 1: Add the plugin's marketplace locally**

```bash
# Inside a Claude Code session:
/plugin marketplace add /abs/path/to/claude-code-plugin
/plugin install pebble@pebble-marketplace
```

Expected: Claude Code reports `pebble` installed.

- [ ] **Step 2: Verify MCP tools list**

Inside the CC session, issue:

```
/mcp
```

Expected: `pebble` server appears with tools: `memory_assert`, `memory_query`, `memory_touch`, `memory_retract`, `memory_read_cell`, `profile_read`, `profile_update`, `skill_save`, `skill_list`, `skill_read`, `trace_read`.

- [ ] **Step 3: Verify skills and commands appear**

Inside the CC session:

```
/help commands
```

Expected: `/pebble`, `/remember`, `/forget`, `/recall`, `/profile` are listed.

- [ ] **Step 4: Round-trip verification**

```
/remember my commit style is gitmoji prefix
/profile
/recall commit style
```

Expected:
1. `/remember` reports a saved skill or profile fact.
2. `/profile` shows the fact.
3. `/recall` returns the matching cell.

- [ ] **Step 5: Restart session to test hot-cache**

Quit Claude Code, start a new session. Expected: the new session's context includes the Pebble profile/skills/foresight block (check via a probe question like "what is my commit style?" — the model should answer without calling memory_query because the hot-cache already injected it).

- [ ] **Step 6: Commit nothing (this task is verification-only)**

```bash
# No file changes; skip commit.
```

---

## Task 18: Full-suite regression + final tag

**Files:** none.

- [ ] **Step 1: Run pebble-mcp full suite**

```bash
cd pebble-mcp && bun test && bun run typecheck
```

Expected: all tests pass. typecheck exits 0.

- [ ] **Step 2: Run plugin smoke test**

```bash
bash claude-code-plugin/tests/smoke.sh
```

Expected: `all hooks + plugin wiring OK`.

- [ ] **Step 3: Tag Plan 2 complete**

```bash
git tag -a pebble-cc-plugin-mvp-v0.0.1 -m "pebble CC plugin MVP v0.0.1 — hooks, skills, commands, reviewer"
git log --oneline pebble-mcp-mvp-v0.0.1..pebble-cc-plugin-mvp-v0.0.1
```

Expected: ~18 commits from Task 1 through Task 17 (Task 17 may have zero new commits).

---

## Definition of done (CC plugin plan)

- `pebble-mcp` has `commit-turn` and `review-turn` CLI commands; Plan 1 test suite still passes.
- `.claude-plugin/plugin.json` is valid and declares the MCP server.
- `skills/pebble`, `skills/pebble-query`, `skills/pebble-save` exist with correct frontmatter.
- `agents/pebble-reviewer.md` restricts tool access to `memory_query|memory_assert|memory_read_cell`.
- Commands `/pebble`, `/remember`, `/forget`, `/recall`, `/profile` exist and compile.
- All four hook scripts are executable, produce the expected JSON shapes, and integrate with `pebble-mcp`.
- `tests/smoke.sh` exits 0 green.
- Interactive CC install (Task 17) confirms the plugin loads, MCP tools are visible, commands run, and restart behavior injects hot-cache.

## What ships AFTER this plan (Plan 3)

- `docs/superpowers/plans/2026-04-22-pebble-03-droid-plugin.md` — Factory Droid plugin shell. Reuses the same `pebble-mcp` binary and CLI extensions added in Task 1 of this plan.
