// src/cli/hot-cache.ts
import { Database } from "bun:sqlite";
import { dbPath } from "../paths";
import { registerProfileTools } from "../mcp/tools/profile";
import { registerSkillTools } from "../mcp/tools/skill";

interface HotCacheArgs {
  target: "cc" | "droid";
  max_skills?: number;
  max_foresight?: number;
}

export async function cliHotCache(args: HotCacheArgs): Promise<void> {
  const db = new Database(dbPath(), { readonly: true });
  try {
    const prof = registerProfileTools({ db });
    const sk = registerSkillTools({ db });
    const profile = await prof.profile_read({});
    const { skills } = await sk.skill_list({});
    const topSkills = skills.slice(0, args.max_skills ?? 5);

    const foresightRows = db.query(`
      SELECT f.inference, f.t_end
      FROM foresight f
      JOIN cells c ON c.id = f.cell_id
      WHERE f.status='active' AND c.retracted_at IS NULL
        AND (f.t_end IS NULL OR f.t_end > ?)
      ORDER BY CASE WHEN f.t_end IS NULL THEN 1 ELSE 0 END, f.t_end ASC
      LIMIT ?
    `).all(Date.now(), args.max_foresight ?? 5) as { inference: string; t_end: number | null }[];

    const lines: string[] = [
      "<!-- pebble hot-cache begin -->",
      "## Profile",
      "",
      `Voice tone: ${profile.voice.tone || "—"}`,
      `Primary languages: ${profile.stack.primary_langs.join(", ") || "—"}`,
      `Frameworks: ${profile.stack.frameworks.join(", ") || "—"}`,
      `Commit style: ${profile.conventions.commit_style || "—"}`,
      "",
      "## Skills",
      "",
      topSkills.length === 0 ? "_No skills yet._" :
        topSkills.map(s => `- **${s.name}**: ${s.description}`).join("\n"),
      "",
      "## Active foresight",
      "",
      foresightRows.length === 0 ? "_No active foresight._" :
        foresightRows.map(f => `- ${f.inference}${f.t_end ? ` _(until ${new Date(f.t_end).toISOString().slice(0,10)})_` : ""}`).join("\n"),
      "",
      "<!-- pebble hot-cache end -->",
    ];
    console.log(lines.join("\n"));
  } finally {
    db.close();
  }
}
