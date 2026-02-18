# Phase 5 — API & Integration: Wire Into Warren

**Duration:** 1 week  
**Crate:** `cortex-server` (new binary crate) + `cortex-proto` (new proto crate)  
**Dependencies:** Phases 1-4 complete  
**New deps:** tonic, tonic-build, prost, axum, async-nats, tokio, tracing

---

## Objective

Wrap cortex-core in a server binary that exposes gRPC for programmatic access, HTTP for debugging and dashboards, and a NATS consumer for automatic knowledge ingest from the Warren event stream. This is where Cortex stops being a library and becomes a service in the swarm.

---

## gRPC API

### Service Definition (cortex.proto)

```protobuf
syntax = "proto3";
package cortex.v1;

import "google/protobuf/timestamp.proto";
import "google/protobuf/struct.proto";

service CortexService {
    // === Nodes ===
    
    // Create a new knowledge node.
    rpc CreateNode(CreateNodeRequest) returns (NodeResponse);
    
    // Get a node by ID.
    rpc GetNode(GetNodeRequest) returns (NodeResponse);
    
    // Update a node's content (triggers re-embedding).
    rpc UpdateNode(UpdateNodeRequest) returns (NodeResponse);
    
    // Soft-delete a node.
    rpc DeleteNode(DeleteNodeRequest) returns (DeleteResponse);
    
    // List nodes with filtering.
    rpc ListNodes(ListNodesRequest) returns (ListNodesResponse);
    
    // === Edges ===
    
    // Create a manual edge between two nodes.
    rpc CreateEdge(CreateEdgeRequest) returns (EdgeResponse);
    
    // Get edges for a node.
    rpc GetEdges(GetEdgesRequest) returns (GetEdgesResponse);
    
    // Delete an edge.
    rpc DeleteEdge(DeleteEdgeRequest) returns (DeleteResponse);
    
    // === Graph Queries ===
    
    // Traverse the graph from starting nodes.
    rpc Traverse(TraverseRequest) returns (SubgraphResponse);
    
    // Find paths between two nodes.
    rpc FindPaths(FindPathsRequest) returns (PathsResponse);
    
    // Get node neighborhood (convenience).
    rpc Neighborhood(NeighborhoodRequest) returns (SubgraphResponse);
    
    // === Search ===
    
    // Semantic similarity search.
    rpc SimilaritySearch(SimilaritySearchRequest) returns (SearchResponse);
    
    // Hybrid search (vector + graph proximity).
    rpc HybridSearch(HybridSearchRequest) returns (HybridSearchResponse);
    
    // === Briefings ===
    
    // Get a synthesised context briefing for an agent.
    rpc GetBriefing(BriefingRequest) returns (BriefingResponse);
    
    // === Admin ===
    
    // Get graph statistics.
    rpc Stats(StatsRequest) returns (StatsResponse);
    
    // Get auto-linker metrics.
    rpc AutoLinkerStatus(AutoLinkerStatusRequest) returns (AutoLinkerStatusResponse);
    
    // Trigger manual auto-linker cycle.
    rpc TriggerAutoLink(TriggerAutoLinkRequest) returns (TriggerAutoLinkResponse);
    
    // Reindex all embeddings (model change).
    rpc Reindex(ReindexRequest) returns (ReindexResponse);
    
    // Health check.
    rpc Health(HealthRequest) returns (HealthResponse);
}

// === Messages ===

message CreateNodeRequest {
    string kind = 1;           // NodeKind as string
    string title = 2;
    string body = 3;
    map<string, google.protobuf.Value> metadata = 4;
    repeated string tags = 5;
    float importance = 6;      // 0.0-1.0, default 0.5
    string source_agent = 7;
    optional string source_session = 8;
    optional string source_channel = 9;
}

message NodeResponse {
    string id = 1;
    string kind = 2;
    string title = 3;
    string body = 4;
    map<string, google.protobuf.Value> metadata = 5;
    repeated string tags = 6;
    float importance = 7;
    string source_agent = 8;
    uint64 access_count = 9;
    google.protobuf.Timestamp created_at = 10;
    google.protobuf.Timestamp updated_at = 11;
    bool has_embedding = 12;
    uint32 edge_count = 13;   // Total connected edges
}

message CreateEdgeRequest {
    string from_id = 1;
    string to_id = 2;
    string relation = 3;     // Relation as string
    float weight = 4;        // Default 1.0 for manual edges
}

message TraverseRequest {
    repeated string start_ids = 1;
    uint32 max_depth = 2;     // 0 = start only
    string direction = 3;     // "outgoing", "incoming", "both"
    repeated string relation_filter = 4;
    repeated string kind_filter = 5;
    float min_weight = 6;
    uint32 limit = 7;
    string strategy = 8;     // "bfs", "dfs", "weighted"
}

message SubgraphResponse {
    repeated NodeResponse nodes = 1;
    repeated EdgeResponse edges = 2;
    map<string, uint32> depths = 3;  // node_id → depth
    uint32 visited_count = 4;
    bool truncated = 5;
}

message SimilaritySearchRequest {
    string query = 1;
    uint32 limit = 2;         // Default 10
    repeated string kind_filter = 3;
    float min_score = 4;      // Default 0.0
}

message HybridSearchRequest {
    string query = 1;
    repeated string anchor_ids = 2;
    float vector_weight = 3;  // Default 0.7
    uint32 limit = 4;
    repeated string kind_filter = 5;
    uint32 max_anchor_depth = 6;  // Default 3
}

message HybridSearchResponse {
    repeated HybridResultEntry results = 1;
}

message HybridResultEntry {
    NodeResponse node = 1;
    float vector_score = 2;
    float graph_score = 3;
    float combined_score = 4;
    optional string nearest_anchor_id = 5;
    optional uint32 nearest_anchor_depth = 6;
}

message BriefingRequest {
    string agent_id = 1;      // e.g. "kai", "dutybound"
    uint32 max_items = 2;     // Max nodes per section. Default 10.
    bool include_contradictions = 3;  // Surface unresolved contradictions
}

message BriefingResponse {
    string agent_id = 1;
    string briefing_text = 2;   // Synthesised markdown text
    repeated BriefingSection sections = 3;
    google.protobuf.Timestamp generated_at = 4;
    uint32 nodes_consulted = 5;
}

message BriefingSection {
    string title = 1;           // e.g. "Identity", "Recent Decisions"
    repeated NodeResponse nodes = 2;
}

message StatsResponse {
    uint64 node_count = 1;
    uint64 edge_count = 2;
    map<string, uint64> nodes_by_kind = 3;
    map<string, uint64> edges_by_relation = 4;
    uint64 db_size_bytes = 5;
}

message HealthResponse {
    bool healthy = 1;
    string version = 2;
    uint64 uptime_seconds = 3;
    StatsResponse stats = 4;
    AutoLinkerStatusResponse auto_linker = 5;
}
```

### gRPC Server Config

```rust
pub struct ServerConfig {
    /// gRPC listen address. Default: 0.0.0.0:9090
    pub grpc_addr: SocketAddr,
    
    /// HTTP listen address. Default: 0.0.0.0:9091
    pub http_addr: SocketAddr,
    
    /// NATS URL. Default: nats://hermes:4222
    pub nats_url: String,
    
    /// Data directory. Default: ./data
    pub data_dir: PathBuf,
    
    /// Auto-linker config.
    pub auto_linker: AutoLinkerConfig,
    
    /// Similarity config.
    pub similarity: SimilarityConfig,
    
    /// Max message size for gRPC. Default: 16MB.
    pub max_message_size: usize,
}
```

---

## HTTP API (Debug & Dashboard)

Axum-based HTTP server running alongside gRPC. Not the primary API — this is for humans, dashboards, and quick debugging.

### Endpoints

```
GET  /health                    → Health check (JSON)
GET  /stats                     → Graph statistics
GET  /nodes?kind=&tag=&limit=   → List nodes (paginated)
GET  /nodes/:id                 → Get node detail
GET  /nodes/:id/neighbors       → Node neighborhood
GET  /edges/:id                 → Get edge detail
GET  /search?q=&limit=          → Similarity search
GET  /graph/viz                 → D3.js graph visualisation (HTML page)
GET  /graph/export              → Full graph export (JSON)
GET  /auto-linker/status        → Auto-linker metrics
POST /auto-linker/trigger       → Manual auto-link cycle
GET  /briefing/:agent_id        → Agent briefing (text/markdown)
```

### Graph Visualisation

The `/graph/viz` endpoint serves a self-contained HTML page with D3.js force-directed graph visualisation. No build step, no npm. Inline JS. Shows nodes as circles (colored by kind), edges as lines (thickness = weight), labels on hover. Interactive: drag, zoom, filter by kind/relation.

This is for Mike to visually explore the graph in a browser. Debug tool, not production feature.

---

## NATS Integration

### Subscriptions

Cortex subscribes to Warren's NATS event stream to automatically ingest knowledge from the swarm's operational events.

```rust
pub struct NatsIngest {
    /// NATS client connected to Hermes.
    client: async_nats::Client,
    
    /// Storage backend for creating nodes.
    storage: Arc<dyn Storage>,
    
    /// Embedding service for immediate embedding.
    embeddings: Arc<dyn EmbeddingService>,
}
```

### Event → Node Mapping

| NATS Subject | Node Kind | Relation Created |
|---|---|---|
| `warren.stage.advanced` | Event | `LedTo` from previous stage event |
| `warren.item.completed` | Event | `LedTo` from all stage events for that item |
| `warren.evidence.submitted` | Fact | `InformedBy` to the related item node |
| `warren.gate.approved` | Decision | `LedTo` to stage advancement event |
| `warren.gate.rejected` | Decision | `Contradicts` evidence that was rejected |
| `warren.interaction.created` | Observation | Auto-linked by similarity |
| `warren.task.picked` | Event | `DependsOn` the item it picked |
| `warren.autonomy.*` | Pattern | `AppliesTo` relevant agent |
| `warren.refinement.*` | Decision | `Supersedes` previous refinement |

### Ingest Pipeline

```
NATS message arrives
    │
    ▼
Parse event payload (JSON)
    │
    ▼
Map to NodeKind + extract title/body/metadata
    │
    ▼
Check dedup: does a node with same title + source already exist?
    │  Yes → skip or update
    │  No ↓
    ▼
Create node (embedding computed inline)
    │
    ▼
Create any explicit edges (e.g., stage event → item)
    │
    ▼
Auto-linker picks up remaining edges on next cycle
```

### Conversation Ingest

Future: when sub-agent sessions publish transcripts to NATS (currently blocked — noted in constraints), Cortex will:
1. Receive conversation turns
2. Extract salient facts, decisions, observations using a lightweight LLM call
3. Create typed nodes for each extracted piece of knowledge
4. Link to the agent and session that produced them

This is the path to automatic knowledge accumulation from every agent interaction.

---

## Docker Integration

### Dockerfile

```dockerfile
FROM rust:1.82-slim AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --bin cortex-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cortex-server /usr/local/bin/cortex-server

# FastEmbed model downloaded on first run and cached in data dir
ENV CORTEX_DATA_DIR=/data
VOLUME /data

EXPOSE 9090 9091

ENTRYPOINT ["cortex-server"]
```

### stack.yaml Addition

```yaml
  cortex:
    image: warren_cortex:latest
    build:
      context: ../cortex
    ports:
      - target: 9090
        published: 9090  # gRPC
      - target: 9091
        published: 9091  # HTTP debug
    volumes:
      - cortex_data:/data
    networks:
      - warren_overlay
    environment:
      - NATS_URL=nats://hermes:4222
      - CORTEX_DATA_DIR=/data
      - CORTEX_GRPC_ADDR=0.0.0.0:9090
      - CORTEX_HTTP_ADDR=0.0.0.0:9091
      - RUST_LOG=cortex=info
    deploy:
      replicas: 1
      restart_policy:
        condition: on-failure
        delay: 10s
        max_attempts: 5
        window: 300s
      resources:
        limits:
          memory: 2G
        reservations:
          memory: 512M
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9091/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 30s
```

### Resource Budget

- **Memory:** 512MB baseline, 2GB limit. Breakdown:
  - redb mmap: ~50MB for 100k nodes
  - HNSW index: ~150MB for 100k × 384-dim vectors
  - Adjacency cache: ~200MB for 500k edges
  - FastEmbed model: ~130MB (loaded once)
  - Overhead: ~100MB
- **CPU:** Minimal at steady state. Spikes during auto-linker cycles and bulk embedding.
- **Disk:** ~500MB for 100k nodes with embeddings. Grows linearly.

---

## Alexandria Migration

One-time migration command built into the binary:

```bash
cortex-server migrate-alexandria \
    --supabase-url $SUPABASE_URL \
    --supabase-key $SUPABASE_KEY \
    --data-dir /data
```

Steps:
1. Connect to Alexandria's Supabase (Warren project: `uaubofpmokvumbqpeymz`)
2. Pull all 54 entries
3. Map `category` → `NodeKind`: fact → Fact, decision → Decision, discovery → Observation
4. Create nodes with original content, metadata preserved
5. Compute embeddings locally with FastEmbed
6. Run one auto-linker cycle to discover relationships
7. Print summary: N nodes imported, M edges created

After migration, Alexandria continues running but Cortex is the source of truth. Agents read from Cortex. Alexandria becomes write-only (legacy ingest) until fully deprecated.

---

## Smoke Test Integration

Add Cortex to the existing smoke test:

```bash
# In smoke-test.sh
check_service "cortex-grpc" "localhost:9090" "grpc"
check_service "cortex-http" "localhost:9091/health" "http"
```

---

## Testing Strategy

### Unit Tests

- gRPC handlers: each RPC method returns correct response for valid input
- gRPC handlers: proper error codes for invalid input (NotFound, InvalidArgument)
- NATS ingest: each event type maps to correct NodeKind and creates proper edges
- NATS ingest: duplicate events are detected and skipped
- Config parsing from environment variables

### Integration Tests

- Full flow: create node via gRPC → search via gRPC → verify found
- Full flow: publish NATS event → wait → query Cortex → verify ingested
- Health endpoint reflects actual state (healthy after startup, stats accurate)
- Alexandria migration with mock Supabase data

### Load Tests

- 100 concurrent gRPC CreateNode requests (target: all succeed, <100ms p99)
- 1000 NATS events/second ingest rate (target: no dropped events, backlog clears within 60s)
- 50 concurrent Traverse requests (target: <200ms p99)

---

## Deliverables

1. `cortex-server` binary with gRPC + HTTP servers
2. `cortex-proto` crate with protobuf definitions and generated code
3. NATS consumer with event → node mapping for all Warren subjects
4. Dockerfile and stack.yaml integration
5. Alexandria migration command
6. Graph visualisation endpoint (D3.js)
7. Smoke test integration
8. Health check endpoint
