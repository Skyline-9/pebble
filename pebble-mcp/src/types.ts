// src/types.ts

export type CellType =
  | "profile" | "preference" | "project"
  | "episodic" | "skill" | "transient";

export interface AtomicFact {
  subject: string;
  predicate: string;
  object: string;
  confidence: number;
}

export type ForesightStatus = "active" | "expired" | "fulfilled" | "invalidated";

export interface Foresight {
  inference: string;
  t_start: string;  // ISO8601
  t_end?: string;
  status: ForesightStatus;
}

export interface Evidence {
  event_id: string;
  kind: "user_query" | "tool_result" | "file_read" | "url_fetch" | "user_edit";
  excerpt?: string;
}

export interface Metadata {
  created_at: string;
  source?: string;
  actor: "reviewer" | "user" | "system" | "judge";
  thread_id?: string;
  project_id?: string;
}

export interface AccessStats {
  count: number;
  last_at: string | null;
  last_query_hash?: string;
}

export interface MemCell {
  id: string;
  type: CellType;
  E: string;
  F: AtomicFact[];
  P?: Foresight;
  M: Metadata;
  confidence: number;
  evidence: Evidence[];
  scene_ids: string[];
  access: AccessStats;
  supersedes?: string[];
  superseded_by?: string;
  retracted_at?: string;
  skill?: SkillPayload;  // present when type === "skill"
}

export interface SkillPayload {
  name: string;
  description: string;
  trigger_phrases: string[];
  body: string;
  allowed_tools?: string[];
  version: string;
  compatibility: string;
  source_events: string[];
}

export interface MemScene {
  id: string;
  label: string;
  description: string;
  cell_ids: string[];
  centroid?: number[];  // V1+ only; undefined in MVP
  created_at: string;
  updated_at: string;
}

// Events — log.jsonl entries ---------------------------------

type EventBase = {
  v: 1;
  id: string;           // ULID
  actor: "reviewer" | "user" | "system" | "judge";
  ts: string;           // ISO8601
};

export type AssertEvent    = EventBase & { ev: "assert"; cell_id: string; cell: MemCell };
export type SupersedeEvent = EventBase & { ev: "supersede"; target: string; by: string; reason: string };
export type RetractEvent   = EventBase & { ev: "retract"; target: string; reason: string };
export type ExpireEvent    = EventBase & { ev: "expire"; target: string; reason: string };
export type ContradictEvent= EventBase & { ev: "contradict"; a: string; b: string; resolution: "flag_both" | "keep_a" | "keep_b" };
export type TouchEvent     = EventBase & { ev: "touch"; target: string; query?: string };
export type CorrectEvent   = EventBase & { ev: "correct"; target: string; diff: Partial<MemCell> };
export type UserEditEvent  = EventBase & { ev: "user_edit"; cell_id: string; diff: Partial<MemCell> };
export type CheckpointEvent= EventBase & { ev: "checkpoint"; at_seq: number; db_hash: string };

export type MemEvent =
  | AssertEvent | SupersedeEvent | RetractEvent | ExpireEvent
  | ContradictEvent | TouchEvent | CorrectEvent | UserEditEvent | CheckpointEvent;

// Profile (singleton, derived) -------------------------------

export interface Profile {
  voice: {
    tone: string;
    vocabulary_dos: string[];
    vocabulary_donts: string[];
    examples: string[];
  };
  stack: {
    primary_langs: string[];
    frameworks: string[];
    tools: string[];
    never_use: string[];
  };
  conventions: {
    commit_style: string;
    code_style: string;
    test_style: string;
    doc_style: string;
  };
  goals: Foresight[];
  updated_at: string;
}
