---
description: Promote a personal Pebble note into this repository's shared knowledge.
argument-hint: <personal note id or topic>
allowed-tools: [personal_note_create, personal_note_promote, note_list]
---

# /promote

The user wants to share a personal note with this repository. Arguments: `$ARGUMENTS`.

Procedure:

1. If `$ARGUMENTS` names an existing personal note id, use it. Otherwise call `personal_note_create { ... }` first to capture the note under `~/.pebble/v1/personal/knowledge/` from the user's description, then use the returned note id.
2. Call `personal_note_promote { note_id: <id>, repository: "." }`.
3. Show the user the resulting diff against `.pebble/knowledge/` before anything is written, and ask for explicit confirmation.
4. On confirmation, apply the promotion. On refusal, leave the personal note untouched and stop.
5. Confirm the new repository note path and claim id to the user. This is an ordinary working-tree diff for them to review and commit.

If `$ARGUMENTS` is empty, ask: "Which personal note should I promote, or what should it say?" and stop.
