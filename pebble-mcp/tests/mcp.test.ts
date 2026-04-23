import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { registerMemoryTools, type MemoryContext } from "../src/mcp/tools/memory";
import { registerProfileTools } from "../src/mcp/tools/profile";
import { registerSkillTools } from "../src/mcp/tools/skill";
import { logPath } from "../src/paths";

let tmp: string;
let db: Database;
let ctx: MemoryContext;

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-mcp-"));
  process.env.PEBBLE_ROOT = tmp;
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
  ctx = { db };
});
afterEach(() => {
  db.close();
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("memory tools", () => {
  test("memory_assert writes event + projects cell", async () => {
    const tools = registerMemoryTools(ctx);
    const result = await tools.memory_assert({
      type: "preference",
      episode: "User prefers TypeScript.",
      facts: [{ subject: "user.lang", predicate: "prefers", object: "typescript", confidence: 0.9 }],
      confidence: 0.9,
    });
    expect(result.ok).toBe(true);
    expect(result.cell_id).toMatch(/^mc_/);
    expect(existsSync(logPath())).toBe(true);
    const row = db.query("SELECT * FROM cells WHERE id = ?").get(result.cell_id!) as any;
    expect(row.type).toBe("preference");
  });

  test("memory_query returns matching cells and writes trace", async () => {
    const tools = registerMemoryTools(ctx);
    await tools.memory_assert({
      type: "preference",
      episode: "User prefers TypeScript for backend.",
      facts: [{ subject: "x", predicate: "is", object: "y", confidence: 0.9 }],
      confidence: 0.9,
    });
    const result = await tools.memory_query({ query: "typescript backend", top_k: 3 });
    expect(result.hits).toHaveLength(1);
    expect(result.trace_recorded).toBe(true);
  });

  test("memory_touch emits touch event", async () => {
    const tools = registerMemoryTools(ctx);
    const { cell_id } = await tools.memory_assert({
      type: "preference",
      episode: "User prefers dark mode.",
      facts: [], confidence: 0.8,
    });
    await tools.memory_touch({ cell_id: cell_id!, query: "dark" });
    const row = db.query("SELECT access_count FROM cells WHERE id=?").get(cell_id!) as any;
    expect(row.access_count).toBe(1);
  });

  test("memory_retract emits retract event", async () => {
    const tools = registerMemoryTools(ctx);
    const { cell_id } = await tools.memory_assert({
      type: "preference", episode: "obsolete", facts: [], confidence: 0.9,
    });
    await tools.memory_retract({ cell_id: cell_id!, reason: "test" });
    const row = db.query("SELECT retracted_at FROM cells WHERE id=?").get(cell_id!) as any;
    expect(row.retracted_at).toBeGreaterThan(0);
  });
});

describe("profile tools", () => {
  test("profile_read returns grouped profile view", async () => {
    const memTools = registerMemoryTools(ctx);
    await memTools.memory_assert({
      type: "profile", episode: "User's lang is TypeScript",
      facts: [{ subject: "user.stack.lang", predicate: "is", object: "typescript", confidence: 0.95 }],
      confidence: 0.95,
    });
    const profTools = registerProfileTools(ctx);
    const p = await profTools.profile_read({});
    expect(p.stack.primary_langs).toContain("typescript");
  });
});

describe("skill tools", () => {
  test("skill_save creates skill cell and SKILL.md", async () => {
    const tools = registerSkillTools(ctx);
    const result = await tools.skill_save({
      name: "commit-style",
      description: "Use gitmoji in commit subjects.",
      body: "Always prefix with gitmoji.",
      trigger_phrases: ["commit", "git commit"],
      confidence: 0.95,
    });
    expect(result.ok).toBe(true);
    expect(result.cell_id).toMatch(/^mc_/);
  });

  test("skill_list returns saved skills", async () => {
    const tools = registerSkillTools(ctx);
    await tools.skill_save({
      name: "test-skill", description: "d", body: "b", trigger_phrases: [], confidence: 0.9,
    });
    const list = await tools.skill_list({});
    expect(list.skills.map(s => s.name)).toContain("test-skill");
  });

  test("skill_read returns full SKILL.md body", async () => {
    const tools = registerSkillTools(ctx);
    await tools.skill_save({
      name: "readable-skill", description: "d", body: "the body content",
      trigger_phrases: [], confidence: 0.9,
    });
    const read = await tools.skill_read({ name: "readable-skill" });
    expect(read.skill?.body).toContain("the body content");
  });
});
