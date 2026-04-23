// src/render/scene.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";
import type { MemCell } from "../types";

function slugFromSceneId(id: string): string {
  return id.replace(/^ms_/, "");
}

export async function renderScene(db: Database, scene_id: string): Promise<void> {
  const scene = db
    .query("SELECT id, label, description, updated_at FROM scenes WHERE id=?")
    .get(scene_id) as { id: string; label: string; description: string; updated_at: number } | null;
  if (!scene) return;

  const members = db.query(`
    SELECT c.id, c.type, c.json, c.confidence, c.retracted_at
    FROM scene_members sm
    JOIN cells c ON c.id = sm.cell_id
    WHERE sm.scene_id = ? AND c.retracted_at IS NULL
    ORDER BY c.created_at
  `).all(scene_id) as { id: string; type: string; json: string; confidence: number; retracted_at: number | null }[];

  const lines: string[] = [
    "---",
    `scene_id: "${scene.id}"`,
    `label: "${scene.label}"`,
    `updated_at: "${new Date().toISOString()}"`,
    "---",
    "",
    `# ${scene.label}`,
    "",
    scene.description || "_No description._",
    "",
    "## Cells",
    "",
  ];

  for (const m of members) {
    const cell = JSON.parse(m.json) as MemCell;
    lines.push(`### \`${cell.id}\` _(${cell.type}, conf ${cell.confidence.toFixed(2)})_`);
    lines.push("");
    lines.push(`> ${cell.E}`);
    lines.push("");
    if (cell.F.length > 0) {
      lines.push("**Facts:**");
      for (const f of cell.F) lines.push(`- \`${f.subject}\` ${f.predicate} **${f.object}**`);
      lines.push("");
    }
  }

  writeVaultFile(`scenes/${slugFromSceneId(scene_id)}.md`, lines.join("\n"));
}
