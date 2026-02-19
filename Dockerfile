FROM rust:1.82-slim AS builder

RUN apt-get update && apt-get install -y protobuf-compiler pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release --bin cortex

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/cortex /usr/local/bin/cortex

ENV CORTEX_DATA_DIR=/data
VOLUME /data

EXPOSE 9090 9091

HEALTHCHECK --interval=30s --timeout=10s --retries=3 --start-period=30s \
    CMD curl -f http://localhost:9091/health || exit 1

ENTRYPOINT ["cortex"]
CMD ["serve"]
