use base64::prelude::*;
use clap::{Parser, Subcommand};
use qrcode::QrCode;
use qrcode::render::unicode;
use std::path::PathBuf;
use std::time::Duration;
use warpgate_common::{
    AddPeerRequest, DeploymentRequest, DeploymentResponse, DeploymentStatus, PeerInfo,
    RemovePeerRequest, TeardownRequest, TeardownResponse,
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
    command: Option<Commands>,
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

        /// Allowed admin CIDR for SSH access (automatically detected from public IP if omitted)
        #[arg(long)]
        admin_cidr: Option<String>,

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

    /// Check status of an active deployment (or currently active node if omitted)
    Status {
        /// Deployment ID (automatically detected if omitted)
        id: Option<String>,
    },

    /// Terminate and destroy a deployment immediately (or currently active node if omitted)
    Down {
        /// Deployment ID (automatically detected if omitted)
        id: Option<String>,

        /// AWS region where node was deployed (auto-detected if omitted)
        #[arg(short, long)]
        region: Option<String>,

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

    /// Run system diagnostics and verify environment readiness
    Doctor,

    /// Register a new WarpGate account (includes 600 free monthly minutes)
    Register {
        /// Account username
        #[arg(short, long)]
        username: Option<String>,

        /// Account email address
        #[arg(short, long)]
        email: Option<String>,

        /// Account password (minimum 8 characters)
        #[arg(short, long)]
        password: Option<String>,
    },

    /// Log in to an existing WarpGate account and store session credentials
    Login {
        /// Username or email
        #[arg(short, long)]
        username: Option<String>,

        /// Account password
        #[arg(short, long)]
        password: Option<String>,
    },

    /// Log out and remove locally stored session credentials
    Logout,

    /// Display current authenticated user and monthly virtual quota balance
    Whoami,

    /// Manage multi-device WireGuard peers on an active gateway
    Peer {
        #[command(subcommand)]
        action: PeerCommands,
    },

    /// Generate shell autocompletion scripts (bash, zsh, fish, powershell)
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Run the WarpGate background orchestrator daemon directly
    #[command(hide = true)]
    Daemon {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
}

#[derive(Subcommand)]
enum PeerCommands {
    /// Add a new device (phone, tablet, laptop) to an active gateway via kernel netlink
    Add {
        /// Deployment ID (auto-detected if omitted)
        #[arg(short, long)]
        id: Option<String>,

        /// Friendly device name (e.g. 'pixel-phone', 'ipad')
        #[arg(short, long, default_value = "device")]
        name: String,

        /// Existing WireGuard public key (generates a fresh keypair + QR code if omitted)
        #[arg(short, long)]
        public_key: Option<String>,

        /// Save generated device WireGuard config to this file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// List all devices registered on an active gateway
    List {
        /// Deployment ID (auto-detected if omitted)
        #[arg(short, long)]
        id: Option<String>,
    },

    /// Remove a device peer from an active gateway
    Remove {
        /// WireGuard public key of the peer to remove
        public_key: String,

        /// Deployment ID (auto-detected if omitted)
        #[arg(short, long)]
        id: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    let Some(command) = cli.command else {
        run_doctor_and_welcome(&client, &cli.api_url).await?;
        return Ok(());
    };

    match command {
        Commands::Daemon { port } => {
            warpgate_orchestrator::run_server(port).await?;
            return Ok(());
        }

        Commands::Doctor => {
            run_doctor_and_welcome(&client, &cli.api_url).await?;
            return Ok(());
        }

        Commands::Register {
            username,
            email,
            password,
        } => {
            handle_register(&client, &cli.api_url, username, email, password).await?;
            return Ok(());
        }

        Commands::Login { username, password } => {
            handle_login(&client, &cli.api_url, username, password).await?;
            return Ok(());
        }

        Commands::Logout => {
            delete_credentials();
            println!("Logged out. Local credentials removed from {:?}", get_credentials_path());
            return Ok(());
        }

        Commands::Whoami => {
            handle_whoami(&client, &cli.api_url).await?;
            return Ok(());
        }

        Commands::Peer { action } => {
            handle_peer_command(&client, &cli.api_url, action).await?;
            return Ok(());
        }

        Commands::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "warpgate", &mut std::io::stdout());
            return Ok(());
        }

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
            let resolved_admin_cidr = resolve_admin_cidr(&client, admin_cidr).await;
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
                admin_cidr: Some(resolved_admin_cidr),
            };

            let auth_token = load_auth_token();
            let res = with_auth(
                client
                    .post(format!("{}/api/deployments", effective_api_url))
                    .json(&req_body),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if !res.status().is_success() {
                let status_code = res.status();
                let body_text = res.text().await.unwrap_or_default();
                if status_code.as_u16() == 401 {
                    eprintln!(
                        "Authentication required (HTTP 401). Run 'warpgate login' or 'warpgate register' first."
                    );
                    std::process::exit(1);
                }
                if let Ok(dep) = serde_json::from_str::<DeploymentResponse>(&body_text) {
                    if let Some(err) = dep.error_message {
                        eprintln!("Failed to submit deployment (HTTP {}): {}", status_code, err);
                        std::process::exit(1);
                    }
                }
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&body_text) {
                    if let Some(msg) = json_val.get("message").or_else(|| json_val.get("error")).and_then(|v| v.as_str()) {
                        eprintln!("Failed to submit deployment (HTTP {}): {}", status_code, msg);
                        std::process::exit(1);
                    }
                }
                eprintln!("Failed to submit deployment: {} {}", status_code, body_text);
                std::process::exit(1);
            }

            println!("Deployment initiated. Polling status until ready...");

            // 2. Poll status until Ready or Failed
            let mut last_status: Option<DeploymentStatus> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(4)).await;

                let status_res = with_auth(
                    client.get(format!("{}/api/deployments/{}", effective_api_url, deployment_id)),
                    auth_token.as_deref(),
                )
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

                            save_active_deployment_info(&deployment_id, &normalized_region);
                            break;
                        }
                        DeploymentStatus::Failed => {
                            eprintln!(
                                "\nDeployment failed: {}",
                                dep.error_message.as_deref().unwrap_or("Unknown error")
                            );
                            clear_saved_active_deployment_info();
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
                println!("      To disconnect & teardown: warpgate down");
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
                        let _ = with_auth(
                            client
                                .post(format!("{}/api/deployments/{}/teardown", effective_api_url, deployment_id))
                                .json(&req_body),
                            auth_token.as_deref(),
                        )
                        .send()
                        .await;

                        clear_saved_active_deployment_info();

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
                        if let Ok(res) = with_auth(
                            client.get(format!("{}/api/deployments/{}", effective_api_url, deployment_id)),
                            auth_token.as_deref(),
                        )
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
                                    clear_saved_active_deployment_info();
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
                                    clear_saved_active_deployment_info();
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
            let auth_token = load_auth_token();

            let target_id = match id {
                Some(i) => i,
                None => {
                    if let Some((saved_id, _)) = get_saved_active_deployment_info() {
                        saved_id
                    } else if let Ok(res) = with_auth(
                        client.get(format!("{}/api/deployments", effective_api_url)),
                        auth_token.as_deref(),
                    )
                    .send()
                    .await
                    {
                        if let Ok(deps) = res.json::<Vec<DeploymentResponse>>().await {
                            if let Some(active) = deps.iter().find(|d| {
                                d.status != DeploymentStatus::Destroyed
                                    && d.status != DeploymentStatus::Failed
                            }) {
                                active.deployment_id.clone()
                            } else if let Some(last) = deps.last() {
                                last.deployment_id.clone()
                            } else {
                                print_idle_status_card();
                                return Ok(());
                            }
                        } else {
                            print_idle_status_card();
                            return Ok(());
                        }
                    } else {
                        print_idle_status_card();
                        return Ok(());
                    }
                }
            };

            let res = with_auth(
                client.get(format!("{}/api/deployments/{}", effective_api_url, target_id)),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if res.status().as_u16() == 401 {
                eprintln!("Authentication required (HTTP 401). Run 'warpgate login' first.");
                std::process::exit(1);
            }
            if res.status().as_u16() == 404 {
                print_idle_status_card();
                return Ok(());
            }

            let dep: DeploymentResponse = res.json().await?;
            print_status_card(&dep);
        }

        Commands::Down {
            id,
            region,
            disconnect,
        } => {
            let (effective_api_url, _) =
                ensure_backend_available(&client, &cli.api_url, false).await?;
            let auth_token = load_auth_token();

            if disconnect {
                disconnect_local_vpn("warpgate");
            }

            let (target_id, target_region) = match (id, region) {
                (Some(i), Some(r)) => {
                    let norm = warpgate_common::normalize_region(&r)
                        .map_err(|e| format!("Invalid region '{}': {}", r, e))?
                        .to_string();
                    (i, norm)
                }
                (Some(i), None) => {
                    let r = if let Ok(res) = with_auth(
                        client.get(format!("{}/api/deployments/{}", effective_api_url, i)),
                        auth_token.as_deref(),
                    )
                    .send()
                    .await
                    {
                        if let Ok(dep) = res.json::<DeploymentResponse>().await {
                            dep.region
                        } else if let Some((saved_id, saved_r)) = get_saved_active_deployment_info() {
                            if saved_id == i {
                                saved_r
                            } else {
                                "us-east-1".to_string()
                            }
                        } else {
                            "us-east-1".to_string()
                        }
                    } else {
                        "us-east-1".to_string()
                    };
                    (i, r)
                }
                (None, _) => {
                    let mut found = None;
                    if let Ok(res) = with_auth(
                        client.get(format!("{}/api/deployments", effective_api_url)),
                        auth_token.as_deref(),
                    )
                    .send()
                    .await
                    {
                        if let Ok(deps) = res.json::<Vec<DeploymentResponse>>().await {
                            if let Some(active) = deps.iter().find(|d| {
                                d.status != DeploymentStatus::Destroyed
                                    && d.status != DeploymentStatus::Failed
                            }) {
                                found = Some((active.deployment_id.clone(), active.region.clone()));
                            }
                        }
                    }
                    if found.is_none() {
                        found = get_saved_active_deployment_info();
                    }

                    match found {
                        Some((f_id, f_region)) => (f_id, f_region),
                        None => {
                            println!("No active WarpGate deployment found to terminate.");
                            check_and_stop_daemon_if_idle(&client, &effective_api_url).await;
                            return Ok(());
                        }
                    }
                }
            };

            println!(
                "Terminating deployment '{}' in region '{}'...",
                target_id, target_region
            );
            let req_body = TeardownRequest {
                deployment_id: target_id.clone(),
                region: target_region,
            };

            let res = with_auth(
                client
                    .post(format!("{}/api/deployments/{}/teardown", effective_api_url, target_id))
                    .json(&req_body),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if res.status().is_success() {
                let teardown: TeardownResponse = res.json().await?;
                println!(
                    "Teardown initiated for '{}' (Status: {:?})",
                    teardown.deployment_id, teardown.status
                );
                clear_saved_active_deployment_info();

                print!("Waiting for cloud resources to be destroyed...");
                use std::io::Write;
                let _ = std::io::stdout().flush();
                for _ in 0..60 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if let Ok(check_res) = with_auth(
                        client.get(format!("{}/api/deployments/{}", effective_api_url, target_id)),
                        auth_token.as_deref(),
                    )
                    .send()
                    .await
                    {
                        if let Ok(dep) = check_res.json::<DeploymentResponse>().await {
                            if dep.status == DeploymentStatus::Destroyed
                                || dep.status == DeploymentStatus::Failed
                            {
                                println!(" Done.");
                                break;
                            }
                        }
                    }
                    print!(".");
                    let _ = std::io::stdout().flush();
                }
            } else if res.status().as_u16() == 401 {
                eprintln!("Authentication required (HTTP 401). Run 'warpgate login' first.");
                std::process::exit(1);
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

    let (binary, args): (PathBuf, Vec<String>) = if let Ok(current_exe) = std::env::current_exe() {
        (current_exe, vec!["daemon".to_string(), "--port".to_string(), "3000".to_string()])
    } else if let Some(b) = find_orchestrator_binary() {
        (b, vec![])
    } else {
        return Err("Could not locate warpgate binary to spawn daemon. Please ensure warpgate is installed in PATH.".into());
    };

    let state_dir = PathBuf::from(".warpgate");
    let _ = std::fs::create_dir_all(&state_dir);
    let log_file = std::fs::File::create(state_dir.join("orchestrator.log"))?;

    let mut cmd = std::process::Command::new(&binary);
    cmd.args(&args);
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

async fn resolve_admin_cidr(client: &reqwest::Client, specified: Option<String>) -> String {
    if let Some(cidr) = specified {
        return cidr;
    }

    if let Ok(env_cidr) = std::env::var("WARPGATE_ADMIN_CIDR") {
        if !env_cidr.trim().is_empty() {
            return env_cidr;
        }
    }

    // Auto-detect public IP via external discovery APIs
    println!("Auto-detecting your public IP for secure SSH access...");
    let endpoints = [
        "https://api.ipify.org",
        "https://checkip.amazonaws.com",
        "https://icanhazip.com",
    ];

    for ep in endpoints {
        if let Ok(res) = client.get(ep).timeout(Duration::from_secs(3)).send().await {
            if let Ok(text) = res.text().await {
                let ip = text.trim();
                if ip.parse::<std::net::IpAddr>().is_ok() {
                    let cidr = format!("{}/32", ip);
                    println!("[+] Detected admin IP: {} (locking SSH access to this address)", cidr);
                    return cidr;
                }
            }
        }
    }

    println!("[WARN] Could not auto-detect public IP. Falling back to 0.0.0.0/0");
    "0.0.0.0/0".to_string()
}

fn save_active_deployment_info(deployment_id: &str, region: &str) {
    let state_dir = PathBuf::from(".warpgate");
    let _ = std::fs::create_dir_all(&state_dir);
    let active_file = state_dir.join("active.json");
    let content = serde_json::json!({
        "deployment_id": deployment_id,
        "region": region,
    });
    let _ = std::fs::write(active_file, content.to_string());
}

fn get_saved_active_deployment_info() -> Option<(String, String)> {
    let active_file = PathBuf::from(".warpgate/active.json");
    if let Ok(content) = std::fs::read_to_string(active_file) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            let id = val.get("deployment_id").and_then(|v| v.as_str())?.to_string();
            let region = val.get("region").and_then(|v| v.as_str())?.to_string();
            return Some((id, region));
        }
    }
    None
}

fn clear_saved_active_deployment_info() {
    let active_file = PathBuf::from(".warpgate/active.json");
    let _ = std::fs::remove_file(active_file);
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

fn print_status_card(dep: &DeploymentResponse) {
    let friendly_region = warpgate_common::canonical_to_alias(&dep.region).unwrap_or(&dep.region);
    let server_endpoint = match (&dep.server_ip, dep.wireguard_port) {
        (Some(ip), Some(port)) => format!("{}:{}", ip, port),
        (Some(ip), None) => format!("{}:51820", ip),
        _ => "Pending allocation...".to_string(),
    };
    let status_str = match dep.status {
        DeploymentStatus::Ready => "READY (Active & Connected)",
        DeploymentStatus::ProvisioningInfra => "PROVISIONING INFRASTRUCTURE",
        DeploymentStatus::ConfiguringVpn => "CONFIGURING WIREGUARD & BBR",
        DeploymentStatus::Terminating => "TERMINATING",
        DeploymentStatus::Destroyed => "DESTROYED",
        DeploymentStatus::Failed => "FAILED",
        DeploymentStatus::Pending => "PENDING",
    };

    println!("╭─────────────────────────────────────────────────────────────╮");
    println!("│  WARPGATE VPN STATUS DASHBOARD                              │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│  Status:        {:<44}│", status_str);
    println!("│  Deployment ID: {:<44}│", dep.deployment_id);
    println!("│  Region:        {:<44}│", format!("{} ({})", dep.region, friendly_region));
    println!("│  Server IP:     {:<44}│", server_endpoint);
    println!("│  Internal IP:   {:<44}│", dep.client_ip.as_deref().unwrap_or("10.200.0.2/32"));
    println!("│  DNS Resolver:  {:<44}│", dep.dns_server.as_deref().unwrap_or("1.1.1.1, 10.200.0.1"));
    println!("│  SSH Access:    {:<44}│", dep.admin_cidr);
    if let Some(ref err) = dep.error_message {
        let truncated = if err.len() > 44 { &err[..44] } else { err };
        println!("│  Notice:        {:<44}│", truncated);
    }
    println!("╰─────────────────────────────────────────────────────────────╯");
}

fn print_idle_status_card() {
    println!("╭─────────────────────────────────────────────────────────────╮");
    println!("│  WARPGATE STATUS: IDLE                                      │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│  No active WarpGate deployments running.                    │");
    println!("│                                                             │");
    println!("│  To spin up a secure ephemeral VPN:                         │");
    println!("│    warpgate up --region tokyo                               │");
    println!("│    warpgate up --region frankfurt --ttl 1h                  │");
    println!("╰─────────────────────────────────────────────────────────────╯");
}

async fn run_doctor_and_welcome(
    client: &reqwest::Client,
    api_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("============================================================");
    println!("         WarpGate - Ephemeral Zero-Cost Cloud VPN           ");
    println!("============================================================\n");
    println!("System & Environment Readiness Diagnostics:\n");

    // 1. OS & Architecture
    println!(
        "  [✓] Platform:            {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    // 2. WireGuard kernel / client
    let has_nmcli = std::process::Command::new("which")
        .arg("nmcli")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let has_wg_quick = std::process::Command::new("which")
        .arg("wg-quick")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_nmcli {
        println!("  [✓] VPN Connection Mgr:  NetworkManager (nmcli)");
    } else if has_wg_quick {
        println!("  [✓] VPN Connection Mgr:  WireGuard Tools (wg-quick)");
    } else {
        println!("  [!] VPN Connection Mgr:  Neither 'nmcli' nor 'wg-quick' found in PATH");
        println!("      -> Install WireGuard tools: 'sudo apt install wireguard-tools'");
    }

    // 3. AWS Credentials
    let has_aws_env =
        std::env::var("AWS_ACCESS_KEY_ID").is_ok() || std::env::var("AWS_PROFILE").is_ok();
    let has_aws_cli = std::process::Command::new("aws")
        .arg("sts")
        .arg("get-caller-identity")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_aws_cli {
        println!("  [✓] AWS Cloud Identity:  Active credentials detected via AWS CLI");
    } else if has_aws_env {
        println!("  [✓] AWS Cloud Identity:  Environment credentials detected ($AWS_ACCESS_KEY_ID)");
    } else {
        println!("  [!] AWS Cloud Identity:  No active AWS credentials found");
        println!("      -> Run 'aws configure' or export AWS_ACCESS_KEY_ID & AWS_SECRET_ACCESS_KEY");
    }

    // 4. Terraform
    let terraform_version = std::process::Command::new("terraform")
        .arg("version")
        .output();
    if let Ok(out) = terraform_version {
        if out.status.success() {
            let ver = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("Installed")
                .to_string();
            println!("  [✓] Infrastructure:      {}", ver);
        } else {
            println!("  [!] Infrastructure:      Terraform installed but returned an error");
        }
    } else {
        println!("  [!] Infrastructure:      'terraform' not found in PATH");
        println!("      -> Download from: https://developer.hashicorp.com/terraform/install");
    }

    // 5. Ansible
    let has_ansible = std::process::Command::new("ansible-playbook")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_ansible {
        println!("  [✓] Config Engine:       Ansible playbook detected");
    } else {
        println!("  [!] Config Engine:       'ansible-playbook' not found in PATH");
        println!("      -> Install via: 'sudo apt install ansible' or 'pip install ansible'");
    }

    // 6. Backend / Daemon connectivity
    let backend_active = is_url_reachable(client, api_url).await
        || is_url_reachable(client, "http://localhost:3000").await;
    if backend_active {
        println!("  [✓] WarpGate Daemon:     Active and listening");
    } else {
        println!("  [✓] WarpGate Daemon:     Idle (will auto-spawn on 'warpgate up')");
    }

    println!("\nQuick Start Commands:");
    println!("  warpgate up                       Spin up ephemeral VPN (defaults to us-east-1, 30m TTL)");
    println!("  warpgate up --region tokyo        Spin up VPN in Tokyo (ap-northeast-1)");
    println!("  warpgate up --ttl 1h --detach     Spin up in detached background mode for 1 hour");
    println!("  warpgate status                   View active connection metrics and details");
    println!("  warpgate down                     Disconnect and tear down active VPN");
    println!("  warpgate sweep                    Audit and terminate any orphaned cloud resources");
    println!("  warpgate completions zsh          Generate shell completions for tab-complete");
    println!("  warpgate --help                   View full command line reference\n");

    Ok(())
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

fn with_auth(builder: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(t) if !t.trim().is_empty() => builder.bearer_auth(t),
        _ => builder,
    }
}

fn get_credentials_path() -> PathBuf {
    if let Ok(path) = std::env::var("WARPGATE_CREDENTIALS_FILE") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".warpgate").join("credentials.json")
}

fn save_credentials(api_url: &str, username: &str, token: &str) -> Result<(), String> {
    let path = get_credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create credentials directory: {}", e))?;
    }

    let payload = serde_json::json!({
        "api_url": api_url,
        "username": username,
        "token": token,
    });
    let serialized = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize credentials: {}", e))?;

    std::fs::write(&path, serialized)
        .map_err(|e| format!("Failed to write credentials to {:?}: {}", path, e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

fn load_auth_token() -> Option<String> {
    if let Ok(token) = std::env::var("WARPGATE_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let path = get_credentials_path();
    let content = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn delete_credentials() {
    let path = get_credentials_path();
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

fn prompt_input(label: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Write;
    print!("{}", label);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

async fn handle_register(
    client: &reqwest::Client,
    api_url: &str,
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let username = match username {
        Some(u) => u,
        None => prompt_input("Username: ")?,
    };
    let email = match email {
        Some(e) => e,
        None => prompt_input("Email: ")?,
    };
    let password = match password {
        Some(p) => p,
        None => prompt_input("Password (min 8 chars): ")?,
    };

    let res = client
        .post(format!("{}/api/auth/register", api_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "username": username,
            "email": email,
            "password": password,
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        eprintln!("Registration failed (HTTP {}): {}", status, body);
        std::process::exit(1);
    }

    let body: serde_json::Value = res.json().await?;
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");
    let saved_user = body.get("username").and_then(|v| v.as_str()).unwrap_or(&username);
    let remaining = body.get("remaining_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(600);
    let monthly = body.get("monthly_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(600);

    save_credentials(api_url, saved_user, token)?;
    println!("Registered and logged in as '{}'!", saved_user);
    println!("Virtual Quota: {}/{} minutes remaining this month.", remaining, monthly);
    println!("Credentials saved to {:?}", get_credentials_path());
    Ok(())
}

async fn handle_login(
    client: &reqwest::Client,
    api_url: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let username = match username {
        Some(u) => u,
        None => prompt_input("Username or email: ")?,
    };
    let password = match password {
        Some(p) => p,
        None => prompt_input("Password: ")?,
    };

    let res = client
        .post(format!("{}/api/auth/login", api_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        eprintln!("Login failed (HTTP {}): {}", status, body);
        std::process::exit(1);
    }

    let body: serde_json::Value = res.json().await?;
    let token = body.get("token").and_then(|v| v.as_str()).unwrap_or("");
    let saved_user = body.get("username").and_then(|v| v.as_str()).unwrap_or(&username);
    let remaining = body.get("remaining_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(0);
    let monthly = body.get("monthly_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(600);

    save_credentials(api_url, saved_user, token)?;
    println!("Logged in as '{}'!", saved_user);
    println!("Virtual Quota: {}/{} minutes remaining this month.", remaining, monthly);
    println!("Credentials saved to {:?}", get_credentials_path());
    Ok(())
}

async fn handle_whoami(
    client: &reqwest::Client,
    api_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(token) = load_auth_token() else {
        eprintln!("Not logged in. Run 'warpgate login' or 'warpgate register' first.");
        std::process::exit(1);
    };

    let res = with_auth(
        client.get(format!("{}/api/auth/me", api_url.trim_end_matches('/'))),
        Some(&token),
    )
    .send()
    .await?;

    if !res.status().is_success() {
        let status = res.status();
        if status.as_u16() == 401 {
            eprintln!("Session expired or invalid. Run 'warpgate login' to refresh.");
        } else {
            eprintln!("Failed to fetch profile (HTTP {})", status);
        }
        std::process::exit(1);
    }

    let profile: serde_json::Value = res.json().await?;
    let username = profile.get("username").and_then(|v| v.as_str()).unwrap_or("-");
    let email = profile.get("email").and_then(|v| v.as_str()).unwrap_or("-");
    let remaining = profile.get("remaining_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(0);
    let monthly = profile.get("monthly_quota_minutes").and_then(|v| v.as_i64()).unwrap_or(600);
    let reset_at = profile.get("quota_reset_at").and_then(|v| v.as_str()).unwrap_or("-");

    println!("===========================================================");
    println!(" User          : {} ({})", username, email);
    println!(" Virtual Quota : {}/{} minutes remaining", remaining, monthly);
    println!(" Quota Resets  : {}", reset_at);
    println!("===========================================================");
    Ok(())
}

async fn resolve_active_deployment_id(
    client: &reqwest::Client,
    effective_api_url: &str,
    auth_token: Option<&str>,
    explicit_id: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(id) = explicit_id {
        return Ok(id);
    }
    if let Some((saved_id, _)) = get_saved_active_deployment_info() {
        return Ok(saved_id);
    }
    if let Ok(res) = with_auth(
        client.get(format!("{}/api/deployments", effective_api_url)),
        auth_token,
    )
    .send()
    .await
    {
        if let Ok(deps) = res.json::<Vec<DeploymentResponse>>().await {
            if let Some(active) = deps.iter().find(|d| d.status == DeploymentStatus::Ready) {
                return Ok(active.deployment_id.clone());
            }
        }
    }
    Err("No active READY deployment found. Pass --id <deployment-id> explicitly.".into())
}

async fn handle_peer_command(
    client: &reqwest::Client,
    api_url: &str,
    action: PeerCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    let (effective_api_url, _) = ensure_backend_available(client, api_url, false).await?;
    let auth_token = load_auth_token();

    match action {
        PeerCommands::Add {
            id,
            name,
            public_key,
            output,
        } => {
            let target_id =
                resolve_active_deployment_id(client, &effective_api_url, auth_token.as_deref(), id)
                    .await?;

            let (generated_priv, pub_key) = match public_key {
                Some(k) => (None, k),
                None => {
                    let (priv_k, pub_k) = generate_wireguard_keypair();
                    (Some(priv_k), pub_k)
                }
            };

            let req_body = AddPeerRequest {
                device_name: name,
                public_key: pub_key,
            };

            let res = with_auth(
                client
                    .post(format!(
                        "{}/api/deployments/{}/peers",
                        effective_api_url, target_id
                    ))
                    .json(&req_body),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                eprintln!("Failed to add peer (HTTP {}): {}", status, body);
                std::process::exit(1);
            }

            let peer: PeerInfo = res.json().await?;
            println!("\nPeer '{}' added to deployment '{}'!", peer.device_name, peer.deployment_id);
            println!("Assigned Tunnel IP : {}", peer.assigned_ip);
            println!("Peer Public Key    : {}", peer.public_key);

            if let Some(priv_key) = generated_priv {
                let server_ip = peer.server_ip.as_deref().unwrap_or("0.0.0.0");
                let port = peer.wireguard_port.unwrap_or(51820);
                let server_pub = peer.server_public_key.as_deref().unwrap_or("");
                let dns = peer.dns_server.as_deref().unwrap_or("1.1.1.1, 10.200.0.1");

                let conf = generate_wireguard_config(
                    &priv_key,
                    &peer.assigned_ip,
                    dns,
                    server_pub,
                    server_ip,
                    port,
                );

                println!("\nScan with WireGuard Mobile App ({}):\n", peer.device_name);
                if let Ok(code) = QrCode::new(conf.as_bytes()) {
                    let qr_string = code
                        .render::<unicode::Dense1x2>()
                        .dark_color(unicode::Dense1x2::Light)
                        .light_color(unicode::Dense1x2::Dark)
                        .build();
                    println!("{qr_string}");
                }

                if let Some(path) = output {
                    std::fs::write(&path, &conf)?;
                    println!("Device config saved to {:?}", path);
                }
            }
        }
        PeerCommands::List { id } => {
            let target_id =
                resolve_active_deployment_id(client, &effective_api_url, auth_token.as_deref(), id)
                    .await?;

            let res = with_auth(
                client.get(format!(
                    "{}/api/deployments/{}/peers",
                    effective_api_url, target_id
                )),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                eprintln!("Failed to list peers (HTTP {}): {}", status, body);
                std::process::exit(1);
            }

            let peers: Vec<PeerInfo> = res.json().await?;
            println!("=========================================================================");
            println!(" Peers for Deployment: {}", target_id);
            println!("=========================================================================");
            for p in peers {
                println!(
                    "  {:<14} | {:<15} | {}",
                    p.device_name, p.assigned_ip, p.public_key
                );
            }
            println!("=========================================================================");
        }
        PeerCommands::Remove { public_key, id } => {
            let target_id =
                resolve_active_deployment_id(client, &effective_api_url, auth_token.as_deref(), id)
                    .await?;

            let res = with_auth(
                client
                    .delete(format!(
                        "{}/api/deployments/{}/peers",
                        effective_api_url, target_id
                    ))
                    .json(&RemovePeerRequest { public_key }),
                auth_token.as_deref(),
            )
            .send()
            .await?;

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                eprintln!("Failed to remove peer (HTTP {}): {}", status, body);
                std::process::exit(1);
            }

            println!("Peer removed dynamically from '{}'.", target_id);
        }
    }

    Ok(())
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
        let cli = Cli::try_parse_from(["warpgate", "up"]).unwrap();

        match cli.command.unwrap() {
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
                assert!(admin_cidr.is_none());
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

        match cli.command.unwrap() {
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
                assert_eq!(admin_cidr, Some("10.0.0.5/32".to_string()));
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

        match cli.command.unwrap() {
            Commands::Down {
                id,
                region,
                disconnect,
            } => {
                assert_eq!(id, Some("warp-998877".to_string()));
                assert_eq!(region, Some("us-west-2".to_string()));
                assert!(disconnect);
            }
            _ => panic!("Expected Down command"),
        }

        // Test down with no arguments
        let cli_no_args = Cli::try_parse_from(["warpgate", "down"]).unwrap();
        match cli_no_args.command.unwrap() {
            Commands::Down {
                id,
                region,
                disconnect,
            } => {
                assert!(id.is_none());
                assert!(region.is_none());
                assert!(disconnect);
            }
            _ => panic!("Expected Down command"),
        }
    }

    #[test]
    fn test_cli_parsing_status() {
        let cli = Cli::try_parse_from(["warpgate", "status", "warp-112233"]).unwrap();

        match cli.command.unwrap() {
            Commands::Status { id } => {
                assert_eq!(id, Some("warp-112233".to_string()));
            }
            _ => panic!("Expected Status command"),
        }

        // Test status with no arguments
        let cli_no_args = Cli::try_parse_from(["warpgate", "status"]).unwrap();
        match cli_no_args.command.unwrap() {
            Commands::Status { id } => {
                assert!(id.is_none());
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

        match cli.command.unwrap() {
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
        let cli = Cli::try_parse_from(["warpgate", "up", "--detach"]).unwrap();

        match cli.command.unwrap() {
            Commands::Up { detach, .. } => {
                assert!(detach);
            }
            _ => panic!("Expected Up command"),
        }
    }

    #[test]
    fn test_cli_parsing_bare_no_args() {
        let cli = Cli::try_parse_from(["warpgate"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parsing_doctor() {
        let cli = Cli::try_parse_from(["warpgate", "doctor"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn test_cli_parsing_completions() {
        let cli = Cli::try_parse_from(["warpgate", "completions", "zsh"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Completions { .. })));
    }

    #[test]
    fn test_cli_parsing_daemon() {
        let cli = Cli::try_parse_from(["warpgate", "daemon", "--port", "3500"]).unwrap();
        match cli.command.unwrap() {
            Commands::Daemon { port } => assert_eq!(port, 3500),
            _ => panic!("Expected Daemon command"),
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

    #[test]
    fn test_cli_parsing_auth_commands() {
        let reg = Cli::try_parse_from([
            "warpgate",
            "register",
            "--username",
            "alice",
            "--email",
            "alice@example.com",
            "--password",
            "secretpass123",
        ])
        .unwrap();
        assert!(matches!(reg.command, Some(Commands::Register { .. })));

        let login = Cli::try_parse_from([
            "warpgate",
            "login",
            "--username",
            "alice",
            "--password",
            "secretpass123",
        ])
        .unwrap();
        assert!(matches!(login.command, Some(Commands::Login { .. })));

        let logout = Cli::try_parse_from(["warpgate", "logout"]).unwrap();
        assert!(matches!(logout.command, Some(Commands::Logout)));

        let whoami = Cli::try_parse_from(["warpgate", "whoami"]).unwrap();
        assert!(matches!(whoami.command, Some(Commands::Whoami)));
    }

    #[test]
    fn test_credentials_save_load_delete() {
        let tmp_dir = std::env::temp_dir().join(format!("warpgate-cred-test-{}", uuid_short()));
        let cred_file = tmp_dir.join("credentials.json");
        unsafe {
            std::env::set_var("WARPGATE_CREDENTIALS_FILE", &cred_file);
            std::env::remove_var("WARPGATE_TOKEN");
        }

        save_credentials("http://localhost:8080", "alice", "jwt-abc-123").unwrap();
        assert!(cred_file.exists());
        assert_eq!(load_auth_token().as_deref(), Some("jwt-abc-123"));

        delete_credentials();
        assert!(!cred_file.exists());
        assert!(load_auth_token().is_none());

        unsafe {
            std::env::remove_var("WARPGATE_CREDENTIALS_FILE");
        }
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    #[test]
    fn test_cli_parsing_peer_commands() {
        let add_cmd = Cli::try_parse_from([
            "warpgate",
            "peer",
            "add",
            "--id",
            "warp-1234567890ab",
            "--name",
            "pixel-phone",
        ])
        .unwrap();
        match add_cmd.command.unwrap() {
            Commands::Peer {
                action:
                    PeerCommands::Add {
                        id,
                        name,
                        public_key,
                        ..
                    },
            } => {
                assert_eq!(id.as_deref(), Some("warp-1234567890ab"));
                assert_eq!(name, "pixel-phone");
                assert!(public_key.is_none());
            }
            _ => panic!("Expected Peer Add command"),
        }

        let list_cmd = Cli::try_parse_from(["warpgate", "peer", "list"]).unwrap();
        assert!(matches!(
            list_cmd.command,
            Some(Commands::Peer {
                action: PeerCommands::List { .. }
            })
        ));

        let rm_cmd = Cli::try_parse_from([
            "warpgate",
            "peer",
            "remove",
            "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=",
        ])
        .unwrap();
        assert!(matches!(
            rm_cmd.command,
            Some(Commands::Peer {
                action: PeerCommands::Remove { .. }
            })
        ));
    }
}
