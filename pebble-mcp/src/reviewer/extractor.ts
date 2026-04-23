// src/reviewer/extractor.ts
import { newCellId } from "../ids";
import type { CellType, MemCell, AtomicFact } from "../types";

export type Transcript = Array<{ role: "user" | "assistant" | "tool"; content: string }>;

interface Rule {
  pattern: RegExp;
  type: CellType;
  makeFacts: (m: RegExpMatchArray) => AtomicFact[];
}

const RULES: Rule[] = [
  {
    pattern: /\bmy\s+(?:primary|main)\s+(?:lang|language)\s+is\s+([\w#.+-]+)/i,
    type: "profile",
    makeFacts: (m) => [{
      subject: "user.stack.lang",
      predicate: "is",
      object: m[1]!.toLowerCase(),
      confidence: 0.95,
    }],
  },
  {
    pattern: /\bI (?:prefer|like|use)\s+([A-Z][\w.+-]{1,40})(?:\s+for\s+(\w+))?/i,
    type: "preference",
    makeFacts: (m) => [{
      subject: "user.prefers",
      predicate: "prefers",
      object: m[1]!,
      confidence: 0.8,
    }],
  },
  {
    pattern: /\bI'?m\s+working\s+on\s+(?:the\s+)?([\w\s-]{3,40})/i,
    type: "project",
    makeFacts: (m) => [{
      subject: "project.current",
      predicate: "is",
      object: m[1]!.trim(),
      confidence: 0.7,
    }],
  },
];

export function extractCandidates(transcript: Transcript): MemCell[] {
  const out: MemCell[] = [];
  const now = new Date().toISOString();
  for (const turn of transcript) {
    if (turn.role !== "user") continue;
    for (const rule of RULES) {
      const m = turn.content.match(rule.pattern);
      if (!m) continue;
      const facts = rule.makeFacts(m);
      const confidence = Math.min(...facts.map(f => f.confidence));
      out.push({
        id: newCellId(),
        type: rule.type,
        E: turn.content.slice(0, 200),
        F: facts,
        M: { created_at: now, actor: "reviewer", source: "user-turn" },
        confidence,
        evidence: [],
        scene_ids: [],
        access: { count: 0, last_at: null },
      });
      break; // only first matching rule per turn
    }
  }
  return out;
}
