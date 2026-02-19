# Configuration Reference

Cortex is configured via `cortex.toml` in your project directory. Run `cortex init` to generate a starter config.

## Full Reference

See **[Configuration Guide](../getting-started/configuration.md)** for the complete `cortex.toml` reference with all sections:

- `[server]` — ports, data directory
- `[auto_linker]` — background linking settings
- `[briefing]` — section ordering, token budget
- `[retention]` — TTL, max nodes, eviction strategy
- `[security]` — encryption at rest
- `[ingest.nats]` — NATS subscription

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CORTEX_DATA_DIR` | Override `server.data_dir` |
| `CORTEX_GRPC_PORT` | Override `server.grpc_port` |
| `CORTEX_HTTP_PORT` | Override `server.http_port` |
| `CORTEX_ENCRYPTION_KEY` | Base64-encoded 256-bit AES key. Required when `security.encryption = true`. Generate with `cortex security generate-key`. |
