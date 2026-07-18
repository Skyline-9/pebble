---
description: Show Pebble index health for this repository — generation, freshness, and issues.
allowed-tools: [Execute]
---

# /pebble

Report this repository's Pebble index health as a compact dashboard.

Procedure:

1. Read `.pebble/pebble.toml` to get `repository_id`. If the file is missing, tell the user "This repository is not registered with Pebble — run `pebble init .` and `pebble register .` to begin" and stop.
2. Execute: `pebble health --repository <repository_id> --json`
3. Execute: `pebble traces --repository <repository_id> --limit 5 --json`
4. Render the health and recent traces as a compact block.
5. If `healthy` is `false`, tell the user the reported issue and suggest `pebble index .` to rebuild.

Example response shape:

```
**Pebble status** (repository_id: acme.widgets)

| Item | Value |
| --- | --- |
| Healthy | yes |
| Generation | 01J... |

**Recent searches**: "auth session validation", "retry backoff config"
```
