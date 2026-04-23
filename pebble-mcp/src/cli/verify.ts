// src/cli/verify.ts
import { Database } from "bun:sqlite";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { dbPath } from "../paths";
import { initSchema } from "../projection/schema";
import { projectAll } from "../projection/projector";
import { readEvents } from "../log/reader";

export async function cliVerify(): Promise<void> {
  const tmp = mkdtempSync(join(tmpdir(), "pebble-verify-"));
  const ephemeralPath = join(tmp, "verify.db");
  try {
    const ephDb = new Database(ephemeralPath);
    initSchema(ephDb);
    const events: any[] = [];
    for await (const ev of readEvents()) events.push(ev);
    projectAll(ephDb, events);

    const mainDb = new Database(dbPath(), { readonly: true });
    const mainCells = (mainDb.query("SELECT COUNT(*) AS n FROM cells").get() as any).n as number;
    const mainEvents = (mainDb.query("SELECT COUNT(*) AS n FROM events").get() as any).n as number;
    const ephCells = (ephDb.query("SELECT COUNT(*) AS n FROM cells").get() as any).n as number;
    const ephEvents = (ephDb.query("SELECT COUNT(*) AS n FROM events").get() as any).n as number;
    mainDb.close();
    ephDb.close();

    if (mainCells === ephCells && mainEvents === ephEvents) {
      console.log(`ok — projection matches replay (${mainCells} cells, ${mainEvents} events)`);
    } else {
      console.error(`MISMATCH — main(${mainCells} cells, ${mainEvents} events) vs replay(${ephCells} cells, ${ephEvents} events)`);
      process.exit(1);
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}
