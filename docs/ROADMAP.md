# WarpGate Project Roadmap

## 📊 Lifecycle Overview

| Phase | Focus | Status | Target Timeline |
| :--- | :--- | :---: | :--- |
| **Phase 1** | Infrastructure & Data Plane Proof of Concept | **Completed** | Weeks 1–3 |
| **Phase 2** | Orchestration, Control Plane & Core Polishing | **Completed** | Weeks 4–6 |
| **Phase 3** | Resilience, Free-Tier Guardrails & Durable State | **Completed** | Weeks 7–8 |
| **Phase 4** | Zero-Cost Auth, Virtual Quotas & Multi-Device Networking | **Upcoming** | Weeks 9–10 |

---

## Phase 1: Infrastructure & Data Plane Proof of Concept (Weeks 1–3) — ✅ COMPLETED

- **Week 1: Manual Validation & Foundation**
  - [x] **AWS Setup:** Configure IAM, create Terraform remote state, and manually launch an Ubuntu EC2 instance.
  - [x] **VPN Configuration:** Install WireGuard, configure `wg0`, enable IP forwarding, and establish NAT rules.
  - [x] **Milestone:** Prove the network path (Laptop → WireGuard → AWS EC2 → Internet) manually.

- **Week 2: Infrastructure as Code (IaC) & Security**
  - [x] **Terraform Modules:** Build reproducible AWS infrastructure (VPC, public subnet, internet gateway, route table, security group, and EC2 instance).
  - [x] **Security & CI/CD Initiation:** Integrate Checkov for static IaC security scanning and configure GitHub Actions workflow.
  - [x] **Milestone:** Successfully automate `terraform apply` and `terraform destroy`.

- **Week 3: Configuration Management & Enhancements**
  - [x] **Core VPN Configuration:** Write Ansible roles to install/configure WireGuard and configure `iptables` NAT routing.
  - [x] **Enhancements & Automation:** Configure AWS Dynamic EC2 Inventory, Linux kernel BBR tuning, AdGuard Home DNS sinkhole, and systemd watchdog.
  - [x] **Milestone:** Achieve fully automated provisioning from Terraform infrastructure to Ansible configuration.

---

## Phase 2: Orchestration, Control Plane & Polish (Weeks 4–6) — ✅ COMPLETED

- **Week 4: Spring Boot & Rust Integration**
  - [x] **Contract Definition:** Align JSON request/response schema across the Spring Boot API and Rust Orchestrator.
  - [x] **Control Plane (Java 21):** Develop Spring Boot REST API (`/api/deployments`) backed by PostgreSQL.
  - [x] **Orchestration (Rust):** Build Tokio async engine coordinating child processes (`terraform apply`, SSH socket polling, `ansible-playbook`).
  - [x] **Milestone:** Trigger end-to-end deployment via REST API call processed asynchronously by Rust.

- **Week 5: CLI, Lifecycle Management, and Testing**
  - [x] **User Interface:** Develop Rust CLI with Clap (`warpgate up --region tokyo --ttl 30m`).
  - [x] **Client Keypair & QR Code:** Client-side Curve25519 (`x25519-dalek`) key generation and in-terminal 2D Unicode QR code rendering.
  - [x] **Lifecycle Engine:** Implement Tokio background timers for automated TTL teardown.
  - [x] **Testing Suite:** Add Rust unit/integration test suites (32 tests passing) and Terratest (Go) validation.
  - [x] **Milestone:** Provision and connect via single CLI command with automatic TTL teardown.

- **Week 6: Scaling & Performance Benchmarking**
  - [x] **Multi-Region Support:** Expand Terraform modules and dynamic inventory to support global AWS regions with friendly aliases (`tokyo`, `frankfurt`, etc.).
  - [x] **Network Benchmarking Suite:** Measure throughput (BBR vs. Cubic under packet loss), latency, and DNS sinkholing via `scripts/run_benchmarks.sh`.
  - [x] **Final Demo Runner:** Add automated lifecycle demo script (`scripts/demo_walkthrough.sh`) supporting both live AWS and dry-run simulation modes.
  - [x] **Milestone:** Full end-to-end multi-region deployment with empirical benchmark verification.

---

## Phase 3: Resilience, Free-Tier Guardrails & Durable State (Weeks 7–8) — ✅ COMPLETED

- **Week 7: AWS Free-Tier Cost Guardrails & Zombie Protection**
  - [x] **KMS Zero-Cost Optimization:** Replace Customer-Managed KMS key in `vpn_node/main.tf` with default AWS-managed CloudWatch encryption to eliminate the $1.00/month CMK charge.
  - [x] **Account Quota Circuit Breaker:** Add hard safety caps (maximum 1 concurrent active instance, maximum 2h TTL) to ensure zero unexpected billing on AWS Free Tier.
  - [x] **Orphan Sweeper Daemon (`warpgate sweep`):** Create a standalone sweeper script / CLI command that queries AWS API across regions for instances tagged `Role=vpn` whose TTL expired, automatically terminating orphaned EC2 nodes and Elastic IPs.
  - [x] **Partial Failure Rollback:** Automatically trigger `terraform destroy` if SSH polling or Ansible playbook execution fails, preventing half-provisioned lingering instances.

- **Week 8: Durable State & Crash Recovery**
  - [x] **Persistent Orchestrator State:** Migrate Rust orchestrator from in-memory `HashMap` to a durable local SQLite store (`.warpgate/orchestrator.db`) with WAL mode.
  - [x] **Orchestrator Crash Recovery:** On engine restart, scan the persistent state and active `.warpgate/deployments/` directories, resuming TTL teardown timers for in-flight nodes and rolling back interrupted ones.
  - [x] **Unified Control Plane Routing:** Direct CLI traffic through the Spring Boot control plane (port 8080) by default, using the Rust engine as the internal worker plane.
  - [x] **Lifecycle Webhooks:** Emit state updates from Rust to Spring Boot (`/api/deployments/{id}/webhook`) so database records sync in real time when background TTL timers expire.

---

## Phase 4: Zero-Cost Auth, Virtual Quotas & Multi-Device Networking (Weeks 9–10) — 📋 UPCOMING

- **Week 9: Zero-Cost Identity & Virtual Quota Management**
  - [ ] **Embedded JWT Authentication:** Implement JWT user registration and login directly in Spring Boot (`/api/auth/register`, `/api/auth/login`) with zero external SaaS costs.
  - [ ] **CLI Session Management:** Add `warpgate login` command to store authentication tokens securely in `~/.warpgate/credentials.json`.
  - [ ] **Virtual Credit / Time Quota System:** Track monthly usage allowances in PostgreSQL (e.g., 600 free minutes per month per user); deduct minutes on deployment creation without needing paid billing processors like Stripe.

- **Week 10: Dynamic WireGuard Peer Management & Speed Optimization**
  - [ ] **Multi-Device Support:** Support multiple client devices (laptop + phone + tablet) per gateway node using a dynamic IP pool (`10.200.0.0/24`).
  - [ ] **Kernel Netlink Peer Injection:** Expose `POST /api/deployments/{id}/peers` to dynamically execute `wg set wg0 peer <KEY> allowed-ips <IP>` at runtime without restarting WireGuard or re-running Ansible playbooks.
  - [ ] **Packer Pre-baked AMIs:** Build a custom Ubuntu Golden AMI containing WireGuard, BBR tuning, and AdGuard pre-installed, reducing provisioning latency from ~2.5 minutes down to < 30 seconds.

---

## Domain & Responsibility Division

| **Domain** | **Foundational Milestones (Phase 1–2)** | **Production & Free-Tier Evolution (Phase 3–4)** |
|---|---|---|
| **Software Development** | Rust Orchestrator, Rust CLI (Clap), HTTP/JSON integration | Durable state persistence (SQLite/Postgres), Crash recovery, CLI session auth |
| **Control Plane & Auth** | Spring Boot REST API, PostgreSQL schema, JPA models | Embedded JWT auth, Virtual credit/time quotas, Real-time state webhooks |
| **Infrastructure & OS** | Terraform modules, AWS networking, Multi-region aliases | Free-tier KMS zero-cost fix, Quota circuit breaker, Packer Golden AMI pipeline |
| **Security & Networking** | WireGuard NAT, Checkov IaC scanning, Key generation | Dynamic multi-device peer injection (`wg set`), Dual-stack IPv6, Port 25 outbound block |
| **Reliability & Lifecycle**| TTL background timers, Cargo tests, Terratest | Standalone Orphan Sweeper (`warpgate sweep`), Failed deployment auto-rollback |
| **Verification & Release** | Architecture docs, Benchmark suite, Demo script | CI/CD automated integration tests, Zero-spend AWS budget alerting |
