import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

let tmp: string;
const BIN = `bun run ${join(import.meta.dir, "..", "src", "index.ts")}`;

beforeEach(() => { tmp = mkdtempSync(join(tmpdir(), "pebble-cti-")); process.env.PEBBLE_ROOT = tmp; });
afterEach(() => { delete process.env.PEBBLE_ROOT; rmSync(tmp, { recursive: true, force: true }); });

describe("commit-turn CLI", () => {
  test("commits all changes with gitmoji subject", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    writeFileSync(join(tmp, "trigger.txt"), "change");
    execSync(`${BIN} commit-turn --turn 7 --adds 2 --retracts 1 --actor claude-code`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    });
    const log = execSync("git log --oneline", { cwd: tmp }).toString();
    expect(log).toMatch(/:memo: pebble: turn 7 \+2 -1/);
  });

  test("no-op when nothing changed", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    // first commit captures init artifacts
    execSync(`${BIN} commit-turn --turn 1 --adds 0 --retracts 0`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    });
    // second call on clean tree should be a no-op
    const out = execSync(`${BIN} commit-turn --turn 2 --adds 0 --retracts 0`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out.toLowerCase()).toMatch(/nothing|no changes/);
  });
});
