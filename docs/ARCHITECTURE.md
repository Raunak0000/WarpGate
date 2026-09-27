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

## Orchestrator API Specification (`warpgate-orchestrator`)

The Rust Orchestrator runs an asynchronous HTTP service (Axum) on port `3000`, coordinating the Terraform and Ansible subprocesses.

### 1. `POST /api/deployments`
Triggers an asynchronous provisioning workflow. Returns `202 Accepted` immediately while execution proceeds in a background Tokio task.

- **Request Body**:
  ```json
  {
    "deployment_id": "warp-01",
    "region": "us-east-1",
    "ttl_minutes": 30,
    "client_public_key": "xI1sB28L/a8k5u1e8w1V9/z6y3u3u5l2p5x3h4v4wHE=",
    "admin_cidr": "0.0.0.0/0"
  }
  ```
- **Response** (`202 Accepted`):
  ```json
  {
    "deployment_id": "warp-01",
    "status": "PROVISIONING_INFRA",
    "server_ip": null,
    "wireguard_port": 51820,
    "server_public_key": null,
    "client_ip": "10.200.0.2/32",
    "dns_server": "10.200.0.1",
    "error_message": null
  }
  ```

### 2. `GET /api/deployments/{id}`
Polls the deployment lifecycle state.

- **Status Progression**:
  `PENDING` ➔ `PROVISIONING_INFRA` (Terraform) ➔ `CONFIGURING_VPN` (Ansible) ➔ `READY` (or `FAILED`)
- **Response** (When `READY`):
  ```json
  {
    "deployment_id": "warp-01",
    "status": "READY",
    "server_ip": "54.123.45.67",
    "wireguard_port": 51820,
    "server_public_key": "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=",
    "client_ip": "10.200.0.2/32",
    "dns_server": "10.200.0.1",
    "error_message": null
  }
  ```

### 3. `POST /api/deployments/{id}/teardown`
Initiates infrastructure teardown (`terraform destroy -auto-approve`).

- **Request Body**:
  ```json
  {
    "deployment_id": "warp-01",
    "region": "us-east-1"
  }
  ```
- **Response**:
  ```json
  {
    "deployment_id": "warp-01",
    "status": "TERMINATING",
    "message": "Teardown initiated"
  }
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
│       │   ├── src/
│       │   │   ├── main.rs
│       │   │   ├── terraform.rs
│       │   │   └── ansible.rs
│       │   └── Cargo.toml
│       └── warpgate-common/
│           ├── src/
│           │   └── lib.rs
│           └── Cargo.toml
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
