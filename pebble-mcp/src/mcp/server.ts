// src/mcp/server.ts
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { Database } from "bun:sqlite";
import { mkdirSync, existsSync } from "node:fs";
import { dbPath, resolvePebbleRoot } from "../paths";
import { initSchema } from "../projection/schema";
import { projectAll } from "../projection/projector";
import { readEvents } from "../log/reader";
import { registerMemoryTools } from "./tools/memory";
import { registerProfileTools } from "./tools/profile";
import { registerSkillTools } from "./tools/skill";
import { registerTraceTools } from "./tools/trace";

const TOOL_SCHEMAS = [
  { name: "memory_assert", description: "Assert a new MemCell into memory.", inputSchema: { type: "object", properties: {
    type: { type: "string", enum: ["profile","preference","project","episodic","skill","transient"] },
    episode: { type: "string" },
    facts: { type: "array", items: { type: "object" } },
    confidence: { type: "number" },
    actor: { type: "string", enum: ["user","reviewer","system"] },
  }, required: ["type","episode","facts","confidence"] } },
  { name: "memory_query", description: "Query memory with BM25 + recency + confidence hybrid scoring.", inputSchema: { type: "object", properties: {
    query: { type: "string" }, top_k: { type: "number" }, turn: { type: "number" },
  }, required: ["query"] } },
  { name: "memory_touch", description: "Record a retrieval hit against a cell.", inputSchema: { type: "object", properties: {
    cell_id: { type: "string" }, query: { type: "string" },
  }, required: ["cell_id"] } },
  { name: "memory_retract", description: "Retract a cell (append-only, still in log).", inputSchema: { type: "object", properties: {
    cell_id: { type: "string" }, reason: { type: "string" },
  }, required: ["cell_id","reason"] } },
  { name: "memory_read_cell", description: "Read a single MemCell by id.", inputSchema: { type: "object", properties: {
    cell_id: { type: "string" },
  }, required: ["cell_id"] } },
  { name: "profile_read", description: "Read the user profile (derived).", inputSchema: { type: "object", properties: {} } },
  { name: "profile_update", description: "Update the user profile with new facts.", inputSchema: { type: "object", properties: {
    facts: { type: "array" },
  }, required: ["facts"] } },
  { name: "skill_save", description: "Save a new SKILL.md-compatible skill.", inputSchema: { type: "object", properties: {
    name: { type: "string" }, description: { type: "string" }, body: { type: "string" },
    trigger_phrases: { type: "array", items: { type: "string" } }, confidence: { type: "number" },
  }, required: ["name","description","body","trigger_phrases","confidence"] } },
  { name: "skill_list", description: "List available skills.", inputSchema: { type: "object", properties: {} } },
  { name: "skill_read", description: "Read a skill by name.", inputSchema: { type: "object", properties: {
    name: { type: "string" },
  }, required: ["name"] } },
  { name: "trace_read", description: "Read retrieval traces for observability.", inputSchema: { type: "object", properties: {
    limit: { type: "number" },
  } } },
];

export async function startServer(): Promise<void> {
  const root = resolvePebbleRoot();
  if (!existsSync(root)) mkdirSync(root, { recursive: true });
  const db = new Database(dbPath());
  initSchema(db);

  const events: any[] = [];
  for await (const ev of readEvents()) events.push(ev);
  projectAll(db, events);

  const mem = registerMemoryTools({ db });
  const prof = registerProfileTools({ db });
  const skl = registerSkillTools({ db });
  const trc = registerTraceTools();
  const toolMap: Record<string, (args: any) => Promise<any>> = {
    memory_assert: mem.memory_assert,
    memory_query: mem.memory_query,
    memory_touch: mem.memory_touch,
    memory_retract: mem.memory_retract,
    memory_read_cell: mem.memory_read_cell,
    profile_read: prof.profile_read,
    profile_update: prof.profile_update,
    skill_save: skl.skill_save,
    skill_list: skl.skill_list,
    skill_read: skl.skill_read,
    trace_read: trc.trace_read,
  };

  const server = new Server(
    { name: "pebble-mcp", version: "0.0.1" },
    { capabilities: { tools: {} } }
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOL_SCHEMAS }));
  server.setRequestHandler(CallToolRequestSchema, async (req) => {
    const fn = toolMap[req.params.name];
    if (!fn) throw new Error(`unknown tool: ${req.params.name}`);
    const result = await fn(req.params.arguments ?? {});
    return { content: [{ type: "text", text: JSON.stringify(result) }] };
  });

  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("pebble-mcp listening on stdio");
}
