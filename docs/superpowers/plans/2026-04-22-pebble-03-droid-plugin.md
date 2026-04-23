# Pebble — Plan 3: Factory Droid Plugin (`factory-droid-plugin`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Factory Droid plugin that wraps `pebble-mcp` into a first-class Droid experience — custom droids, slash commands, skills, AGENTS.md bootstrap, and hooks for hot-cache injection, background review, and per-turn git commits.

**Architecture:** Mirror of the CC plugin with Factory-native primitives. Primary `pebble` droid carries the hot-cache in its system prompt on session start; reviewer/judge droids are invoked via the Task tool for anti-recursion subagent work; hooks.json wires SessionStart/PostCompact/PostToolUse/Stop identically to CC; AGENTS.md at both project and personal levels bootstraps new sessions.

**Tech Stack:** Factory Droid plugin manifest (`.factory-plugin/plugin.json`), bash (POSIX) for hook scripts, `jq` for JSON, `pebble-mcp` binary (Plan 1 + Plan 2 Task 1 CLI extensions).

**Spec reference:** `docs/superpowers/specs/2026-04-22-pebble-design.md` (§7.3, §8).

**Prerequisites:**
- Plan 1 (`pebble-mcp`) complete and tagged `pebble-mcp-mvp-v0.0.1`.
- Plan 2 Task 1 complete: `pebble-mcp commit-turn` and `pebble-mcp review-turn` CLI commands available.
- `pebble-mcp init` has been run.
- `bun`, `jq`, `git`, and the `droid` CLI available on `PATH`.

---

## File structure

```
factory-droid-plugin/
├── .factory-plugin/
│   ├── plugin.json                  # Factory plugin manifest + MCP server
│   └── marketplace.json             # marketplace listing
├── droids/
│   ├── pebble.md                    # primary droid — memory-aware user-facing droid
│   └── pebble-reviewer.md           # subagent droid — background transcript reviewer
├── skills/
│   ├── pebble/SKILL.md              # orchestrator skill (shared concept with CC plugin)
│   ├── pebble-query/SKILL.md        # query composer skill
│   └── pebble-save/SKILL.md         # explicit-save skill
├── commands/
│   ├── pebble.md                    # /pebble status
│   ├── remember.md                  # /remember
│   ├── forget.md                    # /forget
│   ├── recall.md                    # /recall
│   └── profile.md                   # /profile
├── hooks/
│   ├── hooks.json
│   └── scripts/
│       ├── session-start.sh
│       ├── post-compact.sh
│       ├── post-tool-use.sh
│       └── stop.sh
├── AGENTS.md                        # project-level bootstrap (template)
├── personal-AGENTS.md.example       # copy to ~/.factory/AGENTS.md for personal bootstrap
└── tests/
    └── smoke.sh                     # manual install + tool-listing smoke test
```

Each file has one responsibility. Scripts stay ≤40 lines; droids are ≤80 lines of prompt.

---

## Task 1: Factory plugin scaffolding + manifest

**Files:**
- Create: `factory-droid-plugin/.factory-plugin/plugin.json`
- Create: `factory-droid-plugin/.factory-plugin/marketplace.json`
- Create: `factory-droid-plugin/.gitignore`

- [ ] **Step 1: Create `.factory-plugin/plugin.json`**

```json
{
  "name": "pebble",
  "version": "0.0.1",
  "description": "Super-personal memory for Factory Droid — profile, skills, knowledge that persist across sessions and compactions.",
  "author": { "name": "pebble", "email": "pebble@local" },
  "homepage": "https://github.com/pebble/pebble",
  "license": "MIT",
  "keywords": ["memory", "skills", "profile", "pebble"],
  "mcpServers": {
    "pebble": {
      "type": "stdio",
      "command": "pebble-mcp",
      "args": ["serve"],
      "env": { "PEBBLE_ROOT": "${HOME}/.pebble" },
      "disabled": false
    }
  }
}
```

- [ ] **Step 2: Create `.factory-plugin/marketplace.json`**

```json
{
  "name": "pebble-marketplace",
  "description": "Pebble — the personal memory substrate for Factory Droid.",
  "owner": { "name": "pebble", "email": "pebble@local" },
  "plugins": [
    {
      "name": "pebble",
      "description": "Super-personal memory for Factory Droid.",
      "version": "0.0.1",
      "source": "./",
      "author": { "name": "pebble", "email": "pebble@local" }
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

- [ ] **Step 4: Verify manifests parse**

Run:
```bash
cd factory-droid-plugin
jq . .factory-plugin/plugin.json
jq . .factory-plugin/marketplace.json
```
Expected: both print successfully.

- [ ] **Step 5: Commit**

```bash
git add factory-droid-plugin/.factory-plugin/ factory-droid-plugin/.gitignore
git commit -m ":seedling: pebble-droid: scaffold plugin manifest (Task 1)"
```

---

## Task 2: Primary droid — `droids/pebble.md`

The primary user-facing droid. Sets the system prompt to expect the hot-cache from the SessionStart hook.

**Files:**
- Create: `factory-droid-plugin/droids/pebble.md`

- [ ] **Step 1: Create pebble droid**

```markdown
---
name: pebble
description: >-
  Memory-aware droid. Reads the user's profile, skills, and active projects from Pebble on
  startup, recalls per-turn context via the memory_query MCP tool, and saves new facts via
  the pebble-save skill. Use this droid as your default working session when continuity
  across sessions matters.
model: inherit
---

# Pebble — memory-aware droid

You are Pebble. You have access to a persistent memory system via the `pebble` MCP server (tools: `memory_*`, `profile_*`, `skill_*`, `trace_read`).

## At the start of every reply

1. If the SessionStart hook injected a `<!-- pebble hot-cache ... -->` block into your context, trust and use those facts. Do NOT re-query them.
2. If the user's turn hints at past context ("where were we", "my usual approach", "what did I say about"), call `memory_query` with a concise 3–8-word query string. Load the `pebble-query` skill for query-composition guidance.
3. For every memory cell you actually use, call `memory_touch { cell_id, query }` to keep the recency signal fresh.

## When the user asks you to remember something

Load the `pebble-save` skill and follow it. The skill decides between `memory_assert` (facts/preferences/projects) and `skill_save` (reusable procedures).

## When you see a pattern the user repeats

Do NOT auto-save. The background reviewer (via `pebble-reviewer` subagent droid) handles implicit extraction. You only write on explicit user intent or when the reviewer prompts you.

## Using skills

Before generating code or writing commits:

1. Call `skill_list {}`. Scan descriptions for trigger matches.
2. If matched, call `skill_read { name }` and follow the body.

## What NOT to do

- Never call `memory_retract` unless the user explicitly used `/forget` or said "forget that".
- Never call `memory_assert` yourself — saves go through `pebble-save` or `pebble-reviewer`.
- Never dump the entire hot-cache back to the user unless they call `/pebble` or `/profile`.

## Observability

Every `memory_query` writes to `~/.pebble/trace.jsonl`. Suggest `/pebble` or `trace_read` to the user if retrieval looks off.
```

- [ ] **Step 2: Commit**

```bash
git add factory-droid-plugin/droids/pebble.md
git commit -m ":sparkles: pebble-droid: add primary pebble droid (Task 2)"
```

---

## Task 3: Reviewer droid — `droids/pebble-reviewer.md`

The anti-recursive subagent droid invoked by the Task tool from the main droid when the PostToolUse hook flags "time to review".

**Files:**
- Create: `factory-droid-plugin/droids/pebble-reviewer.md`

- [ ] **Step 1: Create reviewer droid**

```markdown
---
name: pebble-reviewer
description: >-
  Background memory reviewer. Extracts user preferences, profile updates, and project context
  from a conversation transcript and calls memory_assert. Invoked by the main droid via the
  Task tool when the PostToolUse hook signals it's time. Anti-recursive: restricted tool
  access, no skills, no slash commands.
model: inherit
---

# Pebble — background reviewer droid

You are invoked in the background to harvest long-lived facts from the current conversation. You are anti-recursive: do not call skills, do not run slash commands, do not invoke other Task subagents. Your only allowed tools are `memory_query`, `memory_assert`, and `memory_read_cell`.

## Input

Your prompt contains either:
- A JSONL transcript slice of the last N user turns, OR
- An instruction to call the `pebble-mcp review-turn --transcript <path>` CLI which does the extraction and assertion directly.

If a transcript path is given, prefer the CLI path — it runs the deterministic rule-based extractor. If a transcript slice is inlined, do the extraction yourself using the rules below.

## Extraction rules (when doing it yourself — AutoSkill principle)

For each user turn (IGNORE assistant and tool turns):

1. **Profile signals.** Pattern: "my (primary|main) language is X" → `profile` cell, confidence ≥0.9.
2. **Preference signals.** Pattern: "I prefer|like|use X for Y" → `preference` cell, confidence 0.75–0.9.
3. **Project signals.** Pattern: "I'm working on X" or "the X project" → `project` cell with foresight `t_end = now + 30d`, confidence 0.7.

## Procedure

For each candidate cell:

1. Dedup via `memory_query { query: <candidate.episode>, top_k: 3 }`. If top hit is a near-duplicate (same subject+object), DROP the candidate.
2. Otherwise call `memory_assert { type, episode, facts, confidence, actor: "reviewer" }`.

## Output

Plain-text summary: `{ asserted: N, dropped: M, notes: [...] }`. The main droid will surface a condensed version to the user if relevant.

## Hard rules

- Never call `memory_retract`.
- Never call `skill_save`. Skills are authored by the user through `/remember` or the `pebble-save` skill.
- Never call any tool outside `memory_query | memory_assert | memory_read_cell`.
- Never invoke another Task subagent.
```

- [ ] **Step 2: Commit**

```bash
git add factory-droid-plugin/droids/pebble-reviewer.md
git commit -m ":sparkles: pebble-droid: add background reviewer droid (Task 3)"
```

---

## Task 4: Skills — pebble orchestrator, pebble-query, pebble-save

The skill bodies are nearly identical to the CC plugin (Plan 2 Tasks 3–5). They live here so Factory Droid can find them at `skills/<name>/SKILL.md` relative to this plugin root.

**Files:**
- Create: `factory-droid-plugin/skills/pebble/SKILL.md`
- Create: `factory-droid-plugin/skills/pebble-query/SKILL.md`
- Create: `factory-droid-plugin/skills/pebble-save/SKILL.md`

- [ ] **Step 1: Create `skills/pebble/SKILL.md`**

```markdown
---
name: pebble
description: >-
  Use the Pebble memory system — store and recall user preferences, active projects, skills,
  and foresight. Activate when the user mentions preferences, projects, past conversations,
  "remember", "forget", or when context continuity across sessions matters.
allowed-tools: [memory_assert, memory_query, memory_touch, memory_retract, memory_read_cell, profile_read, skill_list, skill_read, trace_read]
---

# Pebble — memory orchestrator

You have access to Pebble, a persistent memory system backed by an append-only event log and a queryable SQLite projection. Use it to make the user feel like they have a continuous working relationship with you.

## When to CALL `memory_query`

- The user asks about a past conversation ("did I say...", "what did we decide...")
- The user's request would benefit from knowing their preferences or active projects
- You are about to make a decision that depends on style conventions
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

- Do not call `memory_assert` yourself. Saving is done by the `pebble-save` skill, the `/remember` command, and the `pebble-reviewer` droid.
- Do not bulk-retract cells. Use `/forget` for explicit user intent only.

## Observability

Every `memory_query` writes to `trace.jsonl`. The user can inspect via `trace_read` for debugging retrieval quality.
```

- [ ] **Step 2: Create `skills/pebble-query/SKILL.md`**

```markdown
---
name: pebble-query
description: >-
  Compose a high-quality memory query. Activate when the user's turn would benefit from
  recalling stored context and you need to call memory_query with a well-formed query string.
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

Do not call `memory_query` more than twice per user turn. If two queries return nothing relevant, continue without memory and let the `pebble-reviewer` droid pick up any new context later.
```

- [ ] **Step 3: Create `skills/pebble-save/SKILL.md`**

```markdown
---
name: pebble-save
description: >-
  Save an explicit memory. Activate when the user says "remember", "save", "note that",
  "going forward", or when they've explicitly articulated a preference that should persist
  across sessions.
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
  - `episodic`: one-time fact worth recording. Threshold 0.5.
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

- [ ] **Step 4: Commit**

```bash
git add factory-droid-plugin/skills/
git commit -m ":sparkles: pebble-droid: add pebble/query/save skills (Task 4)"
```

---

## Task 5: Slash commands — /pebble, /remember, /forget, /recall, /profile

Each is a self-contained `.md` file. The shape matches what Factory Droid understands: YAML frontmatter (`description`, optional `argument-hint`, optional `allowed-tools`) plus a body that instructs the droid what to do on invocation.

**Files:**
- Create: `factory-droid-plugin/commands/pebble.md`
- Create: `factory-droid-plugin/commands/remember.md`
- Create: `factory-droid-plugin/commands/forget.md`
- Create: `factory-droid-plugin/commands/recall.md`
- Create: `factory-droid-plugin/commands/profile.md`

- [ ] **Step 1: Create `commands/pebble.md`**

```markdown
---
description: Show Pebble memory status — cell count, event count, top skills, active foresight.
allowed-tools: [Execute]
---

# /pebble

Run `pebble-mcp status` and render the output as a compact dashboard.

Procedure:

1. Execute: `pebble-mcp status`
2. Execute: `pebble-mcp hot-cache-for-droid | head -40`
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

- [ ] **Step 2: Create `commands/remember.md`**

```markdown
---
description: Explicitly remember something. Uses the pebble-save skill.
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

- [ ] **Step 3: Create `commands/forget.md`**

```markdown
---
description: Forget a stored fact. Retracts the matching cell; the event stays in the log.
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

Important: retractions are soft — the event stays in `log.jsonl` for audit.
```

- [ ] **Step 4: Create `commands/recall.md`**

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

- [ ] **Step 5: Create `commands/profile.md`**

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

- [ ] **Step 6: Commit**

```bash
git add factory-droid-plugin/commands/
git commit -m ":sparkles: pebble-droid: add /pebble /remember /forget /recall /profile (Task 5)"
```

---

## Task 6: `hooks/hooks.json` + SessionStart script

**Files:**
- Create: `factory-droid-plugin/hooks/hooks.json`
- Create: `factory-droid-plugin/hooks/scripts/session-start.sh`

- [ ] **Step 1: Create hooks.json**

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume|clear|compact",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/session-start.sh",
            "async": false
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
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/post-compact.sh",
            "async": false
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit|Create",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/post-tool-use.sh",
            "async": true
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
            "command": "${CLAUDE_PLUGIN_ROOT}/hooks/scripts/stop.sh",
            "async": false
          }
        ]
      }
    ]
  }
}
```

> Factory Droid honors `${CLAUDE_PLUGIN_ROOT}` for compatibility with the CC plugin shape. If Droid-specific env vars differ in a future release, adjust once in this file.

- [ ] **Step 2: Create `hooks/scripts/session-start.sh`**

```bash
#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/session-start.sh
# Emits JSON with additionalContext for Factory Droid to inject into the system prompt.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-droid 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

# Factory Droid accepts both the hookSpecificOutput shape (CC-compat) and the SDK top-level
# additionalContext shape. We emit hookSpecificOutput which Droid handles first.
jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $ctx}}'
```

- [ ] **Step 3: Make executable + verify**

```bash
chmod +x factory-droid-plugin/hooks/scripts/session-start.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  out="$(./factory-droid-plugin/hooks/scripts/session-start.sh)"
  echo "$out" | jq -e '.hookSpecificOutput.additionalContext | test("Profile"; "i")'
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: `jq -e` exits 0.

- [ ] **Step 4: Commit**

```bash
git add factory-droid-plugin/hooks/hooks.json factory-droid-plugin/hooks/scripts/session-start.sh
git commit -m ":sparkles: pebble-droid: add hooks.json + SessionStart script (Task 6)"
```

---

## Task 7: PostCompact script

**Files:**
- Create: `factory-droid-plugin/hooks/scripts/post-compact.sh`

- [ ] **Step 1: Create post-compact.sh**

```bash
#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/post-compact.sh
# After compaction, re-inject profile + top skills + active foresight.
set -euo pipefail

CACHE="$(pebble-mcp hot-cache-for-droid 2>/dev/null || echo "")"

if [ -z "$CACHE" ]; then
  exit 0
fi

jq -n --arg ctx "$CACHE" \
  '{hookSpecificOutput: {hookEventName: "PostCompact", additionalContext: $ctx}}'
```

- [ ] **Step 2: Make executable + verify**

```bash
chmod +x factory-droid-plugin/hooks/scripts/post-compact.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  ./factory-droid-plugin/hooks/scripts/post-compact.sh | jq -e '.hookSpecificOutput.hookEventName == "PostCompact"'
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add factory-droid-plugin/hooks/scripts/post-compact.sh
git commit -m ":sparkles: pebble-droid: add PostCompact script (Task 7)"
```

---

## Task 8: PostToolUse script — round-tick and reviewer flag

**Files:**
- Create: `factory-droid-plugin/hooks/scripts/post-tool-use.sh`

- [ ] **Step 1: Create post-tool-use.sh**

```bash
#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/post-tool-use.sh
# Every matched tool use increments the round counter. At threshold, flag the pebble-reviewer.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
COUNTER_FILE="$ROOT/.droid-rounds"
THRESHOLD="${PEBBLE_REVIEW_EVERY:-8}"

mkdir -p "$ROOT"
[ -f "$COUNTER_FILE" ] || echo "0" > "$COUNTER_FILE"

current="$(cat "$COUNTER_FILE")"
next=$((current + 1))
echo "$next" > "$COUNTER_FILE"

if (( next % THRESHOLD != 0 )); then
  exit 0
fi

payload="$(cat || true)"
transcript="$(echo "$payload" | jq -r '.transcript_path // empty' 2>/dev/null || true)"

if [ -n "$transcript" ] && [ -f "$transcript" ]; then
  MSG="Pebble: time to review. Invoke the pebble-reviewer subagent via the Task tool (subagent_type=pebble-reviewer) with the transcript path \`$transcript\`. The reviewer will call \`pebble-mcp review-turn --transcript $transcript\`."
else
  MSG="Pebble: time to review. Invoke the pebble-reviewer subagent via the Task tool (subagent_type=pebble-reviewer) and pass a summary of the last ${THRESHOLD} user turns."
fi

jq -n --arg ctx "$MSG" \
  '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $ctx}}'
```

- [ ] **Step 2: Make executable + verify round behavior**

```bash
chmod +x factory-droid-plugin/hooks/scripts/post-tool-use.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  export PEBBLE_REVIEW_EVERY=3
  mkdir -p "$PEBBLE_ROOT"
  out1="$(./factory-droid-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"
  out2="$(./factory-droid-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"
  out3="$(./factory-droid-plugin/hooks/scripts/post-tool-use.sh < /dev/null)"

  test -z "$out1" || (echo "expected empty on round 1"; exit 1)
  test -z "$out2" || (echo "expected empty on round 2"; exit 1)
  echo "$out3" | jq -e '.hookSpecificOutput.additionalContext | test("pebble-reviewer"; "i")'

  rm -rf "$PEBBLE_ROOT"
)
```

Expected: only round 3 emits JSON containing "pebble-reviewer". `jq -e` exits 0.

- [ ] **Step 3: Commit**

```bash
git add factory-droid-plugin/hooks/scripts/post-tool-use.sh
git commit -m ":sparkles: pebble-droid: add PostToolUse round-tick script (Task 8)"
```

---

## Task 9: Stop script — commit turn

**Files:**
- Create: `factory-droid-plugin/hooks/scripts/stop.sh`

- [ ] **Step 1: Create stop.sh**

```bash
#!/usr/bin/env bash
# factory-droid-plugin/hooks/scripts/stop.sh
# On Stop, compute turn + adds/retracts delta, call pebble-mcp commit-turn, reset round counter.
set -euo pipefail

ROOT="${PEBBLE_ROOT:-$HOME/.pebble}"
TURN_FILE="$ROOT/.droid-turn"
LAST_EVT_FILE="$ROOT/.droid-last-event-count"

mkdir -p "$ROOT"

[ -f "$TURN_FILE" ] || echo "0" > "$TURN_FILE"
turn=$(( $(cat "$TURN_FILE") + 1 ))
echo "$turn" > "$TURN_FILE"

current_events=0
if [ -f "$ROOT/log.jsonl" ]; then
  current_events="$(wc -l < "$ROOT/log.jsonl" | tr -d ' ')"
fi
[ -f "$LAST_EVT_FILE" ] || echo "0" > "$LAST_EVT_FILE"
previous_events="$(cat "$LAST_EVT_FILE")"
echo "$current_events" > "$LAST_EVT_FILE"

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

pebble-mcp commit-turn --turn "$turn" --adds "$adds" --retracts "$retracts" --actor factory-droid || true

echo "0" > "$ROOT/.droid-rounds"
```

- [ ] **Step 2: Make executable + verify**

```bash
chmod +x factory-droid-plugin/hooks/scripts/stop.sh

(
  export PEBBLE_ROOT="$(mktemp -d)"
  pebble-mcp init
  pebble-mcp seed-test-fixture
  ./factory-droid-plugin/hooks/scripts/stop.sh
  log="$(cd "$PEBBLE_ROOT" && git log --oneline)"
  echo "$log" | grep -qE ":memo: pebble: turn 1 \+2 -0"
  # Confirm the actor trailer
  latest="$(cd "$PEBBLE_ROOT" && git log -1 --format=%B)"
  echo "$latest" | grep -q "actor: factory-droid"
  rm -rf "$PEBBLE_ROOT"
)
```

Expected: both greps succeed.

- [ ] **Step 3: Commit**

```bash
git add factory-droid-plugin/hooks/scripts/stop.sh
git commit -m ":sparkles: pebble-droid: add Stop commit-turn script (Task 9)"
```

---

## Task 10: AGENTS.md bootstrap (project + personal template)

Bootstraps both a project-level and a personal-level AGENTS.md so Factory Droid loads Pebble context reliably.

**Files:**
- Create: `factory-droid-plugin/AGENTS.md`
- Create: `factory-droid-plugin/personal-AGENTS.md.example`

- [ ] **Step 1: Create project `AGENTS.md`**

```markdown
# Pebble-enabled Factory Droid project

This project uses the Pebble memory system. The `pebble-mcp` server provides persistent memory across sessions via MCP tools (`memory_*`, `profile_*`, `skill_*`).

## When you start a session

1. The SessionStart hook (`hooks/scripts/session-start.sh`) injects a hot-cache block with the user's profile, top skills, and active foresight. Trust those facts.
2. Load the `pebble` skill from this plugin to learn when to call `memory_query`, `memory_touch`, `profile_read`, and `skill_list`.
3. For explicit saves, load the `pebble-save` skill.

## When the PostToolUse hook signals

If you see a hint in your context about invoking `@pebble-reviewer`, use the Task tool with `subagent_type: "pebble-reviewer"` — the reviewer is anti-recursive and will harvest long-lived facts from recent turns.

## Hard rules

- Never call `memory_retract` without explicit user intent (the `/forget` slash command).
- Never call `memory_assert` directly from the main droid — that's what `pebble-save` and the reviewer are for.
- Git commits to `~/.pebble/` happen automatically on Stop. Do NOT hand-commit there.

## Slash commands

- `/pebble` — show memory status
- `/remember <text>` — explicit save
- `/forget <query>` — retract a cell
- `/recall <query>` — search memory
- `/profile` — show user profile
```

- [ ] **Step 2: Create `personal-AGENTS.md.example`**

```markdown
# Personal Factory Droid — Pebble-enabled

Copy this file to `~/.factory/AGENTS.md` (or append to an existing one) so every Factory Droid session picks up the Pebble bootstrap.

---

You have access to the Pebble memory system via the `pebble` MCP server. When starting any session:

1. Expect a Pebble hot-cache block at the top of your context. It contains the user's profile, top skills, and active foresight. Use it.
2. Load the `pebble` skill when the user's turn hints at past context.
3. Save facts ONLY when the user explicitly asks (`/remember`) — otherwise let the `pebble-reviewer` droid harvest in the background.

Reference: `pebble-mcp status` gives you the current cell/event/skill counts.
```

- [ ] **Step 3: Commit**

```bash
git add factory-droid-plugin/AGENTS.md factory-droid-plugin/personal-AGENTS.md.example
git commit -m ":memo: pebble-droid: add AGENTS.md bootstraps (Task 10)"
```

---

## Task 11: Local install smoke test

**Files:**
- Create: `factory-droid-plugin/tests/smoke.sh`

- [ ] **Step 1: Create smoke.sh**

```bash
#!/usr/bin/env bash
# factory-droid-plugin/tests/smoke.sh
# Smoke test: init, seed, run hooks, verify vault + git + commit actor.
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

echo "==> PostToolUse hook (rounds 1-2; only round 2 emits)"
out1="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
out2="$("$PLUGIN_DIR/hooks/scripts/post-tool-use.sh" < /dev/null)"
test -z "$out1"
echo "$out2" | jq -e '.hookSpecificOutput.additionalContext | test("pebble-reviewer"; "i")' > /dev/null
echo "    ok"

echo "==> Stop hook (commit-turn as factory-droid)"
"$PLUGIN_DIR/hooks/scripts/stop.sh"
latest="$(cd "$ROOT" && git log -1 --format=%B)"
echo "$latest" | grep -qE ":memo: pebble: turn 1 \+[0-9]+ -[0-9]+"
echo "$latest" | grep -q "actor: factory-droid"
echo "    ok"

echo "==> pebble-mcp verify"
pebble-mcp verify

echo "==> review-turn CLI end-to-end"
transcript="$(mktemp)"
cat > "$transcript" <<'JSONL'
{"role":"user","content":"I prefer TypeScript for backend services."}
{"role":"assistant","content":"Noted."}
{"role":"user","content":"My primary language is Python actually."}
JSONL
pebble-mcp review-turn --transcript "$transcript" | grep -qE "asserted: [1-9]"
rm -f "$transcript"
echo "    ok"

echo "==> all hooks + plugin wiring OK"
rm -rf "$ROOT"
```

- [ ] **Step 2: Run smoke test**

```bash
chmod +x factory-droid-plugin/tests/smoke.sh
bash factory-droid-plugin/tests/smoke.sh
```

Expected: prints `all hooks + plugin wiring OK` and exits 0.

- [ ] **Step 3: Commit**

```bash
git add factory-droid-plugin/tests/smoke.sh
git commit -m ":white_check_mark: pebble-droid: add plugin smoke test (Task 11)"
```

---

## Task 12: Register plugin locally (interactive verification)

Human-verified. Confirms the plugin loads inside Factory Droid.

**Files:** none.

- [ ] **Step 1: Add the plugin's marketplace locally**

From a terminal with the Droid CLI:

```bash
droid plugin marketplace add /abs/path/to/factory-droid-plugin
droid plugin install pebble@pebble-marketplace
```

Expected: Droid reports `pebble` installed. It appears in `droid plugin list`.

- [ ] **Step 2: Start a session using the pebble droid**

```bash
droid -d pebble
```

Inside the session:

```
/mcp
```

Expected: `pebble` MCP server appears with tools: `memory_*`, `profile_*`, `skill_*`, `trace_read`.

- [ ] **Step 3: Verify commands**

```
/help commands
```

Expected: `/pebble`, `/remember`, `/forget`, `/recall`, `/profile` are listed.

- [ ] **Step 4: Round-trip**

```
/remember my commit style is gitmoji prefix
/profile
/recall commit style
```

Expected:
1. `/remember` reports a saved fact.
2. `/profile` shows the fact.
3. `/recall` returns the matching cell.

- [ ] **Step 5: Subagent invocation**

Have the user run a few tool calls to trigger the PostToolUse reviewer. Confirm the main droid sees the "invoke pebble-reviewer" hint and uses the Task tool.

```
# Make a few file edits or runs; the PostToolUse threshold fires.
# Watch for the droid invoking the Task tool with subagent_type=pebble-reviewer.
```

Expected: Task(`pebble-reviewer`) runs, outputs `{ asserted: N, dropped: M, notes: [...] }`. At session end, the Stop hook commits a turn with `actor: factory-droid`.

- [ ] **Step 6: Restart session to test hot-cache**

Exit. Start a new `droid -d pebble` session. Probe: "What is my commit style?" Expected: the droid answers without calling `memory_query` (the SessionStart hot-cache covers it).

- [ ] **Step 7: No commit (verification-only)**

```bash
# Skip commit.
```

---

## Task 13: Full-suite regression + final tag

**Files:** none.

- [ ] **Step 1: Run pebble-mcp full suite**

```bash
cd pebble-mcp && bun test && bun run typecheck
```

Expected: all tests pass; typecheck exits 0.

- [ ] **Step 2: Run CC plugin smoke test**

```bash
bash claude-code-plugin/tests/smoke.sh
```

Expected: exits 0.

- [ ] **Step 3: Run Droid plugin smoke test**

```bash
bash factory-droid-plugin/tests/smoke.sh
```

Expected: exits 0.

- [ ] **Step 4: Tag Plan 3 complete**

```bash
git tag -a pebble-droid-plugin-mvp-v0.0.1 -m "pebble Droid plugin MVP v0.0.1 — hooks, droids, skills, commands, reviewer"
git log --oneline pebble-cc-plugin-mvp-v0.0.1..pebble-droid-plugin-mvp-v0.0.1
```

Expected: ~12 commits from Task 1 through Task 12 (Task 12 has zero new commits).

- [ ] **Step 5: Release the full trio**

```bash
git tag -a pebble-mvp-v0.0.1 -m "Pebble MVP v0.0.1 — pebble-mcp core + CC plugin + Droid plugin"
```

---

## Definition of done (Droid plugin plan)

- `.factory-plugin/plugin.json` is valid and declares the `pebble` MCP server.
- `droids/pebble.md` and `droids/pebble-reviewer.md` exist with correct frontmatter and bounded tool access.
- `skills/pebble`, `skills/pebble-query`, `skills/pebble-save` exist with correct frontmatter.
- Commands `/pebble`, `/remember`, `/forget`, `/recall`, `/profile` exist and compile.
- All four hook scripts are executable, produce the expected JSON shapes, and integrate with `pebble-mcp`.
- `tests/smoke.sh` exits 0 green.
- Interactive Droid install (Task 12) confirms the plugin loads, MCP tools are visible, commands run, subagent invocation works, and restart behavior injects hot-cache.
- Git commits from this plugin carry `actor: factory-droid` in the trailer.

## Cross-platform contract (end-state of all three plans)

When Plans 1, 2, and 3 are complete:

- `pebble-mcp` is the single MCP server. Both CC and Droid connect to it.
- The same `~/.pebble/` substrate serves both platforms. Cells saved in CC show up in Droid, and vice-versa.
- Git commits from either platform land in `~/.pebble/.git/` with an `actor:` trailer identifying the source.
- `log.jsonl` is append-only, `flock`'d across writers. Concurrent CC + Droid sessions do not corrupt state.
- Retrieval traces (`trace.jsonl`) include entries from both platforms — the full recall history is one stream.

This fulfills the spec's cross-platform thesis (§7.4): one memory, three surfaces, two platforms, one user.
