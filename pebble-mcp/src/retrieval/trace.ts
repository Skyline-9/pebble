// src/retrieval/trace.ts
import { appendFileSync, createReadStream, existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import readline from "node:readline";
import { tracePath, resolvePebbleRoot } from "../paths";

export interface RetrievalTrace {
  turn: number;
  query_hash: string;
  ts?: string;
  candidates: Array<{
    id: string;
    scores: { bm25: number; recency: number; confidence: number; total: number };
  }>;
  selected: string[];
  injected_tokens: number;
}

function ensureDir() {
  const root = resolvePebbleRoot();
  if (!existsSync(root)) mkdirSync(root, { recursive: true });
  const dir = dirname(tracePath());
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
}

export async function writeTrace(trace: RetrievalTrace): Promise<void> {
  ensureDir();
  const record = { ...trace, ts: trace.ts ?? new Date().toISOString() };
  appendFileSync(tracePath(), JSON.stringify(record) + "\n");
}

export async function* readTraces(): AsyncGenerator<RetrievalTrace & { ts: string }> {
  if (!existsSync(tracePath())) return;
  const rl = readline.createInterface({
    input: createReadStream(tracePath(), { encoding: "utf8" }),
    crlfDelay: Infinity,
  });
  for await (const line of rl) {
    if (!line.trim()) continue;
    try { yield JSON.parse(line); } catch { /* skip malformed */ }
  }
}
