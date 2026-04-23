// src/log/writer.ts
import { appendFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname } from "node:path";
import lockfile from "proper-lockfile";
import { logPath, resolvePebbleRoot } from "../paths";
import type { MemEvent } from "../types";

function ensureDir() {
  const root = resolvePebbleRoot();
  if (!existsSync(root)) mkdirSync(root, { recursive: true });
  const logDir = dirname(logPath());
  if (!existsSync(logDir)) mkdirSync(logDir, { recursive: true });
}

function ensureLogFile() {
  ensureDir();
  if (!existsSync(logPath())) appendFileSync(logPath(), "");
}

export async function appendEvents(events: MemEvent[]): Promise<void> {
  if (events.length === 0) return;
  ensureLogFile();
  const serialized = events.map(e => JSON.stringify(e)).join("\n") + "\n";
  const release = await lockfile.lock(logPath(), {
    retries: { retries: 50, minTimeout: 5, maxTimeout: 50 },
  });
  try {
    appendFileSync(logPath(), serialized, { flag: "a" });
  } finally {
    await release();
  }
}
