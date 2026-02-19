# gRPC API Reference

The Cortex gRPC API is defined in `crates/cortex-proto/proto/cortex.proto`. The server listens on port 9090 by default.

## Service: CortexService

### CreateNode

```protobuf
rpc CreateNode(CreateNodeRequest) returns (NodeResponse);

message CreateNodeRequest {
  string kind = 1;
  string title = 2;
  string body = 3;
  float importance = 4;
  repeated string tags = 5;
  string source_agent = 6;
  map<string, string> metadata = 7;
}
```

### GetNode

```protobuf
rpc GetNode(GetNodeRequest) returns (NodeResponse);

message GetNodeRequest {
  string id = 1;
}
```

### UpdateNode

```protobuf
rpc UpdateNode(UpdateNodeRequest) returns (NodeResponse);
```

### DeleteNode

```protobuf
rpc DeleteNode(DeleteNodeRequest) returns (DeleteResponse);
```

### ListNodes

```protobuf
rpc ListNodes(ListNodesRequest) returns (ListNodesResponse);

message ListNodesRequest {
  string kind = 1;       // optional filter
  string agent = 2;      // optional filter
  uint32 limit = 3;
  uint32 offset = 4;
}
```

### SearchNodes

```protobuf
rpc SearchNodes(SearchRequest) returns (SearchResponse);

message SearchRequest {
  string query = 1;
  uint32 limit = 2;
  string kind_filter = 3;
}
```

### HybridSearch

```protobuf
rpc HybridSearch(HybridSearchRequest) returns (SearchResponse);

message HybridSearchRequest {
  string query = 1;
  uint32 limit = 2;
  float alpha = 3;
  uint32 graph_hops = 4;
}
```

### CreateEdge

```protobuf
rpc CreateEdge(CreateEdgeRequest) returns (EdgeResponse);

message CreateEdgeRequest {
  string from_id = 1;
  string to_id = 2;
  string relation = 3;
  float weight = 4;
}
```

### GetBriefing

```protobuf
rpc GetBriefing(GetBriefingRequest) returns (BriefingResponse);

message GetBriefingRequest {
  string agent_id = 1;
  uint32 max_tokens = 2;
}

message BriefingResponse {
  string text = 1;
  repeated BriefingSection sections = 2;
}
```

### Traverse

```protobuf
rpc Traverse(TraversalRequest) returns (SubgraphResponse);
```

### FindPaths

```protobuf
rpc FindPaths(PathRequest) returns (PathResponse);
```

## Connecting

### Python

```python
import grpc
from cortex_proto import cortex_pb2, cortex_pb2_grpc

channel = grpc.insecure_channel("localhost:9090")
stub = cortex_pb2_grpc.CortexServiceStub(channel)

response = stub.CreateNode(cortex_pb2.CreateNodeRequest(
    kind="fact",
    title="The API uses JWT auth",
    importance=0.7,
))
```

### Rust

```rust
use cortex_client::CortexClient;

let mut client = CortexClient::connect("http://localhost:9090").await?;
let node = client.create_node("fact", "The API uses JWT auth", 0.7).await?;
```
