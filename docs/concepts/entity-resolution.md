# Entity Resolution

Entities are the bridge between agents. When Agent A knows about "Anthropic" and Agent B knows about "Anthropic PBC", Cortex recognizes they are talking about the same thing and connects their knowledge through a shared entity node. This is how siloed agents discover what each other knows -- without direct communication.

## Entity Nodes

Entity nodes are regular nodes with `kind: "entity"`. They are not a separate storage primitive -- the same node, edge, and query infrastructure applies. Two well-known metadata keys define the convention:

- **`entity_type`** (string): Classifies the entity. Well-known values: `agent`, `company`, `person`, `technology`, `project`, `location`, `product`. Custom types are allowed.
- **`aliases`** (array of strings): Alternative names for the entity, used during entity matching.

See [Metadata Conventions](../reference/metadata-conventions.md) for the full list.

## How Entities Are Created

Entities enter the graph through two paths.

**Explicit creation.** You create an entity node directly via the SDK, CLI, or API with `kind: "entity"`. This is the preferred path when you know upfront which entities matter.

**Auto-promotion.** When two or more agents reference the same normalized entity string, the auto-linker promotes it to a first-class entity node. Entity references are extracted from two sources:

1. **Tags** prefixed with `entity-` (e.g., `entity-anthropic`)
2. **`metadata.entities`** -- a JSON array of entity name strings

Normalization lowercases the string, strips leading articles ("the", "a", "an"), and joins words with hyphens. "The Company X" and "company-x" resolve to the same entity.

Promotion is configurable:

- `entity_promote_every_n_cycles` (default: 60) -- how often the promotion scan runs, in auto-linker cycles.
- `entity_promote_min_agents` (default: 2) -- minimum distinct agents referencing an entity before promotion.

When an entity is promoted, the auto-linker creates `references` edges from every mentioning node to the new entity node.

## Relations

Three relation types connect entities to the rest of the graph:

| Relation | Direction | Meaning |
|----------|-----------|---------|
| `references` | knowledge node -> entity | This node mentions or is about this entity |
| `shared_entity` | node -> node | Two nodes from different agents reference the same entity |
| `authored_by` | entity (agent) -> knowledge node | The agent entity that created this knowledge |

The `shared_entity` relation is created by the auto-linker's entity co-occurrence rule. It only fires across different agents -- same-agent co-occurrences are not flagged as cross-agent discoveries.

## Cross-Agent Discovery (Two-Hop Traversal)

The real power of entity nodes is two-hop traversal. Starting from any node, you can follow `references` edges to reach entity nodes, then follow other agents' `references` edges back to their knowledge. The path looks like:

```
Agent A's node --[references]--> Entity <--[references]-- Agent B's node
```

This is how briefings with `scope=shared` work. The briefing engine follows reference edges to entities, then follows other agents' reference edges back to discover cross-agent knowledge. Entities are the hub nodes that connect siloed information -- like a Palantir graph where everything links through the entities.

## Agent Nodes

The deprecated `kind: "agent"` migrates to `kind: "entity"` with `entity_type: "agent"`. Agent identity nodes are just entity nodes. The migration function (`migrate_agent_to_entity`) converts the kind and adds an `entity-type-agent` tag. Existing `kind: "agent"` nodes continue to work but will be migrated automatically during auto-linker cycles.

## Configuration

```toml
[auto_linker]
entity_promote_every_n_cycles = 60   # How often to scan for promotable entities
entity_promote_min_agents = 2        # Min agents before auto-promotion triggers
```

Lower `entity_promote_min_agents` to 1 if you want single-agent entities (useful for solo-agent setups). Increase `entity_promote_every_n_cycles` if promotion scans are too expensive on large graphs.

## CLI

```bash
# Create an entity node
cortex node create --kind entity --title "Anthropic" \
    --metadata '{"entity_type": "company", "aliases": ["anthropic-ai", "Anthropic PBC"]}'

# List all entity nodes
cortex node list --kind entity

# Find nodes referencing a specific entity
cortex search "Anthropic" --kind entity
```
