import { describe, expect, test } from "bun:test";
import type { MemCell, MemEvent, Profile } from "../src/types";

describe("types", () => {
  test("MemCell accepts a minimal profile cell", () => {
    const cell: MemCell = {
      id: "mc_01",
      type: "profile",
      E: "User prefers TypeScript for backend code.",
      F: [{ subject: "user.lang", predicate: "prefers", object: "typescript", confidence: 0.9 }],
      M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
      confidence: 0.9,
      evidence: [],
      scene_ids: [],
      access: { count: 0, last_at: null },
    };
    expect(cell.type).toBe("profile");
  });

  test("MemEvent union has all required event types", () => {
    const ev: MemEvent = {
      v: 1,
      ev: "assert",
      id: "ev_01",
      actor: "reviewer",
      ts: "2026-04-22T00:00:00Z",
      cell_id: "mc_01",
      cell: {} as MemCell,
    };
    expect(ev.ev).toBe("assert");
  });
});
