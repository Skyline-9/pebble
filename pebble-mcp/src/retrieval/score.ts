// src/retrieval/score.ts
import type { Database } from "bun:sqlite";
import { ftsSearch } from "./fts";

export interface HybridHit {
  cell_id: string;
  scores: {
    bm25: number;
    recency: number;
    confidence: number;
    total: number;
  };
}

export interface SearchOpts {
  topK: number;
  weights?: { bm25?: number; recency?: number; confidence?: number };
}

const DEFAULT_WEIGHTS = { bm25: 0.55, recency: 0.2, confidence: 0.25 };

export function hybridSearch(db: Database, query: string, opts: SearchOpts): HybridHit[] {
  const weights = { ...DEFAULT_WEIGHTS, ...(opts.weights ?? {}) };
  const candidates = ftsSearch(db, query, opts.topK * 3);
  if (candidates.length === 0) return [];

  const maxBm25 = Math.max(...candidates.map(c => c.bm25), 1e-6);
  const now = Date.now();

  const hits: HybridHit[] = candidates.map(c => {
    const meta = db
      .query("SELECT confidence, updated_at FROM cells WHERE id=?")
      .get(c.cell_id) as { confidence: number; updated_at: number } | null;
    if (!meta) return null as any;

    const bm25Norm = c.bm25 / maxBm25;
    const ageDays = Math.max(0, (now - meta.updated_at) / 86400000);
    const recency = Math.exp(-0.02 * ageDays);
    const confidence = meta.confidence;

    const total =
      weights.bm25 * bm25Norm +
      weights.recency * recency +
      weights.confidence * confidence;

    return { cell_id: c.cell_id, scores: { bm25: bm25Norm, recency, confidence, total } };
  }).filter(Boolean);

  hits.sort((a, b) => b.scores.total - a.scores.total);
  return hits.slice(0, opts.topK);
}
