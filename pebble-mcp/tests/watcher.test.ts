import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startWatcher } from "../src/watcher/fs";
import { readEvents } from "../src/log/reader";
import { vaultPath } from "../src/paths";

let tmp: string;
beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "pebble-watch-"));
  process.env.PEBBLE_ROOT = tmp;
  mkdirSync(join(vaultPath(), "scenes"), { recursive: true });
});
afterEach(() => {
  delete process.env.PEBBLE_ROOT;
  rmSync(tmp, { recursive: true, force: true });
});

describe("file watcher", () => {
  test("emits user_edit event when a scene file changes", async () => {
    const watcher = await startWatcher();
    try {
      await new Promise(r => setTimeout(r, 200)); // let watcher become ready
      writeFileSync(join(vaultPath(), "scenes", "test.md"), "---\nscene_id: ms_test\n---\n# Test\nChanged.");
      await new Promise(r => setTimeout(r, 600));
      const events = [];
      for await (const ev of readEvents()) events.push(ev);
      const userEdits = events.filter(e => e.ev === "user_edit");
      expect(userEdits.length).toBeGreaterThanOrEqual(1);
    } finally {
      await watcher.stop();
    }
  });
});
