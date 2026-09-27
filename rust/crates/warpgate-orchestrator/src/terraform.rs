use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{error, info};

#[derive(Deserialize, Debug)]
pub struct TerraformOutputValue<T> {
    pub value: T,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct TerraformOutputs {
    pub instance_id: TerraformOutputValue<String>,
    pub public_ip: TerraformOutputValue<String>,
    pub private_ip: Option<TerraformOutputValue<String>>,
    pub region: Option<TerraformOutputValue<String>>,
}

fn get_terraform_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WARPGATE_TERRAFORM_DIR") {
        PathBuf::from(dir)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../infra/terraform")
    }
}

pub async fn run_apply(region: &str, admin_cidr: &str) -> Result<(), String> {
    let tf_dir = get_terraform_dir();
    info!(
        "Running 'terraform apply' in {:?} (region={}, admin_cidr={})",
        tf_dir, region, admin_cidr
    );

    let status = Command::new("terraform")
        .arg("apply")
        .arg("-auto-approve")
        .arg(format!("-var=aws_region={}", region))
        .arg(format!("-var=admin_cidr={}", admin_cidr))
        .current_dir(&tf_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn terraform apply: {}", e))?;

    if status.success() {
        info!("Terraform apply completed successfully");
        Ok(())
    } else {
        let err = format!("Terraform apply failed with exit status: {}", status);
        error!("{}", err);
        Err(err)
    }
}

pub async fn get_outputs() -> Result<TerraformOutputs, String> {
    let tf_dir = get_terraform_dir();
    info!("Reading outputs via 'terraform output -json'");
    let output = Command::new("terraform")
        .arg("output")
        .arg("-json")
        .current_dir(&tf_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to execute terraform output: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Terraform output failed: {}", stderr));
    }
    let parsed: TerraformOutputs = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse terraform output JSON: {}", e))?;
    Ok(parsed)
}

pub async fn run_destroy(region: &str, admin_cidr: &str) -> Result<(), String> {
    let tf_dir = get_terraform_dir();
    info!("Running 'terraform destroy' in {:?}", tf_dir);
    let status = Command::new("terraform")
        .arg("destroy")
        .arg("-auto-approve")
        .arg(format!("-var=aws_region={}", region))
        .arg(format!("-var=admin_cidr={}", admin_cidr))
        .current_dir(&tf_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn terraform destroy: {}", e))?;
    if status.success() {
        info!("Terraform destroy completed successfully.");
        Ok(())
    } else {
        let err = format!("Terraform destroy failed with exit status: {}", status);
        error!("{}", err);
        Err(err)
    }
}
