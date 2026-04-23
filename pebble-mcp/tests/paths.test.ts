import { describe, expect, test } from "bun:test";
import { resolvePebbleRoot, logPath, dbPath, vaultPath, tracePath } from "../src/paths";

describe("paths", () => {
  test("resolvePebbleRoot honors PEBBLE_ROOT env", () => {
    process.env.PEBBLE_ROOT = "/tmp/test-pebble";
    expect(resolvePebbleRoot()).toBe("/tmp/test-pebble");
    delete process.env.PEBBLE_ROOT;
  });
  test("logPath includes log.jsonl", () => {
    process.env.PEBBLE_ROOT = "/tmp/test-pebble";
    expect(logPath()).toBe("/tmp/test-pebble/log.jsonl");
    delete process.env.PEBBLE_ROOT;
  });
  test("all paths resolve under root", () => {
    process.env.PEBBLE_ROOT = "/tmp/test-pebble";
    expect(dbPath()).toBe("/tmp/test-pebble/projection.db");
    expect(vaultPath()).toBe("/tmp/test-pebble/vault");
    expect(tracePath()).toBe("/tmp/test-pebble/trace.jsonl");
    delete process.env.PEBBLE_ROOT;
  });
});
