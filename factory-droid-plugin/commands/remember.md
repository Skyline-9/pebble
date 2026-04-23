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
