# Cortex vs Zep (Graphiti)

Zep's Graphiti is the closest architectural peer to Cortex. Both are temporal knowledge graphs designed for AI agents. Both model entities, relationships, and time.

The differences are in deployment model, intelligence layer, and philosophy. This comparison is honest about where each excels.

## What Zep Does Well

Zep and its Graphiti framework have significant strengths:

- **Temporal knowledge graph backed by research.** Graphiti has a published arXiv paper detailing its temporal knowledge graph approach. The academic foundation is solid and the design decisions are well-documented.

- **94.8% Dialogue Memory Recall.** Zep reports a 94.8% score on the DMR benchmark, which measures how well a system recalls facts from prior conversations. That is a strong result and they publish their methodology.

- **Enterprise-ready.** Zep Cloud is a managed offering with support, SLAs, and the operational maturity that enterprise customers require. If you need a vendor relationship and uptime guarantees, this matters.

- **Proven graph database backends.** Zep uses Neo4j or FalkorDB under the hood. These are mature, well-understood graph databases with large ecosystems, extensive tooling, and known scaling characteristics.

- **Episodic and semantic memory distinction.** Graphiti separates raw episodes (what happened) from semantic entities and relationships (what it means). This is a clean conceptual model that maps well to how conversations generate knowledge.

These are real advantages, particularly for teams that need enterprise support or already operate Neo4j infrastructure.

## What Cortex Does Differently

Cortex makes fundamentally different architectural choices:

- **Embedded.** Cortex uses redb, an embedded ACID database with zero-copy mmap. There is no external database to deploy or manage. Zep requires Neo4j or FalkorDB, which means running a separate database process, managing its storage, and handling its failure modes. For many agent deployments, that operational overhead is not justified by the workload.

- **Self-organizing.** Cortex's auto-linker discovers relationships automatically through configurable rules. You define conditions -- semantic similarity thresholds, temporal proximity windows, shared metadata tags -- and the auto-linker continuously creates and maintains edges. Graphiti relies on an explicit extraction pipeline: LLM calls extract entities and relationships from episodes. This works well but couples the graph structure to the quality and cost of those LLM calls.

- **Trust from topology.** Cortex computes trust scores from graph structure. Corroboration from independent sources increases confidence. Contradictions decrease it. Source reliability propagates through edges. Zep does not have a trust model. All extracted entities and relationships are treated with equal confidence regardless of how many sources support them.

- **Briefing synthesis with roles.** Cortex generates role-based briefings with configurable scope. An agent requests a briefing for its role and receives a structured context document -- not search results, but synthesised knowledge relevant to its current task. Briefing scope can include cross-agent knowledge or restrict to a single agent's graph. Zep provides search and retrieval, but the calling agent must assemble context from the results itself.

- **Single binary.** `cargo install cortex-memory` and you are running. No Docker Compose file, no database provisioning, no connection strings. The entire engine -- storage, graph, vector search, auto-linker -- runs in-process. Zep's quickstart involves standing up multiple services.

- **Configurable auto-linker rules.** Cortex lets you define linking rules declaratively: match conditions, edge types, weight functions. You control how knowledge self-organizes. Zep's relationship discovery is built into the entity extraction pipeline and is less configurable at the graph level.

## Quick Reference

| Capability | Cortex | Zep / Graphiti |
|---|---|---|
| Deployment model | Embedded single binary | Service (Neo4j/FalkorDB + Zep) |
| Storage backend | redb (embedded, ACID) | Neo4j or FalkorDB |
| Relationship discovery | Auto-linker with configurable rules | LLM extraction pipeline |
| Temporal model | `valid_from` / `valid_until` per node | Temporal edges with timestamps |
| Trust scoring | Graph topology-based | Not available |
| Briefing synthesis | Role-based, configurable scope | Not available (search only) |
| Contradiction detection | Built-in | Not available |
| Dialogue recall benchmark | Not published | 94.8% DMR |
| Enterprise cloud | Not available | Zep Cloud with SLAs |
| Episodic memory | Nodes with metadata | First-class episode type |

## When to Choose Zep

Zep is the better choice when:

- You need enterprise support, SLAs, and a managed cloud offering.
- You already run Neo4j or FalkorDB and want to build on that infrastructure.
- You need the highest possible dialogue recall and the DMR benchmark matters to your evaluation.
- You want a managed service where extraction, storage, and retrieval are handled for you.
- Your team has experience with graph databases and prefers explicit schema control.

## When to Choose Cortex

Cortex is the better choice when:

- You want embedded deployment with no external databases. One process, one data directory, zero network dependencies.
- You need self-organizing memory that discovers relationships through configurable rules rather than LLM extraction pipelines.
- You want trust scoring derived from graph topology -- corroboration, contradiction, and source reliability.
- You need configurable briefing roles and cross-agent scope for multi-agent systems.
- You prefer a single binary over a multi-service architecture.
- You want to run on-premise or in air-gapped environments where external database services are not available.

## Architectural Comparison

The fundamental difference is this: Zep is a service. Cortex is an engine.

Zep's pipeline is **extract, store, query**. Episodes go in, an LLM extracts entities and relationships, they are stored in a graph database, and you query them later. The intelligence is in the extraction step.

Cortex's pipeline is **store, auto-organize, synthesise**. Knowledge goes in as nodes, the auto-linker discovers structure continuously, trust propagates through the graph, and briefings synthesise what matters. The intelligence is in the graph itself.

This leads to different cost profiles. Zep's extraction step requires LLM calls per episode, which adds latency and cost proportional to input volume. Cortex's auto-linker runs locally with no LLM dependency, though you can optionally use LLMs for embedding generation.

Zep is better if you need a production-grade cloud service with enterprise support and proven dialogue recall benchmarks. Cortex is better if you want embedded intelligence that self-organizes, computes trust, and generates briefings -- all without leaving the process.

Neither approach is universally superior. They reflect different beliefs about where intelligence should live in an agent memory system.

---

*Last updated: March 2026. If anything here is inaccurate or outdated, open an issue. We want this comparison to be fair.*
