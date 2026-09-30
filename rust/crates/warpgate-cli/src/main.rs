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
    /// URL of the WarpGate API (Orchestrator or Spring Boot Control Plane)
    #[arg(
        long,
        env = "WARPGATE_API_URL",
        default_value = "http://localhost:3000"
    )]
    api_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Provision a new ephemeral WireGuard VPN node
    Up {
        /// Target AWS region
        #[arg(short, long, default_value = "us-east-1")]
        region: String,

        /// Time-to-Live before automated teardown (e.g. '30m', '1h', '45')
        #[arg(short, long, default_value = "30m")]
        ttl: String,

        /// Allowed admin CIDR for SSH access
        #[arg(long, default_value = "0.0.0.0/0")]
        admin_cidr: String,

        /// Save client WireGuard configuration file to this path
        #[arg(short, long)]
        output: Option<PathBuf>,
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

        /// AWS region where node was deployed
        #[arg(short, long, default_value = "us-east-1")]
        region: String,
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
        } => {
            let ttl_minutes = parse_ttl_minutes(&ttl)?;
            let deployment_id = format!("warp-{}", &uuid_short());

            // 1. Generate client WireGuard keypair locally
            let (client_priv, client_pub) = generate_wireguard_keypair();

            println!(
                "Requesting WarpGate deployment '{}' in region '{}' (TTL: {}m)...",
                deployment_id, region, ttl_minutes
            );

            let req_body = DeploymentRequest {
                deployment_id: deployment_id.clone(),
                region: region.clone(),
                ttl_minutes,
                client_public_key: client_pub,
                admin_cidr: Some(admin_cidr),
            };

            let res = client
                .post(format!("{}/api/deployments", cli.api_url))
                .json(&req_body)
                .send()
                .await?;

            if !res.status().is_success() {
                eprintln!("Failed to submit deployment: {}", res.status());
                std::process::exit(1);
            }

            println!("Deployment initiated. Polling status until ready...");

            // 2. Poll status until Ready or Failed
            let mut last_status: Option<DeploymentStatus> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(4)).await;

                let status_res = client
                    .get(format!("{}/api/deployments/{}", cli.api_url, deployment_id))
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
                            break;
                        }
                        DeploymentStatus::Failed => {
                            eprintln!(
                                "\nDeployment failed: {}",
                                dep.error_message.as_deref().unwrap_or("Unknown error")
                            );
                            std::process::exit(1);
                        }
                        _ => {}
                    }
                    last_status = Some(dep.status);
                }
            }
        }

        Commands::Status { id } => {
            let res = client
                .get(format!("{}/api/deployments/{}", cli.api_url, id))
                .send()
                .await?;

            if res.status().as_u16() == 404 {
                println!("Deployment '{}' not found.", id);
                return Ok(());
            }

            let dep: DeploymentResponse = res.json().await?;
            println!("{:#?}", dep);
        }

        Commands::Down { id, region } => {
            println!("Terminating deployment '{}' in region '{}'...", id, region);
            let req_body = TeardownRequest {
                deployment_id: id.clone(),
                region,
            };

            let res = client
                .post(format!("{}/api/deployments/{}/teardown", cli.api_url, id))
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
        }
    }

    Ok(())
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
    let r: u32 = rand::random();
    format!("{:06x}", r & 0xffffff)
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
    let dns = dep.dns_server.as_deref().unwrap_or("10.200.0.1");

    let conf = format!(
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
    );

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
