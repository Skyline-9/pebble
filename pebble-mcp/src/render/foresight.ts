// src/render/foresight.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";

export async function renderForesight(db: Database): Promise<void> {
  const rows = db.query(`
    SELECT f.cell_id, f.inference, f.t_start, f.t_end, f.status, c.confidence
    FROM foresight f
    JOIN cells c ON c.id = f.cell_id
    WHERE f.status = 'active' AND c.retracted_at IS NULL
    ORDER BY CASE WHEN f.t_end IS NULL THEN 1 ELSE 0 END, f.t_end ASC
  `).all() as { cell_id: string; inference: string; t_start: number; t_end: number | null; status: string; confidence: number }[];

  const lines: string[] = [
    "---",
    `updated_at: "${new Date().toISOString()}"`,
    "---",
    "",
    "# Active foresight",
    "",
    "Forward-looking inferences with validity intervals.",
    "",
    "| Cell | Inference | Starts | Ends | Confidence |",
    "| --- | --- | --- | --- | --- |",
  ];
  for (const r of rows) {
    const starts = new Date(r.t_start).toISOString().slice(0, 10);
    const ends = r.t_end ? new Date(r.t_end).toISOString().slice(0, 10) : "—";
    lines.push(`| \`${r.cell_id}\` | ${r.inference} | ${starts} | ${ends} | ${r.confidence.toFixed(2)} |`);
  }
  writeVaultFile("_foresight.md", lines.join("\n"));
}
