# Design Spec: Surgical Repository Migration to GitHub

**Date:** 2026-05-05
**Status:** Approved
**Topic:** Migrating GitLab repo `pebble` to GitHub `Skyline-9/pebble` with history rewriting.

## 1. Goal
Migrate the `pebble` repository from GitLab to GitHub while ensuring all historical commits are attributed to the user's personal email (`luo.richard@gmail.com`) instead of the work email (`Richard.Luo4@T-mobile.com`).

## 2. Approach: Surgical History Rewrite
We will use `git filter-branch` to perform a bulk update of the author and committer metadata across the entire history.

### Workflow Steps

1.  **Identity Rewriting:**
    *   Execute `git filter-branch --env-filter` to swap identities.
    *   Old Identity: `Richard Luo <Richard.Luo4@T-mobile.com>`
    *   New Identity: `Richard Luo <luo.richard@gmail.com>`
    *   Flags: `--tag-name-filter cat -- --all` to ensure all branches and tags are updated.

2.  **GitHub Repository Creation:**
    *   Command: `gh repo create pebble --private --source=. --remote=origin --push`
    *   Note: If `origin` already exists, we will update it using `git remote set-url origin`.

3.  **Verification:**
    *   Check `git log` to confirm the author/email change.
    *   Confirm the new remote URL points to GitHub.
    *   Force push rewritten history if `gh repo create` doesn't handle the force-push of rewritten hashes automatically.

## 3. Architecture & Tools
*   **VCS:** Git
*   **CLI:** GitHub CLI (`gh`)
*   **Identity Mapping:**
    *   `GIT_AUTHOR_EMAIL`
    *   `GIT_AUTHOR_NAME`
    *   `GIT_COMMITTER_EMAIL`
    *   `GIT_COMMITTER_NAME`

## 4. Safety & Rollback
*   Git's native `refs/original/` backup will be preserved until the migration is confirmed successful.
*   Verification of `git log` is a mandatory gate before pushing to GitHub.

## 5. Success Criteria
*   GitHub repository `Skyline-9/pebble` exists and is private.
*   All commits in the `main` branch on GitHub show `luo.richard@gmail.com` as the author.
*   The GitLab remote is either removed or renamed to `gitlab`.
