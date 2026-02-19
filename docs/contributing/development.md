# Development Guide

## Setup

```bash
git clone https://github.com/MikeSquared-Agency/cortex
cd cortex
cargo build --workspace
```

## Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p cortex-core

# With output
cargo test --workspace -- --nocapture

# Ignored tests (require model download ~100MB)
cargo test --workspace -- --ignored
```

## Running the Server

```bash
cargo run -p cortex-server -- serve
# or
cargo run -p cortex-server -- serve --config cortex.toml.example
```

## Running Benchmarks

```bash
cargo bench -p cortex-core
```

Results are in `target/criterion/`.

## Code Quality

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --workspace -- -D warnings

# Check (no compilation)
cargo check --workspace
```

CI enforces all three. Run them before opening a PR.

## Proto Generation

```bash
# Regenerate Rust code from .proto files
cargo build -p cortex-proto
```

The build script handles protoc automatically via `tonic-build`.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `warren` | Enable Warren NATS adapter (default) |

Build without Warren:
```bash
cargo build -p cortex-server --no-default-features
```

## Crate Dependencies

```
cortex-proto  ←  cortex-core
                      ↑
cortex-client       cortex-server
                warren-adapter (optional)
```

cortex-core has no dependency on the server or client. Keep it that way.
