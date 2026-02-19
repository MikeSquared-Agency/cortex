# Rust SDK Reference

## cortex-core (embedded library)

Use `cortex-core` to embed Cortex directly in a Rust application — no server required.

```toml
[dependencies]
cortex-core = "0.1"
```

```rust
use cortex_core::{Cortex, LibraryConfig, Node};

let cx = Cortex::open("./memory.redb", LibraryConfig::default())?;

// Store a node
let node = cx.store(Node::fact("JWT is used for auth", 0.7))?;

// Search
let results = cx.search("authentication", 5)?;

// Briefing
let briefing = cx.briefing("my-agent")?;
```

## cortex-client (gRPC client)

Use `cortex-client` to connect to a running Cortex server.

```toml
[dependencies]
cortex-client = "0.1"
```

```rust
use cortex_client::CortexClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut client = CortexClient::connect("http://localhost:9090").await?;

    let node = client.create_node("fact", "JWT auth", 0.7).await?;
    println!("Created: {}", node.id);

    let results = client.search("authentication", 10).await?;
    for r in results {
        println!("{:.2} {}", r.score, r.title);
    }

    let briefing = client.briefing("my-agent").await?;
    println!("{}", briefing);

    Ok(())
}
```

## Node Constructors

`cortex-core` provides typed constructors for common node kinds:

```rust
Node::fact("title", importance)
Node::decision("title", importance)
Node::event("title", importance)
Node::goal("title", importance)
Node::observation("title", importance)
Node::pattern("title", importance)
```

## LibraryConfig

```rust
let config = LibraryConfig {
    auto_linker: AutoLinkerTomlConfig {
        enabled: true,
        interval_seconds: 60,
        similarity_threshold: 0.75,
        ..Default::default()
    },
    ..Default::default()
};
```
