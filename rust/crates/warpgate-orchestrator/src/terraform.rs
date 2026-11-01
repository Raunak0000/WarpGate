use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
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

fn get_terraform_source_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WARPGATE_TERRAFORM_DIR") {
        PathBuf::from(dir)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../infra/terraform")
    }
}

fn get_deployment_state_root() -> PathBuf {
    if let Ok(dir) = std::env::var("WARPGATE_STATE_DIR") {
        PathBuf::from(dir)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.warpgate/deployments")
    }
}

fn validate_deployment_id(id: &str) -> Result<(), String> {
    let valid = id.strip_prefix("warp-").is_some_and(|suffix| {
        suffix.len() == 12
            && suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });

    if !valid {
        return Err(format!("Invalid deployment ID: {}", id));
    }

    Ok(())
}

fn copy_terraform_config(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|e| format!("Failed to create Terraform directory: {}", e))?;

    for entry in fs::read_dir(source)
        .map_err(|e| format!("Failed to read Terraform source directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        let name = entry.file_name();

        if path.is_dir() {
            if name == ".terraform" || name == ".warpgate" {
                continue;
            }

            let child_destination = destination.join(&name);
            fs::create_dir_all(&child_destination)
                .map_err(|e| format!("Failed to create Terraform subdirectory: {}", e))?;

            copy_terraform_config(&path, &child_destination)?;
        } else {
            let filename = name.to_string_lossy();

            if filename.ends_with(".tf") || filename == ".terraform.lock.hcl" {
                fs::copy(&path, destination.join(name))
                    .map_err(|e| format!("Failed to copy Terraform config: {}", e))?;
            }
        }
    }

    Ok(())
}

fn get_deployment_terraform_dir(deployment_id: &str) -> Result<PathBuf, String> {
    validate_deployment_id(deployment_id)?;

    let source = get_terraform_source_dir();
    let destination = get_deployment_state_root().join(deployment_id);

    if !source.is_dir() {
        return Err(format!(
            "Terraform source directory does not exist: {:?}",
            source
        ));
    }

    copy_terraform_config(&source, &destination)?;

    Ok(destination)
}

pub async fn run_apply(deployment_id: &str, region: &str, admin_cidr: &str) -> Result<(), String> {
    let tf_dir = get_deployment_terraform_dir(deployment_id)?;

    info!(
        "Running Terraform apply for deployment {} in {:?}",
        deployment_id, tf_dir
    );

    let status = Command::new("terraform")
        .arg("init")
        .arg("-input=false")
        .current_dir(&tf_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn terraform init: {}", e))?;

    if !status.success() {
        return Err(format!("Terraform init failed: {}", status));
    }

    let status = Command::new("terraform")
        .arg("apply")
        .arg("-auto-approve")
        .arg("-input=false")
        .arg(format!("-var=deployment_id={}", deployment_id))
        .arg(format!("-var=aws_region={}", region))
        .arg(format!("-var=admin_cidr={}", admin_cidr))
        .current_dir(&tf_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn terraform apply: {}", e))?;

    if status.success() {
        info!("Terraform apply completed for {}", deployment_id);
        Ok(())
    } else {
        let err = format!("Terraform apply failed: {}", status);
        error!("{}", err);
        Err(err)
    }
}

pub async fn get_outputs(deployment_id: &str) -> Result<TerraformOutputs, String> {
    let tf_dir = get_deployment_terraform_dir(deployment_id)?;

    let output = Command::new("terraform")
        .arg("output")
        .arg("-json")
        .current_dir(&tf_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to execute terraform output: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Terraform output failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse Terraform outputs: {}", e))
}

pub async fn run_destroy(
    deployment_id: &str,
    region: &str,
    admin_cidr: &str,
) -> Result<(), String> {
    let tf_dir = get_deployment_terraform_dir(deployment_id)?;

    info!(
        "Running Terraform destroy for deployment {} in {:?}",
        deployment_id, tf_dir
    );

    let status = Command::new("terraform")
        .arg("destroy")
        .arg("-auto-approve")
        .arg("-input=false")
        .arg(format!("-var=deployment_id={}", deployment_id))
        .arg(format!("-var=aws_region={}", region))
        .arg(format!("-var=admin_cidr={}", admin_cidr))
        .current_dir(&tf_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| format!("Failed to spawn terraform destroy: {}", e))?;

    if status.success() {
        info!("Terraform destroy completed for {}", deployment_id);
        Ok(())
    } else {
        let err = format!("Terraform destroy failed: {}", status);
        error!("{}", err);
        Err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_terraform_outputs() {
        let sample_json = r#"{
            "instance_id": {
                "sensitive": false,
                "type": "string",
                "value": "i-0123456789abcdef0"
            },
            "public_ip": {
                "sensitive": false,
                "type": "string",
                "value": "54.210.12.34"
            },
            "private_ip": {
                "sensitive": false,
                "type": "string",
                "value": "10.0.1.50"
            },
            "region": {
                "sensitive": false,
                "type": "string",
                "value": "us-east-1"
            }
        }"#;

        let parsed: TerraformOutputs =
            serde_json::from_str(sample_json).expect("Failed to deserialize outputs");

        assert_eq!(parsed.instance_id.value, "i-0123456789abcdef0");
        assert_eq!(parsed.public_ip.value, "54.210.12.34");
        assert_eq!(
            parsed.private_ip.map(|p| p.value),
            Some("10.0.1.50".to_string())
        );
        assert_eq!(
            parsed.region.map(|r| r.value),
            Some("us-east-1".to_string())
        );
    }

    #[test]
    fn test_deployment_id_validation() {
        assert!(validate_deployment_id("warp-a1b2c3d4e5f6").is_ok());
        assert!(validate_deployment_id("warp-ABCDEF123456").is_err());
        assert!(validate_deployment_id("warp-123").is_err());
        assert!(validate_deployment_id("../outside").is_err());
    }

    #[test]
    fn test_copy_terraform_config_excludes_state_and_secrets() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();

        let root = std::env::temp_dir().join(format!("warpgate-test-{}", unique));
        let source = root.join("source");
        let destination = root.join("destination");

        fs::create_dir_all(source.join("modules/example")).unwrap();

        fs::write(source.join("main.tf"), "terraform config").unwrap();
        fs::write(source.join(".terraform.lock.hcl"), "provider lock").unwrap();
        fs::write(source.join("terraform.tfstate"), "state data").unwrap();
        fs::write(source.join("saved.tfplan"), "plan data").unwrap();
        fs::write(source.join("terraform.tfvars"), "secret values").unwrap();
        fs::create_dir_all(source.join(".terraform")).unwrap();
        fs::write(source.join(".terraform/cache"), "provider cache").unwrap();
        fs::write(source.join("modules/example/main.tf"), "module config").unwrap();

        copy_terraform_config(&source, &destination).unwrap();

        assert!(destination.join("main.tf").exists());
        assert!(destination.join(".terraform.lock.hcl").exists());
        assert!(destination.join("modules/example/main.tf").exists());

        assert!(!destination.join("terraform.tfstate").exists());
        assert!(!destination.join("saved.tfplan").exists());
        assert!(!destination.join("terraform.tfvars").exists());
        assert!(!destination.join(".terraform").exists());

        fs::remove_dir_all(root).unwrap();
    }
}
