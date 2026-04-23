import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

let tmp: string;
const BIN = `bun run ${join(import.meta.dir, "..", "src", "index.ts")}`;

beforeEach(() => { tmp = mkdtempSync(join(tmpdir(), "pebble-cli-")); process.env.PEBBLE_ROOT = tmp; });
afterEach(() => { delete process.env.PEBBLE_ROOT; rmSync(tmp, { recursive: true, force: true }); });

describe("cli", () => {
  test("init creates directory + log + projection", () => {
    const out = execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } }).toString();
    expect(out).toContain("initialized");
    expect(existsSync(join(tmp, "log.jsonl"))).toBe(true);
    expect(existsSync(join(tmp, "projection.db"))).toBe(true);
    expect(existsSync(join(tmp, "vault"))).toBe(true);
  });

  test("status prints cell + event counts", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const out = execSync(`${BIN} status`, { env: { ...process.env, PEBBLE_ROOT: tmp } }).toString();
    expect(out).toMatch(/cells:\s*\d+/i);
    expect(out).toMatch(/events:\s*\d+/i);
  });

  test("verify replays log and asserts projection matches", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const out = execSync(`${BIN} verify`, { env: { ...process.env, PEBBLE_ROOT: tmp } }).toString();
    expect(out).toMatch(/ok|verified|match/i);
  });

  test("hot-cache-for-cc prints injection block with profile + top skills + foresight", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    execSync(`${BIN} seed-test-fixture`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const out = execSync(`${BIN} hot-cache-for-cc`, { env: { ...process.env, PEBBLE_ROOT: tmp } }).toString();
    expect(out).toContain("## Profile");
    expect(out).toContain("## Skills");
    expect(out).toContain("typescript");
  });
});
