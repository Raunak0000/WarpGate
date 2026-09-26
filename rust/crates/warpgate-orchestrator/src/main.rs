use axum::{
    routing::post,
    Router,
    Json,
};
use warpgate_common::{DeploymentRequest, DeploymentResponse};
use std::{net::SocketAddr, println, process::Stdio};
use tokio::process::Command;

async fn run_terraform(deployment_id: &str, region: &str) -> Result<(), String> {
    println!("Starting terraform for deployment {} in region {}", deployment_id, region);

    let tf_dir = "../../infra/terraform";

    let mut child = Command::new("terraform")
        .arg("apply")
        .arg("--auto-approve")
        .arg(format!("--var-region={}", region))
        .current_dir(tf_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn terraform: {}", e))?;

    let status = child.wait().await
        .map_err(|e| format!("Failed to wait on terraform: {}", e))?;

    if status.success() {
        println!("Terraform applied successfully for {}", deployment_id);
        Ok(())
    } else {
        Err(format!("Terraform failed with status: {}", status))
    }
}

async fn create_deployment(Json(payload): Json<DeploymentRequest>) -> Json<DeploymentResponse> {
    println!("Received deployment request for ID: {}", payload.deployment_id);

    // run tf async
    let status_msg = match run_terraform(&payload.deployment_id, &payload.region).await {
        Ok(_) => "success".to_string(),
        Err(e) => {
            println!("Error: {}", e);
            "failed".to_string()
        }
    };

    let response = DeploymentResponse {
        deployment_id: payload.deployment_id,
        status: status_msg,
        server_ip: Some("203.0.113.10".to_string()),
        wireguard_port: Some(51280),
    };

    Json(response)
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/deploy", post(create_deployment));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Orchestrator listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}