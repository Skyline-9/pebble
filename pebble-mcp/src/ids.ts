// src/ids.ts
import { ulid } from "ulid";

export function newCellId(): string {
  return `mc_${ulid()}`;
}

export function newEventId(): string {
  return `ev_${ulid()}`;
}

export function newSceneId(label: string): string {
  const slug = label
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
  return `ms_${slug}`;
}
