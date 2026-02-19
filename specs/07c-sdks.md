> **Status:** IMPLEMENTED

# Phase 7C: Client SDKs

**Status:** Ready to implement after Phase 7A is merged.  
**Dependencies:** Phase 7A (Core Decoupling) — requires the gRPC API to be stable; Phase 7B (CLI/Library) is optional but provides the mock server used in tests.  
**Weeks:** 7–8  

---

## Overview

Four official client SDKs — Rust, Python, TypeScript, and Go — plus testing utilities (mock server, pytest fixtures). All SDKs are generated from the protobuf definition (`crates/cortex-proto/`) plus hand-written convenience layers. Users who prefer gRPC directly can use the generated code; the SDK layer adds ergonomics.

---

## Repository Layout

```
crates/
  cortex-client/             — Rust SDK (crates.io: cortex-client)

sdks/
  python/                    — Python SDK (PyPI: cortex-memory)
    cortex_memory/
      __init__.py
      client.py
      models.py
      testing.py             — pytest fixtures + mock_cortex context manager
    pyproject.toml
    README.md

  typescript/                — TypeScript SDK (npm: @cortex-memory/client)
    src/
      index.ts
      client.ts
      types.ts
    package.json
    tsconfig.json
    README.md

  go/                        — Go SDK (go get github.com/MikeSquared-Agency/cortex-go)
    cortex.go
    client.go
    types.go
    go.mod
    README.md
```

---

## SDK 1: Rust (cortex-client)

### Crate: `crates/cortex-client/`

Thin wrapper over the tonic-generated gRPC client. Published to crates.io as `cortex-client`.

**`crates/cortex-client/Cargo.toml`:**
```toml
[package]
name = "cortex-client"
version = "0.1.0"
edition = "2021"
description = "Rust client for the Cortex graph memory engine"
license = "MIT"
repository = "https://github.com/MikeSquared-Agency/cortex"

[dependencies]
cortex-proto = { path = "../cortex-proto" }
tonic = "0.12"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v7"] }
```

**`crates/cortex-client/src/lib.rs`:**

```rust
use cortex_proto::cortex::v1::{
    cortex_service_client::CortexServiceClient,
    CreateNodeRequest, GetNodeRequest, SearchRequest,
    GetBriefingRequest, TraverseRequest,
};
use tonic::transport::Channel;

pub use cortex_proto::cortex::v1 as proto;

/// A connected Cortex client.
///
/// # Example
/// ```rust
/// use cortex_client::CortexClient;
///
/// let client = CortexClient::connect("http://localhost:9090").await?;
///
/// let node = client.create_node(CreateNodeRequest {
///     kind: "decision".into(),
///     title: "Use Rust for performance-critical paths".into(),
///     body: "Go for I/O-bound, Rust for CPU-bound.".into(),
///     importance: 0.8,
///     ..Default::default()
/// }).await?;
///
/// let results = client.search("language choices", 5).await?;
/// let briefing = client.briefing("kai").await?;
/// ```
pub struct CortexClient {
    inner: CortexServiceClient<Channel>,
}

impl CortexClient {
    /// Connect to a running Cortex server.
    pub async fn connect(addr: impl Into<String>) -> anyhow::Result<Self> {
        let channel = Channel::from_shared(addr.into())?
            .connect()
            .await?;
        Ok(Self { inner: CortexServiceClient::new(channel) })
    }

    /// Create a node.
    pub async fn create_node(&mut self, req: CreateNodeRequest) -> anyhow::Result<proto::Node> {
        let resp = self.inner.create_node(req).await?;
        Ok(resp.into_inner().node.unwrap())
    }

    /// Get a node by ID.
    pub async fn get_node(&mut self, id: &str) -> anyhow::Result<Option<proto::Node>> {
        let resp = self.inner.get_node(GetNodeRequest { id: id.into() }).await?;
        Ok(resp.into_inner().node)
    }

    /// Semantic search.
    pub async fn search(&mut self, query: &str, limit: u32) -> anyhow::Result<Vec<proto::SearchResult>> {
        let resp = self.inner.search_nodes(SearchRequest {
            query: query.into(),
            limit,
            hybrid: false,
            ..Default::default()
        }).await?;
        Ok(resp.into_inner().results)
    }

    /// Hybrid search (vector + graph proximity).
    pub async fn search_hybrid(&mut self, query: &str, limit: u32) -> anyhow::Result<Vec<proto::SearchResult>> {
        let resp = self.inner.search_nodes(SearchRequest {
            query: query.into(),
            limit,
            hybrid: true,
            ..Default::default()
        }).await?;
        Ok(resp.into_inner().results)
    }

    /// Generate a briefing for an agent.
    pub async fn briefing(&mut self, agent_id: &str) -> anyhow::Result<String> {
        let resp = self.inner.get_briefing(GetBriefingRequest {
            agent_id: agent_id.into(),
            ..Default::default()
        }).await?;
        Ok(resp.into_inner().text)
    }

    /// Graph traversal from a node.
    pub async fn traverse(&mut self, node_id: &str, depth: u32) -> anyhow::Result<proto::Subgraph> {
        let resp = self.inner.traverse(TraverseRequest {
            start_node_id: node_id.into(),
            depth,
            ..Default::default()
        }).await?;
        Ok(resp.into_inner().subgraph.unwrap())
    }
}
```

---

## SDK 2: Python (cortex-memory)

### Package: `sdks/python/`

Generated from protobuf via `grpcio-tools`, with a hand-written convenience layer on top.  
Published to PyPI as `cortex-memory`.

**`sdks/python/pyproject.toml`:**
```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "cortex-memory"
version = "0.1.0"
description = "Graph memory for AI agents"
license = { text = "MIT" }
requires-python = ">=3.9"
dependencies = [
    "grpcio>=1.60",
    "grpcio-tools>=1.60",
    "protobuf>=4.25",
]

[project.optional-dependencies]
dev = ["pytest", "pytest-asyncio"]
```

**`sdks/python/cortex_memory/__init__.py`:**
```python
from .client import Cortex
from .models import Node, Edge, SearchResult, Briefing

__all__ = ["Cortex", "Node", "Edge", "SearchResult", "Briefing"]
```

**`sdks/python/cortex_memory/client.py`:**
```python
from __future__ import annotations
from typing import List, Optional
from . import cortex_pb2, cortex_pb2_grpc
import grpc


class Cortex:
    """
    Cortex client — connects to a running Cortex server.

    # Server mode
    cx = Cortex("localhost:9090")

    # OR library mode (embedded, no server)
    cx = Cortex.open("./memory.redb")
    """

    def __init__(self, addr: str):
        """Connect to a running Cortex gRPC server."""
        self._channel = grpc.insecure_channel(addr)
        self._stub = cortex_pb2_grpc.CortexServiceStub(self._channel)

    @classmethod
    def open(cls, path: str) -> "Cortex":
        """
        Library mode: open an embedded database without a server.
        Requires the cortex binary to be available on PATH.
        Starts a local server in a subprocess on a random port.
        """
        import subprocess, socket, time

        port = _find_free_port()
        proc = subprocess.Popen(
            ["cortex", "serve", "--grpc-addr", f"127.0.0.1:{port}"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        # Wait for server to be ready
        for _ in range(20):
            try:
                grpc.channel_ready_future(
                    grpc.insecure_channel(f"127.0.0.1:{port}")
                ).result(timeout=0.5)
                break
            except grpc.FutureTimeoutError:
                time.sleep(0.25)

        instance = cls(f"127.0.0.1:{port}")
        instance._proc = proc
        return instance

    def store(
        self,
        kind: str,
        title: str,
        *,
        body: str = "",
        tags: List[str] = None,
        importance: float = 0.5,
        metadata: dict = None,
    ) -> str:
        """Store a node. Returns the node ID."""
        req = cortex_pb2.CreateNodeRequest(
            kind=kind,
            title=title,
            body=body or title,
            importance=importance,
            tags=tags or [],
        )
        resp = self._stub.CreateNode(req)
        return resp.node.id

    def search(self, query: str, *, limit: int = 10) -> List["SearchResult"]:
        """Semantic similarity search."""
        resp = self._stub.SearchNodes(
            cortex_pb2.SearchRequest(query=query, limit=limit, hybrid=False)
        )
        return [SearchResult(r) for r in resp.results]

    def search_hybrid(self, query: str, *, limit: int = 10) -> List["SearchResult"]:
        """Hybrid search (vector + graph proximity)."""
        resp = self._stub.SearchNodes(
            cortex_pb2.SearchRequest(query=query, limit=limit, hybrid=True)
        )
        return [SearchResult(r) for r in resp.results]

    def briefing(self, agent_id: str) -> str:
        """Generate a context briefing for an agent. Returns markdown text."""
        resp = self._stub.GetBriefing(
            cortex_pb2.GetBriefingRequest(agent_id=agent_id)
        )
        return resp.text

    def traverse(self, node_id: str, *, depth: int = 2, direction: str = "both") -> dict:
        """Graph traversal. Returns subgraph as dict {nodes: [...], edges: [...]}."""
        resp = self._stub.Traverse(
            cortex_pb2.TraverseRequest(
                start_node_id=node_id, depth=depth
            )
        )
        return {"nodes": list(resp.subgraph.nodes), "edges": list(resp.subgraph.edges)}

    def get_node(self, node_id: str) -> Optional["Node"]:
        """Get a node by ID. Returns None if not found."""
        try:
            resp = self._stub.GetNode(cortex_pb2.GetNodeRequest(id=node_id))
            return Node(resp.node) if resp.node else None
        except grpc.RpcError:
            return None

    def close(self):
        self._channel.close()
        if hasattr(self, "_proc"):
            self._proc.terminate()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


class SearchResult:
    def __init__(self, proto):
        self.score = proto.score
        self.node_id = proto.node_id
        self.title = proto.title
        self.kind = proto.kind

    def __repr__(self):
        return f"SearchResult(score={self.score:.2f}, kind={self.kind!r}, title={self.title!r})"


class Node:
    def __init__(self, proto):
        self.id = proto.id
        self.kind = proto.kind
        self.title = proto.title
        self.body = proto.body
        self.importance = proto.importance
        self.tags = list(proto.tags)
        self.created_at = proto.created_at


def _find_free_port() -> int:
    import socket
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]
```

**Full example:**
```python
from cortex_memory import Cortex

# Server mode
cx = Cortex("localhost:9090")

# Store knowledge
cx.store("decision", "Use FastAPI for the backend",
         body="Chose FastAPI over Flask for async support and type hints",
         tags=["backend", "python"],
         importance=0.8)

# Search
results = cx.search("backend technology choices", limit=5)
for r in results:
    print(f"{r.score:.2f} — {r.title}")

# Get briefing
briefing = cx.briefing("my-agent")
print(briefing)  # Ready-to-inject markdown

# Traverse
subgraph = cx.traverse(results[0].node_id, depth=3, direction="outgoing")
```

---

## SDK 3: TypeScript / Node (`@cortex-memory/client`)

### Package: `sdks/typescript/`

Uses `@grpc/grpc-js` and `@grpc/proto-loader`. Published to npm.

**`sdks/typescript/package.json`:**
```json
{
  "name": "@cortex-memory/client",
  "version": "0.1.0",
  "description": "Graph memory for AI agents",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "license": "MIT",
  "scripts": {
    "build": "tsc",
    "test": "jest"
  },
  "dependencies": {
    "@grpc/grpc-js": "^1.10",
    "@grpc/proto-loader": "^0.7"
  },
  "devDependencies": {
    "typescript": "^5.4",
    "@types/node": "^20",
    "jest": "^29",
    "ts-jest": "^29"
  }
}
```

**`sdks/typescript/src/client.ts`:**
```typescript
import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import path from 'path';

export interface StoreOptions {
  kind: string;
  title: string;
  body?: string;
  tags?: string[];
  importance?: number;
}

export interface SearchResult {
  score: number;
  nodeId: string;
  title: string;
  kind: string;
}

export class Cortex {
  private client: any;

  constructor(addr: string) {
    const packageDefinition = protoLoader.loadSync(
      path.join(__dirname, '../../proto/cortex.proto'),
      { keepCase: true, longs: String, enums: String, defaults: true, oneofs: true }
    );
    const proto = grpc.loadPackageDefinition(packageDefinition) as any;
    this.client = new proto.cortex.v1.CortexService(
      addr,
      grpc.credentials.createInsecure()
    );
  }

  async store(options: StoreOptions): Promise<string> {
    return new Promise((resolve, reject) => {
      this.client.CreateNode({
        kind: options.kind,
        title: options.title,
        body: options.body ?? options.title,
        importance: options.importance ?? 0.5,
        tags: options.tags ?? [],
      }, (err: Error, response: any) => {
        if (err) reject(err);
        else resolve(response.node.id);
      });
    });
  }

  async search(query: string, options: { limit?: number; hybrid?: boolean } = {}): Promise<SearchResult[]> {
    return new Promise((resolve, reject) => {
      this.client.SearchNodes({
        query,
        limit: options.limit ?? 10,
        hybrid: options.hybrid ?? false,
      }, (err: Error, response: any) => {
        if (err) reject(err);
        else resolve(response.results.map((r: any) => ({
          score: r.score,
          nodeId: r.node_id,
          title: r.title,
          kind: r.kind,
        })));
      });
    });
  }

  async briefing(agentId: string): Promise<string> {
    return new Promise((resolve, reject) => {
      this.client.GetBriefing({ agent_id: agentId }, (err: Error, response: any) => {
        if (err) reject(err);
        else resolve(response.text);
      });
    });
  }

  async traverse(nodeId: string, depth: number = 2): Promise<{ nodes: any[]; edges: any[] }> {
    return new Promise((resolve, reject) => {
      this.client.Traverse({ start_node_id: nodeId, depth }, (err: Error, response: any) => {
        if (err) reject(err);
        else resolve({
          nodes: response.subgraph?.nodes ?? [],
          edges: response.subgraph?.edges ?? [],
        });
      });
    });
  }
}
```

**`sdks/typescript/src/index.ts`:**
```typescript
export { Cortex } from './client';
export type { StoreOptions, SearchResult } from './client';
```

**Full example:**
```typescript
import { Cortex } from '@cortex-memory/client';

const cx = new Cortex('localhost:9090');

await cx.store({
  kind: 'fact',
  title: 'API rate limit is 1000/min',
  tags: ['api', 'limits'],
});

const results = await cx.search('rate limits');
const briefing = await cx.briefing('my-agent');
console.log(briefing);
```

---

## SDK 4: Go

### Module: `sdks/go/`

Uses `google.golang.org/grpc` and generated protobuf code.

**`sdks/go/go.mod`:**
```go
module github.com/MikeSquared-Agency/cortex-go

go 1.21

require (
    google.golang.org/grpc v1.62.0
    google.golang.org/protobuf v1.33.0
)
```

**`sdks/go/client.go`:**
```go
package cortex

import (
    "context"
    "fmt"
    "time"

    pb "github.com/MikeSquared-Agency/cortex-go/proto"
    "google.golang.org/grpc"
    "google.golang.org/grpc/credentials/insecure"
)

// Client is a connected Cortex client.
type Client struct {
    conn   *grpc.ClientConn
    svc    pb.CortexServiceClient
}

// Connect creates a new Client connected to the given address.
//
// Example:
//   client, err := cortex.Connect("localhost:9090")
func Connect(addr string) (*Client, error) {
    conn, err := grpc.Dial(addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
    if err != nil {
        return nil, fmt.Errorf("cortex: connect: %w", err)
    }
    return &Client{conn: conn, svc: pb.NewCortexServiceClient(conn)}, nil
}

// Close closes the connection.
func (c *Client) Close() error {
    return c.conn.Close()
}

// Node represents a knowledge node.
type Node struct {
    Kind       string
    Title      string
    Body       string
    Tags       []string
    Importance float32
}

// SearchResult represents a single search hit.
type SearchResult struct {
    Score  float32
    NodeID string
    Title  string
    Kind   string
}

// CreateNode stores a new node. Returns the node ID.
func (c *Client) CreateNode(ctx context.Context, n Node) (string, error) {
    resp, err := c.svc.CreateNode(ctx, &pb.CreateNodeRequest{
        Kind:       n.Kind,
        Title:      n.Title,
        Body:       orDefault(n.Body, n.Title),
        Importance: n.Importance,
        Tags:       n.Tags,
    })
    if err != nil {
        return "", fmt.Errorf("cortex: CreateNode: %w", err)
    }
    return resp.Node.Id, nil
}

// Search performs a semantic similarity search.
func (c *Client) Search(ctx context.Context, query string, limit int) ([]SearchResult, error) {
    resp, err := c.svc.SearchNodes(ctx, &pb.SearchRequest{
        Query: query, Limit: int32(limit), Hybrid: false,
    })
    if err != nil {
        return nil, fmt.Errorf("cortex: Search: %w", err)
    }
    results := make([]SearchResult, len(resp.Results))
    for i, r := range resp.Results {
        results[i] = SearchResult{Score: r.Score, NodeID: r.NodeId, Title: r.Title, Kind: r.Kind}
    }
    return results, nil
}

// Briefing generates a context briefing for an agent.
func (c *Client) Briefing(ctx context.Context, agentID string) (string, error) {
    resp, err := c.svc.GetBriefing(ctx, &pb.GetBriefingRequest{AgentId: agentID})
    if err != nil {
        return "", fmt.Errorf("cortex: Briefing: %w", err)
    }
    return resp.Text, nil
}

func orDefault(s, d string) string {
    if s == "" { return d }
    return s
}
```

**Full example:**
```go
import (
    cortex "github.com/MikeSquared-Agency/cortex-go"
    "context"
    "fmt"
)

func main() {
    client, err := cortex.Connect("localhost:9090")
    if err != nil { panic(err) }
    defer client.Close()

    ctx := context.Background()

    id, _ := client.CreateNode(ctx, cortex.Node{
        Kind:  "event",
        Title: "Deployed v2.1 to production",
        Tags:  []string{"deploy", "production"},
    })
    fmt.Println("Created node:", id)

    results, _ := client.Search(ctx, "deployment history", 10)
    for _, r := range results {
        fmt.Printf("%.2f — %s\n", r.Score, r.Title)
    }

    briefing, _ := client.Briefing(ctx, "ops-agent")
    fmt.Println(briefing)
}
```

---

## Testing Utilities

### Mock Server

The `cortex mock-server` CLI command (part of Phase 7B) starts a server that returns canned data. For SDK tests, we also provide language-specific test utilities.

#### Python: `cortex_memory.testing`

**`sdks/python/cortex_memory/testing.py`:**
```python
from contextlib import contextmanager
from unittest.mock import MagicMock
from typing import Generator
from . import Cortex


@contextmanager
def mock_cortex() -> Generator[Cortex, None, None]:
    """
    Context manager that returns a Cortex client backed by an in-memory mock.
    No server required. Suitable for unit tests.

    Usage:
        from cortex_memory.testing import mock_cortex

        def test_my_agent():
            with mock_cortex() as cx:
                cx.store("fact", "test data")
                results = cx.search("test")
                assert len(results) == 1
    """
    cx = MockCortex()
    yield cx


class MockCortex:
    """In-memory Cortex implementation for testing."""

    def __init__(self):
        self._nodes = {}
        self._call_log = []

    def store(self, kind: str, title: str, *, body: str = "", tags=None,
              importance: float = 0.5, metadata: dict = None) -> str:
        import uuid
        node_id = str(uuid.uuid4())
        self._nodes[node_id] = {
            "id": node_id, "kind": kind, "title": title, "body": body or title,
            "tags": tags or [], "importance": importance,
        }
        self._call_log.append(("store", kind, title))
        return node_id

    def search(self, query: str, *, limit: int = 10):
        from . import SearchResult
        # Simple title-contains match for testing
        matches = [
            MagicMock(score=0.9, node_id=nid, title=n["title"], kind=n["kind"])
            for nid, n in self._nodes.items()
            if query.lower() in n["title"].lower()
        ]
        return matches[:limit]

    def briefing(self, agent_id: str) -> str:
        return f"[Mock briefing for {agent_id}]"

    def get_node(self, node_id: str):
        return self._nodes.get(node_id)

    def traverse(self, node_id: str, *, depth: int = 2, direction: str = "both"):
        return {"nodes": [], "edges": []}

    def assert_stored(self, kind: str, title: str):
        """Assert that a node was stored with the given kind and title."""
        for entry in self._call_log:
            if entry[0] == "store" and entry[1] == kind and entry[2] == title:
                return
        raise AssertionError(f"Expected store({kind!r}, {title!r}) but it was not called")
```

**pytest fixture:**
```python
import pytest
from cortex_memory.testing import mock_cortex

@pytest.fixture
def cortex():
    with mock_cortex() as cx:
        yield cx

def test_my_agent(cortex):
    cortex.store("fact", "test data")
    results = cortex.search("test")
    assert len(results) == 1
    assert results[0].kind == "fact"

def test_store_and_retrieve(cortex):
    node_id = cortex.store("decision", "Use FastAPI",
                            body="Async support", importance=0.8)
    node = cortex.get_node(node_id)
    assert node["title"] == "Use FastAPI"

def test_briefing(cortex):
    briefing = cortex.briefing("test-agent")
    assert "test-agent" in briefing
```

#### TypeScript: Jest helpers

**`sdks/typescript/src/testing.ts`:**
```typescript
export class MockCortex {
  private nodes: Map<string, any> = new Map();
  private callLog: Array<{ method: string; args: any[] }> = [];

  async store(options: any): Promise<string> {
    const id = crypto.randomUUID();
    this.nodes.set(id, { id, ...options });
    this.callLog.push({ method: 'store', args: [options] });
    return id;
  }

  async search(query: string): Promise<any[]> {
    return Array.from(this.nodes.values())
      .filter(n => n.title?.toLowerCase().includes(query.toLowerCase()))
      .map(n => ({ score: 0.9, nodeId: n.id, title: n.title, kind: n.kind }));
  }

  async briefing(agentId: string): Promise<string> {
    return `[Mock briefing for ${agentId}]`;
  }

  assertStored(kind: string, title: string): void {
    const found = this.callLog.some(
      e => e.method === 'store' && e.args[0].kind === kind && e.args[0].title === title
    );
    if (!found) throw new Error(`Expected store({kind: ${kind}, title: ${title}}) was not called`);
  }
}
```

---

## Definition of Done

- [ ] `cortex-client` crate compiles and connects to a running Cortex server
- [ ] `CortexClient::connect("http://localhost:9090").await` succeeds
- [ ] `client.create_node(...)`, `client.search(...)`, `client.briefing(...)`, `client.traverse(...)` all work end-to-end
- [ ] `pip install cortex-memory` installs successfully (or `pip install -e .` locally)
- [ ] `from cortex_memory import Cortex; cx = Cortex("localhost:9090")` works
- [ ] `cx.store("fact", "test", importance=0.7)` returns a node ID string
- [ ] `cx.search("test", limit=5)` returns a list of `SearchResult` objects with `.score`, `.title`, `.kind`
- [ ] `cx.briefing("my-agent")` returns a markdown string
- [ ] `cx.traverse(node_id, depth=2)` returns `{"nodes": [...], "edges": [...]}`
- [ ] `Cortex.open("./memory.redb")` starts an embedded server and returns a working client
- [ ] Python `mock_cortex()` context manager works without a running server
- [ ] `pytest` fixture using `mock_cortex` passes
- [ ] `npm install @cortex-memory/client` installs successfully (or local link)
- [ ] TypeScript `new Cortex("localhost:9090")` connects
- [ ] `await cx.store({kind: "fact", title: "test"})` returns a string ID
- [ ] `await cx.search("test")` returns typed results
- [ ] `await cx.briefing("my-agent")` returns a string
- [ ] `MockCortex` TypeScript class works in Jest tests
- [ ] `go get github.com/MikeSquared-Agency/cortex-go` compiles
- [ ] `cortex.Connect("localhost:9090")` works in Go
- [ ] `client.CreateNode(ctx, cortex.Node{Kind: "event", Title: "..."})` returns an ID
- [ ] `client.Search(ctx, "query", 10)` returns `[]SearchResult`
- [ ] `client.Briefing(ctx, "agent")` returns a string
- [ ] All SDK examples in this spec compile and run against a live server
- [ ] `cargo test -p cortex-client` passes
