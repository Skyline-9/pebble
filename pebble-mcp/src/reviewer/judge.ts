// src/reviewer/judge.ts
import type { Database } from "bun:sqlite";
import { hybridSearch } from "../retrieval/score";
import type { CellType, MemCell } from "../types";

export type JudgeAction = "assert" | "supersede" | "merge" | "flag_both" | "discard";

export interface JudgeDecision {
  action: JudgeAction;
  reason: string;
  supersede_target?: string;
  contradict_target?: string;
}

/** Type-aware confidence thresholds per spec §6.5. */
const CONFIDENCE_THRESHOLDS: Record<CellType, number> = {
  profile: 0.9,
  preference: 0.75,
  project: 0.7,
  episodic: 0.5,
  skill: 0.8,
  transient: 0.5,
};

const DUPLICATE_SIMILARITY_THRESHOLD = 0.85;

export function judgeCandidate(db: Database, candidate: MemCell): JudgeDecision {
  const threshold = CONFIDENCE_THRESHOLDS[candidate.type];
  if (candidate.confidence < threshold) {
    return {
      action: "discard",
      reason: `confidence ${candidate.confidence.toFixed(2)} below ${candidate.type} threshold ${threshold}`,
    };
  }

  const hits = hybridSearch(db, candidate.E, { topK: 3 });
  if (hits.length > 0 && hits[0]!.scores.bm25 >= DUPLICATE_SIMILARITY_THRESHOLD) {
    return {
      action: "discard",
      reason: `duplicate of ${hits[0]!.cell_id} (bm25 ${hits[0]!.scores.bm25.toFixed(2)})`,
    };
  }

  return { action: "assert", reason: "passed confidence + dedup gates" };
}
