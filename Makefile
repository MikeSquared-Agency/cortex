.PHONY: help build test run docker-build docker-up docker-down clean

help:
	@echo "Cortex - Graph Memory Engine"
	@echo ""
	@echo "Available targets:"
	@echo "  build         - Build all crates in release mode"
	@echo "  test          - Run all tests"
	@echo "  run           - Run cortex-server locally"
	@echo "  docker-build  - Build Docker image"
	@echo "  docker-up     - Start Docker Compose stack"
	@echo "  docker-down   - Stop Docker Compose stack"
	@echo "  clean         - Clean build artifacts"

build:
	cargo build --release

test:
	cargo test --all

run:
	cargo run --bin cortex-server

docker-build:
	docker build -t cortex:latest .

docker-up:
	docker-compose up -d

docker-down:
	docker-compose down

clean:
	cargo clean
	rm -rf data/

check:
	cargo clippy --all-targets --all-features

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check
