# WarpGate Architecture

## System Overview

WarpGate is architected into four distinct layers providing an end-to-end ephemeral, on-demand VPN service:

1. **Client / User Interface Layer (`warpgate-cli` in Rust)**
   - Fast terminal CLI built with Clap (`up`, `status`, `down`).
   - Automatically generates client-side Curve25519/X25519 WireGuard keypairs on the fly (`x25519-dalek`) so private keys never leave the user's computer.
   - Live polling indicators displaying provisioning progress (`[1/2] Provisioning AWS Infra` ➔ `[2/2] Configuring WireGuard & BBR` ➔ `Ready`).
   - Renders 2D Unicode QR codes directly in the terminal (`qrcode`) for instant mobile device scanning, and optionally exports `wg0.conf` configuration files.

2. **Control Plane (Spring Boot / Java 21)**
   - Enterprise record-keeping and business logic layer.
   - Persists deployment records, target regions, TTL expiration timestamps, and status states in a relational PostgreSQL database via Spring Data JPA.
   - Exposes REST endpoints (`/api/deployments`) for clients, web dashboards, and audit logs.

3. **Orchestration Plane (Rust `warpgate-orchestrator`)**
   - High-performance, zero-overhead asynchronous systems engine powered by Tokio.
   - Manages asynchronous child processes for `terraform apply`, JSON output extraction, SSH socket polling (`TcpStream`), `ansible-playbook` execution, and `terraform destroy`.
   - **TTL Automated Teardown Engine**: Background Tokio timers that track deployment lifecycles and automatically trigger `terraform destroy` when Time-to-Live expires.

4. **Data Plane (WireGuard + Ubuntu 22.04 + AWS)**
   - The hardened network layer that securely routes and encrypts all outbound Internet traffic.
   - Enhanced with Linux kernel BBR TCP congestion control, AdGuard Home DNS sinkholing, and systemd watchdog auto-shutdown.

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
│
├── .github/
│   └── workflows/
│       └── terraform-security.yml
│
├── backend/
│   └── springboot/
│       ├── src/
│       │   ├── main/
│       │   │   ├── java/
│       │   │   │   └── com/
│       │   │   │       └── warpgate/
│       │   │   │           ├── controller/
│       │   │   │           │   └── DeploymentController.java
│       │   │   │           │
│       │   │   │           ├── dto/
│       │   │   │           │   └── DeploymentRequest.java
│       │   │   │           │
│       │   │   │           ├── model/
│       │   │   │           │   └── Deployment.java
│       │   │   │           │
│       │   │   │           ├── repository/
│       │   │   │           │   └── DeploymentRepository.java
│       │   │   │           │
│       │   │   │           ├── service/
│       │   │   │           │   └── DeploymentService.java
│       │   │   │           │
│       │   │   │           └── WarpGateApplication.java
│       │   │   │
│       │   │   └── resources/
│       │   │       └── application.properties
│       │   │
│       │   └── test/
│       │       └── java/
│       │           └── com/
│       │               └── warpgate/
│       │
│       └── pom.xml
│
├── rust/
│   ├── Cargo.toml
│   │
│   └── crates/
│       ├── warpgate-cli/
│       │   ├── src/
│       │   │   └── main.rs
│       │   └── Cargo.toml
│       │
│       ├── warpgate-orchestrator/
│       │   ├── src/
│       │   │   ├── main.rs
│       │   │   ├── terraform.rs
│       │   │   └── ansible.rs
│       │   └── Cargo.toml
│       │
│       └── warpgate-common/
│           ├── src/
│           │   └── lib.rs
│           └── Cargo.toml
│
├── infra/
│   ├── terraform/
│   │   ├── main.tf
│   │   ├── variables.tf
│   │   ├── outputs.tf
│   │   ├── providers.tf
│   │   ├── terraform.tfvars
│   │   ├── .terraform.lock.hcl
│   │   │
│   │   └── modules/
│   │       └── vpn_node/
│   │           ├── main.tf
│   │           ├── variables.tf
│   │           └── outputs.tf
│   │
│   └── ansible/
│       ├── ansible.cfg
│       ├── site.yml
│       │
│       ├── inventory/
│       │   └── aws_ec2.yml
│       │
│       └── roles/
│           ├── wireguard/
│           │   ├── tasks/
│           │   │   └── main.yml
│           │   ├── handlers/
│           │   │   └── main.yml
│           │   └── templates/
│           │       └── wg0.conf.j2
│           │
│           ├── bbr/
│           │   └── tasks/
│           │       └── main.yml
│           │
│           ├── adguard/
│           │   └── tasks/
│           │       └── main.yml
│           │
│           └── watchdog/
│               └── tasks/
│                   └── main.yml
│
├── docs/
│   ├── ARCHITECTURE.md
│   ├── ROADMAP.md
│   └── CAREER.md
│
├── .gitignore
├── LICENSE
└── README.md
