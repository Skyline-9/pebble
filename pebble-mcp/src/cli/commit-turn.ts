// src/cli/commit-turn.ts
import { commitTurn } from "../git/turn-commit";

interface Args {
  turn?: number;
  adds?: number;
  retracts?: number;
  actor?: "claude-code" | "factory-droid" | "cli";
}

function parseArgs(argv: string[]): Args {
  const out: Args = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const v = argv[i + 1];
    if (k === "--turn" && v !== undefined)         { out.turn = Number(v); i++; }
    else if (k === "--adds" && v !== undefined)    { out.adds = Number(v); i++; }
    else if (k === "--retracts" && v !== undefined){ out.retracts = Number(v); i++; }
    else if (k === "--actor" && v !== undefined)   { out.actor = v as Args["actor"]; i++; }
  }
  return out;
}

export function cliCommitTurn(argv: string[]): void {
  const args = parseArgs(argv);
  const result = commitTurn({
    turn: args.turn ?? 0,
    adds: args.adds ?? 0,
    retracts: args.retracts ?? 0,
    actor: args.actor,
  });
  if (!result.committed) {
    console.log("nothing to commit");
    return;
  }
  console.log(`committed turn ${args.turn ?? 0}`);
}
