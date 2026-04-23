// src/watcher/fs.ts
import chokidar, { type FSWatcher } from "chokidar";
import { vaultPath } from "../paths";
import { appendEvents } from "../log/writer";
import { newEventId } from "../ids";
import { existsSync, readFileSync } from "node:fs";
import matter from "gray-matter";
import type { UserEditEvent } from "../types";

export interface Watcher {
  stop(): Promise<void>;
}

const DEBOUNCE_MS = 200;

export async function startWatcher(): Promise<Watcher> {
  const root = vaultPath();
  const watcher: FSWatcher = chokidar.watch(`${root}/**/*.md`, {
    ignoreInitial: true,
    awaitWriteFinish: { stabilityThreshold: DEBOUNCE_MS, pollInterval: 50 },
  });

  const onChange = async (path: string) => {
    try {
      if (!existsSync(path)) return;
      const raw = readFileSync(path, "utf8");
      const parsed = matter(raw);
      const fm = parsed.data as Record<string, unknown>;
      const target_id = (fm.cell_id ?? fm.scene_id ?? path.split("/").pop()) as string;
      const ev: UserEditEvent = {
        v: 1,
        ev: "user_edit",
        id: newEventId(),
        actor: "user",
        ts: new Date().toISOString(),
        cell_id: String(target_id),
        diff: { M: { created_at: new Date().toISOString(), actor: "user" } as any },
      };
      await appendEvents([ev]);
    } catch (err) {
      console.error(`[pebble-watcher] change emit failed for ${path}:`, err);
    }
  };

  watcher.on("change", onChange);
  watcher.on("add", onChange);

  return {
    stop: async () => { await watcher.close(); },
  };
}
