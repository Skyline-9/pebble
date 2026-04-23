// src/render/vault.ts
import { mkdirSync, existsSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { vaultPath } from "../paths";

export function ensureVault(): void {
  const root = vaultPath();
  if (!existsSync(root)) mkdirSync(root, { recursive: true });
  for (const sub of ["scenes", "skills"]) {
    const p = join(root, sub);
    if (!existsSync(p)) mkdirSync(p, { recursive: true });
  }
}

export function writeVaultFile(relPath: string, content: string): void {
  ensureVault();
  const full = join(vaultPath(), relPath);
  if (!existsSync(dirname(full))) mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, content, "utf8");
}
