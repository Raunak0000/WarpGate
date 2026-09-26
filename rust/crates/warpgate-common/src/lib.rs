use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentRequest {
    pub deployment_id: String,
    pub region: String,
    pub ttl_minutes: u32,
    pub client_public_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentResponse {
    pub deployment_id: String,
    pub status: String,
    pub server_ip: Option<String>,
    pub wireguard_port: Option<u16>,
}
