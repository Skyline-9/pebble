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
