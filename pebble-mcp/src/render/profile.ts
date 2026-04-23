// src/render/profile.ts
import type { Database } from "bun:sqlite";
import { writeVaultFile } from "./vault";

interface FactRow {
  subject: string;
  object: string;
  confidence: number;
}

function groupBySection(rows: FactRow[]): Record<string, FactRow[]> {
  const out: Record<string, FactRow[]> = { voice: [], stack: [], conventions: [], goals: [], other: [] };
  for (const r of rows) {
    const top = r.subject.split(".")[1];
    const bucket = top === "voice" ? "voice"
      : top === "stack" ? "stack"
      : top === "conventions" ? "conventions"
      : top === "goal" ? "goals"
      : "other";
    out[bucket]!.push(r);
  }
  return out;
}

function section(name: string, rows: FactRow[]): string {
  if (rows.length === 0) return "";
  const lines = [`## ${name}`, ""];
  for (const r of rows) {
    const sub = r.subject.split(".").slice(2).join(".") || r.subject;
    lines.push(`- **${sub}**: ${r.object} _(conf ${r.confidence.toFixed(2)})_`);
  }
  lines.push("");
  return lines.join("\n");
}

export async function renderProfile(db: Database): Promise<void> {
  const rows = db.query(`
    SELECT f.subject, f.object, f.confidence
    FROM facts f
    JOIN cells c ON c.id = f.cell_id
    WHERE c.type = 'profile' AND c.retracted_at IS NULL
    ORDER BY f.subject
  `).all() as FactRow[];

  const grouped = groupBySection(rows);
  const updated = new Date().toISOString();
  const body = [
    "---",
    `updated_at: "${updated}"`,
    "---",
    "",
    "# Profile",
    "",
    "> Rendered from profile-type MemCells. Edit this file in Obsidian; changes flow back to the log.",
    "",
    section("Voice", grouped.voice!),
    section("Stack", grouped.stack!),
    section("Conventions", grouped.conventions!),
    section("Goals", grouped.goals!),
    section("Other", grouped.other!),
  ].filter(Boolean).join("\n");

  writeVaultFile("profile.md", body);
}
