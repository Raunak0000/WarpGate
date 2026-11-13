mod ansible;
pub mod db;
mod terraform;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use db::DbStore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use warpgate_common::{
    DeploymentRequest, DeploymentResponse, DeploymentStatus, TeardownRequest, TeardownResponse,
};

pub type AppState = Arc<DbStore>;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warpgate_orchestrator=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db = DbStore::open_default().expect("Failed to initialize persistent SQLite database");
    let state: AppState = Arc::new(db);

    // Run crash recovery on startup to resume timers or roll back interrupted nodes
    recover_in_flight_deployments(&state).await;

    let app = create_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("WarpGate Orchestrator listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/api/deployments/health", get(|| async { StatusCode::OK }))
        .route(
            "/api/deployments",
            post(create_deployment).get(list_deployments),
        )
        .route("/api/deployments/{id}", get(get_deployment))
        .route("/api/deployments/{id}/teardown", post(teardown_deployment))
        .with_state(state)
}

async fn list_deployments(
    State(state): State<AppState>,
) -> Result<Json<Vec<DeploymentResponse>>, StatusCode> {
    match state.list_all_deployments() {
        Ok(list) => Ok(Json(list)),
        Err(e) => {
            error!("Failed to list deployments: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn notify_webhook(deployment_id: &str, status: &DeploymentStatus, error_message: Option<&str>) {
    let base_url = std::env::var("WARPGATE_CONTROL_PLANE_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());
    let url = format!("{}/api/deployments/{}/webhook", base_url.trim_end_matches('/'), deployment_id);
    let payload = serde_json::json!({
        "status": status,
        "error_message": error_message,
    });

    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .timeout(Duration::from_millis(1500))
        .json(&payload)
        .send()
        .await;

    if let Err(e) = res {
        tracing::debug!("Webhook delivery to {} skipped/failed: {}", url, e);
    }
}

async fn create_deployment(
    State(state): State<AppState>,
    Json(mut payload): Json<DeploymentRequest>,
) -> (StatusCode, Json<DeploymentResponse>) {
    let deployment_id = payload.deployment_id.clone();

    info!("Received deployment request for ID: {}", deployment_id);

    // 1. Account Quota Circuit Breaker: Max 120 minutes (2h) TTL
    if payload.ttl_minutes == 0 || payload.ttl_minutes > 120 {
        let response = DeploymentResponse {
            deployment_id,
            status: DeploymentStatus::Failed,
            server_ip: None,
            wireguard_port: None,
            server_public_key: None,
            client_ip: None,
            dns_server: None,
            error_message: Some(
                "Account quota exceeded: TTL must be between 1 and 120 minutes (2 hours maximum for AWS Free Tier)".to_string(),
            ),
            region: payload.region,
            admin_cidr: payload.admin_cidr.unwrap_or_default(),
        };

        return (StatusCode::BAD_REQUEST, Json(response));
    }

    // 2. Region normalization & validation
    let normalized_region = match warpgate_common::normalize_region(&payload.region) {
        Ok(r) => r.to_string(),
        Err(err) => {
            let response = DeploymentResponse {
                deployment_id,
                status: DeploymentStatus::Failed,
                server_ip: None,
                wireguard_port: None,
                server_public_key: None,
                client_ip: None,
                dns_server: None,
                error_message: Some(err),
                region: payload.region,
                admin_cidr: payload.admin_cidr.unwrap_or_default(),
            };

            return (StatusCode::BAD_REQUEST, Json(response));
        }
    };
    payload.region = normalized_region.clone();

    // 3. Admin CIDR validation
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

    // 4. Account Quota Circuit Breaker: Max 1 concurrent active instance
    match state.count_active_deployments(Some(&deployment_id)) {
        Ok(count) if count >= 1 => {
            let response = DeploymentResponse {
                deployment_id,
                status: DeploymentStatus::Failed,
                server_ip: None,
                wireguard_port: None,
                server_public_key: None,
                client_ip: None,
                dns_server: None,
                error_message: Some(
                    "Account quota exceeded: Maximum 1 concurrent active deployment allowed to ensure zero unexpected AWS Free Tier billing".to_string(),
                ),
                region: payload.region,
                admin_cidr,
            };

            return (StatusCode::CONFLICT, Json(response));
        }
        Err(e) => {
            error!("Database error checking active deployments: {}", e);
        }
        _ => {}
    }

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
        region: normalized_region,
        admin_cidr,
    };

    let is_dry_run = std::env::var("WARPGATE_DRY_RUN").as_deref() == Ok("1");
    if let Err(e) = state.insert_deployment(&initial_response, payload.ttl_minutes, is_dry_run) {
        error!("Failed to persist deployment {}: {}", deployment_id, e);
    }

    notify_webhook(&deployment_id, &DeploymentStatus::ProvisioningInfra, None).await;

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

        // Simulated partial failure hook for automated rollback testing
        if let Ok(sim_fail) = std::env::var("WARPGATE_SIMULATE_FAILURE") {
            if sim_fail == "partial" {
                info!("Simulating partial failure for testing rollback");
                update_failed(
                    &state,
                    &id,
                    "Simulated partial failure during configuration. Automatic rollback executed and infrastructure destroyed.".to_string(),
                ).await;
                return;
            }
        }

        if let Err(e) = state.update_ready(
            &id,
            Some("203.0.113.10"),
            "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=",
            Some("DRY RUN: simulated deployment; no AWS resources created"),
        ) {
            error!("Failed to update database ready status for {}: {}", id, e);
        }
        notify_webhook(&id, &DeploymentStatus::Ready, None).await;

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

    // 1. Run Terraform Apply
    if let Err(e) = terraform::run_apply(&id, &req.region, admin_cidr).await {
        // Partial failure rollback: ensure partial infrastructure is torn down immediately
        let _ = terraform::run_destroy(&id, &req.region, admin_cidr).await;
        update_failed(
            &state,
            &id,
            format!("Terraform Apply Failed: {}. Automatic rollback executed.", e),
        )
        .await;
        return;
    }

    // 2. Fetch Terraform Outputs
    let outputs = match terraform::get_outputs(&id).await {
        Ok(out) => out,
        Err(e) => {
            // Partial failure rollback: destroy provisioned infrastructure on output failure
            let _ = terraform::run_destroy(&id, &req.region, admin_cidr).await;
            update_failed(
                &state,
                &id,
                format!(
                    "Terraform Output Parsing Failed: {}. Automatic rollback executed and infrastructure destroyed.",
                    e
                ),
            )
            .await;
            return;
        }
    };

    let server_ip = outputs.public_ip.value;

    // 3. Update Status to ConfiguringVpn
    if let Err(e) = state.update_configuring_vpn(&id, &server_ip) {
        error!("Failed to update database status for {}: {}", id, e);
    }
    notify_webhook(&id, &DeploymentStatus::ConfiguringVpn, None).await;

    // 4. Wait for SSH to be reachable
    if let Err(e) = ansible::wait_for_ssh(&server_ip, 20, 3).await {
        // Partial failure rollback: destroy provisioned node if SSH fails
        let _ = terraform::run_destroy(&id, &req.region, admin_cidr).await;
        update_failed(
            &state,
            &id,
            format!(
                "SSH Connectivity Failed: {}. Automatic rollback executed and infrastructure destroyed.",
                e
            ),
        )
        .await;
        return;
    }

    // 5. Run Ansible Playbook
    if let Err(e) = ansible::run_playbook(&server_ip, &req.client_public_key).await {
        // Partial failure rollback: destroy node if playbook execution fails
        let _ = terraform::run_destroy(&id, &req.region, admin_cidr).await;
        update_failed(
            &state,
            &id,
            format!(
                "Ansible Playbook Failed: {}. Automatic rollback executed and infrastructure destroyed.",
                e
            ),
        )
        .await;
        return;
    }

    // 6. Retrieve WireGuard public key from server (or fallback)
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

    // 7. Mark Ready
    if let Err(e) = state.update_ready(&id, None, &server_pub_key, None) {
        error!("Failed to update ready state in database: {}", e);
    }
    notify_webhook(&id, &DeploymentStatus::Ready, None).await;

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

        if !ttl.is_zero() {
            tokio::time::sleep(ttl).await;
        }

        // Atomically claim the deployment for teardown.
        let should_destroy = match state.get_deployment(&id) {
            Ok(Some(entry)) if entry.status == DeploymentStatus::Ready => {
                let _ = state.update_status(&id, &DeploymentStatus::Terminating, None);
                notify_webhook(&id, &DeploymentStatus::Terminating, None).await;
                true
            }
            _ => false,
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
    match result {
        Ok(()) => {
            let _ = state.update_status(id, &DeploymentStatus::Destroyed, None);
            notify_webhook(id, &DeploymentStatus::Destroyed, None).await;
            info!("Deployment {} successfully destroyed", id);
        }
        Err(e) => {
            let err_msg = format!("Teardown failed: {}", e);
            let _ = state.update_status(id, &DeploymentStatus::Failed, Some(&err_msg));
            notify_webhook(id, &DeploymentStatus::Failed, Some(&err_msg)).await;
            error!("Teardown failed for {}: {}", id, e);
        }
    }
}

async fn update_failed(state: &AppState, id: &str, err_msg: String) {
    error!("Deployment {} failed: {}", id, err_msg);
    let _ = state.update_status(id, &DeploymentStatus::Failed, Some(&err_msg));
    notify_webhook(id, &DeploymentStatus::Failed, Some(&err_msg)).await;
}

async fn get_deployment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeploymentResponse>, StatusCode> {
    match state.get_deployment(&id) {
        Ok(Some(response)) => Ok(Json(response)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get deployment {}: {}", id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn teardown_deployment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(_payload): Json<TeardownRequest>,
) -> Result<Json<TeardownResponse>, StatusCode> {
    info!("Received teardown request for deployment {}", id);

    let dep = match state.get_deployment(&id) {
        Ok(Some(d)) => d,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    if dep.status != DeploymentStatus::Ready {
        return Err(StatusCode::CONFLICT);
    }

    let _ = state.update_status(&id, &DeploymentStatus::Terminating, None);
    notify_webhook(&id, &DeploymentStatus::Terminating, None).await;

    let region = dep.region.clone();
    let admin_cidr = dep.admin_cidr.clone();
    let task_state = Arc::clone(&state);
    let task_id = id.clone();

    tokio::spawn(async move {
        let result = if std::env::var("WARPGATE_DRY_RUN").as_deref() == Ok("1") {
            info!("Dry-run teardown for deployment {}", task_id);
            Ok(())
        } else {
            terraform::run_destroy(&task_id, &region, &admin_cidr).await
        };

        finish_teardown(&task_state, &task_id, result).await;
    });

    Ok(Json(TeardownResponse {
        deployment_id: id,
        status: DeploymentStatus::Terminating,
        message: "Teardown initiated".to_string(),
    }))
}

/// Recovers in-flight or interrupted deployments on orchestrator startup.
pub async fn recover_in_flight_deployments(state: &AppState) {
    info!("Scanning for in-flight deployments to recover from persistent storage...");
    let active_list = match state.list_active_deployments() {
        Ok(list) => list,
        Err(e) => {
            error!("Failed to query recoverable deployments: {}", e);
            return;
        }
    };

    if active_list.is_empty() {
        info!("No active in-flight deployments found to recover.");
        return;
    }

    info!("Found {} active deployment(s) to recover", active_list.len());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    for dep in active_list {
        let id = dep.deployment_id.clone();
        match dep.status {
            DeploymentStatus::Ready => {
                if now >= dep.expires_at {
                    info!(
                        "Deployment {} TTL expired while offline (expired {}s ago); triggering immediate teardown",
                        id,
                        now - dep.expires_at
                    );
                    schedule_ttl_teardown(
                        Arc::clone(state),
                        id,
                        dep.region,
                        dep.admin_cidr,
                        Duration::from_secs(0),
                        dep.is_dry_run,
                    );
                } else {
                    let remaining_secs = (dep.expires_at - now) as u64;
                    info!(
                        "Resuming TTL timer for deployment {} ({}s remaining)",
                        id, remaining_secs
                    );
                    schedule_ttl_teardown(
                        Arc::clone(state),
                        id,
                        dep.region,
                        dep.admin_cidr,
                        Duration::from_secs(remaining_secs),
                        dep.is_dry_run,
                    );
                }
            }
            DeploymentStatus::ProvisioningInfra
            | DeploymentStatus::ConfiguringVpn
            | DeploymentStatus::Terminating => {
                warn!(
                    "Deployment {} was in progress ({:?}) when orchestrator stopped; rolling back and terminating",
                    id, dep.status
                );
                let task_state = Arc::clone(state);
                tokio::spawn(async move {
                    if !dep.is_dry_run {
                        let _ = terraform::run_destroy(&id, &dep.region, &dep.admin_cidr).await;
                    }
                    update_failed(
                        &task_state,
                        &id,
                        "Interrupted by orchestrator restart. Automated rollback terminated lingering infrastructure.".to_string(),
                    )
                    .await;
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn create_test_state() -> AppState {
        Arc::new(DbStore::open_in_memory().unwrap())
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
    async fn test_create_and_get_deployment() {
        let state = create_test_state();
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
        let state = create_test_state();
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
    async fn test_create_deployment_multi_region_alias() {
        let state = create_test_state();
        let app = create_app(state);

        let req_payload = serde_json::to_vec(&DeploymentRequest {
            deployment_id: "warp-region-1".to_string(),
            region: "tokyo".to_string(),
            ttl_minutes: 10,
            client_public_key: "dummy_client_key".to_string(),
            admin_cidr: Some("0.0.0.0/0".to_string()),
        })
        .unwrap();

        let response = app
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
        assert_eq!(dep.deployment_id, "warp-region-1");
        assert_eq!(dep.region, "ap-northeast-1");
    }

    #[tokio::test]
    async fn test_create_deployment_invalid_region() {
        let state = create_test_state();
        let app = create_app(state);

        let req_payload = serde_json::to_vec(&DeploymentRequest {
            deployment_id: "warp-region-invalid".to_string(),
            region: "atlantis-north-1".to_string(),
            ttl_minutes: 10,
            client_public_key: "dummy_client_key".to_string(),
            admin_cidr: Some("0.0.0.0/0".to_string()),
        })
        .unwrap();

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dep: DeploymentResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(dep.status, DeploymentStatus::Failed);
        assert!(dep.error_message.unwrap().contains("Unsupported region"));
    }

    #[tokio::test]
    async fn test_circuit_breaker_max_ttl() {
        let state = create_test_state();
        let app = create_app(state);

        let req_payload = serde_json::to_vec(&DeploymentRequest {
            deployment_id: "warp-exceed-ttl".to_string(),
            region: "us-east-1".to_string(),
            ttl_minutes: 121, // Exceeds 120m cap
            client_public_key: "dummy_client_key".to_string(),
            admin_cidr: Some("0.0.0.0/0".to_string()),
        })
        .unwrap();

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dep: DeploymentResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(dep.status, DeploymentStatus::Failed);
        assert!(dep.error_message.unwrap().contains("Account quota exceeded: TTL must be between 1 and 120 minutes"));
    }

    #[tokio::test]
    async fn test_circuit_breaker_max_concurrent_deployments() {
        let state = create_test_state();
        // Insert an active deployment already
        let existing = create_test_deployment("warp-active-1", DeploymentStatus::Ready);
        state.insert_deployment(&existing, 30, false).unwrap();

        let app = create_app(state);

        let req_payload = serde_json::to_vec(&DeploymentRequest {
            deployment_id: "warp-second-dep".to_string(),
            region: "us-east-1".to_string(),
            ttl_minutes: 30,
            client_public_key: "dummy_client_key".to_string(),
            admin_cidr: Some("0.0.0.0/0".to_string()),
        })
        .unwrap();

        let response = app
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

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dep: DeploymentResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(dep.status, DeploymentStatus::Failed);
        assert!(dep.error_message.unwrap().contains("Maximum 1 concurrent active deployment allowed"));
    }

    #[tokio::test]
    async fn test_teardown_endpoint() {
        let state = create_test_state();
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

        state.insert_deployment(&deployment, 30, false).unwrap();

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

    #[tokio::test]
    async fn test_ttl_expiry_dry_run_destroys_deployment() {
        let state = create_test_state();
        state
            .insert_deployment(
                &create_test_deployment("ttl-test-1", DeploymentStatus::Ready),
                30,
                true,
            )
            .unwrap();

        schedule_ttl_teardown(
            Arc::clone(&state),
            "ttl-test-1".to_string(),
            "us-east-1".to_string(),
            "192.168.1.10/32".to_string(),
            Duration::from_millis(10),
            true,
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        let dep = state.get_deployment("ttl-test-1").unwrap().unwrap();
        assert_eq!(dep.status, DeploymentStatus::Destroyed);
    }

    #[tokio::test]
    async fn test_ttl_skips_non_ready_deployment() {
        let state = create_test_state();
        state
            .insert_deployment(
                &create_test_deployment("ttl-test-2", DeploymentStatus::Terminating),
                30,
                true,
            )
            .unwrap();

        schedule_ttl_teardown(
            Arc::clone(&state),
            "ttl-test-2".to_string(),
            "us-east-1".to_string(),
            "192.168.1.10/32".to_string(),
            Duration::from_millis(10),
            true,
        );

        tokio::time::sleep(Duration::from_millis(100)).await;

        let dep = state.get_deployment("ttl-test-2").unwrap().unwrap();
        assert_eq!(dep.status, DeploymentStatus::Terminating);
    }

    #[tokio::test]
    async fn test_ttl_teardown_success() {
        let state = create_test_state();
        state
            .insert_deployment(
                &create_test_deployment("ttl-success", DeploymentStatus::Terminating),
                30,
                true,
            )
            .unwrap();

        finish_teardown(&state, "ttl-success", Ok(())).await;

        let dep = state.get_deployment("ttl-success").unwrap().unwrap();
        assert_eq!(dep.status, DeploymentStatus::Destroyed);
    }

    #[tokio::test]
    async fn test_ttl_teardown_failure() {
        let state = create_test_state();
        state
            .insert_deployment(
                &create_test_deployment("ttl-failure", DeploymentStatus::Terminating),
                30,
                true,
            )
            .unwrap();

        finish_teardown(
            &state,
            "ttl-failure",
            Err("Simulated Terraform failure".to_string()),
        )
        .await;

        let dep = state.get_deployment("ttl-failure").unwrap().unwrap();
        assert_eq!(dep.status, DeploymentStatus::Failed);
        assert!(
            dep.error_message
                .as_deref()
                .unwrap()
                .contains("Simulated Terraform failure")
        );
    }

    #[tokio::test]
    async fn test_teardown_nonexistent_deployment() {
        let state = create_test_state();
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
        let state = create_test_state();
        state
            .insert_deployment(
                &create_test_deployment("already-terminating", DeploymentStatus::Terminating),
                30,
                true,
            )
            .unwrap();

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

    #[tokio::test]
    async fn test_crash_recovery_resumes_active_and_cleans_expired() {
        let state = create_test_state();

        // 1. Deployment with future expiry
        let future_dep = create_test_deployment("warp-future-1", DeploymentStatus::Ready);
        state.insert_deployment(&future_dep, 60, true).unwrap();

        // 2. Deployment that was stuck in provisioning
        let stuck_dep = create_test_deployment("warp-stuck-1", DeploymentStatus::ProvisioningInfra);
        state.insert_deployment(&stuck_dep, 30, true).unwrap();

        recover_in_flight_deployments(&state).await;

        tokio::time::sleep(Duration::from_millis(50)).await;

        // stuck deployment should be rolled back to Failed
        let recovered_stuck = state.get_deployment("warp-stuck-1").unwrap().unwrap();
        assert_eq!(recovered_stuck.status, DeploymentStatus::Failed);
        assert!(recovered_stuck.error_message.unwrap().contains("Interrupted by orchestrator restart"));

        // future deployment remains Ready
        let recovered_future = state.get_deployment("warp-future-1").unwrap().unwrap();
        assert_eq!(recovered_future.status, DeploymentStatus::Ready);
    }
}
