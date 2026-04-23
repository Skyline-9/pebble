import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { extractCandidates, type Transcript } from "../src/reviewer/extractor";
import { snapshotProjection, restoreProjection, shouldCheckpoint } from "../src/projection/checkpoint";
import { projectEvent } from "../src/projection/projector";
import { checkpointDir } from "../src/paths";
import { existsSync, readdirSync } from "node:fs";
import type { MemCell, AssertEvent } from "../src/types";

let tmp: string;
let db: Database;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-ext-"));
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
  process.env.PEBBLE_ROOT = tmp;
});
afterEach(() => {
  try { db.close(); } catch {}
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("extractor (rule-based MVP)", () => {
  test("extracts profile candidate from 'my primary language is X'", () => {
    const transcript: Transcript = [
      { role: "user", content: "My primary language is TypeScript." },
    ];
    const candidates = extractCandidates(transcript);
    expect(candidates).toHaveLength(1);
    expect(candidates[0]!.type).toBe("profile");
    expect(candidates[0]!.E).toContain("TypeScript");
  });

  test("extracts project candidate from 'I'm working on X' utterance", () => {
    const transcript: Transcript = [
      { role: "user", content: "I'm working on the auth refactor this week." },
    ];
    const candidates = extractCandidates(transcript);
    expect(candidates[0]!.type).toBe("project");
  });

  test("ignores model turns entirely (AutoSkill principle)", () => {
    const transcript: Transcript = [
      { role: "user", content: "What's the time?" },
      { role: "assistant", content: "I prefer TypeScript for everything." },
    ];
    const candidates = extractCandidates(transcript);
    expect(candidates).toHaveLength(0);
  });

  test("returns empty on unmatched transcripts", () => {
    const transcript: Transcript = [
      { role: "user", content: "Hello" },
    ];
    expect(extractCandidates(transcript)).toEqual([]);
  });
});

const sampleCell = (id: string): MemCell => ({
  id, type: "preference",
  E: `Episode for ${id}`,
  F: [{ subject: "x", predicate: "is", object: "y", confidence: 0.8 }],
  M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
  confidence: 0.8, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
});
const assertE = (id: string, cell_id: string, cell: MemCell): AssertEvent => ({
  v: 1, ev: "assert", id, actor: "reviewer", ts: "2026-04-22T00:00:00Z", cell_id, cell,
});

describe("checkpoint", () => {
  test("snapshot writes a checkpoint file", async () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    const path = await snapshotProjection(db, 1);
    expect(existsSync(path)).toBe(true);
    expect(readdirSync(checkpointDir()).length).toBeGreaterThan(0);
  });

  test("restore replays from latest checkpoint", async () => {
    projectEvent(db, assertE("ev_01", "mc_a", sampleCell("mc_a")));
    const path = await snapshotProjection(db, 1);
    const freshPath = join(tmp, "restored.db");
    const fresh = new Database(freshPath);
    initSchema(fresh);
    await restoreProjection(fresh, path);
    const restored = new Database(freshPath, { readonly: true });
    const rows = restored.query("SELECT id FROM cells").all() as any[];
    expect(rows).toHaveLength(1);
    restored.close();
  });

  test("shouldCheckpoint triggers every 500 events", () => {
    expect(shouldCheckpoint(0)).toBe(false);
    expect(shouldCheckpoint(499)).toBe(false);
    expect(shouldCheckpoint(500)).toBe(true);
    expect(shouldCheckpoint(999)).toBe(false);
    expect(shouldCheckpoint(1000)).toBe(true);
  });
});
