# Prompt Management Guide

This guide walks through the practical workflow for managing prompts in Cortex: creating, versioning, branching, binding to agents, and monitoring performance.

## Creating Your First Prompt

### Via Migration File

The fastest way to bootstrap prompts is a migration file:

```json
[
  {
    "slug": "helpful-assistant",
    "type": "system",
    "branch": "main",
    "sections": {
      "identity": "You are a helpful, concise assistant.",
      "constraints": "Always cite sources. Never fabricate information.",
      "tools": "You have access to web search and code execution."
    },
    "metadata": {},
    "tags": ["assistant", "general"]
  },
  {
    "slug": "coding-assistant",
    "type": "system",
    "branch": "main",
    "sections": {
      "identity": "You are an expert software engineer.",
      "constraints": "Write clean, tested code. Explain your reasoning.",
      "tools": "You have access to code execution and file operations."
    },
    "metadata": {
      "context_weights": {
        "task_coding": 0.9,
        "user_frustrated": 0.6,
        "energy_high": 0.3
      }
    },
    "tags": ["assistant", "coding"]
  }
]
```

```bash
# Preview what will be created
cortex prompt migrate prompts.json --dry-run

# Import
cortex prompt migrate prompts.json
```

### Via HTTP API

```bash
curl -X POST http://localhost:9091/prompts \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "helpful-assistant",
    "type": "system",
    "sections": {
      "identity": "You are a helpful assistant."
    }
  }'
```

## Versioning

Create a new version when you want to update a prompt:

```bash
curl -X POST http://localhost:9091/prompts/helpful-assistant/versions \
  -H "Content-Type: application/json" \
  -d '{
    "sections": {
      "identity": "You are a helpful and thorough assistant.",
      "constraints": "Be accurate. Ask clarifying questions when needed."
    },
    "author": "admin"
  }'
```

View the version history:

```bash
cortex prompt get helpful-assistant --version 1
cortex prompt get helpful-assistant              # always shows HEAD
```

## Branching

Experiment without affecting the main prompt:

```bash
# Fork to a new branch
curl -X POST http://localhost:9091/prompts/helpful-assistant/branch \
  -H "Content-Type: application/json" \
  -d '{
    "new_branch": "experiment-concise",
    "from_branch": "main"
  }'

# Create versions on the branch
curl -X POST http://localhost:9091/prompts/helpful-assistant/versions \
  -H "Content-Type: application/json" \
  -d '{
    "branch": "experiment-concise",
    "sections": {
      "identity": "You are extremely concise.",
      "constraints": "Maximum 2 sentences per response."
    }
  }'

# List all branches
cortex prompt list
```

## Binding Prompts to Agents

```bash
# Create agent nodes
cortex node create --kind agent --title "kai"
cortex node create --kind agent --title "research-bot"

# Bind prompts with weights
cortex agent bind kai helpful-assistant --weight 1.0
cortex agent bind kai coding-assistant --weight 0.8

# Check bindings
cortex agent show kai

# Get the merged effective prompt
cortex agent resolve kai
```

## Context-Aware Selection

If your prompts have `context_weights` in metadata, selection adapts to the conversation:

```bash
# User is frustrated and working on code
cortex agent select kai --sentiment 0.2 --task-type coding --epsilon 0.1

# User is relaxed, casual chat
cortex agent select kai --sentiment 0.9 --task-type casual

# High exploration mode (try different variants)
cortex agent select kai --epsilon 0.5
```

## Recording Observations

After each interaction, tell Cortex how it went:

```bash
cortex agent observe kai \
  --variant-id 019abc... \
  --variant-slug coding-assistant \
  --sentiment-score 0.9 \
  --correction-count 0 \
  --task-outcome success
```

Or via the HTTP API:

```bash
curl -X POST http://localhost:9091/agents/kai/observe \
  -H "Content-Type: application/json" \
  -d '{
    "variant_id": "019abc...",
    "variant_slug": "coding-assistant",
    "sentiment_score": 0.9,
    "correction_count": 0,
    "task_outcome": "success",
    "token_cost": 1200
  }'
```

This:
1. Creates an observation node in the graph
2. Updates the agent→prompt edge weight via EMA (α=0.1)
3. Records a swap observation if the active variant changed

## Monitoring Performance

```bash
# Aggregate stats for a prompt
cortex prompt performance coding-assistant

# Recent history for an agent
cortex agent history kai --limit 20

# Via HTTP
curl "http://localhost:9091/prompts/coding-assistant/performance?limit=50"
```

## Inheritance

Create specialised prompts that inherit from a base:

```bash
# Create a base prompt
curl -X POST http://localhost:9091/prompts \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "base-agent",
    "type": "system",
    "sections": {
      "identity": "You are a professional AI assistant.",
      "safety": "Never provide harmful information.",
      "format": "Use markdown formatting."
    }
  }'

# Create a child that inherits and overrides
# (Link via inherits_from edge in the graph)
cortex edge create \
  --from <child-node-id> \
  --to <parent-node-id> \
  --relation inherits_from
```

When resolved, the child gets all parent sections, with its own `override_sections` taking precedence.

## Best Practices

1. **Start with migration files** — Version-control your prompts alongside your code
2. **Use branches for experiments** — Don't modify main until you've validated
3. **Set context_weights** — Let Cortex adapt to the conversation automatically
4. **Record every interaction** — More observations = better selection
5. **Monitor performance** — Check `cortex prompt performance` regularly
6. **Use inheritance** — Share safety/formatting rules via a base prompt
7. **Keep ε > 0** — Always explore a little to discover better variants
