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
