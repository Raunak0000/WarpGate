# WarpGate
**Ephemeral Multi-Region Cloud VPN Orchestrator**

WarpGate provisions a temporary, self-destructing secure VPN gateway on AWS, configures a WireGuard tunnel automatically, validates connectivity, and tears down the infrastructure when the Time-to-Live (TTL) expires.

## Getting Started
Please refer to the documentation for details on architecture, planning, and career talking points:
- **[Roadmap](docs/ROADMAP.md)**: The 6-week project timeline and task breakdown.
- **[Architecture](docs/ARCHITECTURE.md)**: Detailed system design and repository structure.

## Quick Start

### 1. Launch the Orchestration Plane
```bash
cargo run --bin warpgate-orchestrator
```
The orchestrator starts on `http://localhost:3000`, handling asynchronous Terraform and Ansible provisioning.

### 2. Connect via the CLI
```bash
# Provision a temporary VPN node with a 30-minute TTL
cargo run --bin warpgate-cli -- up --region us-east-1 --ttl 30m

# Check status of an active deployment
cargo run --bin warpgate-cli -- status <deployment-id>

# Teardown a deployment immediately
cargo run --bin warpgate-cli -- down <deployment-id>
```

When the node is ready, the CLI generates a terminal QR code for instant mobile WireGuard scanning.

Feel free to open an issue or a PR.

## License

This project is licensed under [GNU GPLv3](./LICENSE).