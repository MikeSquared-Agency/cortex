# Coding Agent Memory

Cortex gives coding agents (Cursor, Copilot, Claude Code, or custom) a persistent memory layer that survives across sessions. The key insight: coding agents need to remember constraints, track architectural decisions, and know when those decisions get superseded by newer ones. Without this, agents rediscover the same context every session or, worse, act on stale decisions.

## Configuration

Add a `[briefing.roles]` section to your `cortex.toml` to tell the briefing engine how to categorize knowledge for a coding agent:

```toml
[briefing.roles]
identity    = ["agent"]
persistent  = ["constraint", "architecture-decision"]
trackable   = ["task", "milestone"]
temporal    = ["commit", "deployment", "incident"]
reviewable  = ["pattern", "anti-pattern", "code-smell"]
superseding = ["dependency", "api-version"]
```

Each role controls how nodes appear in the agent's briefing:

- **identity** -- the agent's own node. Always shown at the top of a briefing.
- **persistent** -- constraints and architecture decisions that the agent must always know. These appear in every briefing, regardless of age.
- **trackable** -- active tasks and milestones. Shown while in progress, removed when completed.
- **temporal** -- time-ordered events like commits and deployments. Shown in reverse chronological order with a rolling window.
- **reviewable** -- patterns and anti-patterns that surface for periodic review. The briefing engine rotates these so the agent re-evaluates them over time.
- **superseding** -- dependencies and API versions where only the latest version matters. When a newer node supersedes an older one, the briefing shows only the current version.

## Storing Architectural Decisions

Store architecture decision records (ADRs) as `architecture-decision` nodes with the rationale in the body and high importance so they persist in briefings:

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

# Store the decision
adr_id = cx.store(
    "architecture-decision",
    "Use PostgreSQL for transactional data",
    body="We evaluated DynamoDB, CockroachDB, and PostgreSQL. "
         "PostgreSQL wins on operational simplicity, tooling maturity, "
         "and the team's existing expertise. DynamoDB was ruled out due "
         "to vendor lock-in constraints.",
    importance=0.9,
    source_agent="coding-agent",
    tags=["database", "adr-001"],
)

# Link it to the constraint that drove the decision
constraint_id = cx.store(
    "constraint",
    "No vendor lock-in for core infrastructure",
    body="All core infrastructure must be portable across cloud providers.",
    importance=0.95,
    source_agent="coding-agent",
    tags=["infrastructure"],
)
```

The auto-linker detects the semantic overlap between the decision body (which mentions vendor lock-in) and the constraint, and creates an edge between them automatically.

## Tracking Patterns

Store observed patterns and anti-patterns as `reviewable` nodes. The briefing engine periodically resurfaces them so the agent can verify they still hold:

```python
cx.store(
    "pattern",
    "Repository pattern for database access",
    body="All database queries go through repository classes. "
         "Direct SQL in service layers is prohibited.",
    importance=0.7,
    source_agent="coding-agent",
    tags=["architecture", "data-access"],
)

cx.store(
    "anti-pattern",
    "God service classes",
    body="Service classes exceeding 500 lines indicate missing abstractions. "
         "Split by bounded context.",
    importance=0.7,
    source_agent="coding-agent",
    tags=["architecture", "maintainability"],
)
```

When the agent later stores an observation like "OrderService is 800 lines and growing", the auto-linker connects it to the god-service anti-pattern via semantic similarity. The next briefing surfaces both the observation and the anti-pattern together.

## Superseding Decisions

When a dependency changes or an architecture decision is revised, store the new version. The auto-linker creates `supersedes` edges between nodes in the `superseding` role that share tags or high semantic similarity:

```python
# Original dependency record
cx.store(
    "dependency",
    "React 18 for frontend",
    body="Using React 18 with concurrent features enabled.",
    importance=0.8,
    source_agent="coding-agent",
    tags=["frontend", "react"],
)

# Later, when upgrading:
cx.store(
    "dependency",
    "React 19 for frontend",
    body="Upgraded to React 19. Server Components now available. "
         "Removed legacy Suspense workarounds.",
    importance=0.8,
    source_agent="coding-agent",
    tags=["frontend", "react"],
)
```

The briefing engine shows only "React 19 for frontend" in the superseding section. The older node is still in the graph (and reachable via search or traversal) but no longer clutters the briefing.

## Temporal Context

Use `valid_from` and `valid_until` metadata for facts that are true only during a specific period. The briefing engine filters out nodes outside their validity window:

```python
cx.store(
    "api-version",
    "Payment API v2 is the current version",
    body="v2 introduced idempotency keys and webhook signatures. "
         "v1 is deprecated but still operational until 2026-06-01.",
    importance=0.8,
    source_agent="coding-agent",
    tags=["payments", "api"],
    metadata={"valid_from": "2025-09-15"},
)
```

When v3 ships, store the new version with its own `valid_from` and update the v2 node's `valid_until`. The briefing engine automatically stops showing v2 and starts showing v3.

## Example Briefing Output

A coding agent calling `cx.briefing("coding-agent", compact=True)` with the roles above receives output structured like this:

```
## Identity
- coding-agent: Full-stack development assistant for the payments platform.

## Constraints (persistent)
- No vendor lock-in for core infrastructure
- All public endpoints require idempotency keys

## Architecture Decisions (persistent)
- Use PostgreSQL for transactional data [adr-001]
- Event sourcing for payment state transitions [adr-007]

## Active Tasks (trackable)
- Migrate checkout flow to React 19 Server Components [in-progress]

## Recent Events (temporal)
- 2026-03-14: Deployed payment-service v2.4.1
- 2026-03-12: Incident — webhook retry storm (resolved)

## Patterns for Review (reviewable)
- Repository pattern for database access
- Anti-pattern: God service classes

## Current Versions (superseding)
- React 19 for frontend (supersedes React 18)
- Payment API v2 is the current version
```

## Quick Setup

```bash
cortex init --template coding
cortex serve
```

This generates a `cortex.toml` pre-configured with the coding agent roles shown above. Start the server, connect your agent, and begin storing decisions.
