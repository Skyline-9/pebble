import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { initSchema, SCHEMA_VERSION } from "../src/projection/schema";

let tmp: string;
let db: Database;

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-schema-"));
  db = new Database(join(tmp, "test.db"));
});
afterEach(() => {
  db.close();
  rmSync(tmp, { recursive: true, force: true });
});

describe("schema", () => {
  test("initSchema creates all tables", () => {
    initSchema(db);
    const tables = db
      .query("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
      .all() as { name: string }[];
    const names = tables.map(t => t.name);
    expect(names).toContain("cells");
    expect(names).toContain("facts");
    expect(names).toContain("scenes");
    expect(names).toContain("scene_members");
    expect(names).toContain("foresight");
    expect(names).toContain("events");
    expect(names).toContain("meta");
  });

  test("initSchema creates FTS5 virtual table", () => {
    initSchema(db);
    const fts = db
      .query("SELECT name FROM sqlite_master WHERE name='cells_fts'")
      .get() as { name: string } | null;
    expect(fts?.name).toBe("cells_fts");
  });

  test("initSchema is idempotent", () => {
    initSchema(db);
    initSchema(db);
    const version = db.query("SELECT value FROM meta WHERE key='schema_version'").get() as { value: string };
    expect(version.value).toBe(String(SCHEMA_VERSION));
  });
});
