# Briefings

A briefing is a structured context document generated on demand for a specific agent. It answers: "what does this agent need to know right now?"

## Generating a Briefing

```bash
cortex briefing my-agent
```

```python
briefing = cx.briefing("my-agent")
# Inject into LLM system prompt
```

## Roles

Briefing sections are driven by roles, not hardcoded kinds. Each role defines a retrieval strategy. Kinds are mapped to roles in `cortex.toml`:

| Role | Strategy | Default kinds |
|------|----------|---------------|
| identity | Always present, first section | agent |
| persistent | Always present, standing context | preference |
| trackable | Graph traversal from agent node | goal |
| temporal | Time-windowed recent items | event |
| reviewable | Ranked by access + importance | pattern |
| superseding | Newer facts replace older ones | fact, decision |

Plus two automatic sections: **contradictions** (always runs, surfaces unresolved conflicts) and **auto-discovered** (catch-all for kinds not mapped to any role).

### Custom Role Mappings

Override roles for your agent type:

```toml
# Coding agent
[briefing.roles]
identity    = ["agent"]
persistent  = ["constraint", "architecture-decision"]
trackable   = ["task", "milestone"]
temporal    = ["commit", "deployment", "incident"]
reviewable  = ["pattern", "anti-pattern", "code-smell"]
superseding = ["dependency", "api-version"]
```

See the [Coding Agent](../guides/coding-agent.md), [Research Agent](../guides/research-agent.md), and [Browser Agent](../guides/browser-agent.md) guides for complete examples.

## Scope

Briefings support three scope levels:

- **Agent** (default): only the requesting agent's knowledge
- **Shared**: agent's knowledge plus cross-agent context about shared entities
- **Unified**: multi-agent briefing for orchestrators, spans multiple agents

```bash
# Agent scope (default)
cortex briefing kai

# Shared scope -- includes cross-agent entity context
cortex briefing kai --scope shared

# Unified scope -- multi-agent briefing for an orchestrator
cortex briefing --agents kai,scout,lily
```

API: `GET /briefing/kai?scope=shared`

Shared scope works via two-hop entity traversal: agent -> entity -> other agents' knowledge. See [Entity Resolution](./entity-resolution.md) for how this works.

## Trust-Aware Ranking

When trust scoring is enabled, briefing nodes are ranked by a combination of importance (0.6 weight) and trust score (0.4 weight). Nodes with unresolved contradictions are flagged.

Trust/importance weights are configurable in `[briefing]` config. See [Trust Scoring](./trust-scoring.md) for the full model.

## Configuration

```toml
[briefing]
max_tokens = 2000

[briefing.roles]
identity    = ["agent"]
persistent  = ["preference"]
trackable   = ["goal"]
temporal    = ["event"]
reviewable  = ["pattern"]
superseding = ["fact", "decision"]

# Optional section title overrides
[briefing.titles]
persistent = "Standing constraints"
temporal = "Recent activity"
```

Remove roles you don't need. Reduce `max_tokens` for tighter context budgets.

## Caching

Briefings are cached in memory. The cache is invalidated whenever the `graph_version` counter increments (i.e., any mutation). Pre-warming is available for known agent IDs via `CORTEX_BRIEFING_AGENTS`.

## gRPC

```protobuf
rpc GetBriefing(GetBriefingRequest) returns (BriefingResponse);

message GetBriefingRequest {
  string agent_id = 1;
  uint32 max_tokens = 2;
  BriefingScope scope = 3;         // AGENT, SHARED, or UNIFIED
  repeated string agent_ids = 4;   // For UNIFIED scope
}
```
