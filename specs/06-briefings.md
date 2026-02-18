# Phase 6 — Cortex Briefings: Agent Context Synthesis

**Duration:** 1 week  
**Crate:** `cortex-core` (briefing engine) + `cortex-server` (API + cache)  
**Dependencies:** Phases 1-5 complete  
**New deps:** None (uses existing graph + vector infrastructure)

---

## Objective

Build the briefing synthesiser — the system that replaces flat file context loading (AGENTS.md, MEMORY.md, SOUL.md) with dynamically generated, graph-aware context briefings. An agent doesn't load a static file at boot. It asks Cortex: "What do I need to know?" and gets back a tailored, current, de-duplicated context document.

This is the capstone. Everything in Phases 1-5 was building toward this.

---

## The Problem With Static Files

Today, agents load context from flat files:
- `SOUL.md` → personality and voice (now in PromptForge, but still flat text)
- `AGENTS.md` → workspace rules and conventions
- `MEMORY.md` → curated long-term memory (manually maintained)
- `memory/YYYY-MM-DD.md` → daily logs (manually written)
- `USER.md` → info about Mike (manually updated)

Problems:
1. **Stale.** Files are only as current as the last time someone edited them.
2. **Redundant.** Same information scattered across multiple files.
3. **No relationships.** "We chose Go for Dispatch" and "Dispatch is I/O-bound" are in different files with no connection.
4. **No prioritisation.** Everything has equal weight. A 6-month-old fact gets the same prominence as yesterday's critical decision.
5. **No personalisation.** Every agent gets the same dump. Kai doesn't need DutyBound's implementation details, and DutyBound doesn't need Kai's social context.
6. **Token waste.** Loading everything burns context window on irrelevant information.

---

## Briefing Architecture

### What A Briefing Contains

A briefing is a structured markdown document with sections, ordered by relevance. Each section pulls from the graph based on the requesting agent's identity and role.

```
# Cortex Briefing — {agent_id}
Generated: {timestamp} | Nodes consulted: {count}

## Identity
{Agent node: who you are, your role, your capabilities}

## Active Context
{Recent decisions, events, and facts relevant to your current work}

## Key Relationships
{People, agents, and systems you interact with}

## Patterns & Lessons
{Distilled patterns that apply to your work}

## Goals
{Active goals relevant to your scope}

## Unresolved
{Contradictions, open questions, stale facts flagged for review}

## Recent Events
{Last 48h of events in your domain}
```

### BriefingEngine

```rust
pub struct BriefingEngine {
    storage: Arc<dyn Storage>,
    graph: Arc<dyn GraphEngine>,
    vectors: Arc<dyn VectorIndex>,
    embeddings: Arc<dyn EmbeddingService>,
    cache: BriefingCache,
    config: BriefingConfig,
}

pub struct BriefingConfig {
    /// Maximum nodes per section. Default: 10.
    pub max_items_per_section: usize,
    
    /// Maximum total nodes in a briefing. Default: 50.
    pub max_total_items: usize,
    
    /// Maximum character length of final briefing text.
    /// Prevents token budget blowout. Default: 8000 chars (~2000 tokens).
    pub max_chars: usize,
    
    /// How far back to look for "recent" events. Default: 48 hours.
    pub recent_window: Duration,
    
    /// Cache TTL. Briefings are expensive to generate.
    /// Default: 5 minutes.
    pub cache_ttl: Duration,
    
    /// Include contradictions section. Default: true.
    pub include_contradictions: bool,
    
    /// Minimum importance score for inclusion. Default: 0.3.
    pub min_importance: f32,
    
    /// Minimum edge weight for traversal. Default: 0.2.
    pub min_weight: f32,
}
```

---

## Section Generation

Each section has a dedicated generator that queries the graph and produces ranked, de-duplicated content.

### Identity Section

```rust
fn generate_identity(&self, agent_id: &str) -> Result<BriefingSection> {
    // 1. Find the agent node
    let agent_node = self.find_agent_node(agent_id)?;
    
    // 2. Get directly connected preferences and capabilities
    let prefs = self.graph.neighbors(
        agent_node.id,
        TraversalDirection::Incoming,
        Some(vec![Relation::AppliesTo]),
    )?;
    
    // 3. Filter to Preference and Fact kinds
    let identity_nodes: Vec<_> = prefs
        .into_iter()
        .filter(|(n, _)| matches!(n.kind, NodeKind::Preference | NodeKind::Fact))
        .take(self.config.max_items_per_section)
        .collect();
    
    // 4. Render
    Ok(BriefingSection {
        title: "Identity".into(),
        nodes: identity_nodes,
    })
}
```

### Active Context Section

The most complex section. Uses hybrid retrieval anchored on the agent's recent activity.

```rust
fn generate_active_context(&self, agent_id: &str) -> Result<BriefingSection> {
    // 1. Find recent events involving this agent (last 48h)
    let recent = self.storage.list_nodes(NodeFilter {
        source_agent: Some(agent_id.into()),
        created_after: Some(Utc::now() - self.config.recent_window),
        kinds: Some(vec![NodeKind::Decision, NodeKind::Event, NodeKind::Fact]),
        ..Default::default()
    })?;
    
    // 2. Use recent nodes as anchors for hybrid search
    let anchor_ids: Vec<_> = recent.iter().map(|n| n.id).collect();
    
    // 3. Hybrid search: "what's relevant to what this agent has been doing?"
    // Use a generic query embedding of the agent's recent activity
    let context_query = recent.iter()
        .map(|n| n.data.title.as_str())
        .collect::<Vec<_>>()
        .join(". ");
    
    let results = self.hybrid_search(HybridQuery {
        query_text: context_query,
        anchors: anchor_ids,
        vector_weight: 0.5,  // Equal weight — we want graph-connected AND relevant
        limit: self.config.max_items_per_section,
        kind_filter: Some(vec![NodeKind::Decision, NodeKind::Fact, NodeKind::Event]),
        max_anchor_depth: 3,
    })?;
    
    // 4. De-duplicate against identity section
    // 5. Rank by combined score
    // 6. Render
    Ok(BriefingSection {
        title: "Active Context".into(),
        nodes: results.into_iter().map(|r| r.node).collect(),
    })
}
```

### Patterns & Lessons Section

```rust
fn generate_patterns(&self, agent_id: &str) -> Result<BriefingSection> {
    // 1. Find all patterns that apply to this agent
    let patterns = self.graph.traverse(TraversalRequest {
        start: vec![self.find_agent_node(agent_id)?.id],
        max_depth: Some(2),
        direction: TraversalDirection::Incoming,
        relation_filter: Some(vec![Relation::AppliesTo, Relation::InstanceOf]),
        kind_filter: Some(vec![NodeKind::Pattern]),
        strategy: TraversalStrategy::Weighted,  // Highest-weight patterns first
        ..Default::default()
    })?;
    
    // 2. Sort by importance × edge weight × recency
    // 3. Take top N
    // 4. Render with supporting evidence (1-hop linked observations)
}
```

### Unresolved Section

```rust
fn generate_unresolved(&self, agent_id: &str) -> Result<BriefingSection> {
    // 1. Find all Contradicts edges in agent's neighborhood
    let contradictions = self.graph.traverse(TraversalRequest {
        start: vec![self.find_agent_node(agent_id)?.id],
        max_depth: Some(3),
        direction: TraversalDirection::Both,
        relation_filter: Some(vec![Relation::Contradicts]),
        ..Default::default()
    })?;
    
    // 2. Find facts with low confidence (low importance + low edge weight)
    // 3. Find nodes marked for review by auto-linker
    // 4. Present both sides of contradictions
}
```

---

## Briefing Cache

Briefings are expensive to generate (multiple traversals, hybrid searches, rendering). Cache aggressively.

```rust
pub struct BriefingCache {
    /// Cached briefings keyed by agent_id.
    cache: HashMap<String, CachedBriefing>,
    
    /// TTL for cached briefings.
    ttl: Duration,
}

pub struct CachedBriefing {
    pub briefing: BriefingResponse,
    pub generated_at: Instant,
    pub graph_version: u64,  // Monotonic counter, incremented on any write
}

impl BriefingCache {
    fn get(&self, agent_id: &str, current_graph_version: u64) -> Option<&BriefingResponse> {
        if let Some(cached) = self.cache.get(agent_id) {
            // Valid if:
            // 1. Within TTL
            // 2. Graph hasn't changed since generation
            if cached.generated_at.elapsed() < self.ttl 
                && cached.graph_version == current_graph_version 
            {
                return Some(&cached.briefing);
            }
        }
        None
    }
}
```

The `graph_version` counter ensures that if new knowledge arrives (via NATS ingest or manual creation), cached briefings are invalidated immediately. No stale data.

---

## Rendering

### Markdown Output

The default rendering produces clean markdown suitable for injection into an LLM's system prompt or context window.

```rust
pub struct MarkdownRenderer;

impl BriefingRenderer for MarkdownRenderer {
    fn render(&self, briefing: &Briefing) -> String {
        let mut out = String::new();
        
        out.push_str(&format!(
            "# Cortex Briefing — {}\n_Generated: {} | {} nodes consulted_\n\n",
            briefing.agent_id,
            briefing.generated_at.format("%Y-%m-%d %H:%M UTC"),
            briefing.nodes_consulted,
        ));
        
        for section in &briefing.sections {
            out.push_str(&format!("## {}\n", section.title));
            for node in &section.nodes {
                out.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    node.data.title,
                    node.kind.as_str(),
                    truncate(&node.data.body, 200),
                ));
            }
            out.push('\n');
        }
        
        // Enforce max_chars
        if out.len() > self.config.max_chars {
            out.truncate(self.config.max_chars);
            out.push_str("\n\n_[Briefing truncated — increase max_chars for full context]_\n");
        }
        
        out
    }
}
```

### Compact Output

For constrained context windows, a compact renderer that uses bullet points and abbreviations:

```rust
pub struct CompactRenderer {
    max_chars: usize,
}

impl BriefingRenderer for CompactRenderer {
    fn render(&self, briefing: &Briefing) -> String {
        // Title-only rendering: just the node titles, grouped by section
        // ~4x more nodes in same token budget
    }
}
```

---

## PromptForge Integration

Cortex doesn't replace PromptForge — they work together.

**PromptForge** → SOUL (personality, voice, behaviour rules)  
**Cortex** → Context (knowledge, decisions, relationships, patterns)

Agent boot sequence becomes:
1. Pull SOUL from PromptForge (`/api/v1/prompts/{agent}-soul/versions/latest`)
2. Pull context briefing from Cortex (`GetBriefing(agent_id)`)
3. Combine: SOUL as system prompt, briefing as context injection
4. Agent is ready with current identity AND current knowledge

This separation means:
- Changing an agent's personality (PromptForge) doesn't affect their knowledge
- New knowledge (Cortex) doesn't require SOUL version bumps
- Different agents with the same SOUL get different briefings (personalised context)

---

## Agent Access Tracking

When an agent reads a briefing, every node included gets an access bump:

```rust
fn on_briefing_served(&self, briefing: &Briefing) {
    for section in &briefing.sections {
        for node in &section.nodes {
            self.storage.increment_access(node.id);
            // This reinforces edges connected to this node,
            // slowing decay on knowledge that's actively used
        }
    }
}
```

This creates the feedback loop: useful knowledge gets accessed → edges reinforced → knowledge stays prominent → appears in future briefings. Unused knowledge decays.

---

## Pre-Computation

For agents that boot frequently (workers, heartbeat agents), generating a fresh briefing every time is wasteful. The pre-computation system maintains ready-to-serve briefings.

```rust
pub struct BriefingPrecomputer {
    /// Agents to pre-compute briefings for.
    agents: Vec<String>,
    
    /// Recompute interval. Default: 5 minutes.
    interval: Duration,
}

impl BriefingPrecomputer {
    async fn run(&self) {
        loop {
            for agent_id in &self.agents {
                // Only recompute if graph has changed
                if self.cache.is_stale(agent_id, self.graph_version()) {
                    let briefing = self.engine.generate(agent_id).await;
                    self.cache.put(agent_id, briefing);
                }
            }
            tokio::time::sleep(self.interval).await;
        }
    }
}
```

Pre-computed agents (configured at startup):
- `kai` — main orchestrator, boots on every session
- `dutybound` — engineering lead, boots frequently for missions
- Workers don't get pre-computed briefings — they get focused task briefings from their handoff files, not full agent briefings

---

## File Ingest

Drop-folder ingest for bootstrapping knowledge from existing files.

```rust
pub struct FileIngest {
    /// Watch directory for new files.
    watch_dir: PathBuf,
    
    /// Supported formats.
    formats: Vec<&'static str>,  // md, txt, json, yaml, csv, log
}
```

**Process:**
1. File appears in `$CORTEX_DATA_DIR/ingest/`
2. Cortex detects it (filesystem watcher or poll)
3. Split into chunks (by heading for markdown, by line count for others)
4. For each chunk:
   a. Classify into NodeKind (heuristic: contains "decided" → Decision, contains a date → Event, etc.)
   b. Check for duplicates against existing nodes
   c. Create node with embedding
   d. Auto-linker wires it into the graph on next cycle
5. Move processed file to `$CORTEX_DATA_DIR/ingest/processed/`

**Bootstrap path:** Drop existing `MEMORY.md`, `memory/*.md`, `USER.md` into ingest folder. Cortex extracts structured knowledge and wires it into the graph. The flat files become obsolete.

### Classification Heuristics

```rust
fn classify_chunk(text: &str) -> NodeKind {
    let lower = text.to_lowercase();
    
    if lower.contains("decided") || lower.contains("chose") || lower.contains("picked") {
        NodeKind::Decision
    } else if lower.contains("goal") || lower.contains("target") || lower.contains("aim") {
        NodeKind::Goal
    } else if lower.contains("prefers") || lower.contains("likes") || lower.contains("hates") {
        NodeKind::Preference
    } else if lower.contains("pattern") || lower.contains("lesson") || lower.contains("always") {
        NodeKind::Pattern
    } else if lower.contains("happened") || lower.contains("deployed") || lower.contains("merged") {
        NodeKind::Event
    } else if lower.contains("noticed") || lower.contains("observed") || lower.contains("saw that") {
        NodeKind::Observation
    } else {
        NodeKind::Fact  // Default: if unsure, it's a fact
    }
}
```

These heuristics are a starting point. For higher accuracy, route chunks through a lightweight LLM call with a classification prompt. But the local heuristic approach means zero API cost and works offline.

---

## Dashboard Integration

The Cortex briefing should be viewable on `darlington.dev`:

- `/cortex` page on the dashboard showing:
  - Graph stats (node/edge counts by type)
  - Recent auto-linker activity
  - Briefing preview for any agent
  - D3.js graph visualisation (served from Cortex HTTP API)

This is a frontend task for the Darlington repo, not Cortex itself. Cortex provides the API; the dashboard consumes it.

---

## Testing Strategy

### Unit Tests

- Identity section includes agent's preferences and capabilities
- Active context section uses hybrid search correctly
- Patterns section traverses AppliesTo edges
- Unresolved section surfaces contradictions
- Max items per section enforced
- Max total items enforced
- Max chars truncation works
- Cache returns cached briefing when graph unchanged
- Cache invalidates when graph version increments
- Access tracking increments node access_count
- Markdown rendering produces valid markdown
- Compact rendering fits within char limit

### Integration Tests

- Full briefing generation for a realistic agent graph (50+ nodes)
- Briefing changes when new knowledge is added
- Briefing excludes decayed/pruned edges
- Briefing includes recently reinforced knowledge
- Pre-computation updates cache on schedule
- File ingest: drop a markdown file → verify nodes created and linked

### Qualitative Tests

- Generate briefing for "kai" with real Warren knowledge graph
- Compare to current MEMORY.md — does the briefing capture the same information?
- Compare to current AGENTS.md — are the rules and conventions present?
- Have an agent boot with briefing instead of flat files — does it behave correctly?
- Measure token count: briefing should be <2000 tokens for practical use

---

## Deliverables

1. `BriefingEngine` with section generators (identity, active context, patterns, unresolved, recent events)
2. Briefing cache with graph-version invalidation
3. Markdown and compact renderers with char limits
4. Pre-computation for configured agents
5. File ingest with classification heuristics
6. Access tracking and reinforcement on briefing serve
7. PromptForge integration documentation (SOUL + briefing composition)
8. Migration guide: flat files → Cortex briefings
