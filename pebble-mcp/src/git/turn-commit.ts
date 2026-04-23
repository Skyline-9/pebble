// src/git/turn-commit.ts
import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { resolvePebbleRoot } from "../paths";

function runGit(args: string, opts: { silent?: boolean } = {}): string {
  const cwd = resolvePebbleRoot();
  try {
    return execSync(`git ${args}`, { cwd, stdio: opts.silent ? ["ignore","pipe","ignore"] : "pipe" }).toString();
  } catch (err: any) {
    if (opts.silent) return "";
    throw err;
  }
}

export function ensureGitRepo(): void {
  const gitDir = join(resolvePebbleRoot(), ".git");
  if (!existsSync(gitDir)) {
    runGit("init --initial-branch=main", { silent: true });
    runGit(`config user.email "pebble@local"`, { silent: true });
    runGit(`config user.name "Pebble"`, { silent: true });
  }
}

export interface TurnCommitArgs {
  turn: number;
  adds: number;
  retracts: number;
  actor?: "claude-code" | "factory-droid" | "cli";
}

export function commitTurn(args: TurnCommitArgs): { committed: boolean } {
  ensureGitRepo();
  runGit("add -A", { silent: true });
  const status = runGit("status --porcelain", { silent: true });
  if (!status.trim()) return { committed: false };
  const subject = `:memo: pebble: turn ${args.turn} +${args.adds} -${args.retracts}`;
  const body = args.actor ? `actor: ${args.actor}` : "";
  const msg = body ? `${subject}\n\n${body}` : subject;
  const esc = msg.replace(/"/g, '\\"');
  runGit(`commit -m "${esc}" --no-verify`, { silent: true });
  return { committed: true };
}
