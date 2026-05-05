# Repository Migration and History Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the `pebble` repository from GitLab to GitHub with rewritten commit history (personal email attribution).

**Architecture:** Use `git filter-branch` to surgically replace author/committer emails across all commits, then provision a new GitHub repo via `gh` CLI and push the rewritten history.

**Tech Stack:** Git, GitHub CLI (`gh`).

---

### Task 1: History Rewriting

**Files:**
- Modify: Git internal history (all branches)

- [ ] **Step 1: Rewrite author and committer emails**

```powershell
git filter-branch --env-filter '
OLD_EMAIL="Richard.Luo4@T-mobile.com"
CORRECT_NAME="Richard Luo"
CORRECT_EMAIL="luo.richard@gmail.com"
if [ "$GIT_COMMITTER_EMAIL" = "$OLD_EMAIL" ]
then
    export GIT_COMMITTER_NAME="$CORRECT_NAME"
    export GIT_COMMITTER_EMAIL="$CORRECT_EMAIL"
fi
if [ "$GIT_AUTHOR_EMAIL" = "$OLD_EMAIL" ]
then
    export GIT_AUTHOR_NAME="$CORRECT_NAME"
    export GIT_AUTHOR_EMAIL="$CORRECT_EMAIL"
fi
' --tag-name-filter cat -- --all
```

- [ ] **Step 2: Verify the rewrite locally**

Run: `git log -n 10 --format='%h %an <%ae> %s'`
Expected: All listed commits show `Richard Luo <luo.richard@gmail.com>`

- [ ] **Step 3: Commit the change (internal git state)**
Note: `filter-branch` is already a permanent change to the local history. No manual commit needed, but we should confirm `refs/original/` exists as a backup.
Run: `ls .git/refs/original/`
Expected: Directory exists or refs are listed.

### Task 2: GitHub Repository Provisioning

**Files:**
- Modify: Git remotes

- [ ] **Step 1: Create the new GitHub repository**

Run: `gh repo create pebble --private --source=. --remote=origin --push`
Expected: Repo created on GitHub, remote `origin` updated, and initial push attempted.

- [ ] **Step 2: Handle remote URL conflict (if necessary)**
If `gh repo create` fails because `origin` already exists and points to GitLab:
Run: `git remote rename origin gitlab; gh repo create pebble --private --source=. --remote=origin --push`

- [ ] **Step 3: Force push rewritten history to GitHub**
Since hashes changed, a force push is required for all branches.
Run: `git push origin main --force`
Expected: `main` branch updated on GitHub with new hashes.

### Task 3: Final Verification and Cleanup

**Files:**
- Modify: Git remotes

- [ ] **Step 1: Verify GitHub remote logs**

Run: `gh repo view --web` (Wait for browser or check CLI output)
Alternatively, check via CLI: `git log origin/main -n 5 --format='%h %an <%ae> %s'`
Expected: Remote commits match local rewritten commits.

- [ ] **Step 2: Remove GitLab remote (Optional but recommended)**

Run: `git remote remove gitlab`
Expected: Only `origin` (GitHub) remains.

- [ ] **Step 3: Confirm success**

Run: `git remote -v`
Expected: `origin` points to `https://github.com/Skyline-9/pebble.git`
