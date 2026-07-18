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
