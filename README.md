# NATS Forge (Experimental)
A Rust tool for generating and managing NATS server configurations and topologies.

- Decentralized JWT support
- NSC wrapper for managing NATS operators, accounts, and users
- Supports hub-leaf and other complex topologies
- JetStream configuration

## Prerequisites

- Rust toolchain
- NSC (NATS Server Configuration) installed and in PATH
- NATS Server installed for running the generated configurations

## Building

```bash
# Build release version
cargo build --release

# Binary will be available at
./target/release/natsforge
```

## Usage

```bash
# Basic usage
./target/release/natsforge --config <path-to-config.json>

# Example configurations
./target/release/natsforge --config examples/microservice-mesh.json
./target/release/natsforge --config examples/multi-region-hub-leaf.json

# The tool will generate:
# - Operator and account JWTs
# - User credentials
# - Server configurations
```
