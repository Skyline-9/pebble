---
description: Search this repository's code and knowledge notes with citations.
argument-hint: <query>
allowed-tools: [search, evidence_read]
---

# /search

The user wants to search this repository's code and notes. Arguments: `$ARGUMENTS`.

Procedure:

1. Call `search { query: "$ARGUMENTS", repository: ".", budget_tokens: 6000, max_results: 8 }`.
2. Read each returned candidate's citation (file path and line range, symbol, or note claim and revision).
3. For any citation you want to quote directly, call `evidence_read { repository: ..., revision: ..., path: ..., start_line: ..., end_line: ... }` to fetch the exact lines.
4. Synthesize a grounded answer. Cite every claim as `path:start-end` (code) or `note_id#claim` (notes). Never state a fact from the evidence packet without its citation.
5. If the evidence is incomplete or contradictory, say so explicitly instead of guessing.

If there are no results: "No matches for '$ARGUMENTS' in this repository's index. Try `/pebble` to check index health, or broaden the query."
