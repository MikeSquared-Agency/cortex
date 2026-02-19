# Configuration

Cortex is configured via `cortex.toml` in your project directory. Run `cortex init` to generate a starter config, or create the file manually.

If no `cortex.toml` exists, Cortex uses built-in defaults suitable for development.

## Full Example

```toml
[server]
grpc_port = 9090
http_port = 9091
data_dir = "./data"

[auto_linker]
enabled = true
interval_seconds = 60
similarity_threshold = 0.75
max_edges_per_node = 20

[briefing]
max_tokens = 2000
sections = ["identity", "goals", "patterns", "active_context"]

[retention]
enabled = true
max_age_days = 90
max_nodes = 50000

[ingest.nats]
url = "nats://localhost:4222"
subjects = ["warren.>"]
```

## [server]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `grpc_port` | u16 | `9090` | Port for the gRPC API |
| `http_port` | u16 | `9091` | Port for the HTTP API and graph visualiser |
| `data_dir` | string | `"./data"` | Directory for the redb database file |

## [auto_linker]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether to run the auto-linker background task |
| `interval_seconds` | u64 | `60` | How often the auto-linker runs |
| `similarity_threshold` | f32 | `0.75` | Minimum embedding cosine similarity to create an edge |
| `max_edges_per_node` | usize | `20` | Maximum outgoing similarity edges per node |

## [briefing]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_tokens` | usize | `2000` | Approximate token budget for briefing output |
| `sections` | list | all | Ordered list of sections to include |

Available sections: `identity`, `goals`, `patterns`, `unresolved`, `active_context`.

## [retention]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether to enforce retention limits |
| `max_age_days` | u64 | `90` | Soft-delete nodes older than this many days |
| `max_nodes` | u64 | `50000` | Trim oldest nodes when count exceeds this |

## [ingest.nats]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | — | NATS server URL |
| `subjects` | list | — | NATS subjects to subscribe to |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CORTEX_DATA_DIR` | Override `data_dir` |
| `CORTEX_GRPC_PORT` | Override `grpc_port` |
| `CORTEX_HTTP_PORT` | Override `http_port` |
| `CORTEX_ENCRYPTION_KEY` | AES-256-GCM key (base64) — enables at-rest encryption |
