#!/usr/bin/env bash
# Generate Go gRPC stubs from the Cortex proto definition.
#
# Usage (from sdks/go/):
#   go generate ./...
# or:
#   ./scripts/generate_protos.sh
#
# Prerequisites:
#   go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
#   go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
#   protoc must be on PATH

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GO_SDK_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(dirname "$(dirname "$GO_SDK_DIR")")"
PROTO_FILE="$REPO_ROOT/crates/cortex-proto/proto/cortex.proto"
PROTO_DIR="$(dirname "$PROTO_FILE")"
OUT_DIR="$GO_SDK_DIR/proto"

mkdir -p "$OUT_DIR"

echo "Generating from: $PROTO_FILE"
echo "Output dir:      $OUT_DIR"

GO_PKG="github.com/MikeSquared-Agency/cortex/sdks/go/proto"

protoc \
    -I"$PROTO_DIR" \
    --go_out="$OUT_DIR" \
    --go_opt=paths=source_relative \
    --go_opt=Mcortex.proto="$GO_PKG" \
    --go-grpc_out="$OUT_DIR" \
    --go-grpc_opt=paths=source_relative \
    --go-grpc_opt=Mcortex.proto="$GO_PKG" \
    "$PROTO_FILE"

echo "Done. Generated proto stubs in $OUT_DIR/"
