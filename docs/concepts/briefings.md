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

## Sections

Briefings are composed of ordered sections. Section order matters: graph-based sections (Patterns, Goals, Unresolved) run before Active Context so they can stake claims on the most important nodes before the general-purpose section fills the remaining budget.

### Identity
Agent identity node — who this agent is, its role and capabilities.

### Goals
Active goal nodes associated with the agent. Ordered by importance.

### Patterns
Recurring patterns from the graph — things that have been observed multiple times.

### Unresolved
Knowledge marked as unresolved, contradicted, or requiring follow-up.

### Active Context
Recent nodes involving this agent, ranked by `importance x recency`. This is the most general section and runs last.

## Configuration

```toml
[briefing]
max_tokens = 2000
sections = ["identity", "goals", "patterns", "unresolved", "active_context"]
```

Remove sections you don't need. Reduce `max_tokens` for tighter context budgets.

## Caching

Briefings are cached in memory. The cache is invalidated whenever the `graph_version` counter increments (i.e., any mutation). Pre-warming is available for known agent IDs via `CORTEX_BRIEFING_AGENTS`.

## gRPC

```protobuf
rpc GetBriefing(GetBriefingRequest) returns (BriefingResponse);

message GetBriefingRequest {
  string agent_id = 1;
  uint32 max_tokens = 2;
}
```
