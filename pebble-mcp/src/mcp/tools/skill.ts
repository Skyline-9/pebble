// src/mcp/tools/skill.ts
import type { Database } from "bun:sqlite";
import { newCellId, newEventId } from "../../ids";
import { appendEvents } from "../../log/writer";
import { projectEvent } from "../../projection/projector";
import type { MemCell, AssertEvent, SkillPayload } from "../../types";

export interface SkillContext { db: Database; }

export interface SkillSaveArgs {
  name: string;
  description: string;
  body: string;
  trigger_phrases: string[];
  confidence: number;
  allowed_tools?: string[];
  version?: string;
  compatibility?: string;
}

export function registerSkillTools(ctx: SkillContext) {
  const { db } = ctx;

  async function skill_save(args: SkillSaveArgs): Promise<{ ok: true; cell_id: string }> {
    const cell_id = newCellId();
    const now = new Date().toISOString();
    const skill: SkillPayload = {
      name: args.name,
      description: args.description,
      body: args.body,
      trigger_phrases: args.trigger_phrases,
      allowed_tools: args.allowed_tools,
      version: args.version ?? "0.1.0",
      compatibility: args.compatibility ?? "claude-code>=1.0 OR factory-droid>=0.5",
      source_events: [],
    };
    const cell: MemCell = {
      id: cell_id,
      type: "skill",
      E: `Skill: ${args.name} — ${args.description}`,
      F: [],
      M: { created_at: now, actor: "user" },
      confidence: args.confidence,
      evidence: [],
      scene_ids: [],
      access: { count: 0, last_at: null },
      skill,
    };
    const ev: AssertEvent = {
      v: 1, ev: "assert", id: newEventId(), actor: "user", ts: now,
      cell_id, cell,
    };
    await appendEvents([ev]);
    projectEvent(db, ev);
    return { ok: true, cell_id };
  }

  async function skill_list(_args: Record<string, never>): Promise<{ skills: Array<{ name: string; description: string; cell_id: string }> }> {
    const rows = db.query(`
      SELECT id, json FROM cells WHERE type='skill' AND retracted_at IS NULL
      ORDER BY access_count DESC, confidence DESC
    `).all() as { id: string; json: string }[];
    const skills = rows.map(r => {
      const cell = JSON.parse(r.json) as MemCell;
      return { name: cell.skill!.name, description: cell.skill!.description, cell_id: cell.id };
    });
    return { skills };
  }

  async function skill_read(args: { name: string }): Promise<{ skill: SkillPayload | null }> {
    const rows = db.query(`
      SELECT json FROM cells WHERE type='skill' AND retracted_at IS NULL
    `).all() as { json: string }[];
    for (const r of rows) {
      const cell = JSON.parse(r.json) as MemCell;
      if (cell.skill?.name === args.name) return { skill: cell.skill };
    }
    return { skill: null };
  }

  return { skill_save, skill_list, skill_read };
}
