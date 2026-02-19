# Phase D — Client SDKs

**Duration:** 2 weeks  
**Dependencies:** Phase A, B complete (C not required)  
**Goal:** First-class client libraries for Rust, Python, TypeScript, and Go.

---

## D1. Rust Client (cortex-client)

Thin wrapper over tonic-generated gRPC stubs with convenience methods.

### Crate: `cortex-client`

```rust
pub struct CortexClient {
    inner: proto::cortex_service_client::CortexServiceClient<tonic::transport::Channel>,
}

impl CortexClient {
    pub async fn connect(addr: &str) -> Result<Self> { ... }
    
    // Convenience methods that wrap gRPC calls
    pub async fn store(&self, kind: &str, title: &str) -> NodeBuilder { ... }
    pub async fn get(&self, id: NodeId) -> Result<Option<Node>> { ... }
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> { ... }
    pub async fn hybrid_search(&self, query: &str) -> HybridSearchBuilder { ... }
    pub async fn briefing(&self, agent_id: &str) -> Result<Briefing> { ... }
    pub async fn traverse(&self, start: NodeId) -> TraversalBuilder { ... }
    pub async fn stats(&self) -> Result<Stats> { ... }
    pub async fn health(&self) -> Result<HealthStatus> { ... }
}
```

Published to crates.io as `cortex-client`.

### Tests
- Connect, create node, retrieve, search round-trip
- All builder patterns produce valid gRPC requests
- Connection failure → clear error
- Reconnection on transient failures

---

## D2. Python SDK (cortex-memory)

### Architecture

```
cortex-memory/
├── pyproject.toml
├── src/
│   └── cortex_memory/
│       ├── __init__.py
│       ├── client.py          # gRPC client wrapper
│       ├── embedded.py        # Library mode (via PyO3 bindings)
│       ├── types.py           # Dataclasses for Node, Edge, SearchResult, Briefing
│       ├── testing.py         # Mock server, pytest fixtures
│       └── _proto/            # Generated protobuf code
└── tests/
```

### Client Mode (gRPC)

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

# Store
node = cx.store("decision", "Use FastAPI",
    body="Async + type hints",
    tags=["backend", "python"],
    importance=0.8)

# Search
results = cx.search("backend choices", limit=5)
for r in results:
    print(f"{r.score:.2f} — {r.title}")

# Hybrid search
results = cx.hybrid_search("infrastructure",
    anchors=[agent_id],
    vector_weight=0.7,
    limit=10)

# Briefing
briefing = cx.briefing("my-agent")
print(briefing.text)
print(briefing.sections)  # List of BriefingSection

# Traverse
subgraph = cx.traverse(node_id, depth=3, direction="outgoing")
for node in subgraph.nodes:
    print(f"  {node.title} (depth {subgraph.depth(node.id)})")
```

### Embedded Mode (PyO3)

```python
from cortex_memory import Cortex

# Opens redb file directly — no server needed
cx = Cortex.open("./memory.redb")

# Same API as client mode
cx.store("fact", "Python is great")
results = cx.search("programming languages")
```

Implementation: PyO3 bindings to `cortex-core`. Compiled as a native Python extension. Distributed as wheels (manylinux, macOS, Windows).

### Testing Utilities

```python
# pytest plugin
from cortex_memory.testing import mock_cortex, cortex_fixture

@pytest.fixture
def cx():
    """In-memory Cortex for testing."""
    with mock_cortex() as cx:
        yield cx

def test_agent_memory(cx):
    cx.store("fact", "Test data", body="Details here")
    results = cx.search("test")
    assert len(results) == 1
    assert results[0].title == "Test data"
```

### Distribution
- PyPI: `pip install cortex-memory`
- Wheels for: Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64)
- Requires Python 3.9+

---

## D3. TypeScript SDK (@cortex-memory/client)

### Architecture

```
cortex-ts/
├── package.json
├── tsconfig.json
├── src/
│   ├── index.ts
│   ├── client.ts           # gRPC client
│   ├── types.ts             # TypeScript interfaces
│   └── testing.ts           # Mock client for tests
├── proto/
│   └── cortex.proto         # Copied from main repo
└── tests/
```

### Usage

```typescript
import { Cortex } from '@cortex-memory/client';

const cx = new Cortex('localhost:9090');

// Store
const node = await cx.store({
    kind: 'fact',
    title: 'API rate limit is 1000/min',
    tags: ['api', 'limits'],
    importance: 0.7,
});

// Search
const results = await cx.search('rate limits', { limit: 5 });
results.forEach(r => console.log(`${r.score.toFixed(2)} — ${r.title}`));

// Briefing
const briefing = await cx.briefing('my-agent');
console.log(briefing.text);

// Traverse
const graph = await cx.traverse(nodeId, { depth: 2, direction: 'outgoing' });

// Typed results
interface SearchResult {
    nodeId: string;
    title: string;
    body: string;
    kind: string;
    score: number;
    distance: number;
}
```

### Testing

```typescript
import { MockCortex } from '@cortex-memory/client/testing';

const cx = new MockCortex();
await cx.store({ kind: 'fact', title: 'Test' });
const results = await cx.search('test');
expect(results).toHaveLength(1);
```

### Distribution
- npm: `@cortex-memory/client`
- Uses `@grpc/grpc-js` for gRPC
- Supports Node.js 18+ and Bun
- ESM and CJS dual publish

---

## D4. Go SDK (cortex-go)

### Architecture

```
cortex-go/
├── go.mod
├── cortex.go              # Main client
├── types.go               # Go structs
├── options.go             # Functional options pattern
├── testing.go             # Mock client
├── proto/                 # Generated protobuf code
│   └── cortex/
└── examples/
    └── basic/
```

### Usage

```go
package main

import (
    "fmt"
    cortex "github.com/MikeSquared-Agency/cortex-go"
)

func main() {
    client, err := cortex.Connect("localhost:9090")
    if err != nil { panic(err) }
    defer client.Close()
    
    // Store
    node, err := client.Store("event", "Deployed v2.1",
        cortex.WithBody("Production deployment completed"),
        cortex.WithTags("deploy", "production"),
        cortex.WithImportance(0.7),
    )
    
    // Search
    results, err := client.Search("deployment history", cortex.Limit(10))
    for _, r := range results {
        fmt.Printf("%.2f — %s\n", r.Score, r.Title)
    }
    
    // Briefing
    briefing, err := client.Briefing("ops-agent")
    fmt.Println(briefing.Text)
    
    // Traverse
    graph, err := client.Traverse(nodeID,
        cortex.Depth(3),
        cortex.Direction(cortex.Outgoing),
    )
}
```

### Distribution
- `go get github.com/MikeSquared-Agency/cortex-go`
- Requires Go 1.21+
- Uses `google.golang.org/grpc`

---

## D5. Integration Examples

### LangChain (Python)

```python
from langchain.memory import BaseChatMessageHistory
from cortex_memory import Cortex

class CortexMemory(BaseChatMessageHistory):
    def __init__(self, cortex_url="localhost:9090", agent_id="langchain"):
        self.cx = Cortex(cortex_url)
        self.agent_id = agent_id
    
    def add_message(self, message):
        self.cx.store("event", f"{message.type}: {message.content[:100]}",
            body=message.content,
            source=self.agent_id)
    
    def get_context(self) -> str:
        return self.cx.briefing(self.agent_id).text
    
    def clear(self):
        pass  # Cortex handles decay naturally
```

### CrewAI

```python
from crewai import Agent, Crew
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

researcher = Agent(
    role="Researcher",
    memory=cx.briefing("researcher"),
    # ... 
)
```

### OpenClaw

```toml
# Agent boot sequence pulls briefing from Cortex
# instead of reading flat MEMORY.md/AGENTS.md files
[cortex]
url = "localhost:9090"
agent_id = "kai"
```

---

## Deliverables

1. `cortex-client` Rust crate on crates.io
2. `cortex-memory` Python package on PyPI (gRPC + PyO3 embedded mode)
3. `@cortex-memory/client` TypeScript package on npm
4. `cortex-go` Go module on GitHub
5. Testing utilities for all 4 SDKs (mock clients, fixtures)
6. Integration examples: LangChain, CrewAI, OpenClaw
7. SDK documentation with code examples
