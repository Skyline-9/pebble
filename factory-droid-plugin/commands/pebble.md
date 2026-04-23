---
description: Show Pebble memory status — cell count, event count, top skills, active foresight.
allowed-tools: [Execute]
---

# /pebble

Run `pebble-mcp status` and render the output as a compact dashboard.

Procedure:

1. Execute: `pebble-mcp status`
2. Execute: `pebble-mcp hot-cache-for-droid | head -40`
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
