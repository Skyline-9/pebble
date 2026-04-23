// src/projection/checkpoint.ts
import type { Database } from "bun:sqlite";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { checkpointDir } from "../paths";

export const CHECKPOINT_INTERVAL = 500;

export function shouldCheckpoint(seq: number): boolean {
  return seq > 0 && seq % CHECKPOINT_INTERVAL === 0;
}

export async function snapshotProjection(db: Database, seq: number): Promise<string> {
  if (!existsSync(checkpointDir())) mkdirSync(checkpointDir(), { recursive: true });
  const target = join(checkpointDir(), `${String(seq).padStart(8, "0")}.db`);
  db.query(`VACUUM INTO ?`).run(target);
  return target;
}

export async function restoreProjection(db: Database, checkpointPath: string): Promise<void> {
  const dbFilename = (db as any).filename;
  db.close();
  copyFileSync(checkpointPath, dbFilename);
}
