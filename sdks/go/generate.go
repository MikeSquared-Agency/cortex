//go:build ignore

// generate.go triggers protobuf code generation for the Go SDK.
//
// Run from sdks/go/:
//
//	go generate ./...
//
// Prerequisites:
//
//	go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
//	go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
//	# protoc must be on PATH
//
//go:generate sh scripts/generate_protos.sh
package cortex
