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
    pub region: String,
    pub admin_cidr: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_status_serialization() {
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::Pending).unwrap(),
            "\"PENDING\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::ProvisioningInfra).unwrap(),
            "\"PROVISIONING_INFRA\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::ConfiguringVpn).unwrap(),
            "\"CONFIGURING_VPN\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::Ready).unwrap(),
            "\"READY\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::Failed).unwrap(),
            "\"FAILED\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::Terminating).unwrap(),
            "\"TERMINATING\""
        );
        assert_eq!(
            serde_json::to_string(&DeploymentStatus::Destroyed).unwrap(),
            "\"DESTROYED\""
        );
    }

    #[test]
    fn test_deployment_status_deserialization() {
        assert_eq!(
            serde_json::from_str::<DeploymentStatus>("\"PENDING\"").unwrap(),
            DeploymentStatus::Pending
        );
        assert_eq!(
            serde_json::from_str::<DeploymentStatus>("\"PROVISIONING_INFRA\"").unwrap(),
            DeploymentStatus::ProvisioningInfra
        );
        assert_eq!(
            serde_json::from_str::<DeploymentStatus>("\"READY\"").unwrap(),
            DeploymentStatus::Ready
        );
        assert_eq!(
            serde_json::from_str::<DeploymentStatus>("\"FAILED\"").unwrap(),
            DeploymentStatus::Failed
        );
    }

    #[test]
    fn test_deployment_request_roundtrip() {
        let req = DeploymentRequest {
            deployment_id: "warp-test-123".to_string(),
            region: "us-east-1".to_string(),
            ttl_minutes: 30,
            client_public_key: "xI1sB28L/a8k5u1e8w1V9/z6y3u3u5l2p5x3h4v4wHE=".to_string(),
            admin_cidr: Some("1.2.3.4/32".to_string()),
        };

        let json_str = serde_json::to_string(&req).unwrap();
        let parsed: DeploymentRequest = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.deployment_id, "warp-test-123");
        assert_eq!(parsed.region, "us-east-1");
        assert_eq!(parsed.ttl_minutes, 30);
        assert_eq!(parsed.admin_cidr, Some("1.2.3.4/32".to_string()));
    }

    #[test]
    fn test_deployment_response_roundtrip() {
        let resp = DeploymentResponse {
            deployment_id: "warp-abc".to_string(),
            status: DeploymentStatus::Ready,
            server_ip: Some("54.123.45.67".to_string()),
            wireguard_port: Some(51820),
            server_public_key: Some("M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=".to_string()),
            client_ip: Some("10.200.0.2/32".to_string()),
            dns_server: Some("1.1.1.1, 10.200.0.1".to_string()),
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "1.2.3.4/32".to_string(),
        };

        let json_str = serde_json::to_string(&resp).unwrap();
        let parsed: DeploymentResponse = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed.deployment_id, "warp-abc");
        assert_eq!(parsed.status, DeploymentStatus::Ready);
        assert_eq!(parsed.server_ip.as_deref(), Some("54.123.45.67"));
        assert_eq!(parsed.wireguard_port, Some(51820));
        assert_eq!(parsed.region, "us-east-1");
        assert_eq!(parsed.admin_cidr, "1.2.3.4/32");
        assert!(parsed.error_message.is_none());
    }

    #[test]
    fn test_teardown_roundtrip() {
        let req = TeardownRequest {
            deployment_id: "warp-abc".to_string(),
            region: "us-west-2".to_string(),
        };

        let req_json = serde_json::to_string(&req).unwrap();
        let req_parsed: TeardownRequest = serde_json::from_str(&req_json).unwrap();

        assert_eq!(req_parsed.deployment_id, "warp-abc");

        let resp = TeardownResponse {
            deployment_id: "warp-abc".to_string(),
            status: DeploymentStatus::Terminating,
            message: "Teardown initiated".to_string(),
        };

        let resp_json = serde_json::to_string(&resp).unwrap();
        let resp_parsed: TeardownResponse = serde_json::from_str(&resp_json).unwrap();

        assert_eq!(resp_parsed.status, DeploymentStatus::Terminating);
    }
}
