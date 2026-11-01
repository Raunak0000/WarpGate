# WarpGate Final Demo & Lifecycle Walkthrough

This document outlines the step-by-step verification of the complete WarpGate lifecycle:
1. **Interactive Node Provisioning (`warpgate up`)**
2. **Curve25519 Client Keypair Generation (`x25519-dalek`)**
3. **In-Terminal 2D Unicode QR Code Rendering & `wg0.conf` Export**
4. **Lifecycle State Polling (`warpgate status`)**
5. **Automated TTL Teardown / Immediate Teardown (`warpgate down`)**

---

## 1. Quick Demonstration Execution

WarpGate provides an automated demo runner (`scripts/demo_walkthrough.sh`) supporting both safe simulation (`--dry-run`, default) and live AWS execution (`--live`):

```bash
# Execute the full end-to-end demo walkthrough in simulated mode:
./scripts/demo_walkthrough.sh

# Or run against live AWS infrastructure in a specific region:
./scripts/demo_walkthrough.sh --live --region frankfurt --ttl 30m
```

---

## 2. Walkthrough Steps Explained

### Step 1: Client-Side Key Generation & Provisioning Request
When the user executes:
```bash
cargo run --bin warpgate-cli -- up --region tokyo --ttl 30m --admin-cidr 0.0.0.0/0
```

1. **Regional Alias Resolution**: The CLI resolves human-friendly aliases like `tokyo` into the canonical AWS region `ap-northeast-1` (or `frankfurt` -> `eu-central-1`).
2. **Curve25519 Keypair Generation**: Using `x25519-dalek`, the CLI creates a fresh private/public keypair on the local machine. The private key never leaves the client.
3. **Dispatch to Orchestrator**: The public key, target region, and TTL are submitted to the Axum Orchestrator (`POST /api/deployments`).

### Step 2: Asynchronous State Progression
The CLI displays interactive live status updates as the node transitions through its lifecycle:
```text
[1/2] Provisioning AWS Infrastructure (VPC, Subnet, EIP, EC2)...
[2/2] Configuring WireGuard, BBR Kernel Tuning & AdGuard DNS...
✓ Node is READY! Endpoint: 203.0.113.10:51820
```

### Step 3: WireGuard QR Code & Configuration Export
Upon readiness:
- The WireGuard client configuration (`wg0.conf`) is assembled:
  ```ini
  [Interface]
  PrivateKey = <client-private-key>
  Address = 10.200.0.2/32
  DNS = 1.1.1.1, 10.200.0.1

  [Peer]
  PublicKey = <server-public-key>
  Endpoint = <server-public-ip>:51820
  AllowedIPs = 0.0.0.0/0, ::/0
  PersistentKeepalive = 25
  ```
- A **2D Unicode QR code** is rendered directly in the terminal for instant mobile scanning via the WireGuard iOS / Android app.
- On Linux laptops, NetworkManager (`nmcli`) or `wg-quick` can automatically connect the tunnel.

### Step 4: Status Inspection
Any active deployment can be inspected at any time:
```bash
cargo run --bin warpgate-cli -- status warp-c6c0ef3d9dd7
```
```text
DeploymentResponse {
    deployment_id: "warp-c6c0ef3d9dd7",
    status: Ready,
    server_ip: Some("54.123.45.67"),
    wireguard_port: Some(51820),
    server_public_key: Some("M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I="),
    client_ip: Some("10.200.0.2/32"),
    dns_server: Some("1.1.1.1, 10.200.0.1"),
    region: "ap-northeast-1",
    admin_cidr: "0.0.0.0/0"
}
```

### Step 5: Automated TTL Teardown & Teardown on Demand
The deployment will terminate automatically when its Time-to-Live expires via the background Tokio timer in `warpgate-orchestrator`:
- Automatically triggers `terraform destroy -auto-approve`
- Releases AWS Elastic IP, EC2 node, VPC, and KMS log groups
- Updates state to `DESTROYED`

Alternatively, the user can terminate the gateway on demand:
```bash
cargo run --bin warpgate-cli -- down warp-c6c0ef3d9dd7 --region tokyo
```
```text
Terminating deployment 'warp-c6c0ef3d9dd7' in region 'ap-northeast-1'...
Teardown initiated for 'warp-c6c0ef3d9dd7' (Status: Terminating)
```
Status inspection immediately confirms the gateway has been destroyed.
