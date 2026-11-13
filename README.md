# WarpGate

Ephemeral multi-region WireGuard VPN on AWS.

WarpGate provisions on-demand WireGuard VPN gateways on AWS with Linux TCP BBR congestion control, AdGuard Home DNS sinkholing, and automated time-to-live (TTL) teardown.

---

## Features

- **Automated teardown**: Gateways automatically destroy themselves after a configurable TTL (for example, 30m or 1h) to prevent unwanted cloud costs.
- **Multi-region**: Deploy to AWS regions using region codes (`us-east-1`, `eu-central-1`) or short aliases (`tokyo`, `frankfurt`, `london`).
- **Client-side keys**: Curve25519 WireGuard keypairs are generated locally on your machine. Private keys never leave your device.
- **Client connection**: Generates a terminal QR code for the WireGuard mobile app, or connects your Linux machine directly via NetworkManager or `wg-quick`.
- **TCP BBR**: Enables Linux TCP BBR congestion control on the gateway for improved throughput under packet loss.
- **Ad and tracker blocking**: Runs an embedded AdGuard Home instance on the gateway to filter ads and trackers locally.

---

## Prerequisites

- **[Rust](https://rustup.rs/)** (1.75+)
- **[Terraform](https://developer.hashicorp.com/terraform/install)** (1.5+)
- **[Ansible](https://docs.ansible.com/ansible/latest/installation_guide/intro_installation.html)**
- **AWS credentials** configured (`aws configure` or `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`)
- **SSH key** at `~/.ssh/warpgate_key` (or set `WARPGATE_SSH_PUBLIC_KEY`)

To test without AWS credentials, see [Dry-Run Mode](#dry-run-mode).

---

## Installation

### Shell installer (Linux and macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/Raunak0000/WarpGate/main/install.sh | bash
```

### From source

```bash
cargo install --path rust/crates/warpgate-cli
```

---

## Quick Start

### 1. Check your environment

Run `warpgate` with no arguments to run the system check:

```bash
warpgate
```

```text
WarpGate - Ephemeral Cloud VPN

System and environment diagnostics:
  [OK] Platform:            linux (x86_64)
  [OK] VPN Connection Mgr:  NetworkManager (nmcli)
  [OK] AWS Cloud Identity:  Active credentials detected via AWS CLI
  [OK] Infrastructure:      Terraform v1.15.9
  [OK] Config Engine:       Ansible playbook detected
  [OK] WarpGate Daemon:     Idle (auto-spawns on 'warpgate up')
```

### 2. Start a VPN gateway

```bash
# Deploy a gateway in Tokyo with a 30-minute TTL
warpgate up --region tokyo --ttl 30m
```

When ready:
1. Scan the terminal QR code with the WireGuard mobile app to connect a phone.
2. On Linux, your connection is configured automatically.
3. The background daemon handles teardown when the TTL expires.

### 3. Check status

```bash
warpgate status
```

### 4. Tear down

```bash
warpgate down
```

---

## Dry-Run Mode

You can test the CLI without an AWS account or cloud infrastructure:

```bash
# Run in dry-run mode
WARPGATE_DRY_RUN=1 warpgate up --region tokyo --ttl 5m --no-connect --detach

# Check status
warpgate status

# Tear down
WARPGATE_DRY_RUN=1 warpgate down
```

---

## CLI Reference

```
Usage: warpgate [OPTIONS] [COMMAND]

Commands:
  up           Provision a new ephemeral WireGuard VPN node
  status       Check status of an active deployment
  down         Terminate and destroy an active deployment
  sweep        Scan for and clean up orphaned cloud resources
  doctor       Run system diagnostics and verify environment readiness
  completions  Generate shell autocompletions (bash, zsh, fish, powershell)
```

### `warpgate up` flags

| Flag | Description | Default |
| :--- | :--- | :--- |
| `-r, --region <REGION>` | Target AWS region or alias (`tokyo`, `frankfurt`, `us-east-1`) | `us-east-1` |
| `-t, --ttl <DURATION>` | Time-to-Live before automated teardown (`15m`, `30m`, `1h`) | `30m` |
| `--admin-cidr <CIDR>` | Allowed IPv4 CIDR for SSH access | Auto-detected |
| `-d, --detach` | Run in background mode without blocking | `false` |
| `-o, --output <PATH>` | Save generated client `wg0.conf` to a file | Optional |
| `--no-connect` | Skip connecting the local machine to the VPN | `false` |

---

## Documentation

- [Architecture](docs/ARCHITECTURE.md): System design, layered components, and API spec.
- [Benchmark Report](docs/BENCHMARK_REPORT.md): TCP BBR vs CUBIC throughput tests and DNS latency.
- [Walkthrough](docs/DEMO_WALKTHROUGH.md): Step-by-step lifecycle walkthrough and verification.
- [Roadmap](docs/ROADMAP.md): Project timeline and milestones.

---

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).
