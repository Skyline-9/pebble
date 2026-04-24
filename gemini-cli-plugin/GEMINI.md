# Pebble-enabled Gemini CLI project

This project uses the Pebble memory system. The `pebble-mcp` server provides persistent memory across sessions via MCP tools (`memory_*`, `profile_*`, `skill_*`).

## When you start a session

1. The `SessionStart` hook (`hooks/scripts/session-start.sh`) injects a hot-cache block (via `hookSpecificOutput.additionalContext`) with the user's profile, top skills, and active foresight. Trust those facts.
2. Load the `pebble` skill from this extension to learn when to call `memory_query`, `memory_touch`, `profile_read`, and `skill_list`.
3. For explicit saves, load the `pebble-save` skill.

## When the AfterTool hook signals

If you see a hint in your context about invoking `pebble-reviewer`, delegate to the `pebble-reviewer` sub-agent from this extension. The reviewer is anti-recursive and will harvest long-lived facts from recent turns.

## Hard rules

- Never call `memory_retract` without explicit user intent (the `/forget` slash command).
- Never call `memory_assert` directly from the main agent — that's what `pebble-save` and the reviewer are for.
- Git commits to `~/.pebble/` happen automatically on `AfterAgent`. Do NOT hand-commit there.

## Slash commands

- `/pebble` — show memory status
- `/remember <text>` — explicit save
- `/forget <query>` — retract a cell
- `/recall <query>` — search memory
- `/profile` — show user profile
