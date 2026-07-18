import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import { appendEvents } from "../log/writer";
import { dbPath, resolvePebbleRoot } from "../paths";
import { projectAll } from "../projection/projector";
import { initSchema } from "../projection/schema";
import type { AssertEvent, MemCell } from "../types";

export async function seedBenchmark(count: number): Promise<void> {
  if (!Number.isSafeInteger(count) || count < 1 || count > 100_000) {
    throw new Error("cells must be an integer from 1 through 100000");
  }

  mkdirSync(resolvePebbleRoot(), { recursive: true });
  const db = new Database(dbPath());
  initSchema(db);
  const events: AssertEvent[] = [];

  for (let index = 0; index < count; index += 1) {
    const suffix = index.toString().padStart(6, "0");
    const timestamp = new Date(
      Date.UTC(2026, 0, 1, 0, 0, index % 60),
    ).toISOString();
    const cell: MemCell = {
      id: `mc_benchmark_${suffix}`,
      type: "project",
      E: `Benchmark repository fact ${suffix}`,
      F: [{
        subject: `benchmark.symbol.${suffix}`,
        predicate: "defined_in",
        object: `src/module_${suffix}.ts`,
        confidence: 1,
      }],
      M: { created_at: timestamp, actor: "system", source: "benchmark" },
      confidence: 1,
      evidence: [],
      scene_ids: [],
      access: { count: 0, last_at: null },
    };
    events.push({
      v: 1,
      ev: "assert",
      id: `ev_benchmark_${suffix}`,
      actor: "system",
      ts: timestamp,
      cell_id: cell.id,
      cell,
    });
  }

  try {
    for (let offset = 0; offset < events.length; offset += 500) {
      const batch = events.slice(offset, offset + 500);
      await appendEvents(batch);
      projectAll(db, batch);
    }
  } finally {
    db.close();
  }
}
