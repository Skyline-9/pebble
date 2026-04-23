#!/usr/bin/env bun
import { startServer } from "./mcp/server";
import { cliInit } from "./cli/init";
import { cliVerify } from "./cli/verify";
import { cliStatus } from "./cli/status";
import { cliHotCache } from "./cli/hot-cache";
import { cliSeedTestFixture } from "./cli/seed-test-fixture";

const cmd = process.argv[2] ?? "serve";

async function main() {
  switch (cmd) {
    case "serve":               await startServer(); break;
    case "init":                cliInit(); break;
    case "verify":              await cliVerify(); break;
    case "status":              cliStatus(); break;
    case "hot-cache-for-cc":    await cliHotCache({ target: "cc" }); break;
    case "hot-cache-for-droid": await cliHotCache({ target: "droid" }); break;
    case "seed-test-fixture":   await cliSeedTestFixture(); break;
    default:
      console.error(`unknown command: ${cmd}`);
      console.error("usage: pebble-mcp [serve|init|verify|status|hot-cache-for-cc|hot-cache-for-droid|seed-test-fixture]");
      process.exit(1);
  }
}

main().catch(err => { console.error(err); process.exit(1); });
