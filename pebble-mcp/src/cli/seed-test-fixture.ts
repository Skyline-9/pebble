// src/cli/seed-test-fixture.ts (dev-only)
import { Database } from "bun:sqlite";
import { dbPath } from "../paths";
import { initSchema } from "../projection/schema";
import { registerMemoryTools } from "../mcp/tools/memory";
import { registerSkillTools } from "../mcp/tools/skill";

export async function cliSeedTestFixture(): Promise<void> {
  const db = new Database(dbPath());
  initSchema(db);
  const mem = registerMemoryTools({ db });
  const sk = registerSkillTools({ db });
  await mem.memory_assert({
    type: "profile",
    episode: "Primary language is typescript.",
    facts: [{ subject: "user.stack.lang", predicate: "is", object: "typescript", confidence: 0.95 }],
    confidence: 0.95,
    actor: "user",
  });
  await sk.skill_save({
    name: "commit-style",
    description: "gitmoji-prefixed commits.",
    body: "Use gitmoji prefix.",
    trigger_phrases: ["commit"],
    confidence: 0.9,
  });
  db.close();
  console.log("seeded");
}
