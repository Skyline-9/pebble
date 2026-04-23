// src/cli/status.ts
import { Database } from "bun:sqlite";
import { existsSync } from "node:fs";
import { dbPath, resolvePebbleRoot, logPath } from "../paths";

export function cliStatus(): void {
  const root = resolvePebbleRoot();
  console.log(`root: ${root}`);
  if (!existsSync(dbPath())) { console.log("projection.db: absent (run `pebble-mcp init`)"); return; }
  const db = new Database(dbPath(), { readonly: true });
  const cells = (db.query("SELECT COUNT(*) AS n FROM cells WHERE retracted_at IS NULL").get() as any).n;
  const events = (db.query("SELECT COUNT(*) AS n FROM events").get() as any).n;
  const skills = (db.query("SELECT COUNT(*) AS n FROM cells WHERE type='skill' AND retracted_at IS NULL").get() as any).n;
  db.close();
  console.log(`log: ${logPath()} (${existsSync(logPath()) ? "present" : "missing"})`);
  console.log(`cells: ${cells}`);
  console.log(`events: ${events}`);
  console.log(`skills: ${skills}`);
}
