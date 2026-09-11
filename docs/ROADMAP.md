# WarpGate Project Roadmap

## Phase 1: Infrastructure & Data Plane Proof of Concept (Weeks 1-3)

- **Week 1: Manual Validation & Foundation**
  - [x] **AWS Setup:** Configure IAM, create Terraform remote state, and manually launch an Ubuntu EC2 instance.
  - [x] **VPN Configuration:** Install WireGuard, configure `wg0`, enable IP forwarding, and establish NAT rules.
  - [x] **Milestone:** Prove the network path (Laptop → WireGuard → AWS EC2 → Internet) manually.

- **Week 2: Infrastructure as Code (IaC) & Security**
  - [x] **Partner A - Terraform Modules:** Build reproducible AWS infrastructure (VPC, public subnet, internet gateway, route table, security group, and EC2 instance).
  - [ ] **Partner B - Security & CI/CD Initiation:** Integrate Checkov for static IaC security scanning and begin setting up GitHub Actions.
  - [ ] **Milestone:** Successfully run `terraform apply` and `terraform destroy`.

- **Week 3: Configuration Management & Enhancements**
  - [ ] **Automation:** Utilize Ansible with an AWS dynamic EC2 inventory to connect via SSH.
  - [ ] **Ansible Roles:** Develop distinct roles for the base setup, kernel tuning, networking, WireGuard, AdGuard Home, and a systemd watchdog.
  - [ ] **Milestone:** Achieve a fully automated flow from Terraform creation to Ansible configuration.

## Phase 2: Orchestration, Control Plane & Polish (Weeks 4-6)

- **Week 4: The Spring Boot & Rust Integration**
  - [ ] **Contract Definition:** Agree on the JSON request/response structures between the Spring Boot API and the Rust Orchestrator.
  - [ ] **Control Plane (Java 21):** Develop the Spring Boot REST API (`/api/deployments`) backed by PostgreSQL.
  - [ ] **Orchestration (Rust):** Build the async Rust engine to execute Terraform and Ansible via subprocesses.
  - [ ] **Milestone:** Trigger an infrastructure deployment via a REST API call to Spring Boot, processed asynchronously by Rust.

- **Week 5: CLI, Lifecycle Management, and Testing**
  - [ ] **User Interface:** Develop the Rust CLI using Clap (`warpgate up --region tokyo --ttl 30m`).
  - [ ] **Lifecycle Engine:** Implement the TTL automated teardown mechanism.
  - [ ] **Testing Suite:** Write Rust tests and deploy Terratest (Go) to validate end-to-end infrastructure.

- **Week 6: Scaling & Performance Benchmarking**
  - [ ] **Multi-Region:** Expand Terraform modules to support additional AWS regions (e.g., `us-east-1`, `eu-central-1`).
  - [ ] **Benchmarking:** Compare default settings versus BBR-tuned settings using `iperf3`, `ping`, and `dig`.
  - [ ] **Final Demo:** Execute the full lifecycle from the `warpgate up` command, connecting via QR code, and ending with automated teardown.

---

## Team Division

| **Domain** | **Partner A** | **Partner B** |
|---|---|---|
| **Software Development** | Spring Boot API, PostgreSQL schema, REST controllers | Rust Orchestrator, Rust CLI (Clap), HTTP/JSON integration |
| **Infrastructure & OS** | Terraform modules, AWS networking, Multi-region scaling | Ansible roles, Linux kernel tuning, AdGuard DNS setup |
| **Security & VPN** | Checkov IaC scanning, IAM provisioning | WireGuard configuration, Key generation, NAT rules |
| **Testing & Lifecycle** | Terratest (Go) integration, TTL deployment states | Cargo tests, performance benchmarking, instance watchdog |
| **Pipeline & Release** | Architecture documentation, API design | GitHub Actions CI/CD, Final demo script, Performance report |
