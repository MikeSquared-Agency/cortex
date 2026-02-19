# Contributing to Cortex

Thank you for your interest in contributing!

## Getting Started

1. Fork and clone the repository
2. Install Rust: https://rustup.rs
3. Build: `cargo build --workspace`
4. Test: `cargo test --workspace`

## Development Setup

```bash
# Run all tests
cargo test --workspace

# Run the server locally
cargo run -p cortex-server -- serve

# Run benchmarks
cargo bench -p cortex-core

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

## Project Structure

```
crates/
  cortex-core/     Core library (types, storage, graph, linker, briefing, vector)
  cortex-server/   Server binary + CLI (gRPC, HTTP, all subcommands)
  cortex-proto/    Protobuf definitions
  cortex-client/   Rust client SDK
  warren-adapter/  Warren NATS adapter (internal)

sdks/
  python/          Python SDK (cortex-memory on PyPI)
  typescript/      TypeScript SDK (@cortex-memory/client on npm)
  go/              Go SDK

specs/             Phase specifications
docs/              Documentation site
examples/          Runnable example projects
```

## Contribution Guidelines

- **Code style:** `cargo fmt --all` before committing. CI enforces this.
- **Tests:** Add tests for new functionality. `cargo test --workspace` must pass.
- **Clippy:** `cargo clippy --workspace -- -D warnings` must be clean.
- **Docs:** Update relevant docs in `docs/` for user-facing changes.
- **Specs:** If implementing a new phase, read the spec in `specs/` first.

## Pull Requests

- One PR per feature/fix
- Include a clear description of what changed and why
- Reference the relevant spec if applicable (e.g. "Implements Phase 7A")
- Keep PRs focused — no bundling unrelated changes

## Issues

Use GitHub Issues for bugs and feature requests. For questions, use GitHub Discussions.

## Releasing

Releases are tagged with `vX.Y.Z`. The CI pipeline publishes to crates.io and Docker Hub automatically on tag push.
