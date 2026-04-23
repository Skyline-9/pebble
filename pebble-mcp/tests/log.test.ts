import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { appendEvents } from "../src/log/writer";
import { readEvents, countEvents } from "../src/log/reader";
import { logPath } from "../src/paths";
import type { MemEvent } from "../src/types";

let tmp: string;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-log-"));
  process.env.PEBBLE_ROOT = tmp;
});
afterEach(() => {
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

const sampleEvent = (ev: "assert" | "touch", id: string): MemEvent =>
  ev === "assert"
    ? { v: 1, ev, id, actor: "reviewer", ts: new Date().toISOString(), cell_id: "mc_a", cell: {} as any }
    : { v: 1, ev, id, actor: "system", ts: new Date().toISOString(), target: "mc_a" };

describe("log writer", () => {
  test("appends a batch of events to log.jsonl", async () => {
    await appendEvents([sampleEvent("assert", "ev_01"), sampleEvent("touch", "ev_02")]);
    const content = readFileSync(join(tmp, "log.jsonl"), "utf8");
    const lines = content.trim().split("\n");
    expect(lines).toHaveLength(2);
    expect(JSON.parse(lines[0]!).id).toBe("ev_01");
  });

  test("concurrent writes do not interleave within a batch", async () => {
    const batchA = Array.from({ length: 100 }, (_, i) => sampleEvent("touch", `ev_a_${i}`));
    const batchB = Array.from({ length: 100 }, (_, i) => sampleEvent("touch", `ev_b_${i}`));
    await Promise.all([appendEvents(batchA), appendEvents(batchB)]);
    const content = readFileSync(join(tmp, "log.jsonl"), "utf8");
    const lines = content.trim().split("\n");
    expect(lines).toHaveLength(200);
    const firstA = lines.findIndex(l => l.includes('"ev_a_0"'));
    const lastA = lines.findIndex(l => l.includes('"ev_a_99"'));
    const firstB = lines.findIndex(l => l.includes('"ev_b_0"'));
    const lastB = lines.findIndex(l => l.includes('"ev_b_99"'));
    const aBeforeB = lastA < firstB;
    const bBeforeA = lastB < firstA;
    expect(aBeforeB || bBeforeA).toBe(true);
  });
});

describe("log reader", () => {
  test("reads back what was written in order", async () => {
    const events = [sampleEvent("assert", "ev_01"), sampleEvent("touch", "ev_02"), sampleEvent("touch", "ev_03")];
    await appendEvents(events);
    const readBack: MemEvent[] = [];
    for await (const ev of readEvents()) readBack.push(ev);
    expect(readBack.map(e => e.id)).toEqual(["ev_01", "ev_02", "ev_03"]);
  });

  test("countEvents returns total event count", async () => {
    await appendEvents([sampleEvent("assert", "ev_01"), sampleEvent("touch", "ev_02")]);
    expect(await countEvents()).toBe(2);
  });

  test("reader handles empty log", async () => {
    const readBack: MemEvent[] = [];
    for await (const ev of readEvents()) readBack.push(ev);
    expect(readBack).toEqual([]);
  });

  test("reader skips malformed lines with warning", async () => {
    await appendEvents([sampleEvent("assert", "ev_01")]);
    const { appendFileSync } = await import("node:fs");
    appendFileSync(logPath(), "not-json\n");
    await appendEvents([sampleEvent("touch", "ev_02")]);
    const readBack: MemEvent[] = [];
    for await (const ev of readEvents()) readBack.push(ev);
    expect(readBack.map(e => e.id)).toEqual(["ev_01", "ev_02"]);
  });
});
