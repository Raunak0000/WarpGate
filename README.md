# WarpGate

**Ephemeral Multi-Region Cloud VPN Orchestrator**

WarpGate provisions disposable, self-destructing WireGuard VPN gateways on AWS with automated network tuning (Linux TCP BBR), tracker/ad blocking (AdGuard Home), and automated Time-to-Live (TTL) teardown.

---

## ✨ Features

- **⚡ Ephemeral & Self-Destructing**: Automated TTL lifecycle engine terminates AWS infrastructure after expiration—never leave orphaned cloud resources running.
- **🌍 Multi-Region Support**: Deploy to global AWS regions using canonical IDs (`us-east-1`, `eu-central-1`) or friendly aliases (`tokyo`, `frankfurt`, `london`).
- **🔒 Zero-Trust Key Generation**: Curve25519 (X25519) WireGuard keypairs are generated client-side—private keys never leave your machine.
- **📱 Instant Mobile & Laptop Connection**: Renders an in-terminal 2D QR code for instant mobile app scanning, and automatically configures Linux laptops via NetworkManager (`nmcli`) or `wg-quick`.
- **🚀 High-Throughput Kernel Tuning**: Linux TCP BBR congestion control maintains high speeds even under packet loss (up to 4.3x throughput over standard Cubic).
- **🛡️ Built-in Ad & Telemetry Blocking**: Embedded AdGuard Home DNS sinkhole resolves queries locally and filters trackers at the gateway.

---

## 🏗️ Architecture

```
                       USER
                        │
                        ▼
               ┌─────────────────┐
               │    Rust CLI     │  (Generates Curve25519 keys, renders QR, connects host)
               └────────┬────────┘
                        │
                        ▼
               ┌─────────────────┐
               │   Rust Engine   │  (Axum + Tokio async orchestration & TTL timers)
               └────────┬────────┘
                        │
           ┌────────────┴────────────┐
           ▼                         ▼
    ┌─────────────┐           ┌─────────────┐
    │  Terraform  │           │   Ansible   │
    │ (AWS Infra) │           │ (WireGuard) │
    └──────┬──────┘           └──────┬──────┘
           │                         │
           └────────────┬────────────┘
                        ▼
               ┌─────────────────┐
               │   AWS EC2 Node  │  (WireGuard + TCP BBR + AdGuard DNS Sinkhole)
               └────────┬────────┘
                        │
                        ▼
                     INTERNET
```

---

## 📋 Prerequisites

Before deploying live cloud gateways, ensure you have:

- **[Rust & Cargo](https://rustup.rs/)** (1.75+)
- **[Terraform](https://developer.hashicorp.com/terraform/install)** (1.5+)
- **[Ansible](https://docs.ansible.com/ansible/latest/installation_guide/intro_installation.html)**
- **AWS Credentials** configured (`aws configure` or `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`)
- **SSH Key** at `~/.ssh/warpgate_key` (or set `WARPGATE_SSH_PUBLIC_KEY`)

> **Tip**: Want to explore WarpGate without AWS credentials or cloud spend? Use the **[Dry-Run Mode](#-dry-run-mode-no-aws-required)**!

---

## 🚀 Installation & Quick Start

### 1. One-Line Universal Install

Install the standalone `warpgate` binary instantly (Linux / macOS):

```bash
curl -fsSL https://raw.githubusercontent.com/Raunak0000/WarpGate/main/install.sh | bash
```

Or install from source with Cargo:
```bash
cargo install --path rust/crates/warpgate-cli
```

### 2. Verify Your Environment

Run `warpgate` with no arguments to run the system readiness check and view the quick-start guide:

```bash
warpgate
```

```text
============================================================
         WarpGate - Ephemeral Zero-Cost Cloud VPN           
============================================================

System & Environment Readiness Diagnostics:

  [✓] Platform:            linux (x86_64)
  [✓] VPN Connection Mgr:  NetworkManager (nmcli)
  [✓] AWS Cloud Identity:  Active credentials detected via AWS CLI
  [✓] Infrastructure:      Terraform v1.15.9
  [✓] Config Engine:       Ansible playbook detected
  [✓] WarpGate Daemon:     Idle (will auto-spawn on 'warpgate up')
```

### 3. Spin Up an Ephemeral VPN Gateway

```bash
# Provision a gateway in Tokyo with a 30-minute TTL (auto-detects public IP & auto-connects)
warpgate up --region tokyo --ttl 30m
```

Once provisioning completes:
1. The terminal displays a **QR code**—scan it with the [WireGuard mobile app](https://www.wireguard.com/install/) to connect your phone.
2. On Linux laptops, your network connects automatically via NetworkManager (`nmcli`) or `wg-quick`.
3. The background orchestrator daemon is automatically spawned and managed for you.

### 4. Check Connection Status

View the live terminal status dashboard at any time (no deployment ID needed):

```bash
warpgate status
```

### 5. Teardown on Demand

When finished, tear down the infrastructure and disconnect with one command (or simply let the TTL auto-destruct it):

```bash
warpgate down
```

---

## 🧪 Dry-Run Mode (No AWS Required)

To test WarpGate's CLI workflow and simulated lifecycle without an AWS account:

```bash
# Spin up in simulated dry-run mode (detached background session)
WARPGATE_DRY_RUN=1 warpgate up --region tokyo --ttl 5m --no-connect --detach

# Check status
warpgate status

# Clean teardown
WARPGATE_DRY_RUN=1 warpgate down
```

---

## 💻 CLI Reference

```
Usage: warpgate [OPTIONS] [COMMAND]

Commands:
  up           Provision a new ephemeral WireGuard VPN node
  status       Check status of an active deployment (or active node if omitted)
  down         Terminate and destroy a deployment immediately (auto-detects active node)
  sweep        Scan for and sweep orphaned cloud infrastructure and lingering Elastic IPs
  doctor       Run system diagnostics and verify environment readiness
  completions  Generate shell autocompletion scripts (bash, zsh, fish, powershell)
```

### `warpgate up` Options

| Flag | Description | Default |
| :--- | :--- | :--- |
| `-r, --region <REGION>` | Target AWS region or alias (`tokyo`, `frankfurt`, `us-east-1`, etc.) | `us-east-1` |
| `-t, --ttl <DURATION>` | Time-to-Live before automated teardown (`15m`, `30m`, `1h`) | `30m` |
| `--admin-cidr <CIDR>` | Allowed IPv4 CIDR for administrative SSH access | *Auto-detected* |
| `-d, --detach` | Run in detached background mode (non-blocking) | `false` |
| `-o, --output <PATH>` | Save generated client `wg0.conf` configuration to a specific file | Optional |
| `--no-connect` | Skip automatically connecting local laptop to the VPN tunnel | `false` |

---

## 📚 Documentation

For architecture details, benchmarks, and design decisions:

- **[Architecture Deep Dive](docs/ARCHITECTURE.md)**: Layered system design, Axum API contracts, and security controls.
- **[Performance Benchmark Report](docs/BENCHMARK_REPORT.md)**: TCP BBR vs. CUBIC throughput analysis, packet loss tests, and DNS latency.
- **[Demo Walkthrough](docs/DEMO_WALKTHROUGH.md)**: Complete step-by-step lifecycle walkthrough and terminal QR verification.
- **[Project Roadmap](docs/ROADMAP.md)**: Six-week timeline and implementation milestones.

---

## 📄 License

This project is licensed under the [GNU General Public License v3.0](LICENSE).
