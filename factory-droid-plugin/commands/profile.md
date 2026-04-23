---
description: Show the current user profile — voice, stack, conventions.
allowed-tools: [profile_read]
---

# /profile

Call `profile_read {}` and render the result as a compact markdown view:

```
# Your Pebble profile

**Voice**
- Tone: <profile.voice.tone>
- Dos: <comma-sep list>
- Don'ts: <comma-sep list>

**Stack**
- Primary langs: <comma-sep>
- Frameworks: <comma-sep>
- Tools: <comma-sep>
- Never use: <comma-sep>

**Conventions**
- Commit: <profile.conventions.commit_style>
- Code: <profile.conventions.code_style>
- Test: <profile.conventions.test_style>
- Doc: <profile.conventions.doc_style>

_Last updated: <profile.updated_at>_
```

If any field is empty, write `_not set_`.
