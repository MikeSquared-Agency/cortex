# Changelog

All notable changes to Cortex are documented in this file.

## [0.2.0] - 2026-03-14

### Added
- **Mutation Hooks** — `MutationHook` trait + `HookRegistry` for node/edge write callbacks. Register hooks to be notified on every create, update, or delete.
- **SSE Event Stream** — `GET /events/stream` with optional `?events=` filter for real-time graph change notifications. Supports `node.created`, `node.updated`, `node.deleted`, `edge.created`, `edge.updated`, `edge.deleted`.
- **Query DSL** — String filter expressions compiled to `NodeFilter`: `kind:decision AND importance>0.7`, `(kind:fact OR kind:pattern) AND tags:architecture`.
- **Schema Validation** — Per-kind metadata schemas in `[schemas.*]` config. Define required fields, types, ranges, and allowed values. Validated at write time via the write gate.

### Changed
- Write gate now runs 4 checks: substance, specificity, conflict, schema.
- HTTP write endpoints now fire mutation hooks (previously gRPC-only). This means SSE events are emitted for all write paths.
- Schema validation is enforced in both HTTP and gRPC write handlers.
- `create_node` and `patch_node` HTTP endpoints now accept a `metadata` field.
- Panicking hooks are isolated via `catch_unwind` to prevent one bad hook from crashing the write path.

### Fixed
- HTTP-created nodes now fire mutation hooks (were silently skipped before).
- Schema validation was defined but not wired into any server write path.

## [0.1.0]

Initial release: embedded graph memory with redb storage, HNSW vector search, auto-linking, briefings, hybrid search, prompt versioning, gRPC + HTTP APIs.
