#!/usr/bin/env bun
import { startServer } from "./mcp/server";

const cmd = process.argv[2];

async function main() {
  switch (cmd) {
    case undefined:
    case "serve":
      await startServer();
      break;
    default:
      console.error(`unknown command: ${cmd}`);
      console.error("usage: pebble-mcp [serve]");
      process.exit(1);
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
