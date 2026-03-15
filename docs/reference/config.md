# Configuration Reference

Cortex is configured via `cortex.toml` in your project directory. Run `cortex init` to generate a starter config.

## Full Reference

See **[Configuration Guide](../getting-started/configuration.md)** for the complete `cortex.toml` reference with all sections:

- `[server]` -- ports, data directory
- `[auto_linker]` -- background linking settings, entity promotion, configurable rules
- `[briefing]` -- token budget, role-to-kind mapping
- `[briefing.roles]` -- map node kinds to briefing roles
- `[trust]` -- trust scoring parameters
- `[trust.weights]` -- relative importance of trust signals
- `[retention]` -- TTL, max nodes, eviction strategy
- `[security]` -- encryption at rest
- `[ingest.nats]` -- NATS subscription
- `[write_gate]` -- write quality checks configuration
- `[schemas.*]` -- per-kind metadata schemas

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

## Trust Scoring

Compute trust from graph topology. See [Trust Scoring](../concepts/trust-scoring.md) for the full model.

```toml
[trust]
corroboration_saturation = 3
corroboration_min_weight = 0.6
contradiction_weight = 0.3
access_saturation = 20
freshness_halflife = 90

[trust.weights]
corroboration = 0.30
contradiction = 0.25
source = 0.20
access = 0.15
freshness = 0.10
```

## Briefing Roles

Map node kinds to briefing roles. See [Briefings](../concepts/briefings.md) for role descriptions.

```toml
[briefing.roles]
identity    = ["agent"]
persistent  = ["preference"]
trackable   = ["goal"]
temporal    = ["event"]
reviewable  = ["pattern"]
superseding = ["fact", "decision"]
```

## Auto-Linker Rules

Configurable structural rules for the auto-linker. See [Auto-Linker](../concepts/auto-linker.md) for condition types.

```toml
[auto_linker]
entity_promote_every_n_cycles = 60
entity_promote_min_agents = 2

[[auto_linker.rules]]
name = "decision-leads-to-event"
from_kind = "decision"
to_kind = "event"
relation = "led_to"
weight = 0.8
condition = { type = "temporal_proximity", window_minutes = 60 }
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CORTEX_DATA_DIR` | Override `server.data_dir` |
| `CORTEX_GRPC_PORT` | Override `server.grpc_port` |
| `CORTEX_HTTP_PORT` | Override `server.http_port` |
| `CORTEX_ENCRYPTION_KEY` | Base64-encoded 256-bit AES key. Required when `security.encryption = true`. Generate with `cortex security generate-key`. |
