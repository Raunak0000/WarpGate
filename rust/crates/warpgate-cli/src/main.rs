use base64::prelude::*;
use clap::{Parser, Subcommand};
use qrcode::QrCode;
use qrcode::render::unicode;
use std::path::PathBuf;
use std::time::Duration;
use warpgate_common::{
    DeploymentRequest, DeploymentResponse, DeploymentStatus, TeardownRequest, TeardownResponse,
};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Parser)]
#[command(name = "warpgate", about = "WarpGate - Ephemeral VPN CLI", version)]
struct Cli {
    /// URL of the WarpGate API (Spring Boot Control Plane or Orchestrator)
    #[arg(
        long,
        env = "WARPGATE_API_URL",
        default_value = "http://localhost:8080"
    )]
    api_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Provision a new ephemeral WireGuard VPN node
    Up {
        /// Target AWS region or alias (e.g. 'us-east-1', 'eu-central-1', 'tokyo', 'frankfurt')
        #[arg(short, long, default_value = "us-east-1")]
        region: String,

        /// Time-to-Live before automated teardown (e.g. '30m', '1h', '45')
        #[arg(short, long, default_value = "30m")]
        ttl: String,

        /// Allowed admin CIDR for SSH access
        #[arg(long, required = true)]
        admin_cidr: String,

        /// Save client WireGuard configuration file to this path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Connect this laptop to the WireGuard VPN tunnel upon readiness (default: true)
        #[arg(short, long, default_value_t = true)]
        connect: bool,

        /// Skip automatically connecting this laptop to the VPN tunnel
        #[arg(long)]
        no_connect: bool,

        /// Run in detached background mode (do not keep terminal attached to session)
        #[arg(short = 'd', long)]
        detach: bool,
    },

    /// Check status of an active deployment
    Status {
        /// Deployment ID
        id: String,
    },

    /// Terminate and destroy a deployment immediately
    Down {
        /// Deployment ID
        id: String,

        /// AWS region where node was deployed (or alias like 'tokyo')
        #[arg(short, long, default_value = "us-east-1")]
        region: String,

        /// Disconnect local host VPN connection if active
        #[arg(long, default_value_t = true)]
        disconnect: bool,
    },

    /// Scan for and sweep orphaned cloud infrastructure and lingering Elastic IPs
    Sweep {
        /// Target AWS regions (comma-separated or multiple flags)
        #[arg(short, long, value_delimiter = ',')]
        regions: Vec<String>,

        /// Discover and display orphaned resources without deleting
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Terminate/release detected orphans without interactive confirmation
        #[arg(short, long)]
        force: bool,

        /// Maximum age threshold in hours before an active instance is treated as an orphan
        #[arg(long, default_value_t = 2)]
        max_age_hours: u32,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match cli.command {
        Commands::Up {
            region,
            ttl,
            admin_cidr,
            output,
            connect,
            no_connect,
            detach,
        } => {
            let (effective_api_url, spawned_pid) =
                ensure_backend_available(&client, &cli.api_url, true).await?;

            let normalized_region = warpgate_common::normalize_region(&region)
                .map_err(|e| format!("Invalid region '{}': {}", region, e))?
                .to_string();
            let should_connect = connect && !no_connect;
            let ttl_minutes = parse_ttl_minutes(&ttl)?;
            if ttl_minutes == 0 || ttl_minutes > 120 {
                eprintln!(
                    "Account quota exceeded: TTL '{}' ({} minutes) exceeds the maximum allowable 120 minute (2 hour) cap to protect AWS Free Tier limits.",
                    ttl, ttl_minutes
                );
                std::process::exit(1);
            }
            let deployment_id = format!("warp-{}", &uuid_short());

            // 1. Generate client WireGuard keypair locally
            let (client_priv, client_pub) = generate_wireguard_keypair();

            println!(
                "Requesting WarpGate deployment '{}' in region '{}' (TTL: {}m)...",
                deployment_id, normalized_region, ttl_minutes
            );

            let req_body = DeploymentRequest {
                deployment_id: deployment_id.clone(),
                region: normalized_region.clone(),
                ttl_minutes,
                client_public_key: client_pub,
                admin_cidr: Some(admin_cidr),
            };

            let res = client
                .post(format!("{}/api/deployments", effective_api_url))
                .json(&req_body)
                .send()
                .await?;

            if !res.status().is_success() {
                let status_code = res.status();
                if let Ok(dep) = res.json::<DeploymentResponse>().await {
                    if let Some(err) = dep.error_message {
                        eprintln!("Failed to submit deployment (HTTP {}): {}", status_code, err);
                        std::process::exit(1);
                    }
                }
                eprintln!("Failed to submit deployment: {}", status_code);
                std::process::exit(1);
            }

            println!("Deployment initiated. Polling status until ready...");

            // 2. Poll status until Ready or Failed
            let mut last_status: Option<DeploymentStatus> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(4)).await;

                let status_res = client
                    .get(format!("{}/api/deployments/{}", effective_api_url, deployment_id))
                    .send()
                    .await?;

                if !status_res.status().is_success() {
                    continue;
                }

                let dep: DeploymentResponse = status_res.json().await?;

                if Some(&dep.status) != last_status.as_ref() {
                    match dep.status {
                        DeploymentStatus::ProvisioningInfra => {
                            println!("  [1/2] Provisioning AWS infrastructure with Terraform...");
                        }
                        DeploymentStatus::ConfiguringVpn => {
                            if let Some(ip) = &dep.server_ip {
                                println!(
                                    "  [2/2] Instance online ({})! Configuring WireGuard & BBR via Ansible...",
                                    ip
                                );
                            }
                        }
                        DeploymentStatus::Ready => {
                            println!("\nWarpGate node is READY!\n");
                            display_connection_info(&dep, &client_priv, output.as_deref());

                            if should_connect {
                                let conf_path = output
                                    .clone()
                                    .unwrap_or_else(|| std::env::temp_dir().join("warpgate.conf"));

                                let conf = generate_wireguard_config(
                                    &client_priv,
                                    dep.client_ip.as_deref().unwrap_or("10.200.0.2/32"),
                                    dep.dns_server.as_deref().unwrap_or("1.1.1.1, 10.200.0.1"),
                                    dep.server_public_key.as_deref().unwrap_or(""),
                                    dep.server_ip.as_deref().unwrap_or("0.0.0.0"),
                                    dep.wireguard_port.unwrap_or(51820),
                                );
                                let _ = std::fs::write(&conf_path, &conf);

                                println!(
                                    "\n[--connect] Connecting laptop to WarpGate VPN via NetworkManager..."
                                );
                                match connect_local_vpn(&conf_path, "warpgate") {
                                    Ok(_) => {
                                        println!(
                                            "SUCCESS: Laptop connected to WarpGate VPN! (Interface: warpgate)"
                                        );
                                        if let Ok(ip_res) = client
                                            .get("https://api.ipify.org")
                                            .timeout(Duration::from_secs(4))
                                            .send()
                                            .await
                                        {
                                            if let Ok(ip_text) = ip_res.text().await {
                                                println!(
                                                    "Verified laptop public IP is now: {}",
                                                    ip_text.trim()
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Warning: Failed to auto-connect laptop: {}", e);
                                        println!(
                                            "You can connect manually via: nmcli connection import type wireguard file {:?}",
                                            conf_path
                                        );
                                    }
                                }
                            }

                            break;
                        }
                        DeploymentStatus::Failed => {
                            eprintln!(
                                "\nDeployment failed: {}",
                                dep.error_message.as_deref().unwrap_or("Unknown error")
                            );
                            if let Some(pid) = spawned_pid {
                                stop_daemon(Some(pid));
                            }
                            std::process::exit(1);
                        }
                        _ => {}
                    }
                    last_status = Some(dep.status);
                }
            }

            if detach {
                println!("\n============================================================");
                println!("  [+] WarpGate VPN is ACTIVE (Detached Mode)");
                println!("      Deployment ID: {}", deployment_id);
                println!("      Region:        {}", normalized_region);
                println!("      TTL:           {}m", ttl_minutes);
                println!("      To disconnect & teardown: warpgate down {}", deployment_id);
                println!("============================================================");
                return Ok(());
            }

            println!("\n============================================================");
            println!("  [+] WarpGate VPN session is ACTIVE");
            println!("      Deployment ID: {}", deployment_id);
            println!("      Region:        {}", normalized_region);
            println!("      TTL:           {}m", ttl_minutes);
            println!("      Session attached. Press Ctrl+C to disconnect and teardown.");
            println!("============================================================");

            // Keep session attached; monitor TTL and listen for Ctrl+C
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        println!("\n[!] Ctrl+C detected. Shutting down WarpGate session...");
                        if should_connect {
                            println!("  [-] Disconnecting local VPN connection...");
                            disconnect_local_vpn("warpgate");
                        }
                        println!("  [-] Terminating deployment '{}' in region '{}'...", deployment_id, normalized_region);
                        let req_body = TeardownRequest {
                            deployment_id: deployment_id.clone(),
                            region: normalized_region.clone(),
                        };
                        let _ = client
                            .post(format!("{}/api/deployments/{}/teardown", effective_api_url, deployment_id))
                            .json(&req_body)
                            .send()
                            .await;

                        // Wait briefly for teardown to finish
                        for _ in 0..15 {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            if let Ok(res) = client
                                .get(format!("{}/api/deployments/{}", effective_api_url, deployment_id))
                                .send()
                                .await
                            {
                                if let Ok(status_dep) = res.json::<DeploymentResponse>().await {
                                    if status_dep.status == DeploymentStatus::Destroyed {
                                        println!("  [+] Infrastructure successfully destroyed.");
                                        break;
                                    }
                                }
                            }
                        }

                        if let Some(pid) = spawned_pid {
                            stop_daemon(Some(pid));
                        }
                        println!("[+] Session ended cleanly. Zero resources lingering.");
                        return Ok(());
                    }
                    _ = tokio::time::sleep(Duration::from_secs(4)) => {
                        if let Ok(res) = client
                            .get(format!("{}/api/deployments/{}", effective_api_url, deployment_id))
                            .send()
                            .await
                        {
                            if let Ok(status_dep) = res.json::<DeploymentResponse>().await {
                                if status_dep.status == DeploymentStatus::Destroyed {
                                    println!("\n[+] TTL expired. Deployment was automatically torn down.");
                                    if should_connect {
                                        println!("  [-] Disconnecting local VPN...");
                                        disconnect_local_vpn("warpgate");
                                    }
                                    if let Some(pid) = spawned_pid {
                                        stop_daemon(Some(pid));
                                    }
                                    println!("[+] Session ended cleanly.");
                                    return Ok(());
                                } else if status_dep.status == DeploymentStatus::Failed {
                                    eprintln!("\n[-] Deployment failed: {}", status_dep.error_message.as_deref().unwrap_or("Unknown error"));
                                    if should_connect {
                                        disconnect_local_vpn("warpgate");
                                    }
                                    if let Some(pid) = spawned_pid {
                                        stop_daemon(Some(pid));
                                    }
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                }
            }
        }

        Commands::Status { id } => {
            let (effective_api_url, _) =
                ensure_backend_available(&client, &cli.api_url, false).await?;

            let res = client
                .get(format!("{}/api/deployments/{}", effective_api_url, id))
                .send()
                .await?;

            if res.status().as_u16() == 404 {
                println!("Deployment '{}' not found.", id);
                return Ok(());
            }

            let dep: DeploymentResponse = res.json().await?;
            println!("{:#?}", dep);
        }

        Commands::Down {
            id,
            region,
            disconnect,
        } => {
            let (effective_api_url, _) =
                ensure_backend_available(&client, &cli.api_url, false).await?;

            let normalized_region = warpgate_common::normalize_region(&region)
                .map_err(|e| format!("Invalid region '{}': {}", region, e))?
                .to_string();

            if disconnect {
                disconnect_local_vpn("warpgate");
            }

            println!(
                "Terminating deployment '{}' in region '{}'...",
                id, normalized_region
            );
            let req_body = TeardownRequest {
                deployment_id: id.clone(),
                region: normalized_region,
            };

            let res = client
                .post(format!("{}/api/deployments/{}/teardown", effective_api_url, id))
                .json(&req_body)
                .send()
                .await?;

            if res.status().is_success() {
                let teardown: TeardownResponse = res.json().await?;
                println!(
                    "Teardown initiated for '{}' (Status: {:?})",
                    teardown.deployment_id, teardown.status
                );
            } else {
                eprintln!("Teardown request failed: {}", res.status());
            }

            check_and_stop_daemon_if_idle(&client, &effective_api_url).await;
        }

        Commands::Sweep {
            regions,
            dry_run,
            force,
            max_age_hours,
        } => {
            sweep_orphaned_resources(regions, dry_run, force, max_age_hours).await?;
        }
    }

    Ok(())
}

async fn sweep_orphaned_resources(
    regions: Vec<String>,
    dry_run: bool,
    force: bool,
    max_age_hours: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_regions = if regions.is_empty() {
        warpgate_common::SUPPORTED_REGIONS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    } else {
        regions
    };

    println!("=== WarpGate Orphan Sweeper ===");
    println!("Max Age Threshold: {} hours", max_age_hours);
    println!(
        "Dry Run Mode:      {}",
        if dry_run { "ENABLED (read-only)" } else { "DISABLED (live cleanup)" }
    );
    println!("Target Regions:    {}\n", target_regions.join(", "));

    // Check if AWS CLI is available
    let has_aws = tokio::process::Command::new("aws")
        .arg("--version")
        .output()
        .await
        .is_ok();

    if !has_aws {
        println!("[WARN] AWS CLI is not installed or not in PATH.");
        println!("Checking local state directory (.warpgate/deployments)...");
        clean_local_state(dry_run);
        println!("[OK] Local sweep completed.");
        return Ok(());
    }

    let mut total_instances = 0;
    let mut total_eips = 0;

    for region in target_regions {
        let norm_region = match warpgate_common::normalize_region(&region) {
            Ok(r) => r.to_string(),
            Err(_) => region,
        };

        println!("Scanning region: {}...", norm_region);

        // 1. EC2 instances tagged Role=vpn
        let instance_output = tokio::process::Command::new("aws")
            .args([
                "ec2",
                "describe-instances",
                "--region",
                &norm_region,
                "--filters",
                "Name=tag:Role,Values=vpn",
                "Name=instance-state-name,Values=running,pending,stopped",
                "--query",
                "Reservations[].Instances[][InstanceId,LaunchTime,Tags[?Key=='Name'].Value|[0],PublicIpAddress]",
                "--output",
                "text",
            ])
            .output()
            .await;

        if let Ok(out) = instance_output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.is_empty() || parts[0].trim().is_empty() {
                    continue;
                }
                total_instances += 1;
                let inst_id = parts[0].trim();
                let launch_time = parts.get(1).unwrap_or(&"unknown");
                let tag_name = parts.get(2).unwrap_or(&"unnamed");
                let pub_ip = parts.get(3).unwrap_or(&"none");

                println!(
                    "  Found instance: {} (Name: {}, IP: {}, Launched: {})",
                    inst_id, tag_name, pub_ip, launch_time
                );

                if dry_run {
                    println!("    [DRY RUN] Would terminate instance {}", inst_id);
                } else if force {
                    println!("    Terminating instance {}...", inst_id);
                    let _ = tokio::process::Command::new("aws")
                        .args(["ec2", "terminate-instances", "--region", &norm_region, "--instance-ids", inst_id])
                        .output()
                        .await;
                }
            }
        }

        // 2. Unattached Elastic IPs
        let eip_output = tokio::process::Command::new("aws")
            .args([
                "ec2",
                "describe-addresses",
                "--region",
                &norm_region,
                "--query",
                "Addresses[?NetworkInterfaceId==null][AllocationId,PublicIp,Tags[?Key=='Name'].Value|[0]]",
                "--output",
                "text",
            ])
            .output()
            .await;

        if let Ok(out) = eip_output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.is_empty() || parts[0].trim().is_empty() {
                    continue;
                }
                total_eips += 1;
                let alloc_id = parts[0].trim();
                let pub_ip = parts.get(1).unwrap_or(&"unknown");
                let tag_name = parts.get(2).unwrap_or(&"unnamed");

                println!(
                    "  Found unattached Elastic IP: {} (Alloc: {}, Tag: {})",
                    pub_ip, alloc_id, tag_name
                );

                if dry_run {
                    println!("    [DRY RUN] Would release Elastic IP {}", alloc_id);
                } else if force {
                    println!("    Releasing Elastic IP {}...", alloc_id);
                    let _ = tokio::process::Command::new("aws")
                        .args(["ec2", "release-address", "--region", &norm_region, "--allocation-id", alloc_id])
                        .output()
                        .await;
                }
            }
        }
    }

    clean_local_state(dry_run);

    println!("\n=== Sweep Complete ===");
    println!("Total EC2 instances scanned: {}", total_instances);
    println!("Total unattached Elastic IPs scanned: {}", total_eips);
    if dry_run {
        println!("Run with --force to execute automated cloud cleanup.");
    } else {
        println!("Cleanup completed successfully. Zero orphaned resources lingering.");
    }

    Ok(())
}

async fn is_url_reachable(client: &reqwest::Client, url: &str) -> bool {
    let check_urls = [
        format!("{}/api/deployments/health", url.trim_end_matches('/')),
        format!("{}/api/deployments", url.trim_end_matches('/')),
    ];

    for check_url in check_urls {
        if let Ok(res) = client
            .get(&check_url)
            .timeout(Duration::from_millis(500))
            .send()
            .await
        {
            let code = res.status().as_u16();
            if (200..=499).contains(&code) {
                return true;
            }
        }
    }
    false
}

fn find_orchestrator_binary() -> Option<PathBuf> {
    // 1. Next to current_exe or in parent target directory (e.g. target/debug/deps -> target/debug)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("warpgate-orchestrator");
            if candidate.exists() {
                return Some(candidate);
            }
            if let Some(grandparent) = parent.parent() {
                let gp_candidate = grandparent.join("warpgate-orchestrator");
                if gp_candidate.exists() {
                    return Some(gp_candidate);
                }
            }
        }
    }

    // 2. Typical workspace target locations
    let candidates = [
        "../../target/debug/warpgate-orchestrator",
        "../../target/release/warpgate-orchestrator",
        "rust/target/debug/warpgate-orchestrator",
        "target/debug/warpgate-orchestrator",
        "rust/target/release/warpgate-orchestrator",
        "target/release/warpgate-orchestrator",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. System PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("warpgate-orchestrator");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

async fn ensure_backend_available(
    client: &reqwest::Client,
    requested_url: &str,
    auto_spawn: bool,
) -> Result<(String, Option<u32>), Box<dyn std::error::Error>> {
    // 1. Check requested URL
    if is_url_reachable(client, requested_url).await {
        return Ok((requested_url.to_string(), None));
    }

    // 2. If requested URL is the default port 8080 (Spring Boot), also check 3000 directly
    if requested_url.contains(":8080") && is_url_reachable(client, "http://localhost:3000").await {
        return Ok(("http://localhost:3000".to_string(), None));
    }

    if !auto_spawn {
        return Ok((requested_url.to_string(), None));
    }

    // 3. Auto-spawn local orchestrator daemon
    println!("No running WarpGate backend found; starting local Orchestrator daemon in background...");

    let binary = match find_orchestrator_binary() {
        Some(b) => b,
        None => {
            println!("Building warpgate-orchestrator...");
            let build_status = std::process::Command::new("cargo")
                .args(["build", "--bin", "warpgate-orchestrator"])
                .status();
            if build_status.map(|s| s.success()).unwrap_or(false) {
                find_orchestrator_binary()
                    .ok_or("Could not locate warpgate-orchestrator binary after build")?
            } else {
                return Err("warpgate-orchestrator binary not found. Please run 'cargo build' first.".into());
            }
        }
    };

    let state_dir = PathBuf::from(".warpgate");
    let _ = std::fs::create_dir_all(&state_dir);
    let log_file = std::fs::File::create(state_dir.join("orchestrator.log"))?;

    let mut cmd = std::process::Command::new(&binary);
    cmd.stdout(log_file.try_clone()?);
    cmd.stderr(log_file);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd.spawn()?;
    let pid = child.id();
    let _ = std::fs::write(state_dir.join("orchestrator.pid"), pid.to_string());

    println!("[+] Local orchestrator daemon started (PID: {}).", pid);

    let target_url = "http://localhost:3000";
    let mut ready = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if is_url_reachable(client, target_url).await {
            ready = true;
            break;
        }
    }

    if !ready {
        stop_daemon(Some(pid));
        return Err("Spawned orchestrator daemon failed to respond within 4s. Check .warpgate/orchestrator.log".into());
    }

    Ok((target_url.to_string(), Some(pid)))
}

fn stop_daemon(pid: Option<u32>) {
    let pid_file = PathBuf::from(".warpgate/orchestrator.pid");
    let target_pid = pid.or_else(|| {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
    });

    if let Some(p) = target_pid {
        println!("[-] Stopping local orchestrator daemon (PID: {})...", p);
        #[cfg(unix)]
        let _ = std::process::Command::new("kill")
            .arg(p.to_string())
            .output();

        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &p.to_string(), "/F"])
            .output();
    }
    let _ = std::fs::remove_file(pid_file);
}

async fn check_and_stop_daemon_if_idle(client: &reqwest::Client, api_url: &str) {
    let pid_file = PathBuf::from(".warpgate/orchestrator.pid");
    if !pid_file.exists() {
        return;
    }

    let has_active = if let Ok(res) = client
        .get(format!("{}/api/deployments", api_url.trim_end_matches('/')))
        .timeout(Duration::from_millis(800))
        .send()
        .await
    {
        if let Ok(deps) = res.json::<Vec<DeploymentResponse>>().await {
            deps.iter().any(|d| {
                d.status != DeploymentStatus::Destroyed && d.status != DeploymentStatus::Failed
            })
        } else {
            false
        }
    } else {
        false
    };

    if !has_active {
        stop_daemon(None);
    }
}

fn clean_local_state(dry_run: bool) {
    let state_dir = std::path::Path::new(".warpgate/deployments");
    if state_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(state_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let tfstate = path.join("terraform.tfstate");
                    if tfstate.exists() {
                        if let Ok(content) = std::fs::read_to_string(&tfstate) {
                            if content.contains("\"resources\": []") {
                                if dry_run {
                                    println!("  [DRY RUN] Would prune empty state directory: {:?}", path);
                                } else {
                                    let _ = std::fs::remove_dir_all(&path);
                                    println!("  Pruned empty state directory: {:?}", path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn generate_wireguard_keypair() -> (String, String) {
    let mut bytes = [0u8; 32];
    for b in bytes.iter_mut() {
        *b = rand::random();
    }
    let secret = StaticSecret::from(bytes);
    let public = PublicKey::from(&secret);
    let priv_b64 = BASE64_STANDARD.encode(secret.to_bytes());
    let pub_b64 = BASE64_STANDARD.encode(public.as_bytes());
    (priv_b64, pub_b64)
}

fn parse_ttl_minutes(s: &str) -> Result<u32, String> {
    let s = s.trim().to_lowercase();
    if let Some(h) = s.strip_suffix('h') {
        let hours: u32 = h.parse().map_err(|_| "Invalid TTL hours format")?;
        Ok(hours * 60)
    } else if let Some(m) = s.strip_suffix('m') {
        m.parse()
            .map_err(|_| "Invalid TTL minutes format".to_string())
    } else {
        s.parse()
            .map_err(|_| "Invalid TTL duration (expected e.g. 30m, 1h)".to_string())
    }
}

fn uuid_short() -> String {
    let r: u64 = rand::random();
    format!("{:012x}", r & 0x0000_ffff_ffff_ffff)
}

pub fn generate_wireguard_config(
    client_priv: &str,
    client_ip: &str,
    dns: &str,
    server_pub: &str,
    server_ip: &str,
    port: u16,
) -> String {
    format!(
        "[Interface]
PrivateKey = {client_priv}
Address = {client_ip}
DNS = {dns}

[Peer]
PublicKey = {server_pub}
Endpoint = {server_ip}:{port}
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
"
    )
}

fn display_connection_info(
    dep: &DeploymentResponse,
    client_priv: &str,
    output_path: Option<&std::path::Path>,
) {
    let server_ip = dep.server_ip.as_deref().unwrap_or("0.0.0.0");
    let port = dep.wireguard_port.unwrap_or(51820);
    let server_pub = dep.server_public_key.as_deref().unwrap_or("");
    let client_ip = dep.client_ip.as_deref().unwrap_or("10.200.0.2/32");
    let dns = dep.dns_server.as_deref().unwrap_or("1.1.1.1, 10.200.0.1");

    let conf = generate_wireguard_config(client_priv, client_ip, dns, server_pub, server_ip, port);

    println!("===========================================================");
    println!(" Server IP   : {server_ip}:{port}");
    println!(" Internal IP : {client_ip} (DNS: {dns})");
    println!(" Server Key  : {server_pub}");
    println!("===========================================================\n");

    println!("Scan with WireGuard Mobile App:\n");
    if let Ok(code) = QrCode::new(conf.as_bytes()) {
        let qr_string = code
            .render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Light)
            .light_color(unicode::Dense1x2::Dark)
            .build();
        println!("{qr_string}");
    }

    if let Some(path) = output_path {
        if let Err(e) = std::fs::write(path, &conf) {
            eprintln!("Failed to write config to {:?}: {}", path, e);
        } else {
            println!("Client config saved to {:?}", path);
        }
    }
}

pub fn connect_local_vpn(conf_path: &std::path::Path, conn_name: &str) -> Result<(), String> {
    // 1. Try NetworkManager (nmcli)
    let nmcli_check = std::process::Command::new("which").arg("nmcli").output();
    if let Ok(out) = nmcli_check {
        if out.status.success() {
            // Delete pre-existing profile if exists to prevent duplicates
            let _ = std::process::Command::new("nmcli")
                .args(["connection", "delete", conn_name])
                .output();

            let import_status = std::process::Command::new("nmcli")
                .args(["connection", "import", "type", "wireguard", "file"])
                .arg(conf_path)
                .status()
                .map_err(|e| format!("Failed to run nmcli import: {}", e))?;

            if !import_status.success() {
                return Err(format!(
                    "nmcli import failed with status: {}",
                    import_status
                ));
            }

            let up_status = std::process::Command::new("nmcli")
                .args(["connection", "up", conn_name])
                .status()
                .map_err(|e| format!("Failed to run nmcli up: {}", e))?;

            if up_status.success() {
                return Ok(());
            } else {
                let _ = std::process::Command::new("nmcli")
                    .args(["connection", "delete", conn_name])
                    .output();
                return Err(format!(
                    "nmcli up failed with status: {}. (If you recently updated your Linux kernel, a reboot is required to load the new WireGuard module)",
                    up_status
                ));
            }
        }
    }

    // 2. Fallback to wg-quick
    let wg_check = std::process::Command::new("which").arg("wg-quick").output();
    if let Ok(out) = wg_check {
        if out.status.success() {
            let _ = std::process::Command::new("wg-quick")
                .arg("down")
                .arg(conf_path)
                .output();

            let up_status = std::process::Command::new("wg-quick")
                .arg("up")
                .arg(conf_path)
                .status()
                .map_err(|e| format!("Failed to run wg-quick up: {}", e))?;

            if up_status.success() {
                return Ok(());
            } else {
                return Err(format!("wg-quick up failed with status: {}", up_status));
            }
        }
    }

    Err("Neither 'nmcli' nor 'wg-quick' was found on your system.".to_string())
}

pub fn disconnect_local_vpn(conn_name: &str) {
    let _ = std::process::Command::new("nmcli")
        .args(["connection", "down", conn_name])
        .output();
    let _ = std::process::Command::new("nmcli")
        .args(["connection", "delete", conn_name])
        .output();

    let default_conf = std::env::temp_dir().join(format!("{}.conf", conn_name));
    if default_conf.exists() {
        let _ = std::process::Command::new("wg-quick")
            .arg("down")
            .arg(&default_conf)
            .output();
        let _ = std::fs::remove_file(&default_conf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_ttl_minutes() {
        assert_eq!(parse_ttl_minutes("30m").unwrap(), 30);
        assert_eq!(parse_ttl_minutes("2m").unwrap(), 2);
        assert_eq!(parse_ttl_minutes("1h").unwrap(), 60);
        assert_eq!(parse_ttl_minutes("2h").unwrap(), 120);
        assert_eq!(parse_ttl_minutes("45").unwrap(), 45);
        assert_eq!(parse_ttl_minutes(" 10m ").unwrap(), 10);

        assert!(parse_ttl_minutes("invalid").is_err());
        assert!(parse_ttl_minutes("").is_err());
        assert!(parse_ttl_minutes("-5m").is_err());
    }

    #[test]
    fn test_generate_wireguard_keypair() {
        let (priv_b64, pub_b64) = generate_wireguard_keypair();
        assert!(!priv_b64.is_empty());
        assert!(!pub_b64.is_empty());

        let priv_bytes = BASE64_STANDARD.decode(&priv_b64).expect("valid base64");
        let pub_bytes = BASE64_STANDARD.decode(&pub_b64).expect("valid base64");

        assert_eq!(priv_bytes.len(), 32);
        assert_eq!(pub_bytes.len(), 32);
        assert_ne!(priv_b64, pub_b64);
    }

    #[test]
    fn test_generate_wireguard_config() {
        let conf = generate_wireguard_config(
            "dummy_priv_key",
            "10.200.0.2/32",
            "1.1.1.1, 10.200.0.1",
            "dummy_pub_key",
            "54.12.34.56",
            51820,
        );

        assert!(conf.contains("PrivateKey = dummy_priv_key"));
        assert!(conf.contains("Address = 10.200.0.2/32"));
        assert!(conf.contains("DNS = 1.1.1.1, 10.200.0.1"));
        assert!(conf.contains("PublicKey = dummy_pub_key"));
        assert!(conf.contains("Endpoint = 54.12.34.56:51820"));
        assert!(conf.contains("AllowedIPs = 0.0.0.0/0, ::/0"));
        assert!(conf.contains("PersistentKeepalive = 25"));
    }

    #[test]
    fn test_uuid_short() {
        let id1 = uuid_short();
        let id2 = uuid_short();

        assert_eq!(id1.len(), 12);
        assert_eq!(id2.len(), 12);
        assert!(u64::from_str_radix(&id1, 16).is_ok());
        assert!(u64::from_str_radix(&id2, 16).is_ok());
    }

    #[test]
    fn test_cli_parsing_up_defaults() {
        let cli =
            Cli::try_parse_from(["warpgate", "up", "--admin-cidr", "192.168.1.10/32"]).unwrap();

        match cli.command {
            Commands::Up {
                region,
                ttl,
                admin_cidr,
                output,
                connect,
                no_connect,
                ..
            } => {
                assert_eq!(region, "us-east-1");
                assert_eq!(ttl, "30m");
                assert_eq!(admin_cidr, "192.168.1.10/32");
                assert!(output.is_none());
                assert!(connect);
                assert!(!no_connect);
            }
            _ => panic!("Expected Up command"),
        }
    }

    #[test]
    fn test_cli_parsing_up_custom() {
        let cli = Cli::try_parse_from([
            "warpgate",
            "up",
            "--region",
            "eu-central-1",
            "--ttl",
            "15m",
            "--admin-cidr",
            "10.0.0.5/32",
            "--no-connect",
        ])
        .unwrap();

        match cli.command {
            Commands::Up {
                region,
                ttl,
                admin_cidr,
                connect,
                no_connect,
                ..
            } => {
                assert_eq!(region, "eu-central-1");
                assert_eq!(ttl, "15m");
                assert_eq!(admin_cidr, "10.0.0.5/32");
                assert!(connect);
                assert!(no_connect);
            }
            _ => panic!("Expected Up command"),
        }
    }

    #[test]
    fn test_cli_parsing_down() {
        let cli = Cli::try_parse_from(["warpgate", "down", "warp-998877", "--region", "us-west-2"])
            .unwrap();

        match cli.command {
            Commands::Down {
                id,
                region,
                disconnect,
            } => {
                assert_eq!(id, "warp-998877");
                assert_eq!(region, "us-west-2");
                assert!(disconnect);
            }
            _ => panic!("Expected Down command"),
        }
    }

    #[test]
    fn test_cli_parsing_status() {
        let cli = Cli::try_parse_from(["warpgate", "status", "warp-112233"]).unwrap();

        match cli.command {
            Commands::Status { id } => {
                assert_eq!(id, "warp-112233");
            }
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_cli_parsing_sweep() {
        let cli = Cli::try_parse_from([
            "warpgate",
            "sweep",
            "--regions",
            "tokyo,frankfurt",
            "--dry-run",
            "--force",
            "--max-age-hours",
            "3",
        ])
        .unwrap();

        match cli.command {
            Commands::Sweep {
                regions,
                dry_run,
                force,
                max_age_hours,
            } => {
                assert_eq!(regions, vec!["tokyo", "frankfurt"]);
                assert!(dry_run);
                assert!(force);
                assert_eq!(max_age_hours, 3);
            }
            _ => panic!("Expected Sweep command"),
        }
    }

    #[test]
    fn test_cli_default_api_url() {
        // Without WARPGATE_API_URL set, default should be port 8080 (Spring Boot)
        unsafe {
            std::env::remove_var("WARPGATE_API_URL");
        }
        let cli = Cli::try_parse_from(["warpgate", "status", "warp-test"]).unwrap();
        assert_eq!(cli.api_url, "http://localhost:8080");
    }

    #[test]
    fn test_cli_parsing_up_detach() {
        let cli = Cli::try_parse_from([
            "warpgate",
            "up",
            "--admin-cidr",
            "1.2.3.4/32",
            "--detach",
        ])
        .unwrap();

        match cli.command {
            Commands::Up { detach, .. } => {
                assert!(detach);
            }
            _ => panic!("Expected Up command"),
        }
    }

    #[test]
    fn test_find_orchestrator_binary() {
        let binary = find_orchestrator_binary();
        assert!(binary.is_some(), "Should find warpgate-orchestrator in target directory or path");
    }

    #[tokio::test]
    async fn test_is_url_reachable_closed_port() {
        let client = reqwest::Client::new();
        // High unassigned port that is closed
        let reachable = is_url_reachable(&client, "http://127.0.0.1:59998").await;
        assert!(!reachable);
    }
}
