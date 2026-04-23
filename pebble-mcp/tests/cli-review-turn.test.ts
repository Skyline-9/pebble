import { describe, expect, test, beforeEach, afterEach } from "bun:test";
import { execSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Database } from "bun:sqlite";

let tmp: string;
const BIN = `bun run ${join(import.meta.dir, "..", "src", "index.ts")}`;

beforeEach(() => { tmp = mkdtempSync(join(tmpdir(), "pebble-rt-")); process.env.PEBBLE_ROOT = tmp; });
afterEach(() => { delete process.env.PEBBLE_ROOT; rmSync(tmp, { recursive: true, force: true }); });

describe("review-turn CLI", () => {
  test("extracts candidates from JSONL transcript and asserts them", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const transcriptPath = join(tmp, "transcript.jsonl");
    const lines = [
      JSON.stringify({ role: "user", content: "I prefer TypeScript for backend work." }),
      JSON.stringify({ role: "assistant", content: "Got it." }),
      JSON.stringify({ role: "user", content: "My primary language is Haskell actually." }),
    ];
    writeFileSync(transcriptPath, lines.join("\n") + "\n");
    const out = execSync(`${BIN} review-turn --transcript ${transcriptPath}`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out).toMatch(/asserted:\s*[1-9]/i);

    const db = new Database(join(tmp, "projection.db"), { readonly: true });
    const count = (db.query("SELECT COUNT(*) AS n FROM cells WHERE retracted_at IS NULL").get() as any).n;
    db.close();
    expect(count).toBeGreaterThan(0);
  });

  test("skips empty transcripts gracefully", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const transcriptPath = join(tmp, "empty.jsonl");
    writeFileSync(transcriptPath, "");
    const out = execSync(`${BIN} review-turn --transcript ${transcriptPath}`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out).toMatch(/asserted:\s*0/i);
  });

  test("parses Factory session format (nested message envelope with content array)", () => {
    execSync(`${BIN} init`, { env: { ...process.env, PEBBLE_ROOT: tmp } });
    const transcriptPath = join(tmp, "factory.jsonl");
    const lines = [
      JSON.stringify({ type: "session_start" }),
      JSON.stringify({
        type: "message",
        message: {
          role: "user",
          content: [
            { type: "text", text: "<system-reminder>boilerplate</system-reminder>" },
            { type: "text", text: "I prefer TypeScript for backend work." },
          ],
        },
      }),
      JSON.stringify({
        type: "message",
        message: {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "..." },
            { type: "tool_use", id: "t1", name: "Read", input: {} },
          ],
        },
      }),
      JSON.stringify({
        type: "message",
        message: {
          role: "user",
          content: [{ type: "text", text: "My primary language is Haskell actually." }],
        },
      }),
    ];
    writeFileSync(transcriptPath, lines.join("\n") + "\n");
    const out = execSync(`${BIN} review-turn --transcript ${transcriptPath}`, {
      env: { ...process.env, PEBBLE_ROOT: tmp },
    }).toString();
    expect(out).toMatch(/asserted:\s*[1-9]/i);
    expect(out).toMatch(/seen:\s*[1-9]/i);
  });
});
