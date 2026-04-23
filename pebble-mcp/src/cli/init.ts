// src/cli/init.ts
import { Database } from "bun:sqlite";
import { mkdirSync, existsSync, appendFileSync } from "node:fs";
import { resolvePebbleRoot, logPath, dbPath, vaultPath, tracePath, checkpointDir } from "../paths";
import { initSchema } from "../projection/schema";
import { ensureGitRepo } from "../git/turn-commit";

export function cliInit(): void {
  const root = resolvePebbleRoot();
  if (!existsSync(root)) mkdirSync(root, { recursive: true });
  if (!existsSync(vaultPath())) mkdirSync(vaultPath(), { recursive: true });
  if (!existsSync(checkpointDir())) mkdirSync(checkpointDir(), { recursive: true });
  if (!existsSync(logPath())) appendFileSync(logPath(), "");
  if (!existsSync(tracePath())) appendFileSync(tracePath(), "");
  const db = new Database(dbPath());
  initSchema(db);
  db.close();
  ensureGitRepo();
  console.log(`pebble initialized at ${root}`);
}
