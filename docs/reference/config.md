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
- `[write_gate]` — write quality checks configuration
- `[schemas.*]` — per-kind metadata schemas

## Schema Validation

Define per-kind metadata schemas in `cortex.toml`. When a schema is defined for a kind, all nodes of that kind have their `metadata` fields validated at write time. Kinds without schemas pass freely.

```toml
[schemas.decision]
required_fields = ["rationale"]

[schemas.decision.fields.rationale]
type = "string"

[schemas.decision.fields.priority]
type = "number"
min = 1.0
max = 5.0

[schemas.decision.fields.status]
type = "string"
allowed_values = ["proposed", "accepted", "rejected"]
```

### Field Types

- `string` — JSON string value
- `number` — JSON number value (supports `min` and `max` constraints)
- `boolean` — JSON boolean value
- `array` — JSON array value

### Constraints

| Constraint | Applies to | Description |
|-----------|-----------|-------------|
| `required_fields` | Kind | Fields that must be present in metadata |
| `type` | Field | Expected JSON type |
| `min` | Number | Minimum allowed value |
| `max` | Number | Maximum allowed value |
| `allowed_values` | String | Enum-like constraint on string values |

Schema violations produce a 422 response with `gate.check == "schema"` and details about each violated constraint.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CORTEX_DATA_DIR` | Override `server.data_dir` |
| `CORTEX_GRPC_PORT` | Override `server.grpc_port` |
| `CORTEX_HTTP_PORT` | Override `server.http_port` |
| `CORTEX_ENCRYPTION_KEY` | Base64-encoded 256-bit AES key. Required when `security.encryption = true`. Generate with `cortex security generate-key`. |
