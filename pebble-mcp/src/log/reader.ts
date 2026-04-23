// src/log/reader.ts
import { createReadStream, existsSync } from "node:fs";
import readline from "node:readline";
import { logPath } from "../paths";
import type { MemEvent } from "../types";

export async function* readEvents(): AsyncGenerator<MemEvent> {
  if (!existsSync(logPath())) return;
  const rl = readline.createInterface({
    input: createReadStream(logPath(), { encoding: "utf8" }),
    crlfDelay: Infinity,
  });
  for await (const line of rl) {
    if (!line.trim()) continue;
    try {
      yield JSON.parse(line) as MemEvent;
    } catch (err) {
      console.error(`[pebble-log] skipping malformed line: ${line.slice(0, 80)}`);
    }
  }
}

export async function countEvents(): Promise<number> {
  let n = 0;
  for await (const _ of readEvents()) n++;
  return n;
}
