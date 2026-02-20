# Quick Start — 5 Minutes to Memory

This guide gets you from zero to a working Cortex instance in 5 minutes.

## 1. Install

### Linux / macOS (recommended)

```bash
curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh
```

The script detects your OS and architecture, downloads the right binary, and places it in `~/.local/bin/` (or `/usr/local/bin/` if run as root). No Rust toolchain, no `protoc`, no system dependencies required.

### Via Cargo

```bash
cargo install cortex-memory
```

### Docker

```bash
docker pull mikesquared/cortex:latest
```

### Manual download

Download a pre-built binary from the [latest release](https://github.com/MikeSquared-Agency/cortex/releases/latest):

| Platform | Architecture | File |
|----------|-------------|------|
| Linux    | x86_64      | `cortex-linux-x86_64.tar.gz` |
| Linux    | arm64       | `cortex-linux-arm64.tar.gz` |
| macOS    | x86_64      | `cortex-macos-x86_64.tar.gz` |
| macOS    | arm64 (M1+) | `cortex-macos-arm64.tar.gz` |

Extract and place the `cortex` binary somewhere on your `$PATH`.

## 2. Initialise

```bash
mkdir my-project && cd my-project
cortex init
```

Answer the prompts — the defaults work fine for getting started.

## 3. Start

```bash
cortex serve
# Cortex is now running at localhost:9090 (gRPC) and localhost:9091 (HTTP)
```

## 4. Store Knowledge

```bash
cortex node create \
  --kind fact \
  --title "The API uses JWT authentication" \
  --importance 0.7
```

## 5. Search

```bash
cortex search "authentication"
# 1. 0.94  fact  The API uses JWT authentication  [id: abc123]
```

## 6. Get a Briefing

```bash
cortex briefing my-agent
```

## 7. Explore the Graph

Open [http://localhost:9091/viz](http://localhost:9091/viz) in your browser to see a live force-directed visualisation of your knowledge graph.

## 8. Create a Prompt

```bash
cortex prompt migrate prompts.json
```

Where `prompts.json` contains:

```json
[
  {
    "slug": "helpful-assistant",
    "type": "system",
    "branch": "main",
    "sections": {
      "identity": "You are a helpful assistant.",
      "constraints": "Be concise and accurate."
    },
    "metadata": {},
    "tags": ["assistant"]
  }
]
```

Or create prompts via the HTTP API:

```bash
curl -X POST http://localhost:9091/prompts \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "helpful-assistant",
    "type": "system",
    "sections": { "identity": "You are a helpful assistant." }
  }'
```

## 9. Bind a Prompt to an Agent

```bash
# Create an agent node
cortex node create --kind agent --title "my-agent"

# Bind the prompt
cortex agent bind my-agent helpful-assistant --weight 1.0

# Verify the binding
cortex agent show my-agent
```

## 10. Select and Observe

```bash
# Ask Cortex which variant to use right now
cortex agent select my-agent --task-type coding --sentiment 0.7

# After the interaction, record how it went
cortex agent observe my-agent \
  --variant-id UUID --variant-slug helpful-assistant \
  --sentiment-score 0.8 --task-outcome success
```

Cortex updates the edge weight using an exponential moving average, so good prompts rise and bad ones fade — automatically.

## Next Steps

- [Build your first agent](./first-agent.md)
- [Python SDK](../reference/python-sdk.md)
- [Configuration reference](./configuration.md)
- [CLI reference](../reference/cli.md)
- [Prompt System concepts](../concepts/prompt-system.md)
- [Prompt Management guide](../guides/prompt-management.md)
