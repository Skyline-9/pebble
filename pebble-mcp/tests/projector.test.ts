import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { projectEvent, projectAll } from "../src/projection/projector";
import type { MemCell, AssertEvent, TouchEvent, RetractEvent } from "../src/types";

let tmp: string;
let db: Database;

const sampleCell = (id: string, type: MemCell["type"] = "preference"): MemCell => ({
  id,
  type,
  E: `Episode for ${id}`,
  F: [{ subject: "x", predicate: "is", object: "y", confidence: 0.8 }],
  M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
  confidence: 0.8,
  evidence: [],
  scene_ids: [],
  access: { count: 0, last_at: null },
});

const assertE = (id: string, cell_id: string, cell: MemCell): AssertEvent => ({
  v: 1, ev: "assert", id, actor: "reviewer", ts: "2026-04-22T00:00:00Z", cell_id, cell,
});
const touchE = (id: string, target: string): TouchEvent => ({
  v: 1, ev: "touch", id, actor: "system", ts: "2026-04-22T00:00:01Z", target,
});
const retractE = (id: string, target: string): RetractEvent => ({
  v: 1, ev: "retract", id, actor: "user", ts: "2026-04-22T00:00:02Z", target, reason: "test",
});

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-proj-"));
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
  process.env.PEBBLE_ROOT = tmp;
});
afterEach(() => {
  db.close();
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("projector", () => {
  test("assert creates a cell", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    const row = db.query("SELECT * FROM cells WHERE id='mc_a'").get() as any;
    expect(row.id).toBe("mc_a");
    expect(row.type).toBe("preference");
  });

  test("assert populates facts table", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    const facts = db.query("SELECT * FROM facts WHERE cell_id='mc_a'").all() as any[];
    expect(facts).toHaveLength(1);
    expect(facts[0].subject).toBe("x");
  });

  test("touch increments access count", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    projectEvent(db, touchE("ev_02", "mc_a"));
    projectEvent(db, touchE("ev_03", "mc_a"));
    const row = db.query("SELECT access_count FROM cells WHERE id='mc_a'").get() as any;
    expect(row.access_count).toBe(2);
  });

  test("retract sets retracted_at", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    projectEvent(db, retractE("ev_02", "mc_a"));
    const row = db.query("SELECT retracted_at FROM cells WHERE id='mc_a'").get() as any;
    expect(row.retracted_at).toBeGreaterThan(0);
  });

  test("projector is idempotent — replay produces identical state", () => {
    const events = [
      assertE("ev_01", "mc_a", sampleCell("mc_a")),
      assertE("ev_02", "mc_b", sampleCell("mc_b")),
      touchE("ev_03", "mc_a"),
      retractE("ev_04", "mc_b"),
    ];
    projectAll(db, events);
    const snapshot1 = db.query("SELECT id, access_count, retracted_at FROM cells ORDER BY id").all();
    projectAll(db, events);
    const snapshot2 = db.query("SELECT id, access_count, retracted_at FROM cells ORDER BY id").all();
    expect(snapshot2).toEqual(snapshot1);
  });

  test("projector writes events table for audit", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    projectEvent(db, touchE("ev_02", "mc_a"));
    const evts = db.query("SELECT id, type FROM events ORDER BY seq").all() as any[];
    expect(evts).toEqual([
      { id: "ev_01", type: "assert" },
      { id: "ev_02", type: "touch" },
    ]);
  });

  test("FTS5 index is populated on assert", () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    const rows = db.query("SELECT cell_id FROM cells_fts WHERE cells_fts MATCH 'episode'").all() as any[];
    expect(rows).toHaveLength(1);
    expect(rows[0].cell_id).toBe("mc_a");
  });
});
