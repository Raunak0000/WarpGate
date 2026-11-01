mod ansible;
mod terraform;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
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
    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("WarpGate Orchestrator listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/api/deployments", post(create_deployment))
        .route("/api/deployments/{id}", get(get_deployment))
        .route("/api/deployments/{id}/teardown", post(teardown_deployment))
        .with_state(state)
}

async fn create_deployment(
    State(state): State<AppState>,
    Json(payload): Json<DeploymentRequest>,
) -> (StatusCode, Json<DeploymentResponse>) {
    let deployment_id = payload.deployment_id.clone();

    info!("Received deployment request for ID: {}", deployment_id);

    let Some(admin_cidr) = payload.admin_cidr.clone() else {
        let response = DeploymentResponse {
            deployment_id,
            status: DeploymentStatus::Failed,
            server_ip: None,
            wireguard_port: None,
            server_public_key: None,
            client_ip: None,
            dns_server: None,
            error_message: Some("admin_cidr is required".to_string()),
            region: payload.region,
            admin_cidr: String::new(),
        };

        return (StatusCode::BAD_REQUEST, Json(response));
    };

    let dns =
        std::env::var("WARPGATE_DNS_SERVER").unwrap_or_else(|_| "1.1.1.1, 10.200.0.1".to_string());

    let initial_response = DeploymentResponse {
        deployment_id: deployment_id.clone(),
        status: DeploymentStatus::ProvisioningInfra,
        server_ip: None,
        wireguard_port: Some(51820),
        server_public_key: None,
        client_ip: Some("10.200.0.2/32".to_string()),
        dns_server: Some(dns),
        error_message: None,
        region: payload.region.clone(),
        admin_cidr,
    };

    {
        let mut store = state.write().await;
        store.insert(deployment_id.clone(), initial_response.clone());
    }

    let task_state = Arc::clone(&state);

    tokio::spawn(async move {
        run_deployment_pipeline(task_state, payload).await;
    });

    (StatusCode::ACCEPTED, Json(initial_response))
}

async fn run_deployment_pipeline(state: AppState, req: DeploymentRequest) {
    let id = req.deployment_id.clone();
    let admin_cidr = req
        .admin_cidr
        .as_deref()
        .expect("admin_cidr must be validated before provisioning");
    // Safe integration test: never invokes Terraform or Ansible.
    if std::env::var("WARPGATE_DRY_RUN").as_deref() == Ok("1") {
        info!("Dry run enabled for deployment {}", id);

        {
            let mut store = state.write().await;

            if let Some(entry) = store.get_mut(&id) {
                entry.status = DeploymentStatus::Ready;
                entry.server_ip = Some("203.0.113.10".to_string());
                entry.server_public_key =
                    Some("M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=".to_string());
                entry.error_message =
                    Some("DRY RUN: simulated deployment; no AWS resources created".to_string());
            }
        }

        schedule_ttl_teardown(
            Arc::clone(&state),
            id.clone(),
            req.region.clone(),
            admin_cidr.to_string(),
            Duration::from_secs(req.ttl_minutes as u64 * 60),
            true,
        );

        return;
    }

    // Run Terraform Apply
    if let Err(e) = terraform::run_apply(&id, &req.region, admin_cidr).await {
        update_failed(&state, &id, format!("Terraform Apply Failed: {}", e)).await;
        return;
    }

    // Fetch Terraform Outputs
    let outputs = match terraform::get_outputs(&id).await {
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

    // Retrieve WireGuard public key from server (or fallback)
    let server_pub_key = match ansible::get_server_public_key(&server_ip).await {
        Ok(key) => key,
        Err(e) => {
            warn!(
                "Failed to retrieve dynamic public key: {}. Falling back to default key.",
                e
            );
            "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=".to_string()
        }
    };

    // Mark Ready
    {
        let mut store = state.write().await;
        if let Some(entry) = store.get_mut(&id) {
            entry.status = DeploymentStatus::Ready;
            entry.server_public_key = Some(server_pub_key);
        }
    }
    info!("Deployment {} successfully provisioned and configured!", id);

    schedule_ttl_teardown(
        Arc::clone(&state),
        id.clone(),
        req.region.clone(),
        admin_cidr.to_string(),
        Duration::from_secs(req.ttl_minutes as u64 * 60),
        false,
    );
}

fn schedule_ttl_teardown(
    state: AppState,
    id: String,
    region: String,
    admin_cidr: String,
    ttl: Duration,
    dry_run: bool,
) {
    tokio::spawn(async move {
        info!("TTL timer started for {} ({:?})", id, ttl);

        tokio::time::sleep(ttl).await;

        // Atomically claim the deployment for teardown.
        let should_destroy = {
            let mut store = state.write().await;

            match store.get_mut(&id) {
                Some(entry) if entry.status == DeploymentStatus::Ready => {
                    entry.status = DeploymentStatus::Terminating;
                    true
                }
                _ => false,
            }
        };

        if !should_destroy {
            info!(
                "Skipping TTL teardown for {} because it is no longer READY",
                id
            );
            return;
        }

        let result = if dry_run {
            info!("DRY RUN: simulated TTL teardown for {}", id);
            Ok(())
        } else {
            terraform::run_destroy(&id, &region, &admin_cidr).await
        };

        finish_teardown(&state, &id, result).await;

        info!("TTL teardown processing completed for {}", id);
    });
}

async fn finish_teardown(state: &AppState, id: &str, result: Result<(), String>) {
    let mut store = state.write().await;

    if let Some(entry) = store.get_mut(id) {
        match result {
            Ok(()) => {
                entry.status = DeploymentStatus::Destroyed;
                entry.error_message = None;
            }
            Err(e) => {
                entry.status = DeploymentStatus::Failed;
                entry.error_message = Some(format!("Teardown failed: {}", e));
            }
        }
    }
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
    Json(_payload): Json<TeardownRequest>,
) -> Result<Json<TeardownResponse>, StatusCode> {
    info!("Received teardown request for deployment {}", id);

    // Atomically claim the deployment for teardown and retrieve
    // the original infrastructure configuration.
    let (region, admin_cidr) = {
        let mut store = state.write().await;

        let entry = store.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

        if entry.status != DeploymentStatus::Ready {
            return Err(StatusCode::CONFLICT);
        }

        entry.status = DeploymentStatus::Terminating;

        (entry.region.clone(), entry.admin_cidr.clone())
    };

    let task_state = Arc::clone(&state);
    let task_id = id.clone();

    tokio::spawn(async move {
        let result = if std::env::var("WARPGATE_DRY_RUN").as_deref() == Ok("1") {
            info!("Dry-run teardown for deployment {}", task_id);
            Ok(())
        } else {
            terraform::run_destroy(&task_id, &region, &admin_cidr).await
        };

        let mut store = task_state.write().await;

        if let Some(entry) = store.get_mut(&task_id) {
            match result {
                Ok(_) => {
                    entry.status = DeploymentStatus::Destroyed;
                    info!("Deployment {} successfully destroyed", task_id);
                }
                Err(e) => {
                    entry.status = DeploymentStatus::Failed;
                    entry.error_message = Some(format!("Teardown failed: {}", e));
                    error!("Teardown failed for {}: {}", task_id, e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_create_and_get_deployment() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));
        let app = create_app(state);

        let req_payload = serde_json::to_vec(&DeploymentRequest {
            deployment_id: "warp-test-1".to_string(),
            region: "us-east-1".to_string(),
            ttl_minutes: 5,
            client_public_key: "dummy_client_key".to_string(),
            admin_cidr: Some("0.0.0.0/0".to_string()),
        })
        .unwrap();

        // 1. POST /api/deployments
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/deployments")
                    .header("content-type", "application/json")
                    .body(Body::from(req_payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dep: DeploymentResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(dep.deployment_id, "warp-test-1");
        assert_eq!(dep.status, DeploymentStatus::ProvisioningInfra);

        // 2. GET /api/deployments/warp-test-1
        let get_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/deployments/warp-test-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(get_response.status(), StatusCode::OK);
        let get_bytes = get_response.into_body().collect().await.unwrap().to_bytes();
        let get_dep: DeploymentResponse = serde_json::from_slice(&get_bytes).unwrap();
        assert_eq!(get_dep.deployment_id, "warp-test-1");
    }

    #[tokio::test]
    async fn test_get_deployment_not_found() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));
        let app = create_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/deployments/nonexistent-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_teardown_endpoint() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        let deployment = DeploymentResponse {
            deployment_id: "warp-test-td".to_string(),
            status: DeploymentStatus::Ready,
            server_ip: Some("203.0.113.10".to_string()),
            wireguard_port: Some(51820),
            server_public_key: None,
            client_ip: Some("10.200.0.2/32".to_string()),
            dns_server: Some("1.1.1.1".to_string()),
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "192.168.1.10/32".to_string(),
        };

        state
            .write()
            .await
            .insert("warp-test-td".to_string(), deployment);

        let app = create_app(Arc::clone(&state));

        let teardown_payload = serde_json::to_vec(&TeardownRequest {
            deployment_id: "warp-test-td".to_string(),
            region: "us-east-1".to_string(),
        })
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/deployments/warp-test-td/teardown")
                    .header("content-type", "application/json")
                    .body(Body::from(teardown_payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let td_resp: TeardownResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(td_resp.deployment_id, "warp-test-td");
        assert_eq!(td_resp.status, DeploymentStatus::Terminating);
    }

    fn create_test_deployment(id: &str, status: DeploymentStatus) -> DeploymentResponse {
        DeploymentResponse {
            deployment_id: id.to_string(),
            status,
            server_ip: Some("203.0.113.10".to_string()),
            wireguard_port: Some(51820),
            server_public_key: None,
            client_ip: Some("10.200.0.2/32".to_string()),
            dns_server: Some("1.1.1.1".to_string()),
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "192.168.1.10/32".to_string(),
        }
    }

    #[tokio::test]
    async fn test_ttl_expiry_dry_run_destroys_deployment() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        state.write().await.insert(
            "ttl-test-1".to_string(),
            create_test_deployment("ttl-test-1", DeploymentStatus::Ready),
        );

        schedule_ttl_teardown(
            Arc::clone(&state),
            "ttl-test-1".to_string(),
            "us-east-1".to_string(),
            "192.168.1.10/32".to_string(),
            Duration::from_millis(10),
            true,
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        let store = state.read().await;
        assert_eq!(
            store.get("ttl-test-1").unwrap().status,
            DeploymentStatus::Destroyed
        );
    }

    #[tokio::test]
    async fn test_ttl_skips_non_ready_deployment() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        state.write().await.insert(
            "ttl-test-2".to_string(),
            create_test_deployment("ttl-test-2", DeploymentStatus::Terminating),
        );

        schedule_ttl_teardown(
            Arc::clone(&state),
            "ttl-test-2".to_string(),
            "us-east-1".to_string(),
            "192.168.1.10/32".to_string(),
            Duration::from_millis(10),
            true,
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        let store = state.read().await;
        assert_eq!(
            store.get("ttl-test-2").unwrap().status,
            DeploymentStatus::Terminating
        );
    }

    #[tokio::test]
    async fn test_ttl_teardown_success() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        state.write().await.insert(
            "ttl-success".to_string(),
            create_test_deployment("ttl-success", DeploymentStatus::Terminating),
        );

        finish_teardown(&state, "ttl-success", Ok(())).await;

        let store = state.read().await;
        assert_eq!(
            store.get("ttl-success").unwrap().status,
            DeploymentStatus::Destroyed
        );
    }

    #[tokio::test]
    async fn test_ttl_teardown_failure() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        state.write().await.insert(
            "ttl-failure".to_string(),
            create_test_deployment("ttl-failure", DeploymentStatus::Terminating),
        );

        finish_teardown(
            &state,
            "ttl-failure",
            Err("Simulated Terraform failure".to_string()),
        )
        .await;

        let store = state.read().await;
        let deployment = store.get("ttl-failure").unwrap();

        assert_eq!(deployment.status, DeploymentStatus::Failed);
        assert!(
            deployment
                .error_message
                .as_deref()
                .unwrap()
                .contains("Simulated Terraform failure")
        );
    }

    #[tokio::test]
    async fn test_teardown_nonexistent_deployment() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));
        let app = create_app(state);

        let payload = serde_json::to_vec(&TeardownRequest {
            deployment_id: "missing".to_string(),
            region: "us-east-1".to_string(),
        })
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/deployments/missing/teardown")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_teardown_already_terminating() {
        let state: AppState = Arc::new(RwLock::new(HashMap::new()));

        state.write().await.insert(
            "already-terminating".to_string(),
            create_test_deployment("already-terminating", DeploymentStatus::Terminating),
        );

        let app = create_app(Arc::clone(&state));

        let payload = serde_json::to_vec(&TeardownRequest {
            deployment_id: "already-terminating".to_string(),
            region: "us-east-1".to_string(),
        })
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/deployments/already-terminating/teardown")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
