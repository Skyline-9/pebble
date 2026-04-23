---
description: Show Pebble memory status — cell count, event count, top skills, active foresight.
allowed-tools: [Bash]
---

# /pebble

Run the `pebble-mcp status` CLI and render the output as a compact dashboard.

Procedure:

1. Run: `pebble-mcp status`
2. Run: `pebble-mcp hot-cache-for-cc | head -40`
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
