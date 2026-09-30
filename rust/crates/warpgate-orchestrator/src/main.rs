mod ansible;
mod terraform;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::{collections::HashMap, matches};
use tokio::sync::RwLock;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use warpgate_common::{
    DeploymentRequest, DeploymentResponse, DeploymentStatus, TeardownRequest, TeardownResponse,
};

type AppState = Arc<RwLock<HashMap<String, DeploymentResponse>>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warpgate_orchestrator=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state: AppState = Arc::new(RwLock::new(HashMap::new()));

    let app = Router::new()
        .route("/api/deployments", post(create_deployment))
        .route("/api/deployments/{id}", get(get_deployment))
        .route("/api/deployments/{id}/teardown", post(teardown_deployment))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("WarpGate Orchestrator listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_deployment(
    State(state): State<AppState>,
    Json(payload): Json<DeploymentRequest>,
) -> (StatusCode, Json<DeploymentResponse>) {
    let deployment_id = payload.deployment_id.clone();
    info!("Received deployment request for ID: {}", deployment_id);

    let initial_response = DeploymentResponse {
        deployment_id: deployment_id.clone(),
        status: DeploymentStatus::ProvisioningInfra,
        server_ip: None,
        wireguard_port: Some(51820),
        server_public_key: None,
        client_ip: Some("10.200.0.2/32".to_string()),
        dns_server: Some("10.200.0.1".to_string()),
        error_message: None,
    };

    {
        let mut store = state.write().await;
        store.insert(deployment_id.clone(), initial_response.clone());
    }

    // Spawn async background provisioning workflow
    let task_state = Arc::clone(&state);
    tokio::spawn(async move {
        run_deployment_pipeline(task_state, payload).await;
    });

    (StatusCode::ACCEPTED, Json(initial_response))
}

async fn run_deployment_pipeline(state: AppState, req: DeploymentRequest) {
    let id = req.deployment_id.clone();
    let admin_cidr = req.admin_cidr.as_deref().unwrap_or("0.0.0.0/0");

    // Run Terraform Apply
    if let Err(e) = terraform::run_apply(&req.region, admin_cidr).await {
        update_failed(&state, &id, format!("Terraform Apply Failed: {}", e)).await;
        return;
    }

    // Fetch Terraform Outputs
    let outputs = match terraform::get_outputs().await {
        Ok(out) => out,
        Err(e) => {
            update_failed(
                &state,
                &id,
                format!("Terraform Output Parsing Failed: {}", e),
            )
            .await;
            return;
        }
    };

    let server_ip = outputs.public_ip.value;

    // Update Status to ConfiguringVpn
    {
        let mut store = state.write().await;
        if let Some(entry) = store.get_mut(&id) {
            entry.status = DeploymentStatus::ConfiguringVpn;
            entry.server_ip = Some(server_ip.clone());
        }
    }

    // Wait for SSH to be reachable
    if let Err(e) = ansible::wait_for_ssh(&server_ip, 20, 3).await {
        update_failed(&state, &id, format!("SSH Connectivity Failed: {}", e)).await;
        return;
    }

    // Run Ansible Playbook
    if let Err(e) = ansible::run_playbook(&server_ip, &req.client_public_key).await {
        update_failed(&state, &id, format!("Ansible Playbook Failed: {}", e)).await;
        return;
    }

    // Mark Ready
    {
        let mut store = state.write().await;
        if let Some(entry) = store.get_mut(&id) {
            entry.status = DeploymentStatus::Ready;
            // The real server public key corresponding to wg0.conf.j2 private key
            entry.server_public_key =
                Some("M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=".to_string());
        }
    }
    info!("Deployment {} successfully provisioned and configured!", id);

    let ttl_minutes = req.ttl_minutes;
    let auto_state = Arc::clone(&state);
    let auto_id = id.clone();
    let auto_region = req.region.clone();
    let auto_admin_cidr = admin_cidr.to_string();

    tokio::spawn(async move {
        info!(
            "TTL timer started for {} ({} minutes)",
            auto_id, ttl_minutes
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(ttl_minutes as u64 * 60)).await;

        let should_destroy = {
            let store = auto_state.read().await;
            matches!(
                store.get(&auto_id).map(|d| &d.status),
                Some(DeploymentStatus::Ready)
            )
        };

        if should_destroy {
            info!(
                "TTL expired for deployment {}. Initiating automated teardown...",
                auto_id
            );
            {
                let mut store = auto_state.write().await;
                if let Some(entry) = store.get_mut(&auto_id) {
                    entry.status = DeploymentStatus::Terminating;
                }
            }
            let res = terraform::run_destroy(&auto_region, &auto_admin_cidr).await;
            let mut store = auto_state.write().await;
            if let Some(entry) = store.get_mut(&auto_id) {
                match res {
                    Ok(_) => {
                        info!("Automated TTL teardown completed for {}", auto_id);
                        entry.status = DeploymentStatus::Destroyed;
                    }
                    Err(e) => {
                        error!("Automated TTL teardown FAILED for {}: {}", auto_id, e);
                        entry.status = DeploymentStatus::Failed;
                        entry.error_message = Some(format!("TTL teardown failed: {}", e));
                    }
                }
            }
        }
    });
}

async fn update_failed(state: &AppState, id: &str, err_msg: String) {
    error!("Deployment {} failed: {}", id, err_msg);
    let mut store = state.write().await;
    if let Some(entry) = store.get_mut(id) {
        entry.status = DeploymentStatus::Failed;
        entry.error_message = Some(err_msg);
    }
}

async fn get_deployment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeploymentResponse>, StatusCode> {
    let store = state.read().await;
    match store.get(&id) {
        Some(response) => Ok(Json(response.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn teardown_deployment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<TeardownRequest>,
) -> Result<Json<TeardownResponse>, StatusCode> {
    info!("Received teardown request for deployment {}", id);

    {
        let mut store = state.write().await;
        if let Some(entry) = store.get_mut(&id) {
            entry.status = DeploymentStatus::Terminating;
        }
    }

    let task_state = Arc::clone(&state);
    let task_id = id.clone();
    tokio::spawn(async move {
        // Run terraform destroy
        let res = terraform::run_destroy(&payload.region, "0.0.0.0/0").await;
        let mut store = task_state.write().await;
        if let Some(entry) = store.get_mut(&task_id) {
            match res {
                Ok(_) => entry.status = DeploymentStatus::Destroyed,
                Err(e) => {
                    entry.status = DeploymentStatus::Failed;
                    entry.error_message = Some(format!("Teardown failed: {}", e));
                }
            }
        }
    });

    Ok(Json(TeardownResponse {
        deployment_id: id,
        status: DeploymentStatus::Terminating,
        message: "Teardown initiated".to_string(),
    }))
}
