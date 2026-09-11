# WarpGate Architecture

## System Overview

WarpGate relies on three distinct layers to provide an ephemeral, on-demand VPN service:

1. **Control Plane (Spring Boot / Java 21)**
   - Manages deployments, users, status tracking, and TTL lifecycles.
   - Provides a REST API (`/api/deployments`) and stores metadata in PostgreSQL.

2. **Orchestration Plane (Rust)**
   - Executes the actual deployment workflow using asynchronous background jobs (Tokio).
   - Manages local processes for Terraform and Ansible, acting as the bridge between the API and the infrastructure.

3. **Data Plane (WireGuard + Ubuntu + AWS)**
   - The physical network layer that encrypts and carries the user's internet traffic.
   - Enhanced with BBR kernel tuning and AdGuard Home for DNS sinkholing.

## Diagram

```text
                         USER
                          │
                          ▼
                 ┌─────────────────┐
                 │   Rust CLI      │
                 └────────┬────────┘
                          │
                          ▼
                 ┌─────────────────┐
                 │  Spring Boot    │
                 │  Control Plane  │
                 └────────┬────────┘
                          │
                          ▼
                 ┌─────────────────────┐
                 │ Rust Orchestrator   │
                 │ (Terraform/Ansible) │
                 └────────┬────────────┘
                          │
             ┌────────────┴────────────┐
             ▼                         ▼
      ┌─────────────┐           ┌─────────────┐
      │ Terraform   │           │ Ansible     │
      │ (AWS Infra) │           │ (Linux/VPN) │
      └──────┬──────┘           └──────┬──────┘
             │                         │
             └────────────┬────────────┘
                          ▼
                 ┌─────────────────┐
                 │   AWS EC2       │
                 │   (WireGuard)   │
                 └────────┬────────┘
                          │
                          ▼
                       INTERNET
```

## Repository Structure

```text
WarpGate/
├── backend/
│   └── springboot/
│       ├── src/
│       └── pom.xml
│
├── rust/
│   ├── Cargo.toml
│   └── crates/
│       ├── warpgate-cli/
│       ├── warpgate-orchestrator/
│       └── warpgate-common/
│
├── infra/
│   ├── terraform/
│   │   ├── main.tf
│   │   └── modules/
│   │       └── vpn_node/
│   │
│   └── ansible/
│       ├── site.yml
│       ├── inventory/
│       └── roles/
│           ├── base/
│           ├── kernel_tuning/
│           ├── networking/
│           ├── wireguard/
│           └── watchdog/
│
├── tests/
│   ├── terratest/
│   └── rust/
│
├── docs/
│   ├── ARCHITECTURE.md
│   ├── ROADMAP.md
│   └── CAREER.md
│
└── README.md
```
