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
