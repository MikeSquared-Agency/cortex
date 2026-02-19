# Installation

## cargo (recommended)

```bash
cargo install cortex-memory
```

This installs the `cortex` binary. Requires Rust 1.75+.

## Docker

```bash
docker run -d \
  -p 9090:9090 \
  -p 9091:9091 \
  -v $(pwd)/data:/data \
  mikesquared/cortex:latest
```

Ports:
- `9090` — gRPC API
- `9091` — HTTP API + graph visualiser

Data is persisted to `/data` inside the container. Mount a volume to keep data across restarts.

## Build from source

```bash
git clone https://github.com/MikeSquared-Agency/cortex
cd cortex
cargo build --release -p cortex-server
./target/release/cortex serve
```

## System requirements

- Linux x86_64, ARM64, or macOS (Apple Silicon and Intel)
- 256 MB RAM minimum (1 GB recommended for large graphs)
- No external database required
