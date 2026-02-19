# Quick Start — 5 Minutes to Memory

This guide gets you from zero to a working Cortex instance in 5 minutes.

## 1. Install

### Linux / macOS (cargo)
```bash
cargo install cortex-memory
```

### Docker
```bash
docker pull mikesquared/cortex:latest
```

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

## Next Steps

- [Build your first agent](./first-agent.md)
- [Python SDK](../reference/python-sdk.md)
- [Configuration reference](./configuration.md)
- [CLI reference](../reference/cli.md)
