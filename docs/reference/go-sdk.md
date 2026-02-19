# Go SDK Reference

## Install

```bash
go get github.com/MikeSquared-Agency/cortex/sdks/go
```

## Quick Start

```go
package main

import (
    "context"
    "fmt"
    "log"

    cortex "github.com/MikeSquared-Agency/cortex/sdks/go"
)

func main() {
    client, err := cortex.Connect("localhost:9090")
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    ctx := context.Background()

    // Store a node
    id, err := client.CreateNode(ctx, cortex.Node{
        Kind:       "fact",
        Title:      "The API uses JWT authentication",
        Importance: 0.8,
    })
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println("Created:", id)

    // Search
    results, err := client.Search(ctx, "authentication", 5)
    if err != nil {
        log.Fatal(err)
    }
    for _, r := range results {
        fmt.Printf("%.2f  %s\n", r.Score, r.Title)
    }

    // Briefing
    briefing, err := client.GetBriefing(ctx, "my-agent")
    if err != nil {
        log.Fatal(err)
    }
    fmt.Println(briefing)
}
```

## cortex.Connect(addr) (*Client, error)

Create a new connected client.

- `addr` — gRPC address, e.g. `"localhost:9090"`

## client.Close() error

Release the gRPC connection. Call with `defer`.

## client.CreateNode(ctx, Node) (string, error)

Store a new knowledge node. Returns the node ID.

```go
type Node struct {
    Kind        string
    Title       string
    Body        string
    Importance  float32     // 0.0–1.0
    Tags        []string
    SourceAgent string
    Metadata    map[string]string
}
```

## client.GetNode(ctx, id) (*NodeResponse, error)

Get a node by ID.

## client.Search(ctx, query, limit) ([]SearchResult, error)

Search nodes semantically.

```go
type SearchResult struct {
    ID         string
    Title      string
    Body       string
    Kind       string
    Score      float32
    Importance float32
}
```

## client.GetBriefing(ctx, agentID) (string, error)

Get a context briefing for an agent.

## client.CreateEdge(ctx, fromID, toID, relation string, weight float32) (string, error)

Create an edge between two nodes.

## client.DeleteNode(ctx, id) error

Delete a node.

## Module Path

```
github.com/MikeSquared-Agency/cortex/sdks/go
```

Source lives in `sdks/go/` in the repository.
