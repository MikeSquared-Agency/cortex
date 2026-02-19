#!/usr/bin/env node
/**
 * Cortex MCP Bridge — lightweight MCP server that proxies to Cortex REST API.
 * No Rust binary needed. Just Node.js.
 *
 * Usage in Claude Desktop config:
 * {
 *   "mcpServers": {
 *     "cortex": {
 *       "command": "node",
 *       "args": ["/path/to/cortex-mcp-bridge.js"],
 *       "env": { "CORTEX_URL": "https://cortex.darlington.dev" }
 *     }
 *   }
 * }
 */

const BASE = process.env.CORTEX_URL || "https://cortex.darlington.dev";
const readline = require("readline");

const TOOLS = [
  {
    name: "cortex_store",
    description: "Store knowledge in persistent graph memory. Use to remember facts, decisions, goals, events, patterns, observations.",
    inputSchema: {
      type: "object",
      properties: {
        kind: { type: "string", description: "fact|decision|goal|event|pattern|observation", default: "fact" },
        title: { type: "string", description: "Short summary" },
        body: { type: "string", description: "Full content" },
        tags: { type: "array", items: { type: "string" } },
        importance: { type: "number", description: "0.0-1.0", default: 0.5 },
      },
      required: ["title"],
    },
  },
  {
    name: "cortex_search",
    description: "Search graph memory by meaning. Returns nodes ranked by semantic similarity.",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string" },
        limit: { type: "integer", default: 10 },
      },
      required: ["query"],
    },
  },
  {
    name: "cortex_recall",
    description: "Hybrid search combining semantic similarity and graph structure. More contextual than pure search.",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string" },
        limit: { type: "integer", default: 10 },
      },
      required: ["query"],
    },
  },
  {
    name: "cortex_briefing",
    description: "Generate a context briefing — structured summary of goals, decisions, patterns, and recent context from graph memory.",
    inputSchema: {
      type: "object",
      properties: {
        agent_id: { type: "string", default: "default" },
        compact: { type: "boolean", default: false },
      },
    },
  },
  {
    name: "cortex_traverse",
    description: "Explore connections from a node in the knowledge graph.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        depth: { type: "integer", default: 2 },
        direction: { type: "string", default: "both" },
      },
      required: ["node_id"],
    },
  },
  {
    name: "cortex_relate",
    description: "Create a typed relationship between two nodes.",
    inputSchema: {
      type: "object",
      properties: {
        from_id: { type: "string" },
        to_id: { type: "string" },
        relation: { type: "string", default: "relates-to" },
      },
      required: ["from_id", "to_id"],
    },
  },
];

async function http(method, path, body) {
  const url = `${BASE}${path}`;
  const opts = { method, headers: { "Content-Type": "application/json" } };
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(url, opts);
  return res.json();
}

async function handleTool(name, args) {
  switch (name) {
    case "cortex_store": {
      const r = await http("POST", "/nodes", {
        kind: args.kind || "fact",
        title: args.title,
        body: args.body || args.title,
        tags: args.tags,
        importance: args.importance,
        source_agent: "mcp",
      });
      return `Stored: ${r.data?.title || args.title} (id: ${r.data?.id || "?"})`;
    }
    case "cortex_search": {
      const r = await http("GET", `/search?q=${encodeURIComponent(args.query)}&limit=${args.limit || 10}`);
      const items = (r.data || []).map((i) => `[${i.score?.toFixed(2)}] ${i.node?.title}: ${i.node?.body}`);
      return items.length ? items.join("\n") : "No results found.";
    }
    case "cortex_recall": {
      const r = await http("GET", `/search/hybrid?q=${encodeURIComponent(args.query)}&limit=${args.limit || 10}`);
      const items = (r.data || []).map((i) => `[${i.score?.toFixed(2)}] ${i.title}: ${i.body || ""}`);
      return items.length ? items.join("\n") : "No results found.";
    }
    case "cortex_briefing": {
      const aid = args.agent_id || "default";
      const compact = args.compact ? "true" : "false";
      const r = await http("GET", `/briefing/${encodeURIComponent(aid)}?compact=${compact}`);
      return r.data?.rendered || "No briefing available.";
    }
    case "cortex_traverse": {
      const r = await http("GET", `/nodes/${args.node_id}/neighbors?depth=${args.depth || 2}&direction=${args.direction || "both"}`);
      return JSON.stringify(r.data, null, 2);
    }
    case "cortex_relate": {
      const r = await http("POST", "/edges", {
        from_id: args.from_id,
        to_id: args.to_id,
        relation: args.relation || "relates-to",
      });
      return `Related: ${args.from_id} → [${args.relation || "relates-to"}] → ${args.to_id} (edge: ${r.data?.id || "?"})`;
    }
    default:
      throw new Error(`Unknown tool: ${name}`);
  }
}

function respond(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\n");
}

function respondError(id, code, message) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, error: { code, message } }) + "\n");
}

const rl = readline.createInterface({ input: process.stdin });

rl.on("line", async (line) => {
  let req;
  try { req = JSON.parse(line.trim()); } catch { return; }

  const { id, method, params } = req;

  try {
    switch (method) {
      case "initialize":
        respond(id, {
          protocolVersion: "2024-11-05",
          capabilities: { tools: {} },
          serverInfo: { name: "cortex", version: "0.1.0" },
        });
        break;

      case "notifications/initialized":
        break; // no response needed

      case "tools/list":
        respond(id, { tools: TOOLS });
        break;

      case "tools/call": {
        const text = await handleTool(params.name, params.arguments || {});
        respond(id, { content: [{ type: "text", text }] });
        break;
      }

      case "resources/list":
        respond(id, {
          resources: [
            { uri: "cortex://stats", name: "Graph Statistics", mimeType: "application/json" },
          ],
        });
        break;

      case "resources/read": {
        if (params.uri === "cortex://stats") {
          const r = await http("GET", "/stats");
          respond(id, { contents: [{ uri: params.uri, mimeType: "application/json", text: JSON.stringify(r.data, null, 2) }] });
        } else if (params.uri?.startsWith("cortex://node/")) {
          const nid = params.uri.replace("cortex://node/", "");
          const r = await http("GET", `/nodes/${nid}`);
          respond(id, { contents: [{ uri: params.uri, mimeType: "application/json", text: JSON.stringify(r.data, null, 2) }] });
        } else {
          respondError(id, -32000, `Unknown resource: ${params.uri}`);
        }
        break;
      }

      case "ping":
        respond(id, {});
        break;

      default:
        respondError(id, -32601, `Unknown method: ${method}`);
    }
  } catch (e) {
    respondError(id, -32000, e.message);
  }
});

process.stderr.write(`[cortex-mcp] Bridge ready → ${BASE}\n`);
