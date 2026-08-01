# cortex-mcp-bridge

Node.js MCP bridge for the [Cortex](https://github.com/MikeSquared-Agency/cortex)
self-organizing graph-memory engine.

The bridge runs locally over stdio and proxies its tools to a running Cortex
HTTP server. It does not bundle the Cortex engine: run Cortex locally, or set
`CORTEX_URL` to a remote deployment.

## Requirements

- Node.js 18 or newer
- A Cortex HTTP server, available at `http://localhost:9091` by default

## Run

Start Cortex:

```bash
cortex serve
```

Then start the MCP bridge:

```bash
npx cortex-mcp-bridge
```

To use a remote server or bearer-token authentication:

```bash
CORTEX_URL=https://cortex.example.com \
CORTEX_AUTH_TOKEN=your-token \
npx cortex-mcp-bridge
```

## MCP client configuration

```json
{
  "mcpServers": {
    "cortex": {
      "command": "npx",
      "args": ["-y", "cortex-mcp-bridge"],
      "env": {
        "CORTEX_URL": "http://localhost:9091"
      }
    }
  }
}
```

`CORTEX_AUTH_TOKEN` is optional. When set, the bridge sends it to Cortex as a
bearer token.

## Tools

The bridge exposes `cortex_store`, `cortex_search`, `cortex_recall`,
`cortex_briefing`, `cortex_traverse`, `cortex_relate`, and `cortex_observe`.

## License

MIT
