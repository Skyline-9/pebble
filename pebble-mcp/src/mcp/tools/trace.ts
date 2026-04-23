// src/mcp/tools/trace.ts
import { readTraces } from "../../retrieval/trace";

export function registerTraceTools() {
  async function trace_read(args: { limit?: number }): Promise<{ traces: any[] }> {
    const limit = args.limit ?? 50;
    const traces: any[] = [];
    for await (const t of readTraces()) {
      traces.push(t);
      if (traces.length >= limit) break;
    }
    return { traces: traces.slice(-limit) };
  }
  return { trace_read };
}
