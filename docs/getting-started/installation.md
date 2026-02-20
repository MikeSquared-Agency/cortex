# Installation

## Install Script (recommended)

```bash
curl -sSf https://raw.githubusercontent.com/MikeSquared-Agency/cortex/main/install.sh | sh
```

The script detects your OS and architecture, downloads the correct pre-built binary, and places it in `~/.local/bin/` (or `/usr/local/bin/` if run as root).

Supported platforms:
- Linux x86_64, ARM64
- macOS x86_64, ARM64 (Apple Silicon)

No Rust toolchain, no `protoc`, no system dependencies required.

## Manual Download

Download a pre-built binary from the [latest release](https://github.com/MikeSquared-Agency/cortex/releases/latest):

| Platform | Architecture | File |
|----------|-------------|------|
| Linux    | x86_64      | `cortex-linux-x86_64.tar.gz` |
| Linux    | arm64       | `cortex-linux-arm64.tar.gz` |
| macOS    | x86_64      | `cortex-macos-x86_64.tar.gz` |
| macOS    | arm64 (M1+) | `cortex-macos-arm64.tar.gz` |

Extract and place the `cortex` binary somewhere on your `$PATH`.

## Cargo

```bash
cargo install cortex-memory
```

Requires Rust 1.75+ and `protoc`.

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

## Build from Source

```bash
git clone https://github.com/MikeSquared-Agency/cortex
cd cortex
cargo build --release -p cortex-server
./target/release/cortex serve
```

## System Requirements

- Linux x86_64, ARM64, or macOS (Apple Silicon and Intel)
- 256 MB RAM minimum (1 GB recommended for large graphs)
- No external database required
