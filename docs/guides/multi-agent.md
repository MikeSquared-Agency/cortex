# Multi-Agent Memory

Cortex supports shared memory across multiple agents in a team.

## Architecture

All agents connect to the same Cortex server. Each node records its `source_agent` — the agent that created it. Briefings are scoped per agent but the underlying graph is shared.

```
Agent A ──writes──► Cortex ◄──reads── Agent B
Agent B ──writes──► Cortex ◄──reads── Agent A
           ↑
      Auto-linker connects
      A's knowledge to B's
```

## Patterns

### Specialised Roles

Each agent stores knowledge in its domain. The auto-linker connects related findings across agents automatically.

```python
# Researcher agent stores findings
cx.store("fact", "FastAPI is 3x faster than Flask for async workloads",
         source_agent="researcher", importance=0.8)

# Architect agent reads team knowledge in its briefing
briefing = cx.briefing("architect")
# → Includes findings from researcher if related to architecture topics
```

### Shared Goals

Store team-level goals as high-importance `goal` nodes:

```python
cx.store("goal", "Deliver MVP by Q2",
         source_agent="team", importance=1.0,
         tags=["team", "milestone"])
```

All agents see shared goals in their briefings.

### Handoffs

When an agent hands off a task, it can store the handoff as an `event` node with a `depends_on` edge to the task:

```python
node_id = cx.store("event", "Research phase complete — handoff to writer",
                   source_agent="researcher", importance=0.7)
cx.edge(from_id=node_id, to_id=task_node_id, relation="depends_on")
```

## Prompt Binding and Variant Selection

Each agent can have multiple prompt variants bound to it. Cortex selects the best variant based on context signals and historical performance.

### Binding Prompts to Agents

```bash
# Bind different prompts with different weights
cortex agent bind researcher coding-focused --weight 0.9
cortex agent bind researcher general-assistant --weight 0.5

# Each agent gets the prompt that fits its role
cortex agent bind writer creative-writing --weight 1.0
cortex agent bind writer general-assistant --weight 0.3
```

Or via the HTTP API:

```bash
curl -X PUT http://localhost:9091/agents/researcher/prompts/coding-focused \
  -H "Content-Type: application/json" \
  -d '{ "weight": 0.9 }'
```

### Context-Aware Selection

When an agent needs a prompt, Cortex scores all bound variants against the current context:

```bash
cortex agent select researcher \
  --task-type coding \
  --sentiment 0.3 \
  --correction-rate 0.4 \
  --epsilon 0.1
```

The selection uses epsilon-greedy: most of the time it picks the highest-scoring variant, but occasionally explores alternatives to discover better options.

### Performance Tracking Across a Team

Each agent records observations independently. Compare performance across agents:

```bash
# How is the coding prompt performing for the researcher?
cortex prompt performance coding-focused

# What variants has the writer been using?
cortex agent history writer --limit 10
```

Weights update automatically via EMA — well-performing prompts rise, underperforming ones fade. No manual tuning needed.

## Entity Hub Traversal

Entities are the bridge between agents. When multiple agents store knowledge about the same entity (person, company, technology), Cortex connects them via entity nodes.

```
Agent A's fact ──[references]──► "Anthropic" entity ◄──[references]── Agent B's decision
```

This two-hop pattern is how cross-agent discovery works: follow `references` edges from your knowledge to entities, then follow other agents' `references` edges back to their knowledge.

```python
# Store knowledge that references an entity
cx.store("fact", "Anthropic released Claude 4",
         source_agent="researcher", tags=["entity-anthropic"])

# The auto-linker will connect this to other agents' knowledge about Anthropic
```

See [Entity Resolution](../concepts/entity-resolution.md) for the full model.

## Briefing Scope

Multi-agent setups unlock two additional briefing scopes:

### Shared Scope

Includes the requesting agent's knowledge plus cross-agent context about shared entities. Use when an agent needs awareness of what other agents know about the same topics.

```bash
cortex briefing researcher --scope shared
```

### Unified Scope

Multi-agent briefing for orchestrators. Spans multiple agents and synthesises a combined view.

```bash
cortex briefing --agents researcher,architect,writer
```

## Philosophy: Agents Are Visitors, the Graph Is Persistent

Agents come and go. They crash, restart, get replaced. The graph persists. Design your multi-agent system with this in mind:

- Store knowledge in the graph, not in agent state
- Use briefings to bootstrap agent context on startup
- Let the auto-linker discover cross-agent connections -- don't wire them manually
- Use entities as the coordination mechanism between agents

## Configuration

No special configuration is needed for multi-agent setups. Run a single Cortex server and point all agents at it.

For large teams (10+ agents), increase `max_nodes` in `[retention]` and tune `interval_seconds` in `[auto_linker]` for your throughput.
