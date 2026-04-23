import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { registerMemoryTools } from "../src/mcp/tools/memory";
import { registerSkillTools } from "../src/mcp/tools/skill";
import { renderProfile } from "../src/render/profile";
import { renderSkill } from "../src/render/skill";
import { renderIndex } from "../src/render/index-md";
import { commitTurn, ensureGitRepo } from "../src/git/turn-commit";
import { execSync } from "node:child_process";
import { vaultPath, logPath, dbPath } from "../src/paths";

let tmp: string;
let db: Database;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-int-"));
  process.env.PEBBLE_ROOT = tmp;
  ensureGitRepo();
  db = new Database(join(tmp, "projection.db"));
  initSchema(db);
});
afterEach(() => {
  try { db.close(); } catch {}
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("integration: full turn loop", () => {
  test("profile assert + skill save + query + render + commit", async () => {
    const mem = registerMemoryTools({ db });
    const sk = registerSkillTools({ db });

    const p1 = await mem.memory_assert({
      type: "profile",
      episode: "User's primary language is TypeScript.",
      facts: [{ subject: "user.stack.lang", predicate: "is", object: "typescript", confidence: 0.95 }],
      confidence: 0.95,
      actor: "user",
    });
    expect(p1.ok).toBe(true);

    const sk1 = await sk.skill_save({
      name: "commit-style",
      description: "Use gitmoji in commits.",
      body: "Always gitmoji-prefix.",
      trigger_phrases: ["commit"],
      confidence: 0.9,
    });
    expect(sk1.ok).toBe(true);

    const q = await mem.memory_query({ query: "typescript", top_k: 3, turn: 2 });
    expect(q.hits.length).toBeGreaterThan(0);
    expect(q.trace_recorded).toBe(true);

    await renderProfile(db);
    await renderSkill(db, sk1.cell_id!);
    await renderIndex(db);

    const commit = commitTurn({ turn: 1, adds: 2, retracts: 0 });
    expect(commit.committed).toBe(true);

    expect(existsSync(join(vaultPath(), "profile.md"))).toBe(true);
    expect(existsSync(join(vaultPath(), "skills", "commit-style.md"))).toBe(true);
    expect(existsSync(join(vaultPath(), "_index.md"))).toBe(true);

    const log = execSync("git log --oneline", { cwd: tmp }).toString();
    expect(log.trim().split("\n").length).toBeGreaterThanOrEqual(1);

    expect(existsSync(logPath())).toBe(true);
    expect(existsSync(dbPath())).toBe(true);
  });

  test("replay from log produces identical projection", async () => {
    const mem = registerMemoryTools({ db });
    await mem.memory_assert({
      type: "preference", episode: "User likes vim keybindings.",
      facts: [{ subject: "user.editor", predicate: "uses", object: "vim", confidence: 0.85 }],
      confidence: 0.85,
    });
    await mem.memory_assert({
      type: "project", episode: "Working on Pebble plugin.",
      facts: [{ subject: "project.name", predicate: "is", object: "pebble", confidence: 0.9 }],
      confidence: 0.9,
    });
    const snapshot1 = db.query("SELECT id, type FROM cells ORDER BY id").all();

    const { readEvents } = await import("../src/log/reader");
    const { projectAll } = await import("../src/projection/projector");
    const fresh = new Database(join(tmp, "fresh.db"));
    initSchema(fresh);
    const events: any[] = [];
    for await (const ev of readEvents()) events.push(ev);
    projectAll(fresh, events);
    const snapshot2 = fresh.query("SELECT id, type FROM cells ORDER BY id").all();
    fresh.close();

    expect(snapshot2).toEqual(snapshot1);
  });
});
