// src/render/skill.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";
import type { MemCell } from "../types";

function yamlEscape(s: string): string {
  if (/^[A-Za-z0-9_\-.]+$/.test(s)) return s;
  return `"${s.replace(/"/g, '\\"')}"`;
}

export async function renderSkill(db: Database, cell_id: string): Promise<void> {
  const row = db
    .query("SELECT json, retracted_at FROM cells WHERE id=?")
    .get(cell_id) as { json: string; retracted_at: number | null } | null;
  if (!row || row.retracted_at) return;
  const cell = JSON.parse(row.json) as MemCell;
  if (cell.type !== "skill" || !cell.skill) return;
  const s = cell.skill;

  const frontmatter = [
    "---",
    `name: ${yamlEscape(s.name)}`,
    `description: ${yamlEscape(s.description)}`,
    `version: ${yamlEscape(s.version)}`,
    `compatibility: ${yamlEscape(s.compatibility)}`,
    s.allowed_tools && s.allowed_tools.length
      ? `allowed-tools: [${s.allowed_tools.map(yamlEscape).join(", ")}]`
      : null,
    s.trigger_phrases.length
      ? `trigger_phrases:\n${s.trigger_phrases.map(p => `  - ${yamlEscape(p)}`).join("\n")}`
      : null,
    "---",
    "",
  ].filter(Boolean).join("\n");

  const body = [
    `# ${s.name}`,
    "",
    s.description,
    "",
    "## Body",
    "",
    s.body,
    "",
  ].join("\n");

  writeVaultFile(`skills/${s.name}.md`, frontmatter + body);
}
