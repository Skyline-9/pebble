// src/render/contradictions.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";

export async function renderContradictions(db: Database): Promise<void> {
  const rows = db.query(`
    SELECT id, json, ts FROM events WHERE type = 'contradict' ORDER BY seq DESC
  `).all() as { id: string; json: string; ts: number }[];

  const lines: string[] = [
    "---",
    `updated_at: "${new Date().toISOString()}"`,
    "---",
    "",
    "# Contradictions",
    "",
    "Pairs of cells flagged with `resolution: flag_both`. Resolve with `/resolve mc_a mc_b`.",
    "",
  ];
  if (rows.length === 0) {
    lines.push("_No active contradictions._");
  } else {
    for (const r of rows) {
      const ev = JSON.parse(r.json) as { a: string; b: string; resolution: string };
      lines.push(`> [!contradiction] \`${ev.a}\` vs \`${ev.b}\``);
      lines.push(`> Resolution: **${ev.resolution}**. Flagged at ${new Date(r.ts).toISOString()}.`);
      lines.push("");
    }
  }
  writeVaultFile("_contradictions.md", lines.join("\n"));
}
