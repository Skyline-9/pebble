// src/cli/review-turn.ts
import { Database } from "bun:sqlite";
import { readFileSync, existsSync } from "node:fs";
import { dbPath } from "../paths";
import { initSchema } from "../projection/schema";
import { extractCandidates, type Transcript } from "../reviewer/extractor";
import { judgeCandidate } from "../reviewer/judge";
import { appendEvents } from "../log/writer";
import { projectEvent } from "../projection/projector";
import { newEventId } from "../ids";
import type { AssertEvent } from "../types";

interface Args { transcript?: string; }

function parseArgs(argv: string[]): Args {
  const out: Args = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--transcript" && argv[i + 1] !== undefined) {
      out.transcript = argv[i + 1]; i++;
    }
  }
  return out;
}

function normalizeRole(role: unknown): "user" | "assistant" | "tool" | null {
  if (role === "user" || role === "assistant" || role === "tool") return role;
  return null;
}

function extractTextFromContent(content: unknown): string | null {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return null;
  const parts: string[] = [];
  for (const part of content) {
    if (!part || typeof part !== "object") continue;
    const p = part as { type?: unknown; text?: unknown };
    if (p.type === "text" && typeof p.text === "string") parts.push(p.text);
  }
  return parts.length ? parts.join("\n") : null;
}

function readTranscript(path: string): Transcript {
  if (!existsSync(path)) return [];
  const raw = readFileSync(path, "utf8");
  const lines = raw.split("\n").filter(l => l.trim());
  const turns: Transcript = [];
  for (const line of lines) {
    try {
      const t = JSON.parse(line);
      if (!t || typeof t !== "object") continue;

      // Flat format: {role, content: string}
      if (typeof t.role === "string" && typeof t.content === "string") {
        const role = normalizeRole(t.role);
        if (role) turns.push({ role, content: t.content });
        continue;
      }

      // Factory session format: {type: "message", message: {role, content: array-or-string}}
      if (t.type === "message" && t.message && typeof t.message === "object") {
        const role = normalizeRole((t.message as any).role);
        const text = extractTextFromContent((t.message as any).content);
        if (role && text) turns.push({ role, content: text });
        continue;
      }
    } catch { /* skip malformed */ }
  }
  return turns;
}

export async function cliReviewTurn(argv: string[]): Promise<void> {
  const args = parseArgs(argv);
  if (!args.transcript) {
    console.error("usage: pebble-mcp review-turn --transcript <path>");
    process.exit(1);
  }

  const db = new Database(dbPath());
  initSchema(db);

  try {
    const transcript = readTranscript(args.transcript);
    const candidates = extractCandidates(transcript);
    let asserted = 0, discarded = 0;
    const events: AssertEvent[] = [];
    const now = new Date().toISOString();

    for (const cand of candidates) {
      const decision = judgeCandidate(db, cand);
      if (decision.action !== "assert") { discarded++; continue; }
      const ev: AssertEvent = {
        v: 1, ev: "assert", id: newEventId(), actor: "reviewer", ts: now,
        cell_id: cand.id, cell: cand,
      };
      events.push(ev);
      asserted++;
    }

    if (events.length > 0) {
      await appendEvents(events);
      for (const ev of events) projectEvent(db, ev);
    }
    console.log(`asserted: ${asserted}, discarded: ${discarded}, seen: ${candidates.length}`);
  } finally {
    db.close();
  }
}
