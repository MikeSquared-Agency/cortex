# Why We Built Our Own Graph Memory Engine

*Published: February 2026*

Every AI agent has a memory problem.

The standard solutions — append to a text file, bolt on a vector database, run a separate graph database — all share the same fundamental flaw: they treat memory as an afterthought. The result is agents that forget things they should remember, repeat themselves, and fail to connect the dots.

We needed something better. So we built Cortex.

## The Problem with Vector Databases

Vector databases are great at one thing: "give me things similar to this." But agent memory isn't just about similarity. It's about *relationships*.

If your agent decides to use FastAPI based on a fact about async Python, that causal relationship matters. If two pieces of knowledge contradict each other, that matters. If a pattern has been observed three times, that matters more than a pattern observed once.

Vector databases give you a haystack. Cortex gives you a map.

## What Cortex Does Differently

**Auto-linking.** When you store two related facts, Cortex automatically discovers the relationship via embedding similarity and creates an edge between them. You don't have to manually wire up your knowledge graph — it wires itself.

**Decay.** Knowledge ages. An observation from six months ago is less reliable than one from yesterday. Cortex models this with edge decay: relationships weaken over time unless reinforced by access. Your agent's "working memory" naturally surfaces recent, relevant knowledge.

**Contradiction detection.** When new knowledge conflicts with existing knowledge, Cortex detects the contradiction and tags it as a `contradicts` edge. Your agent can then reason about conflicting information rather than silently ignoring it.

**Briefings.** Instead of dumping your agent's entire knowledge graph into context, Cortex generates targeted briefings: "here's what you need to know for this task." Graph traversal + hybrid search + configurable sections.

## The SQLite Model

We're not building a cloud service. We're building a library.

SQLite didn't win because it was the best database. It won because you could embed it in your app with a single file. No server to manage. No connection strings. No operational overhead.

That's Cortex. Single file. Single binary. No dependencies. Embed it in your Python agent with four lines of code:

```python
from cortex_memory import Cortex
cx = Cortex("localhost:9090")
cx.store("decision", "Use FastAPI", importance=0.8)
print(cx.briefing("my-agent"))
```

Or use it directly in Rust — no server process at all:

```rust
let cx = Cortex::open("./memory.redb", LibraryConfig::default())?;
cx.store(Node::fact("Use FastAPI for the backend", 0.8))?;
```

## Why We Wrote It in Rust

Rust gave us three things we couldn't get elsewhere:

1. **Performance** — HNSW vector search and graph traversal are fast. Embedding generation is CPU-bound but runs locally with no API calls.
2. **Correctness** — The borrow checker caught a class of concurrency bugs during development that would have been subtle race conditions in Go or Python.
3. **Embeddability** — A Rust library can be called from Python, Go, Node, and Ruby via FFI/bindings. A Python library cannot.

The compile times are worth it.

## What's Next

Cortex is open source, MIT licensed, and ready for production. The graph visualiser is live at `/viz`. Import adapters for Obsidian and Notion are planned. A WASM plugin system is in the specs.

**Try it:** `cargo install cortex-memory`
**Docs:** [docs](../getting-started/quickstart.md)
**GitHub:** https://github.com/MikeSquared-Agency/cortex
