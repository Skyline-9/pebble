// src/projection/schema.ts
import type { Database } from "bun:sqlite";

export const SCHEMA_VERSION = 1;

const DDL = [
  `CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
  )`,
  `CREATE TABLE IF NOT EXISTS cells (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    json BLOB NOT NULL,
    confidence REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    retracted_at INTEGER,
    access_count INTEGER NOT NULL DEFAULT 0,
    last_accessed_at INTEGER
  )`,
  `CREATE INDEX IF NOT EXISTS idx_cells_type ON cells(type)`,
  `CREATE INDEX IF NOT EXISTS idx_cells_retracted ON cells(retracted_at)`,
  `CREATE INDEX IF NOT EXISTS idx_cells_last_accessed ON cells(last_accessed_at)`,
  `CREATE TABLE IF NOT EXISTS facts (
    cell_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL NOT NULL,
    FOREIGN KEY(cell_id) REFERENCES cells(id) ON DELETE CASCADE
  )`,
  `CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject)`,
  `CREATE INDEX IF NOT EXISTS idx_facts_cell ON facts(cell_id)`,
  `CREATE TABLE IF NOT EXISTS scenes (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
  )`,
  `CREATE TABLE IF NOT EXISTS scene_members (
    scene_id TEXT NOT NULL,
    cell_id TEXT NOT NULL,
    PRIMARY KEY(scene_id, cell_id),
    FOREIGN KEY(scene_id) REFERENCES scenes(id) ON DELETE CASCADE,
    FOREIGN KEY(cell_id) REFERENCES cells(id) ON DELETE CASCADE
  )`,
  `CREATE TABLE IF NOT EXISTS foresight (
    cell_id TEXT PRIMARY KEY,
    inference TEXT NOT NULL,
    t_start INTEGER NOT NULL,
    t_end INTEGER,
    status TEXT NOT NULL,
    FOREIGN KEY(cell_id) REFERENCES cells(id) ON DELETE CASCADE
  )`,
  `CREATE INDEX IF NOT EXISTS idx_foresight_status ON foresight(status)`,
  `CREATE INDEX IF NOT EXISTS idx_foresight_tend ON foresight(t_end)`,
  `CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    seq INTEGER NOT NULL,
    type TEXT NOT NULL,
    json BLOB NOT NULL,
    ts INTEGER NOT NULL
  )`,
  `CREATE INDEX IF NOT EXISTS idx_events_seq ON events(seq)`,
  `CREATE VIRTUAL TABLE IF NOT EXISTS cells_fts USING fts5(
    cell_id UNINDEXED,
    content,
    tokenize='porter unicode61'
  )`,
];

export function initSchema(db: Database): void {
  db.exec("PRAGMA journal_mode=WAL");
  db.exec("PRAGMA foreign_keys=ON");
  for (const stmt of DDL) db.exec(stmt);
  db.query("INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?)").run(String(SCHEMA_VERSION));
}
