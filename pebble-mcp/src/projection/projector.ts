// src/projection/projector.ts
import type { Database } from "bun:sqlite";
import type { MemCell, MemEvent } from "../types";

function tsToMs(iso: string): number {
  return new Date(iso).getTime();
}

function buildFtsContent(cell: MemCell): string {
  const factsText = cell.F.map(f => `${f.subject} ${f.predicate} ${f.object}`).join(" ");
  return `${cell.E}\n${factsText}`;
}

/** Idempotent projection of a single event. Returns true if applied, false if already-seen. */
export function projectEvent(db: Database, ev: MemEvent): boolean {
  const existing = db.query("SELECT id FROM events WHERE id=?").get(ev.id);
  if (existing) return false;

  const seq = (db.query("SELECT COALESCE(MAX(seq), 0) + 1 AS next FROM events").get() as any).next as number;
  const ts_ms = tsToMs(ev.ts);
  const tx = db.transaction(() => {
    db.query("INSERT INTO events(id, seq, type, json, ts) VALUES(?, ?, ?, ?, ?)")
      .run(ev.id, seq, ev.ev, JSON.stringify(ev), ts_ms);

    switch (ev.ev) {
      case "assert": {
        const cell = ev.cell;
        db.query(`INSERT OR REPLACE INTO cells
          (id, type, json, confidence, created_at, updated_at, retracted_at, access_count, last_accessed_at)
          VALUES(?, ?, ?, ?, ?, ?, NULL, 0, NULL)`).run(
          cell.id, cell.type, JSON.stringify(cell), cell.confidence,
          ts_ms, ts_ms
        );
        db.query("DELETE FROM facts WHERE cell_id=?").run(cell.id);
        const insertFact = db.query("INSERT INTO facts(cell_id, subject, predicate, object, confidence) VALUES(?, ?, ?, ?, ?)");
        for (const f of cell.F) {
          // Skip malformed facts (missing subject/predicate/object) — they're schema-violating
          // payloads that shouldn't have been asserted, but we don't want to crash the projector.
          if (!f || typeof f.subject !== "string" || typeof f.predicate !== "string" || typeof f.object !== "string") continue;
          // Default fact confidence to the cell confidence when missing (MCP clients may omit it).
          const fc = (typeof f.confidence === "number" && Number.isFinite(f.confidence)) ? f.confidence : cell.confidence;
          insertFact.run(cell.id, f.subject, f.predicate, f.object, fc);
        }
        db.query("DELETE FROM cells_fts WHERE cell_id=?").run(cell.id);
        db.query("INSERT INTO cells_fts(cell_id, content) VALUES(?, ?)").run(cell.id, buildFtsContent(cell));
        if (cell.P) {
          db.query(`INSERT OR REPLACE INTO foresight(cell_id, inference, t_start, t_end, status)
            VALUES(?, ?, ?, ?, ?)`).run(
            cell.id, cell.P.inference, tsToMs(cell.P.t_start),
            cell.P.t_end ? tsToMs(cell.P.t_end) : null, cell.P.status
          );
        }
        break;
      }
      case "touch": {
        db.query("UPDATE cells SET access_count=access_count+1, last_accessed_at=? WHERE id=?")
          .run(ts_ms, ev.target);
        break;
      }
      case "retract": {
        db.query("UPDATE cells SET retracted_at=? WHERE id=?").run(ts_ms, ev.target);
        break;
      }
      case "expire": {
        db.query("UPDATE foresight SET status='expired' WHERE cell_id=?").run(ev.target);
        db.query("UPDATE cells SET retracted_at=? WHERE id=? AND type='transient'").run(ts_ms, ev.target);
        break;
      }
      case "supersede": {
        const old = db.query("SELECT json FROM cells WHERE id=?").get(ev.target) as { json: string } | null;
        if (old) {
          const oldCell = JSON.parse(old.json) as MemCell;
          oldCell.superseded_by = ev.by;
          db.query("UPDATE cells SET json=?, retracted_at=?, updated_at=? WHERE id=?")
            .run(JSON.stringify(oldCell), ts_ms, ts_ms, ev.target);
        }
        const nw = db.query("SELECT json FROM cells WHERE id=?").get(ev.by) as { json: string } | null;
        if (nw) {
          const newCell = JSON.parse(nw.json) as MemCell;
          newCell.supersedes = [...(newCell.supersedes ?? []), ev.target];
          db.query("UPDATE cells SET json=?, updated_at=? WHERE id=?")
            .run(JSON.stringify(newCell), ts_ms, ev.by);
        }
        break;
      }
      case "contradict":
      case "correct":
      case "user_edit":
      case "checkpoint": {
        break;
      }
    }
  });
  tx();
  return true;
}

export function projectAll(db: Database, events: Iterable<MemEvent>): number {
  let n = 0;
  for (const ev of events) if (projectEvent(db, ev)) n++;
  return n;
}
