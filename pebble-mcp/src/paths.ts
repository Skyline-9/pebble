// src/paths.ts
import { homedir } from "node:os";
import { join } from "node:path";

export function resolvePebbleRoot(): string {
  return process.env.PEBBLE_ROOT ?? join(homedir(), ".pebble");
}

export const logPath        = () => join(resolvePebbleRoot(), "log.jsonl");
export const dbPath         = () => join(resolvePebbleRoot(), "projection.db");
export const vaultPath      = () => join(resolvePebbleRoot(), "vault");
export const tracePath      = () => join(resolvePebbleRoot(), "trace.jsonl");
export const checkpointDir  = () => join(resolvePebbleRoot(), "checkpoints");
