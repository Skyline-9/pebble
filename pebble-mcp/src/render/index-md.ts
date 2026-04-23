// src/render/index-md.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";

export async function renderIndex(db: Database): Promise<void> {
  const total = (db.query("SELECT COUNT(*) AS n FROM cells").get() as any).n as number;
  const active = (db.query("SELECT COUNT(*) AS n FROM cells WHERE retracted_at IS NULL").get() as any).n as number;
  const byType = db.query("SELECT type, COUNT(*) AS n FROM cells WHERE retracted_at IS NULL GROUP BY type").all() as { type: string; n: number }[];
  const sceneCount = (db.query("SELECT COUNT(*) AS n FROM scenes").get() as any).n as number;

  const lines: string[] = [
    "---",
    `updated_at: "${new Date().toISOString()}"`,
    "---",
    "",
    "# Pebble Index",
    "",
    `- Total cells: **${total}**`,
    `- Active cells: **${active}**`,
    `- Scenes: **${sceneCount}**`,
    "",
    "## Cells by type",
    "",
    "| Type | Count |",
    "| --- | ---: |",
    ...byType.map(r => `| \`${r.type}\` | ${r.n} |`),
    "",
    "## Links",
    "",
    "- [[profile|Profile]]",
    "- [[_foresight|Foresight]]",
    "- [[_contradictions|Contradictions]]",
    "- `scenes/` folder for clusters",
    "- `skills/` folder for SKILL.md files",
  ];
  writeVaultFile("_index.md", lines.join("\n"));
}
