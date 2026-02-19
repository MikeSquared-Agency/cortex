#!/usr/bin/env bash
# Generate Python gRPC stubs from the Cortex proto definition.
#
# Usage (from the sdks/python/ directory):
#   pip install grpcio-tools
#   ./scripts/generate_protos.sh
#
# The generated cortex_pb2.py and cortex_pb2_grpc.py are written into
# cortex_memory/ and should be committed to the repository.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(dirname "$(dirname "$SDK_DIR")")"
PROTO_FILE="$REPO_ROOT/crates/cortex-proto/proto/cortex.proto"
PROTO_DIR="$(dirname "$PROTO_FILE")"
OUT_DIR="$SDK_DIR/cortex_memory"

if [[ ! -f "$PROTO_FILE" ]]; then
    echo "ERROR: proto file not found: $PROTO_FILE" >&2
    exit 1
fi

echo "Generating from: $PROTO_FILE"
echo "Output dir:      $OUT_DIR"

PYTHON="${PYTHON:-python3}"
$PYTHON -m grpc_tools.protoc \
    -I"$PROTO_DIR" \
    -I"$($PYTHON -c 'import grpc_tools; import os; print(os.path.dirname(grpc_tools.__file__))')" \
    --python_out="$OUT_DIR" \
    --grpc_python_out="$OUT_DIR" \
    "$PROTO_FILE"

# Fix the import in cortex_pb2_grpc.py (grpcio-tools generates a bare import)
sed -i 's/^import cortex_pb2 /from . import cortex_pb2 /' "$OUT_DIR/cortex_pb2_grpc.py"

echo "Done. Generated:"
echo "  $OUT_DIR/cortex_pb2.py"
echo "  $OUT_DIR/cortex_pb2_grpc.py"
