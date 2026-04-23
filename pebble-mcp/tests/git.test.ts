import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { commitTurn, ensureGitRepo } from "../src/git/turn-commit";
import { resolvePebbleRoot } from "../src/paths";

let tmp: string;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-git-"));
  process.env.PEBBLE_ROOT = tmp;
  mkdirSync(tmp, { recursive: true });
});
afterEach(() => {
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("turn-commit", () => {
  test("ensureGitRepo initializes git if missing", () => {
    ensureGitRepo();
    const out = execSync("git status", { cwd: resolvePebbleRoot() }).toString();
    expect(out).toContain("On branch");
  });

  test("commitTurn commits all changes with gitmoji subject", () => {
    ensureGitRepo();
    writeFileSync(join(tmp, "file.txt"), "hello");
    const result = commitTurn({ turn: 42, adds: 3, retracts: 1 });
    expect(result.committed).toBe(true);
    const log = execSync("git log --oneline", { cwd: resolvePebbleRoot() }).toString();
    expect(log).toMatch(/:memo: pebble: turn 42 \+3 -1/);
  });

  test("commitTurn with no changes is a no-op", () => {
    ensureGitRepo();
    const result = commitTurn({ turn: 1, adds: 0, retracts: 0 });
    expect(result.committed).toBe(false);
  });
});
