import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { projectEvent } from "../src/projection/projector";
import { hybridSearch } from "../src/retrieval/score";
import { writeTrace, readTraces } from "../src/retrieval/trace";
import { tracePath } from "../src/paths";
import type { MemCell, AssertEvent } from "../src/types";

const cell = (id: string, episode: string, type: MemCell["type"] = "preference", confidence = 0.8): MemCell => ({
  id, type, E: episode,
  F: [{ subject: "x", predicate: "is", object: "y", confidence }],
  M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
  confidence, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
});
const assertE = (id: string, cell: MemCell): AssertEvent => ({
  v: 1, ev: "assert", id, actor: "reviewer", ts: "2026-04-22T00:00:00Z", cell_id: cell.id, cell,
});

let tmp: string;
let db: Database;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-retr-"));
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
  process.env.PEBBLE_ROOT = tmp;
});
afterEach(() => {
  db.close();
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("hybrid search", () => {
  test("returns cells matching query by BM25", () => {
    projectEvent(db, assertE("ev_01", cell("mc_a", "User prefers TypeScript for backend projects.")));
    projectEvent(db, assertE("ev_02", cell("mc_b", "User likes climbing on weekends.")));
    const hits = hybridSearch(db, "typescript backend", { topK: 5 });
    expect(hits.map(h => h.cell_id)).toEqual(["mc_a"]);
  });

  test("excludes retracted cells", () => {
    projectEvent(db, assertE("ev_01", cell("mc_a", "User prefers TypeScript for backend projects.")));
    projectEvent(db, {
      v: 1, ev: "retract", id: "ev_02", actor: "user", ts: "2026-04-22T00:00:01Z",
      target: "mc_a", reason: "test"
    });
    const hits = hybridSearch(db, "typescript", { topK: 5 });
    expect(hits).toHaveLength(0);
  });

  test("higher confidence ranks above lower", () => {
    projectEvent(db, assertE("ev_01", cell("mc_low", "python backend", "preference", 0.3)));
    projectEvent(db, assertE("ev_02", cell("mc_high", "python backend", "preference", 0.95)));
    const hits = hybridSearch(db, "python backend", { topK: 5 });
    expect(hits[0]?.cell_id).toBe("mc_high");
  });

  test("trace shape includes per-cell sub-scores", () => {
    projectEvent(db, assertE("ev_01", cell("mc_a", "typescript backend")));
    const hits = hybridSearch(db, "typescript", { topK: 5 });
    expect(hits[0]?.scores).toHaveProperty("bm25");
    expect(hits[0]?.scores).toHaveProperty("recency");
    expect(hits[0]?.scores).toHaveProperty("confidence");
    expect(hits[0]?.scores).toHaveProperty("total");
  });
});

describe("trace log", () => {
  test("writes trace line to trace.jsonl", async () => {
    await writeTrace({
      turn: 1,
      query_hash: "abc",
      candidates: [
        { id: "mc_a", scores: { bm25: 0.8, recency: 0.5, confidence: 0.9, total: 0.7 } },
      ],
      selected: ["mc_a"],
      injected_tokens: 420,
    });
    expect(existsSync(tracePath())).toBe(true);
    const lines = readFileSync(tracePath(), "utf8").trim().split("\n");
    const parsed = JSON.parse(lines[0]!);
    expect(parsed.turn).toBe(1);
    expect(parsed.selected).toEqual(["mc_a"]);
  });

  test("readTraces yields back written traces in order", async () => {
    await writeTrace({ turn: 1, query_hash: "a", candidates: [], selected: [], injected_tokens: 0 });
    await writeTrace({ turn: 2, query_hash: "b", candidates: [], selected: [], injected_tokens: 0 });
    const got: any[] = [];
    for await (const t of readTraces()) got.push(t);
    expect(got.map(t => t.turn)).toEqual([1, 2]);
  });
});
