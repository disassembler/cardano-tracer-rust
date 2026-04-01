# hermod-rust

# List available commands
default:
    @just --list

# Build all binaries
build:
    cargo build

# Build release binaries
build-release:
    cargo build --release

# Run all tests
test:
    cargo test

# Run only conformance tests (requires hermod-tracing binaries on PATH)
test-conformance:
    cargo test --test conformance

# Check formatting and compilation
check:
    cargo fmt --check
    cargo check

# Format source code
fmt:
    cargo fmt
    nix fmt

# Run hermod-tracer with a Unix socket at /tmp/hermod.sock
tracer:
    cargo run --bin hermod-tracer -- --config config/hermod-tracer.yaml
