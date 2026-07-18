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
