use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeploymentStatus {
    Pending,
    ProvisioningInfra,
    ConfiguringVpn,
    Ready,
    Failed,
    Terminating,
    Destroyed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRequest {
    pub deployment_id: String,
    pub region: String,
    pub ttl_minutes: u32,
    pub client_public_key: String,
    pub admin_cidr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentResponse {
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub server_ip: Option<String>,
    pub wireguard_port: Option<u16>,
    pub server_public_key: Option<String>,
    pub client_ip: Option<String>,
    pub dns_server: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeardownRequest {
    pub deployment_id: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeardownResponse {
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub message: String,
}
