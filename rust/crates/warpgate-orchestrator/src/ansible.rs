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

    let home = std::env::var("HOME").unwrap_or_default();
    let warpgate_key = format!("{}/.ssh/warpgate_key", home);

    let mut cmd = Command::new("ansible-playbook");
    cmd.env("ANSIBLE_HOST_KEY_CHECKING", "False");
    cmd.arg("-i")
        .arg(format!("{},", public_ip))
        .arg("site.yml")
        .arg("-u")
        .arg("ubuntu")
        .arg("-e")
        .arg(format!("wireguard_client_public_key={}", client_public_key));

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
