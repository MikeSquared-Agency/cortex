//! Rust embedded example: use cortex-core as a library without a server.

use anyhow::Result;
use cortex_core::{Cortex, LibraryConfig, Node};

#[tokio::main]
async fn main() -> Result<()> {
    // Open (or create) a Cortex database
    let cx = Cortex::open("./demo.redb", LibraryConfig::default())?;

    println!("=== Storing knowledge ===");

    let n1 = cx.store(Node::fact("The API uses JWT authentication", 0.8))?;
    println!("Stored: {}", n1.id);

    let n2 = cx.store(Node::decision("Use FastAPI for the backend", 0.9)
        .with_body("Chosen for async support and automatic OpenAPI docs."))?;
    println!("Stored: {}", n2.id);

    let n3 = cx.store(Node::goal("Ship MVP by Q2 2026", 1.0))?;
    println!("Stored: {}", n3.id);

    cx.store(Node::observation("JWT tokens expire after 24 hours", 0.6))?;
    cx.store(Node::fact("Python is used for all ML components", 0.7))?;

    println!("\n=== Searching ===");

    let results = cx.search("authentication", 5)?;
    for r in &results {
        println!("  {:.3}  {}  {}", r.score, r.node.kind, r.node.data.title);
    }

    println!("\n=== Briefing ===");

    let briefing = cx.briefing("demo-agent")?;
    if briefing.trim().is_empty() {
        println!("(no briefing content yet — run the auto-linker first)");
    } else {
        println!("{}", briefing);
    }

    println!("\nDone. Database saved to ./demo.redb");
    Ok(())
}
