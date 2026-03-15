# Research Agent Memory

Cortex gives research agents a structured memory for tracking hypotheses, experiments, and findings over long investigations. The key capability: when two findings contradict each other, Cortex detects the conflict and surfaces both in the agent's briefing so nothing gets silently overwritten.

## Configuration

Add a `[briefing.roles]` section to your `cortex.toml` tuned for research workflows:

```toml
[briefing.roles]
identity    = ["agent"]
persistent  = ["hypothesis", "methodology"]
trackable   = ["research-question", "objective"]
temporal    = ["experiment", "observation"]
reviewable  = ["finding", "pattern"]
superseding = ["claim", "measurement", "citation"]
```

Role mapping for research:

- **persistent** -- hypotheses and methodologies that frame the entire investigation. Always visible.
- **trackable** -- active research questions and objectives. Cleared when answered or abandoned.
- **temporal** -- experiments and observations in chronological order. Provides an audit trail of what was tried and when.
- **reviewable** -- findings and patterns that need periodic re-evaluation as new evidence arrives.
- **superseding** -- claims, measurements, and citations where only the latest version matters. When a newer measurement replaces an older one, the briefing shows only the current value.

## Tracking Hypotheses

Store hypotheses as persistent nodes so they remain visible throughout the research:

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

hyp_id = cx.store(
    "hypothesis",
    "Retrieval-augmented generation reduces hallucination rates",
    body="RAG with domain-specific corpora should reduce factual "
         "hallucination by >50% compared to parametric-only generation.",
    importance=0.9,
    source_agent="research-agent",
    tags=["rag", "hallucination", "nlp"],
)
```

When an experiment supports or contradicts this hypothesis, store the result as a temporal `experiment` node. The auto-linker detects semantic overlap between the hypothesis and the experiment, creating `supports` or `contradicts` edges based on the relationship:

```python
cx.store(
    "experiment",
    "RAG hallucination benchmark — medical QA",
    body="Tested RAG vs. baseline on 500 medical questions. "
         "RAG reduced hallucination rate from 23% to 8% (65% reduction). "
         "Supports hypothesis.",
    importance=0.8,
    source_agent="research-agent",
    tags=["rag", "hallucination", "medical"],
)
```

## Contradiction Detection

When two findings conflict, Cortex flags them in the briefing. This happens automatically when findings share tags or high semantic similarity but contain opposing claims.

```python
# Finding A
cx.store(
    "finding",
    "Treatment X is effective for condition Y",
    body="Meta-analysis of 12 RCTs shows statistically significant "
         "improvement (p<0.01, effect size 0.45).",
    importance=0.85,
    source_agent="research-agent",
    tags=["treatment-x", "condition-y", "efficacy"],
)

# Finding B (contradicts A)
cx.store(
    "finding",
    "Treatment X shows no significant effect on condition Y",
    body="Large-scale replication study (n=5000) found no statistically "
         "significant improvement (p=0.34, effect size 0.03).",
    importance=0.85,
    source_agent="research-agent",
    tags=["treatment-x", "condition-y", "efficacy"],
)
```

The auto-linker detects the shared tags and opposing language, creating a `contradicts` edge. The next briefing surfaces both findings with a contradiction alert, prompting the agent to reconcile them rather than silently using whichever it encountered last.

## Entity Graphs

Build entity graphs of papers, authors, and institutions to cross-reference findings by shared entities:

```python
from cortex_memory import EntityType

cx.store_entity("Dr. Jane Smith", entity_type=EntityType.PERSON,
                aliases=["J. Smith", "Smith J."],
                source_agent="research-agent")

cx.store_entity("Stanford NLP Group", entity_type=EntityType.COMPANY,
                aliases=["Stanford NLP"],
                source_agent="research-agent")
```

When findings reference the same author or institution, the auto-linker connects them through the shared entity. This lets the agent answer questions like "What has Dr. Smith published that relates to our hypothesis?" through a single graph traversal.

## Cross-Agent Research

Multiple research agents specializing in different domains can share a single Cortex instance. Each agent writes to its own `source_agent` namespace, but the shared graph means the auto-linker connects findings across agents automatically.

```python
# Biology agent stores a finding
cx.store("finding", "Protein X binds to receptor Y",
         source_agent="bio-agent", importance=0.8,
         tags=["protein-x", "receptor-y"])

# Chemistry agent stores a related finding
cx.store("finding", "Compound Z inhibits receptor Y binding",
         source_agent="chem-agent", importance=0.8,
         tags=["compound-z", "receptor-y"])
```

Use `scope=shared` in briefings to include cross-agent findings:

```python
briefing = cx.briefing("bio-agent", compact=True)
# Includes the chemistry agent's finding about receptor Y
# because the shared entity connects them
```

## Quick Setup

```bash
cortex init --template research
cortex serve
```

This generates a `cortex.toml` with research-oriented roles and retention settings tuned for long-running investigations.
