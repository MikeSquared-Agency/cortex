# CLI Reference

The `cortex` binary provides a complete command-line interface.

## Global Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--config <path>` | `./cortex.toml` | Path to config file |
| `--server <addr>` | `localhost:9090` | gRPC server address (for client commands) |

## Commands

### `cortex init`

Initialise a new Cortex project in the current directory.

```bash
cortex init [--path <dir>]
```

Creates `cortex.toml` with sensible defaults.

### `cortex serve`

Start the Cortex server.

```bash
cortex serve [--config cortex.toml]
```

### `cortex node`

Manage nodes.

```bash
cortex node create --kind <kind> --title <title> [--body <body>] [--importance 0.7] [--tags tag1,tag2]
cortex node get <id>
cortex node list [--kind <kind>] [--limit 50]
cortex node delete <id>
cortex node link --trigger   # Trigger auto-linker
```

### `cortex edge`

Manage edges.

```bash
cortex edge create --from <id> --to <id> --relation <relation> [--weight 0.8]
cortex edge get <id>
cortex edge list --node <id>
```

### `cortex search`

Search nodes by semantic similarity.

```bash
cortex search <query> [--limit 10] [--kind <kind>] [--hybrid] [--alpha 0.7]
```

### `cortex briefing`

Generate a briefing for an agent.

```bash
cortex briefing <agent-id> [--format text|json] [--max-tokens 2000]
```

### `cortex traverse`

Traverse the graph from a starting node.

```bash
cortex traverse <node-id> [--depth 3] [--direction both|outgoing|incoming]
```

### `cortex import`

Import nodes from external sources.

```bash
cortex import nodes <file> --format csv|json
cortex import file <file> [--chunk-size 500]
cortex import dir <directory> [--extensions md,txt]
```

### `cortex export`

Export the graph.

```bash
cortex export [--format json|csv] [--output <file>]
```

### `cortex backup`

Create a backup of the database.

```bash
cortex backup [<destination>]
```

### `cortex audit`

Query the audit log.

```bash
cortex audit [--since 24h] [--node <id>] [--actor <agent>] [--format text|json] [--limit 100]
```

### `cortex security`

Security utilities.

```bash
cortex security generate-key   # Generate an AES-256-GCM encryption key
```

### `cortex stats`

Show server statistics.

```bash
cortex stats
```

### `cortex doctor`

Check server health and configuration.

```bash
cortex doctor
```

### `cortex shell`

Start an interactive REPL.

```bash
cortex shell
```

### `cortex migrate`

Run database migrations.

```bash
cortex migrate
```

### `cortex config`

Show resolved configuration.

```bash
cortex config show
```
