use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{error, info, warn};

fn get_ansible_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WARPGATE_ANSIBLE_DIR") {
        PathBuf::from(dir)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../infra/ansible")
    }
}

/// Polls port 22 on the target IP until SSH is listening.
pub async fn wait_for_ssh(ip: &str, max_retries: u32, delay_secs: u64) -> Result<(), String> {
    let target = format!("{}:22", ip);
    info!("Waiting for SSH to become ready on {}...", target);

    for attempt in 1..=max_retries {
        match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&target)).await {
            Ok(Ok(_stream)) => {
                info!(
                    "SSH port 22 is open on {} (attempt {}/{})",
                    ip, attempt, max_retries
                );
                // Give cloud-init / sshd an additional 5 seconds to settle
                sleep(Duration::from_secs(5)).await;
                return Ok(());
            }
            _ => {
                warn!(
                    "SSH not ready yet on {} ({}/{}). Retrying in {}s...",
                    ip, attempt, max_retries, delay_secs
                );
                sleep(Duration::from_secs(delay_secs)).await;
            }
        }
    }

    Err(format!("Timed out waiting for SSH on {}", target))
}

/// Executes ansible-playbook against the instance public IP.
pub async fn run_playbook(public_ip: &str, client_public_key: &str) -> Result<(), String> {
    let ansible_dir = get_ansible_dir();
    info!(
        "Running ansible-playbook against IP {} from {:?}",
        public_ip, ansible_dir
    );

    let warpgate_key = std::env::var("WARPGATE_SSH_KEY").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.ssh/warpgate_key", home)
    });

    let mut cmd = Command::new("ansible-playbook");
    cmd.env("ANSIBLE_HOST_KEY_CHECKING", "False");
    cmd.arg("-i")
        .arg(format!("{},", public_ip))
        .arg("site.yml")
        .arg("-u")
        .arg("ubuntu")
        .arg("-e")
        .arg(format!("wireguard_client_public_key={}", client_public_key));

    if std::env::var("WARPGATE_GOLDEN_AMI").as_deref() == Ok("1")
        || std::env::var("WARPGATE_AMI_ID").is_ok_and(|v| !v.trim().is_empty())
    {
        cmd.arg("-e").arg("warpgate_prebaked=true");
    }

    if std::path::Path::new(&warpgate_key).exists() {
        cmd.arg("--private-key").arg(&warpgate_key);
    }

    let status = cmd
        .current_dir(&ansible_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn ansible-playbook: {}", e))?;

    if status.success() {
        info!("Ansible playbook finished successfully.");
        Ok(())
    } else {
        let err = format!("Ansible playbook failed with status: {}", status);
        error!("{}", err);
        Err(err)
    }
}

/// Retrieves the WireGuard server's public key over SSH.
pub async fn get_server_public_key(public_ip: &str) -> Result<String, String> {
    let warpgate_key = std::env::var("WARPGATE_SSH_KEY").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.ssh/warpgate_key", home)
    });

    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ConnectTimeout=10");

    if std::path::Path::new(&warpgate_key).exists() {
        cmd.arg("-i").arg(&warpgate_key);
    }

    cmd.arg(format!("ubuntu@{}", public_ip))
        .arg("sudo wg show wg0 public-key");

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to execute SSH: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to retrieve WireGuard public key: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let key = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if key.is_empty() {
        return Err("WireGuard server public key was empty".to_string());
    }

    Ok(key)
}

fn build_ssh_command(public_ip: &str, remote_cmd: &str) -> Command {
    let warpgate_key = std::env::var("WARPGATE_SSH_KEY").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.ssh/warpgate_key", home)
    });

    let mut cmd = Command::new("ssh");
    cmd.arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ConnectTimeout=10");

    if std::path::Path::new(&warpgate_key).exists() {
        cmd.arg("-i").arg(&warpgate_key);
    }

    cmd.arg(format!("ubuntu@{}", public_ip)).arg(remote_cmd);
    cmd
}

/// Dynamically injects a WireGuard peer into the running wg0 interface via netlink (`wg set`).
pub async fn add_wireguard_peer(
    public_ip: &str,
    client_public_key: &str,
    allowed_ip: &str,
) -> Result<(), String> {
    warpgate_common::validate_wireguard_public_key(client_public_key)?;
    let remote_cmd = format!(
        "sudo wg set wg0 peer {} allowed-ips {} && sudo sh -c 'wg-quick save wg0 || true'",
        client_public_key.trim(),
        allowed_ip.trim()
    );
    let output = build_ssh_command(public_ip, &remote_cmd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute SSH for wg set: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to add WireGuard peer: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    info!("Injected WireGuard peer {} ({}) on {}", client_public_key, allowed_ip, public_ip);
    Ok(())
}

/// Dynamically removes a WireGuard peer from the running wg0 interface via netlink (`wg set`).
pub async fn remove_wireguard_peer(
    public_ip: &str,
    client_public_key: &str,
) -> Result<(), String> {
    warpgate_common::validate_wireguard_public_key(client_public_key)?;
    let remote_cmd = format!(
        "sudo wg set wg0 peer {} remove && sudo sh -c 'wg-quick save wg0 || true'",
        client_public_key.trim()
    );
    let output = build_ssh_command(public_ip, &remote_cmd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute SSH for wg remove: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to remove WireGuard peer: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    info!("Removed WireGuard peer {} from {}", client_public_key, public_ip);
    Ok(())
}
