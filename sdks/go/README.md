# cortex-go

Go client SDK for the [Cortex](https://github.com/MikeSquared-Agency/cortex) graph memory engine.

## Installation

```bash
go get github.com/MikeSquared-Agency/cortex-go
```

## Quick start

```go
package main

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

## Proto generation

The Go gRPC stubs in `proto/` are pre-generated from `crates/cortex-proto/proto/cortex.proto`.
To regenerate after a proto change:

```bash
# Install generators (one-time)
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest

# Regenerate
go generate ./...
```
