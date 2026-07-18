#!/usr/bin/env bun
import { startServer } from "./mcp/server";
import { cliInit } from "./cli/init";
import { cliVerify } from "./cli/verify";
import { cliStatus } from "./cli/status";
import { cliHotCache } from "./cli/hot-cache";
import { cliSeedTestFixture } from "./cli/seed-test-fixture";
import { cliCommitTurn } from "./cli/commit-turn";
import { cliReviewTurn } from "./cli/review-turn";
import { cliRenderVault } from "./cli/render-vault";

const cmd = process.argv[2] ?? "serve";
const rest = process.argv.slice(3);

async function main() {
  switch (cmd) {
    case "serve":               await startServer(); break;
    case "init":                cliInit(); break;
    case "verify":              await cliVerify(); break;
    case "status":              cliStatus(); break;
    case "hot-cache-for-cc":     await cliHotCache({ target: "cc" }); break;
    case "hot-cache-for-droid":  await cliHotCache({ target: "droid" }); break;
    case "hot-cache-for-gemini": await cliHotCache({ target: "gemini" }); break;
    case "seed-test-fixture":   await cliSeedTestFixture(); break;
    case "seed-benchmark": {
      const flag = rest[0];
      const count = Number(rest[1]);
      if (flag !== "--cells" || rest.length !== 2) {
        throw new Error("usage: pebble-mcp seed-benchmark --cells <count>");
      }
      const { seedBenchmark } = await import("./cli/seed-benchmark");
      await seedBenchmark(count);
      break;
    }
    case "commit-turn":         cliCommitTurn(rest); break;
    case "review-turn":         await cliReviewTurn(rest); break;
    case "render-vault":        await cliRenderVault(); break;
    default:
      console.error(`unknown command: ${cmd}`);
      console.error("usage: pebble-mcp [serve|init|verify|status|hot-cache-for-cc|hot-cache-for-droid|hot-cache-for-gemini|seed-test-fixture|seed-benchmark|commit-turn|review-turn|render-vault]");
      process.exit(1);
  }
}

main().catch(err => { console.error(err); process.exit(1); });
