// src/mcp/tools/profile.ts
import type { Database } from "bun:sqlite";
import type { Profile, AtomicFact } from "../../types";

export interface ProfileContext { db: Database; }

export function registerProfileTools(ctx: ProfileContext) {
  const { db } = ctx;

  async function profile_read(_args: Record<string, never>): Promise<Profile> {
    const rows = db.query(`
      SELECT f.subject, f.object, f.confidence
      FROM facts f JOIN cells c ON c.id = f.cell_id
      WHERE c.type = 'profile' AND c.retracted_at IS NULL
    `).all() as { subject: string; object: string; confidence: number }[];

    const profile: Profile = {
      voice: { tone: "", vocabulary_dos: [], vocabulary_donts: [], examples: [] },
      stack: { primary_langs: [], frameworks: [], tools: [], never_use: [] },
      conventions: { commit_style: "", code_style: "", test_style: "", doc_style: "" },
      goals: [],
      updated_at: new Date().toISOString(),
    };

    for (const r of rows) {
      const parts = r.subject.split(".");
      if (parts[0] !== "user") continue;
      const section = parts[1];
      const field = parts.slice(2).join(".");
      if (section === "stack") {
        if (field === "lang" || field === "primary_lang") profile.stack.primary_langs.push(r.object);
        else if (field === "framework") profile.stack.frameworks.push(r.object);
        else if (field === "tool") profile.stack.tools.push(r.object);
        else if (field === "never_use") profile.stack.never_use.push(r.object);
      } else if (section === "voice") {
        if (field === "tone") profile.voice.tone = r.object;
        else if (field === "do") profile.voice.vocabulary_dos.push(r.object);
        else if (field === "dont") profile.voice.vocabulary_donts.push(r.object);
      } else if (section === "conventions") {
        if (field === "commit") profile.conventions.commit_style = r.object;
        else if (field === "code") profile.conventions.code_style = r.object;
        else if (field === "test") profile.conventions.test_style = r.object;
        else if (field === "doc") profile.conventions.doc_style = r.object;
      }
    }
    return profile;
  }

  async function profile_update(args: { facts: AtomicFact[] }): Promise<{ ok: true; cell_id: string }> {
    const { registerMemoryTools } = await import("./memory");
    const mem = registerMemoryTools({ db });
    const result = await mem.memory_assert({
      type: "profile",
      episode: args.facts.map(f => `${f.subject} ${f.predicate} ${f.object}`).join("; "),
      facts: args.facts,
      confidence: Math.min(...args.facts.map(f => f.confidence)),
      actor: "user",
    });
    if (!result.ok) throw new Error(result.reason ?? "profile_update rejected");
    return { ok: true, cell_id: result.cell_id! };
  }

  return { profile_read, profile_update };
}
