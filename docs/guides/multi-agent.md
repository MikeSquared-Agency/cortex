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

## Configuration

No special configuration is needed for multi-agent setups. Run a single Cortex server and point all agents at it.

For large teams (10+ agents), increase `max_nodes` in `[retention]` and tune `interval_seconds` in `[auto_linker]` for your throughput.
