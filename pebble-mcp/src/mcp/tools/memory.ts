// src/mcp/tools/memory.ts
import type { Database } from "bun:sqlite";
import { newCellId, newEventId } from "../../ids";
import { appendEvents } from "../../log/writer";
import { projectEvent } from "../../projection/projector";
import { hybridSearch } from "../../retrieval/score";
import { writeTrace } from "../../retrieval/trace";
import type { MemCell, CellType, AtomicFact, AssertEvent, TouchEvent, RetractEvent } from "../../types";
import { createHash } from "node:crypto";
import { judgeCandidate } from "../../reviewer/judge";

export interface MemoryContext {
  db: Database;
}

export interface AssertArgs {
  type: CellType;
  episode: string;
  facts: AtomicFact[];
  confidence: number;
  actor?: "user" | "reviewer" | "system";
  scene_ids?: string[];
  foresight?: { inference: string; t_start: string; t_end?: string };
}

export interface QueryArgs {
  query: string;
  top_k?: number;
  turn?: number;
}

export function registerMemoryTools(ctx: MemoryContext) {
  const { db } = ctx;

  async function memory_assert(args: AssertArgs): Promise<{ ok: boolean; cell_id?: string; reason?: string }> {
    const actor = args.actor ?? "reviewer";
    const cell_id = newCellId();
    const now = new Date().toISOString();

    // Normalize facts: drop malformed entries, default missing confidence to cell confidence.
    // MCP clients occasionally omit per-fact confidence; we accept that but never persist null.
    const rawFacts = Array.isArray(args.facts) ? args.facts : [];
    const facts: AtomicFact[] = [];
    for (const f of rawFacts) {
      if (!f || typeof f.subject !== "string" || typeof f.predicate !== "string" || typeof f.object !== "string") continue;
      const confidence = (typeof f.confidence === "number" && Number.isFinite(f.confidence)) ? f.confidence : args.confidence;
      facts.push({ subject: f.subject, predicate: f.predicate, object: f.object, confidence });
    }

    const cell: MemCell = {
      id: cell_id,
      type: args.type,
      E: args.episode,
      F: facts,
      M: { created_at: now, actor },
      confidence: args.confidence,
      evidence: [],
      scene_ids: args.scene_ids ?? [],
      access: { count: 0, last_at: null },
      P: args.foresight ? { ...args.foresight, status: "active" } : undefined,
    };

    const decision = judgeCandidate(db, cell);
    if (decision.action === "discard") {
      return { ok: false, reason: decision.reason };
    }

    const ev: AssertEvent = {
      v: 1, ev: "assert", id: newEventId(), actor, ts: now,
      cell_id, cell,
    };
    await appendEvents([ev]);
    projectEvent(db, ev);
    return { ok: true, cell_id };
  }

  async function memory_query(args: QueryArgs): Promise<{ hits: any[]; trace_recorded: boolean }> {
    const topK = args.top_k ?? 5;
    const hits = hybridSearch(db, args.query, { topK });
    await writeTrace({
      turn: args.turn ?? 0,
      query_hash: createHash("sha256").update(args.query).digest("hex").slice(0, 16),
      candidates: hits.map(h => ({ id: h.cell_id, scores: h.scores })),
      selected: hits.map(h => h.cell_id),
      injected_tokens: 0,
    });
    return { hits, trace_recorded: true };
  }

  async function memory_touch(args: { cell_id: string; query?: string }): Promise<{ ok: true }> {
    const ev: TouchEvent = {
      v: 1, ev: "touch", id: newEventId(), actor: "system", ts: new Date().toISOString(),
      target: args.cell_id, query: args.query,
    };
    await appendEvents([ev]);
    projectEvent(db, ev);
    return { ok: true };
  }

  async function memory_retract(args: { cell_id: string; reason: string; actor?: "user" | "system" }): Promise<{ ok: true }> {
    const ev: RetractEvent = {
      v: 1, ev: "retract", id: newEventId(), actor: args.actor ?? "user",
      ts: new Date().toISOString(), target: args.cell_id, reason: args.reason,
    };
    await appendEvents([ev]);
    projectEvent(db, ev);
    return { ok: true };
  }

  async function memory_read_cell(args: { cell_id: string }): Promise<{ cell: MemCell | null }> {
    const row = db.query("SELECT json FROM cells WHERE id=?").get(args.cell_id) as { json: string } | null;
    return { cell: row ? JSON.parse(row.json) as MemCell : null };
  }

  return { memory_assert, memory_query, memory_touch, memory_retract, memory_read_cell };
}
