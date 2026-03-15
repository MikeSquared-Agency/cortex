# Self-Organizing Memory

Most agent memory systems are passive storage. You put data in, you get data out, and nothing happens in between. Cortex is different. It is an active memory engine that continuously organizes, connects, and curates the knowledge your agents produce.

You store facts. Cortex discovers what your agents know together.

## What Makes Memory Self-Organizing?

Five capabilities work together in the background to turn a pile of nodes into a structured, trustworthy knowledge graph.

### 1. Auto-Linking

The auto-linker is a background process that runs every `interval_seconds` (default: 60 seconds). Each cycle, it processes newly created or modified nodes and discovers relationships through four mechanisms:

- **Embedding similarity** -- cosine similarity over vector embeddings, with a configurable threshold (default: 0.75).
- **Shared entity references** -- two nodes that mention the same entity (via tags or metadata) get a `shared_entity` edge.
- **Temporal proximity** -- nodes created within a configurable time window are candidates for structural linking.
- **Configurable rules** -- declarative rules in `cortex.toml` that match on node kind, relation type, and custom conditions.

When two nodes are sufficiently related, the auto-linker creates a typed, weighted edge between them. You do not wire the graph by hand. You store knowledge. Cortex finds the connections.

See [Auto-Linker](./auto-linker.md) for the full configuration reference.

### 2. Contradiction Detection

When two nodes express conflicting information, the auto-linker creates a `contradicts` edge and flags both nodes for review. Contradictions are not buried or silently overwritten. They surface in agent briefings under the Unresolved section, so agents can reason about conflicts rather than ignoring them.

Resolved contradictions (where one side is superseded) stop penalizing trust scores. Unresolved contradictions continue to reduce trust until an agent or operator addresses them.

### 3. Knowledge Decay

Edge weights start at 1.0 and fade over time unless reinforced by access. The decay function is exponential:

```
weight(t) = weight_0 * e^(-lambda * t)
```

Accessing a node through search, traversal, or briefing generation reinforces its edges -- resets or slows the decay clock. Important knowledge (high `importance` score) decays more slowly thanks to importance shielding. The result: stale associations dissolve naturally, while knowledge that agents actively use persists. This mirrors how human memory works: things you think about stay sharp, things you ignore fade.

See [Decay and Memory](./decay-and-memory.md) for decay rates and retention policies.

### 4. Trust from Topology

Confidence in Cortex is not a stored field. It is computed from the graph structure at query time, drawing on five signals: corroboration across independent agents, contradiction count, source agent track record, access reinforcement, and freshness. This works like PageRank for knowledge -- authority is derived from structure, not from a static label.

New corroboration immediately increases trust. A new contradiction immediately decreases it. No migration, no stale confidence values.

See [Trust Scoring](./trust-scoring.md) for the combination formula and configuration.

### 5. Entity Promotion

When multiple agents reference the same real-world entity (a person, company, technology, or project), the auto-linker promotes it to a first-class entity node with `kind: "entity"`. Entity nodes become hub nodes in the graph, connecting all related knowledge across agents. Two agents that have never communicated can discover each other's knowledge through a shared entity.

Promotion is configurable: `entity_promote_min_agents` (default: 2) controls how many independent agents must reference an entity before it is promoted.

See [Entity Resolution](./entity-resolution.md) for the full entity model.

## The Feedback Loop

These five capabilities form a virtuous cycle. Auto-linking creates structure from unstructured knowledge. Decay removes noise from that structure. Contradiction detection surfaces conflicts that auto-linking discovers. Trust scoring ranks the surviving knowledge by reliability. Entity promotion connects knowledge across agent boundaries, feeding more context back into the next auto-linker cycle.

Each cycle makes the graph more useful than the last. Early cycles produce broad, low-confidence associations. Over time, decay prunes the weak links, corroboration strengthens the strong ones, and entity hubs emerge as the connective tissue between agents.

## What This Means for Your Agent

Consider two agents: a researcher that gathers technical facts and an architect that records design decisions. Each stores knowledge independently. Neither knows the other exists.

The auto-linker discovers that the researcher's fact about a library's performance characteristics is relevant to the architect's decision to adopt that library. It creates a `similar_to` edge. Both agents reference "PostgreSQL" in their tags, so entity promotion creates a shared entity node linking their knowledge. The architect's briefing now includes the researcher's performance data under the cross-agent section. The researcher's briefing flags a contradiction between two performance benchmarks.

No manual wiring. No shared schema negotiation. No message passing between agents. The graph organized itself.

This is the core value proposition of self-organizing memory: agents that share a Cortex instance automatically benefit from each other's knowledge, without any explicit coordination.

## Further Reading

Each self-organizing capability has its own concept page with full configuration details:

- [Auto-Linker](./auto-linker.md) -- background linking process, rules, similarity thresholds
- [Decay and Memory](./decay-and-memory.md) -- exponential decay, reinforcement, retention policies
- [Trust Scoring](./trust-scoring.md) -- five signals, combination formula, API endpoints
- [Entity Resolution](./entity-resolution.md) -- entity nodes, auto-promotion, cross-agent discovery
- [Briefings](./briefings.md) -- how self-organized knowledge is synthesized for agents

## Getting Started

To see self-organizing memory in action:

1. Follow the [Quickstart](../getting-started/quickstart.md) to set up a Cortex instance.
2. Review the `[auto_linker]` and `[trust]` sections in [`cortex.example.toml`](../../cortex.example.toml) for tuning options.
3. Store nodes from two or more agents and watch the auto-linker create structure.

Run `cortex stats` to monitor auto-linker cycles, edges created, and entities promoted.
