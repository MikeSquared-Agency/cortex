# CLAUDE.md — Cortex Agent Guide

## What is this?

Cortex is an embedded graph memory engine for AI agents. Rust, redb (embedded DB), gRPC + HTTP APIs, vector search (HNSW), auto-linking.

## Repo structure

```
crates/
  cortex-core/     — Storage, graph, vector, auto-linker, types. The library.
  cortex-server/   — gRPC/HTTP server, CLI, config. The binary.
  cortex-client/   — Rust gRPC client crate.
  cortex-proto/    — Protobuf definitions + generated code.
  warren-adapter/  — Optional Warren integration (feature-gated).
sdks/
  typescript/      — @cortex/client npm package
  python/          — cortex-memory PyPI package
  go/              — Go client module
specs/             — Design specs (tagged IMPLEMENTED/DEFERRED)
docs/              — User-facing documentation
examples/          — Integration examples (LangChain, CrewAI, etc.)
```

## Build & test

```bash
cargo test --workspace                          # All 176 tests
cargo test --workspace --no-default-features    # Without warren adapter
cargo build -p cortex-server --release          # Release binary
cargo clippy --workspace                        # Lint
```

## Run

```bash
cortex init                    # Create cortex.toml
cortex serve                   # Start server (gRPC :9090, HTTP :9091)
cortex node create --kind fact --title "..."
cortex search "query"
cortex briefing <agent-id>
cortex shell                   # Interactive REPL
```

## Key types

- `NodeKind` / `Relation` — validated string newtypes (not enums)
- `Cortex::open(path)` — library mode, no server needed
- `Storage` trait — `RedbStorage` implements it
- `IngestAdapter` trait — pluggable event sources
- Config: `cortex.toml` with `#[serde(default)]` on all structs

## Architecture decisions

- **redb** for storage (embedded, ACID, zero-copy mmap)
- **HNSW** for vector search (instant-distance crate)
- **gRPC** (tonic) for production API, **HTTP** (axum) for debug/viz
- **warren-adapter** is optional (`--features warren`), cortex-core has zero network deps
- Auto-linker runs background loop: similarity rules → edges, decay → prune, dedup → merge

## Config

See `cortex.example.toml`. All config structs support partial overrides (serde defaults).

## Feature flags

- `warren` (default) — enables Warren NATS integration
- Without it: standalone graph memory with no network dependencies

## Specs

Check `specs/` — all implemented specs are tagged. Deferred work tracked in GitHub issues.

## Tests

- `crates/cortex-core/` — 128 unit tests (storage, graph, types, filters)
- `crates/cortex-server/` — integration tests
- `sdks/typescript/` — 28 Jest tests
- `sdks/python/` — 30 pytest tests
- `sdks/go/` — 9 Go tests

## Schema evolution — REQUIRED steps

Cortex uses **bincode** (positional binary encoding) for Node/Edge records. Changing field
order, adding fields mid-struct, or removing fields **silently corrupts all existing databases**.

**Any change to `Node`, `NodeData`, `Edge`, or `EdgeProvenance` MUST follow this checklist:**

1. **Bump `CURRENT_SCHEMA_VERSION`** in `crates/cortex-core/src/storage/redb_storage.rs`
2. **Write a migration binary** in `crates/cortex-server/src/bin/` (see `fix_nodes.rs` for a template)
3. **Regenerate golden bytes** so the regression test passes:
   ```bash
   cargo test -p cortex-core generate_golden_node_bytes -- --nocapture
   ```
   Copy the printed byte array into `GOLDEN_NODE_BYTES` in `schema_regression_tests` (same file).
4. **Add fields at the END of the struct** to minimise migration impact.
5. **Test the migration** against a real database before deploying.

The `test_node_schema_golden` test in `redb_storage.rs` will fail immediately if the bincode
format changes without these steps being followed.

## Common pitfalls

- Port 9090 may conflict with existing services — override in `cortex.toml`
- Partial `cortex.toml` is fine — unset fields use defaults
- `cortex-core` must have zero network dependencies — put network stuff in `cortex-server` or adapters
- Node kinds and relations are lowercase strings with hyphens/underscores only
