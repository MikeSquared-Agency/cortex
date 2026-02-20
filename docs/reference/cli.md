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

### `cortex prompt`

Prompt versioning, branching, and migration.

#### `cortex prompt list`

List all prompts (HEAD of each slug and branch).

```bash
cortex prompt list [--branch <branch>] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--branch` | _(all)_ | Filter by branch name |
| `--format` | `table` | Output format |

#### `cortex prompt get`

Show a prompt, resolved with inheritance by default.

```bash
cortex prompt get <slug> [--branch <branch>] [--version <N>] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--branch` | `main` | Branch to resolve from |
| `--version` | _(HEAD)_ | Specific version number |
| `--format` | `table` | Output format |

#### `cortex prompt migrate`

Import prompts from a migration JSON file.

```bash
cortex prompt migrate <file> [--dry-run]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--dry-run` | `false` | Preview without writing to the database |

The migration file is a JSON array of prompt objects:

```json
[
  {
    "slug": "my-prompt",
    "type": "system",
    "branch": "main",
    "sections": { "identity": "You are helpful." },
    "metadata": {},
    "tags": ["assistant"]
  }
]
```

#### `cortex prompt performance`

Show aggregate performance metrics for a prompt.

```bash
cortex prompt performance <slug> [--limit 50] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `50` | Maximum observations to include |
| `--format` | `table` | Output format |

### `cortex agent`

Agent ↔ prompt binding and context-aware selection.

#### `cortex agent list`

List all agent nodes.

```bash
cortex agent list [--format table|json]
```

#### `cortex agent show`

Show prompts bound to an agent.

```bash
cortex agent show <name> [--format table|json]
```

#### `cortex agent bind`

Bind a prompt to an agent (or update the weight of an existing binding).

```bash
cortex agent bind <name> <slug> [--weight 1.0] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--weight` | `1.0` | Edge weight (0.0–1.0); higher = more important |
| `--format` | `table` | Output format |

#### `cortex agent unbind`

Remove a prompt binding from an agent.

```bash
cortex agent unbind <name> <slug>
```

#### `cortex agent resolve`

Show the fully resolved effective prompt for an agent (all bound prompts merged by weight).

```bash
cortex agent resolve <name> [--format text|json]
```

#### `cortex agent select`

Select the best prompt variant for the current context using epsilon-greedy selection.

```bash
cortex agent select <name> [--sentiment 0.5] [--task-type casual] \
  [--correction-rate 0.0] [--topic-shift 0.0] [--energy 0.5] \
  [--epsilon 0.2] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--sentiment` | `0.5` | User sentiment (0.0 frustrated – 1.0 pleased) |
| `--task-type` | `casual` | Task type: coding, planning, casual, crisis, reflection |
| `--correction-rate` | `0.0` | Rolling correction rate (0.0–1.0) |
| `--topic-shift` | `0.0` | Semantic distance from conversation start (0.0–1.0) |
| `--energy` | `0.5` | User energy proxy (0.0–1.0) |
| `--epsilon` | `0.2` | Exploration rate (0.0 = always exploit, 1.0 = always random) |
| `--format` | `table` | Output format |

#### `cortex agent history`

Show variant swap and performance observation history.

```bash
cortex agent history <name> [--limit 20] [--format table|json]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `20` | Maximum history entries |
| `--format` | `table` | Output format |

#### `cortex agent observe`

Record a performance observation and update edge weights via EMA.

```bash
cortex agent observe <name> \
  --variant-id <UUID> --variant-slug <SLUG> \
  [--sentiment-score 0.5] [--correction-count 0] \
  [--task-outcome unknown] [--token-cost <N>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--variant-id` | _(required)_ | UUID of the active prompt variant node |
| `--variant-slug` | _(required)_ | Slug of the prompt variant |
| `--sentiment-score` | `0.5` | Observed sentiment (0.0–1.0) |
| `--correction-count` | `0` | Number of user corrections |
| `--task-outcome` | `unknown` | Outcome: success, partial, failure, unknown |
| `--token-cost` | _(optional)_ | Token cost of the interaction |

The observation score is computed as:

```
score = 0.5 × sentiment + 0.3 × (1 - correction_penalty) + 0.2 × task_success
```

The edge weight is then updated using exponential moving average (α = 0.1):

```
new_weight = 0.9 × old_weight + 0.1 × observation_score
```

### `cortex mcp`

Start an MCP (Model Context Protocol) server for AI agent integration via stdio transport.

```bash
cortex mcp [--data-dir <path>] [--server <addr>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--data-dir` | _(auto)_ | Path to cortex data directory |
| `--server` | _(none)_ | Connect to a running Cortex server via gRPC instead of opening DB directly |
