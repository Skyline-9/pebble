// src/retrieval/fts.ts
import type { Database } from "bun:sqlite";

export interface FtsHit {
  cell_id: string;
  bm25: number;
}

/** Returns raw FTS5 BM25 hits for a query string. Excludes retracted cells. */
export function ftsSearch(db: Database, query: string, limit: number): FtsHit[] {
  // FTS5 treats . - ( ) * ^ etc. as syntax. Strip to whitespace, then collapse.
  const safe = query
    .replace(/[^\p{L}\p{N}\s]/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!safe) return [];
  const sql = `
    SELECT f.cell_id AS cell_id, bm25(cells_fts) AS raw
    FROM cells_fts f
    JOIN cells c ON c.id = f.cell_id
    WHERE cells_fts MATCH ? AND c.retracted_at IS NULL
    ORDER BY raw
    LIMIT ?
  `;
  const rows = db.query(sql).all(safe, limit) as { cell_id: string; raw: number }[];
  return rows.map(r => ({ cell_id: r.cell_id, bm25: Math.max(0, -r.raw) }));
}
