import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { projectEvent } from "../src/projection/projector";
import { judgeCandidate } from "../src/reviewer/judge";
import type { MemCell, AssertEvent } from "../src/types";

const cell = (id: string, type: MemCell["type"], episode: string, confidence: number): MemCell => ({
  id, type, E: episode,
  F: [{ subject: "x", predicate: "is", object: episode, confidence }],
  M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
  confidence, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
});
const assertE = (id: string, c: MemCell): AssertEvent => ({
  v: 1, ev: "assert", id, actor: "reviewer", ts: "2026-04-22T00:00:00Z", cell_id: c.id, cell: c,
});

let tmp: string;
let db: Database;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-judge-"));
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
});
afterEach(() => {
  db.close();
  rmSync(tmp, { recursive: true, force: true });
});

describe("judge", () => {
  test("rejects profile cell below 0.9 confidence threshold", () => {
    const candidate = cell("mc_new", "profile", "User likes x", 0.7);
    const decision = judgeCandidate(db, candidate);
    expect(decision.action).toBe("discard");
    expect(decision.reason).toContain("confidence");
  });

  test("accepts profile cell at 0.9+ confidence", () => {
    const candidate = cell("mc_new", "profile", "User likes x", 0.95);
    const decision = judgeCandidate(db, candidate);
    expect(decision.action).toBe("assert");
  });

  test("accepts project cell at 0.7+ confidence", () => {
    const candidate = cell("mc_new", "project", "Working on auth", 0.75);
    const decision = judgeCandidate(db, candidate);
    expect(decision.action).toBe("assert");
  });

  test("discards near-duplicate via BM25 match", () => {
    projectEvent(db, assertE("ev_01", cell("mc_a", "preference", "User prefers dark mode", 0.8)));
    const candidate = cell("mc_new", "preference", "User prefers dark mode", 0.8);
    const decision = judgeCandidate(db, candidate);
    expect(decision.action).toBe("discard");
    expect(decision.reason).toContain("duplicate");
  });

  test("accepts a new distinct cell", () => {
    projectEvent(db, assertE("ev_01", cell("mc_a", "preference", "User prefers dark mode", 0.8)));
    const candidate = cell("mc_new", "preference", "User likes climbing on weekends", 0.8);
    const decision = judgeCandidate(db, candidate);
    expect(decision.action).toBe("assert");
  });
});
