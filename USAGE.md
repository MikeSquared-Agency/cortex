# Cortex Usage Guide

Complete guide to using Cortex for graph memory management.

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [API Usage](#api-usage)
- [Graph Visualization](#graph-visualization)
- [NATS Integration](#nats-integration)
- [Auto-Linker](#auto-linker)
- [Best Practices](#best-practices)

## Installation

### Install Script (recommended)

```bash
curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh
```

No Rust toolchain or system dependencies required.


### From Source

```bash
git clone https://github.com/MikeSquared-Agency/cortex.git
cd cortex
cargo build --release
```

### Using Docker

```bash
docker pull mikesquared/cortex:latest
docker run -p 9090:9090 -p 9091:9091 -v cortex-data:/data mikesquared/cortex:latest
```

## Quick Start

### 1. Start the Server

```bash
# Local
cargo run --bin cortex-server

# Docker Compose
docker-compose up -d

# Using the installed binary
cortex serve

# With custom config
cargo run --bin cortex-server -- \
  --grpc-addr 0.0.0.0:9090 \
  --http-addr 0.0.0.0:9091 \
  --data-dir ./data
```

### 2. Verify Health

```bash
curl http://localhost:9091/health
```

### 3. Add Your First Node

```bash
curl -X POST http://localhost:9091/nodes \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "Fact",
    "title": "Rust is memory-safe",
    "body": "Rust provides memory safety without garbage collection using ownership",
    "source_agent": "user",
    "importance": 0.8
  }'
```

### 4. Search

```bash
curl "http://localhost:9091/search?q=memory+safety&limit=10"
```

## Configuration

### Environment Variables

```bash
# Server Addresses
export GRPC_ADDR=0.0.0.0:9090
export HTTP_ADDR=0.0.0.0:9091

# Data Storage
export DATA_DIR=/var/lib/cortex

# NATS Configuration
export NATS_ENABLED=true
export NATS_URL=nats://nats.warren.local:4222

# Auto-Linker Settings
export AUTO_LINKER_ENABLED=true
export AUTO_LINKER_INTERVAL=300              # seconds between cycles
export AUTO_LINKER_BATCH_SIZE=100            # nodes per cycle
export AUTO_LINKER_SIMILARITY_THRESHOLD=0.85 # similarity link threshold
export AUTO_LINKER_DEDUP_THRESHOLD=0.95      # deduplication threshold

# Edge Decay
export DECAY_ENABLED=true
export DECAY_HALF_LIFE_DAYS=30
export DECAY_IMPORTANCE_SHIELD=0.7

# Logging
export RUST_LOG=info  # debug, info, warn, error
```

### Command Line Args

```bash
cortex-server --help

USAGE:
    cortex-server [OPTIONS]

OPTIONS:
    --grpc-addr <ADDR>      gRPC server address [default: 0.0.0.0:9090]
    --http-addr <ADDR>      HTTP server address [default: 0.0.0.0:9091]
    --data-dir <PATH>       Data directory [default: ./data]
    --nats-url <URL>        NATS server URL [default: nats://localhost:4222]
    --nats-enabled          Enable NATS consumer
```

## API Usage

### gRPC (Production)

#### Rust Client

```rust
use cortex_proto::cortex_service_client::CortexServiceClient;
use cortex_proto::*;
use tonic::Request;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = CortexServiceClient::connect("http://localhost:9090").await?;

    // Add a node
    let response = client.add_node(Request::new(AddNodeRequest {
        kind: "Fact".to_string(),
        title: "Neural networks learn patterns".to_string(),
        body: "Neural networks use backpropagation to learn".to_string(),
        source_agent: "ai-agent".to_string(),
        source_session: Some("session-123".to_string()),
        source_channel: Some("slack".to_string()),
        importance: 0.7,
    })).await?;

    let node_id = response.into_inner().node_id;
    println!("Created node: {}", node_id);

    // Search
    let results = client.search(Request::new(SearchRequest {
        query: "neural networks".to_string(),
        limit: 10,
        kind: None,
        source_agent: None,
    })).await?;

    for node in results.into_inner().nodes {
        println!("{}: {}", node.id, node.title);
    }

    Ok(())
}
```

#### Python Client

```python
import grpc
from cortex_proto import cortex_pb2, cortex_pb2_grpc

channel = grpc.insecure_channel('localhost:9090')
client = cortex_pb2_grpc.CortexServiceStub(channel)

# Add node
response = client.AddNode(cortex_pb2.AddNodeRequest(
    kind='Fact',
    title='Python is dynamically typed',
    body='Python uses duck typing for flexibility',
    source_agent='python-agent',
    importance=0.6
))

print(f"Created node: {response.node_id}")

# Search
results = client.Search(cortex_pb2.SearchRequest(
    query='python typing',
    limit=10
))

for node in results.nodes:
    print(f"{node.id}: {node.title}")
```

### HTTP (Debug/Exploration)

#### List All Nodes

```bash
curl http://localhost:9091/nodes | jq
```

#### Filter by Kind

```bash
curl "http://localhost:9091/nodes?kind=Decision" | jq
```

#### Get Single Node

```bash
curl http://localhost:9091/nodes/{node_id} | jq
```

#### Search

```bash
# Basic search
curl "http://localhost:9091/search?q=machine+learning" | jq

# With filters
curl "http://localhost:9091/search?q=ML&kind=Fact&limit=20" | jq
```

#### Get Statistics

```bash
curl http://localhost:9091/stats | jq
```

Response:
```json
{
  "node_count": 1523,
  "edge_count": 4891,
  "disk_usage_bytes": 12582912,
  "indexed_nodes": 1523
}
```

#### Export Full Graph

```bash
curl http://localhost:9091/graph/export | jq
```

Returns:
```json
{
  "data": {
    "nodes": [...],
    "edges": [...]
  }
}
```

## Graph Visualization

### Web Interface

1. Open browser to `http://localhost:9091/graph/viz`
2. Interactive D3.js force-directed graph
3. Features:
   - Zoom and pan
   - Drag nodes
   - Hover for details
   - Filter by node kind
   - Toggle labels

### Controls

- **Show Labels**: Toggle node title display
- **Filter by Kind**: Show only specific node types
- **Zoom**: Mouse wheel or pinch
- **Pan**: Click and drag background
- **Pin Node**: Drag node to position

### Color Coding

| Color | Node Kind |
|-------|-----------|
| Blue | Fact |
| Red | Decision |
| Orange | Event |
| Green | Observation |
| Purple | Pattern |
| Pink | Identity |
| Teal | Goal |
| Amber | Constraint |

## NATS Integration

### Subscribe to Warren Events

Cortex automatically consumes events from Warren:

```bash
# Warren publishes events
nats pub warren.evidence.submitted '{
  "type": "evidence.submitted",
  "evidence_id": "ev_123",
  "item_id": "item_456",
  "content": "Completed security audit with 0 critical findings",
  "submitted_by": "security-bot"
}'
```

Cortex converts this to a node:
- **Kind**: Fact
- **Title**: "Evidence: Completed security audit..."
- **Body**: Full content
- **Source**: security-bot
- **Session**: item_456

### Supported Events

| Warren Event | Cortex Node Kind |
|--------------|------------------|
| evidence.submitted | Fact |
| item.completed | Event |
| stage.advanced | Event |
| gate.approved | Decision |
| gate.rejected | Decision |
| interaction.created | Observation |
| task.picked | Event |
| autonomy | Pattern |
| refinement | Decision |

### Deduplication

NATS consumer automatically deduplicates events by:
- Title + source_session match
- Skips duplicate events (logged as debug)

## Auto-Linker

### Overview

The auto-linker runs in the background and:
1. Creates semantic links between similar nodes
2. Applies structural rules (temporal, causality, etc.)
3. Detects and merges duplicates
4. Decays unused edges over time

### Manual Trigger

```bash
curl -X POST http://localhost:9091/auto-linker/trigger
```

### Check Status

```bash
curl http://localhost:9091/auto-linker/status | jq
```

Response:
```json
{
  "cycle_count": 42,
  "processed_nodes": 4200,
  "links_created": {
    "similarity": 231,
    "temporal": 89,
    "source": 156,
    "causality": 45,
    "support": 78,
    "reference": 23
  },
  "dedup_metrics": {
    "merged": 12,
    "superseded": 5,
    "linked": 8
  }
}
```

### Tuning

Adjust thresholds via environment:

```bash
# More aggressive linking (lower threshold)
export AUTO_LINKER_SIMILARITY_THRESHOLD=0.80

# Less aggressive deduplication (higher threshold)
export AUTO_LINKER_DEDUP_THRESHOLD=0.98

# Faster cycles (more CPU usage)
export AUTO_LINKER_INTERVAL=60
```

### Disable Auto-Linker

```bash
export AUTO_LINKER_ENABLED=false
```

## Best Practices

### Node Design

1. **Use Descriptive Titles**: Titles should be standalone summaries
   ```
   Good: "Completed migration to Rust 1.70"
   Bad: "Completed"
   ```

2. **Rich Body Content**: Include context and details
   ```rust
   Node::new(
       NodeKind::Decision,
       "Chose PostgreSQL for primary database",
       "After evaluating MySQL, MongoDB, and PostgreSQL, we selected PostgreSQL for ACID compliance, JSON support, and strong ecosystem. Migration plan: 4 weeks.",
       source,
       0.8
   )
   ```

3. **Set Importance Appropriately**:
   - 0.9+: Critical decisions, major events
   - 0.7-0.8: Important facts, key patterns
   - 0.5-0.6: Supporting facts, observations
   - <0.5: Minor details, low-priority

4. **Use Source Metadata**:
   ```rust
   Source {
       agent: "backend-team",
       session: Some("migration-2024-q1"),
       channel: Some("slack-engineering"),
   }
   ```

### Edge Design

1. **Choose Correct Relations**:
   - **Causes**: "Decision to migrate" → "Migration started"
   - **Supports**: "Benchmark results" → "Database choice"
   - **Contradicts**: "Old policy" → "New policy"
   - **Precedes**: Event sequence

2. **Let Auto-Linker Handle Similarity**: Don't manually create Similar edges

3. **Use Provenance**: Track edge source
   ```rust
   EdgeProvenance::UserDirect  // Manual creation
   EdgeProvenance::AutoSimilarity { score: 0.92 }
   ```

### Performance

1. **Batch Operations**: Use gRPC for bulk ingestion
2. **Set Search Limits**: Don't fetch thousands of results
3. **Filter Early**: Use kind/source filters before search
4. **Index Rebuilds**: Only when necessary (corruption)

### Monitoring

1. **Check Stats Regularly**:
   ```bash
   watch -n 10 'curl -s http://localhost:9091/stats | jq'
   ```

2. **Monitor Auto-Linker**:
   ```bash
   curl http://localhost:9091/auto-linker/status
   ```

3. **Review Logs**:
   ```bash
   docker-compose logs -f cortex | grep ERROR
   ```

### Backup

```bash
# Stop server
docker-compose down

# Backup data directory
tar -czf cortex-backup-$(date +%Y%m%d).tar.gz data/

# Restart
docker-compose up -d

# Using the installed binary
cortex serve
```

### Migration

See [MIGRATION.md](MIGRATION.md) for Alexandria import instructions.

## Troubleshooting

### Server Won't Start

1. Check port conflicts:
   ```bash
   lsof -i :9090
   lsof -i :9091
   ```

2. Verify data directory permissions:
   ```bash
   ls -la data/
   ```

3. Check logs:
   ```bash
   RUST_LOG=debug cargo run --bin cortex-server
   ```

### Search Returns No Results

1. Verify embeddings are generated (check node has embedding field)
2. Check vector index status:
   ```bash
   curl http://localhost:9091/stats | jq .indexed_nodes
   ```
3. Try broader query or lower similarity threshold

### High Memory Usage

1. Reduce auto-linker batch size:
   ```bash
   export AUTO_LINKER_BATCH_SIZE=50
   ```

2. Disable adjacency cache (in code)
3. Increase decay frequency to prune edges

### NATS Connection Failed

1. Verify NATS is running:
   ```bash
   docker-compose ps nats
   ```

2. Test connectivity:
   ```bash
   nats sub "warren.>" --server nats://localhost:4222
   ```

3. Check firewall rules

## Support

- GitHub Issues: https://github.com/MikeSquared-Agency/cortex/issues
- Documentation: https://cortex.warren.dev/docs
- Slack: #cortex-support

## Prompt Management

### Creating Prompts

```bash
cortex prompt migrate prompts.json
```

### Listing and Inspecting

```bash
cortex prompt list
cortex prompt get my-prompt
cortex prompt get my-prompt --version 2
```

### Agent Binding

```bash
cortex agent bind my-agent my-prompt --weight 0.9
cortex agent show my-agent
cortex agent resolve my-agent
```

### Context-Aware Selection

```bash
cortex agent select my-agent --task-type coding --sentiment 0.3 --epsilon 0.2
```

### Performance Tracking

```bash
cortex agent observe my-agent \
  --variant-id UUID --variant-slug my-prompt \
  --sentiment-score 0.8 --task-outcome success

cortex prompt performance my-prompt
cortex agent history my-agent
```
