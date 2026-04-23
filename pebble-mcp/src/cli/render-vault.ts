// src/cli/render-vault.ts
import { Database } from "bun:sqlite";
import { dbPath, vaultPath } from "../paths";
import { renderProfile } from "../render/profile";
import { renderSkill } from "../render/skill";
import { renderScene } from "../render/scene";
import { renderForesight } from "../render/foresight";
import { renderContradictions } from "../render/contradictions";
import { renderIndex } from "../render/index-md";

export async function cliRenderVault(): Promise<void> {
  const db = new Database(dbPath(), { readonly: true });
  try {
    const skillCells = db.query(
      "SELECT id FROM cells WHERE type='skill' AND retracted_at IS NULL"
    ).all() as { id: string }[];
    const sceneIds = db.query("SELECT id FROM scenes").all() as { id: string }[];

    let skillsRendered = 0;
    let scenesRendered = 0;

    await renderProfile(db);
    for (const { id } of skillCells) { await renderSkill(db, id); skillsRendered++; }
    for (const { id } of sceneIds)   { await renderScene(db, id);  scenesRendered++; }
    await renderForesight(db);
    await renderContradictions(db);
    await renderIndex(db);

    console.log(`rendered vault at ${vaultPath()}`);
    console.log(`  profile.md, _foresight.md, _contradictions.md, _index.md`);
    console.log(`  skills/: ${skillsRendered}`);
    console.log(`  scenes/: ${scenesRendered}`);
  } finally {
    db.close();
  }
}
