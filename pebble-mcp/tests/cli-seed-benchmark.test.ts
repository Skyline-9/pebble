import { afterEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { seedBenchmark } from "../src/cli/seed-benchmark";
import { dbPath } from "../src/paths";

describe("seed-benchmark", () => {
  const roots: string[] = [];

  afterEach(() => {
    for (const root of roots.splice(0)) rmSync(root, { recursive: true });
    delete process.env.PEBBLE_ROOT;
  });

  test("creates the requested number of deterministic cells", async () => {
    const root = mkdtempSync(join(tmpdir(), "pebble-benchmark-"));
    roots.push(root);
    process.env.PEBBLE_ROOT = root;

    await seedBenchmark(100);

    const db = new Database(dbPath(), { readonly: true });
    const row = db.query("SELECT COUNT(*) AS count FROM cells").get() as {
      count: number;
    };
    db.close();
    expect(row.count).toBe(100);
  });
});
