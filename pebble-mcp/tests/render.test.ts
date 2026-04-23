import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema } from "../src/projection/schema";
import { projectEvent } from "../src/projection/projector";
import { renderProfile } from "../src/render/profile";
import { renderScene } from "../src/render/scene";
import { renderSkill } from "../src/render/skill";
import { renderForesight } from "../src/render/foresight";
import { renderContradictions } from "../src/render/contradictions";
import { renderIndex } from "../src/render/index-md";
import { vaultPath } from "../src/paths";
import type { MemCell, AssertEvent } from "../src/types";

const profileCell = (id: string, subject: string, object: string, confidence = 0.9): MemCell => ({
  id, type: "profile",
  E: `User's ${subject} is ${object}.`,
  F: [{ subject, predicate: "is", object, confidence }],
  M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
  confidence, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
});
const assertE = (id: string, cell: MemCell): AssertEvent => ({
  v: 1, ev: "assert", id, actor: "reviewer", ts: "2026-04-22T00:00:00Z", cell_id: cell.id, cell,
});

let tmp: string;
let db: Database;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-render-"));
  db = new Database(join(tmp, "test.db"));
  initSchema(db);
  process.env.PEBBLE_ROOT = tmp;
});
afterEach(() => {
  db.close();
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("renderProfile", () => {
  test("writes profile.md with voice/stack/conventions sections", async () => {
    projectEvent(db, assertE("ev_01", profileCell("mc_a", "user.stack.lang", "typescript")));
    projectEvent(db, assertE("ev_02", profileCell("mc_b", "user.voice.tone", "direct-concise")));
    await renderProfile(db);
    const content = readFileSync(join(vaultPath(), "profile.md"), "utf8");
    expect(content).toContain("# Profile");
    expect(content).toContain("typescript");
    expect(content).toContain("direct-concise");
  });

  test("profile.md has YAML frontmatter with updated_at", async () => {
    projectEvent(db, assertE("ev_01", profileCell("mc_a", "user.stack.lang", "python")));
    await renderProfile(db);
    const content = readFileSync(join(vaultPath(), "profile.md"), "utf8");
    expect(content.startsWith("---\n")).toBe(true);
    expect(content).toMatch(/updated_at: "[\d-]+T[\d:]+/);
  });

  test("excludes retracted profile cells", async () => {
    projectEvent(db, assertE("ev_01", profileCell("mc_a", "user.stack.lang", "haskell")));
    projectEvent(db, {
      v: 1, ev: "retract", id: "ev_02", actor: "user", ts: "2026-04-22T00:00:01Z",
      target: "mc_a", reason: "test"
    });
    await renderProfile(db);
    const content = readFileSync(join(vaultPath(), "profile.md"), "utf8");
    expect(content).not.toContain("haskell");
  });
});

describe("renderScene", () => {
  test("writes scene markdown with embedded cells as wikilinks", async () => {
    const cellA: MemCell = {
      id: "mc_a", type: "episodic",
      E: "Discussed auth refactor approach.",
      F: [], M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
      confidence: 0.8, evidence: [], scene_ids: ["ms_auth"], access: { count: 0, last_at: null },
    };
    projectEvent(db, assertE("ev_01", cellA));
    db.query("INSERT INTO scenes(id, label, description, created_at, updated_at) VALUES(?, ?, ?, ?, ?)")
      .run("ms_auth-refactor", "Auth Refactor", "Work related to auth overhaul.", 0, 0);
    db.query("INSERT INTO scene_members(scene_id, cell_id) VALUES(?, ?)")
      .run("ms_auth-refactor", "mc_a");
    await renderScene(db, "ms_auth-refactor");
    const content = readFileSync(join(vaultPath(), "scenes", "auth-refactor.md"), "utf8");
    expect(content).toContain("# Auth Refactor");
    expect(content).toContain("Discussed auth refactor approach");
    expect(content).toContain("mc_a");
  });
});

describe("renderSkill", () => {
  test("writes SKILL.md with CC-compatible frontmatter", async () => {
    const skillCell: MemCell = {
      id: "mc_s1", type: "skill",
      E: "Commit style preference skill.",
      F: [], M: { created_at: "2026-04-22T00:00:00Z", actor: "user" },
      confidence: 0.95, evidence: [], scene_ids: [],
      access: { count: 3, last_at: "2026-04-22T01:00:00Z" },
      skill: {
        name: "commit-style",
        description: "Use gitmoji in commits, conventional format body.",
        trigger_phrases: ["make a commit", "commit this", "git commit"],
        body: "Always prefix commit subject with gitmoji. Keep under 72 chars.",
        version: "1.0.0",
        compatibility: "claude-code>=1.0 OR factory-droid>=0.5",
        source_events: ["ev_xyz"],
      },
    };
    projectEvent(db, assertE("ev_01", skillCell));
    await renderSkill(db, "mc_s1");
    const content = readFileSync(join(vaultPath(), "skills", "commit-style.md"), "utf8");
    expect(content).toContain("---");
    expect(content).toContain("name: commit-style");
    expect(content).toContain("description:");
    expect(content).toContain("Always prefix commit subject with gitmoji");
  });

  test("skipped if cell retracted", async () => {
    const skillCell: MemCell = {
      id: "mc_s2", type: "skill",
      E: "obsolete", F: [], M: { created_at: "2026-04-22T00:00:00Z", actor: "user" },
      confidence: 0.5, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
      skill: {
        name: "obsolete", description: "", trigger_phrases: [], body: "x",
        version: "0.0.1", compatibility: "any", source_events: [],
      },
    };
    projectEvent(db, assertE("ev_01", skillCell));
    projectEvent(db, {
      v: 1, ev: "retract", id: "ev_02", actor: "user", ts: "2026-04-22T00:00:01Z",
      target: "mc_s2", reason: "test",
    });
    await renderSkill(db, "mc_s2");
    expect(existsSync(join(vaultPath(), "skills", "obsolete.md"))).toBe(false);
  });
});

describe("dashboards", () => {
  test("renderForesight writes timeline of active foresight items", async () => {
    const withForesight: MemCell = {
      id: "mc_f1", type: "project",
      E: "Working on auth refactor through Q2.",
      F: [], M: { created_at: "2026-04-22T00:00:00Z", actor: "reviewer" },
      P: { inference: "Will ship auth refactor by 2026-06-30", t_start: "2026-04-22T00:00:00Z", t_end: "2026-06-30T00:00:00Z", status: "active" },
      confidence: 0.7, evidence: [], scene_ids: [], access: { count: 0, last_at: null },
    };
    projectEvent(db, assertE("ev_01", withForesight));
    await renderForesight(db);
    const content = readFileSync(join(vaultPath(), "_foresight.md"), "utf8");
    expect(content).toContain("auth refactor");
    expect(content).toContain("2026-06-30");
  });

  test("renderContradictions writes callout block per contradict event", async () => {
    projectEvent(db, {
      v: 1, ev: "contradict", id: "ev_c1", actor: "judge", ts: "2026-04-22T00:00:00Z",
      a: "mc_x", b: "mc_y", resolution: "flag_both",
    });
    await renderContradictions(db);
    const content = readFileSync(join(vaultPath(), "_contradictions.md"), "utf8");
    expect(content).toContain("[!contradiction]");
    expect(content).toContain("mc_x");
    expect(content).toContain("mc_y");
  });

  test("renderIndex writes dashboard with counts", async () => {
    projectEvent(db, assertE("ev_01", profileCell("mc_a", "user.stack.lang", "typescript")));
    await renderIndex(db);
    const content = readFileSync(join(vaultPath(), "_index.md"), "utf8");
    expect(content).toContain("# Pebble Index");
    expect(content).toMatch(/Total cells:[^\n]*\d+/);
  });
});
