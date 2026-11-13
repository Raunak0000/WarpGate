use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use warpgate_common::{DeploymentResponse, DeploymentStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableDeployment {
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub region: String,
    pub admin_cidr: String,
    pub ttl_minutes: u32,
    pub created_at: i64,
    pub expires_at: i64,
    pub is_dry_run: bool,
}

#[derive(Clone)]
pub struct DbStore {
    conn: Arc<Mutex<Connection>>,
}

fn status_to_str(status: &DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "PENDING",
        DeploymentStatus::ProvisioningInfra => "PROVISIONING_INFRA",
        DeploymentStatus::ConfiguringVpn => "CONFIGURING_VPN",
        DeploymentStatus::Ready => "READY",
        DeploymentStatus::Failed => "FAILED",
        DeploymentStatus::Terminating => "TERMINATING",
        DeploymentStatus::Destroyed => "DESTROYED",
    }
}

fn str_to_status(s: &str) -> Result<DeploymentStatus, String> {
    match s {
        "PENDING" => Ok(DeploymentStatus::Pending),
        "PROVISIONING_INFRA" => Ok(DeploymentStatus::ProvisioningInfra),
        "CONFIGURING_VPN" => Ok(DeploymentStatus::ConfiguringVpn),
        "READY" => Ok(DeploymentStatus::Ready),
        "FAILED" => Ok(DeploymentStatus::Failed),
        "TERMINATING" => Ok(DeploymentStatus::Terminating),
        "DESTROYED" => Ok(DeploymentStatus::Destroyed),
        other => Err(format!("Unknown deployment status: {}", other)),
    }
}

pub fn get_default_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("WARPGATE_DB_PATH") {
        PathBuf::from(path)
    } else {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.warpgate");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("orchestrator.db")
    }
}

impl DbStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open SQLite database: {}", e))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init_tables()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory SQLite database: {}", e))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init_tables()?;
        Ok(store)
    }

    pub fn open_default() -> Result<Self, String> {
        let path = get_default_db_path();
        Self::open(path)
    }

    fn init_tables(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS deployments (
                 deployment_id TEXT PRIMARY KEY,
                 status TEXT NOT NULL,
                 server_ip TEXT,
                 wireguard_port INTEGER,
                 server_public_key TEXT,
                 client_ip TEXT,
                 dns_server TEXT,
                 error_message TEXT,
                 region TEXT NOT NULL,
                 admin_cidr TEXT NOT NULL,
                 ttl_minutes INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL,
                 is_dry_run INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_deployments_status ON deployments(status);",
        )
        .map_err(|e| format!("Failed to initialize database tables: {}", e))?;
        Ok(())
    }

    pub fn insert_deployment(
        &self,
        resp: &DeploymentResponse,
        ttl_minutes: u32,
        is_dry_run: bool,
    ) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let expires_at = now + (ttl_minutes as i64 * 60);

        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO deployments (
                deployment_id, status, server_ip, wireguard_port, server_public_key,
                client_ip, dns_server, error_message, region, admin_cidr,
                ttl_minutes, created_at, expires_at, is_dry_run
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                resp.deployment_id,
                status_to_str(&resp.status),
                resp.server_ip,
                resp.wireguard_port,
                resp.server_public_key,
                resp.client_ip,
                resp.dns_server,
                resp.error_message,
                resp.region,
                resp.admin_cidr,
                ttl_minutes,
                now,
                expires_at,
                if is_dry_run { 1 } else { 0 },
            ],
        )
        .map_err(|e| format!("Failed to insert deployment {}: {}", resp.deployment_id, e))?;

        Ok(())
    }

    pub fn get_deployment(&self, deployment_id: &str) -> Result<Option<DeploymentResponse>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT deployment_id, status, server_ip, wireguard_port, server_public_key,
                        client_ip, dns_server, error_message, region, admin_cidr
                 FROM deployments WHERE deployment_id = ?1",
            )
            .map_err(|e| e.to_string())?;

        let result = stmt
            .query_row(params![deployment_id], |row| {
                let status_str: String = row.get(1)?;
                let status = str_to_status(&status_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;

                Ok(DeploymentResponse {
                    deployment_id: row.get(0)?,
                    status,
                    server_ip: row.get(2)?,
                    wireguard_port: row.get(3)?,
                    server_public_key: row.get(4)?,
                    client_ip: row.get(5)?,
                    dns_server: row.get(6)?,
                    error_message: row.get(7)?,
                    region: row.get(8)?,
                    admin_cidr: row.get(9)?,
                })
            })
            .optional()
            .map_err(|e| format!("Failed to query deployment {}: {}", deployment_id, e))?;

        Ok(result)
    }

    pub fn update_status(
        &self,
        deployment_id: &str,
        status: &DeploymentStatus,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE deployments SET status = ?1, error_message = ?2 WHERE deployment_id = ?3",
            params![status_to_str(status), error_message, deployment_id],
        )
        .map_err(|e| format!("Failed to update status for {}: {}", deployment_id, e))?;

        Ok(())
    }

    pub fn update_configuring_vpn(
        &self,
        deployment_id: &str,
        server_ip: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE deployments SET status = 'CONFIGURING_VPN', server_ip = ?1 WHERE deployment_id = ?2",
            params![server_ip, deployment_id],
        )
        .map_err(|e| format!("Failed to update configuring_vpn for {}: {}", deployment_id, e))?;

        Ok(())
    }

    pub fn update_ready(
        &self,
        deployment_id: &str,
        server_ip: Option<&str>,
        server_public_key: &str,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        if let Some(ip) = server_ip {
            conn.execute(
                "UPDATE deployments SET status = 'READY', server_ip = ?1, server_public_key = ?2, error_message = ?3 WHERE deployment_id = ?4",
                params![ip, server_public_key, error_message, deployment_id],
            )
        } else {
            conn.execute(
                "UPDATE deployments SET status = 'READY', server_public_key = ?1, error_message = ?2 WHERE deployment_id = ?3",
                params![server_public_key, error_message, deployment_id],
            )
        }
        .map_err(|e| format!("Failed to update ready for {}: {}", deployment_id, e))?;

        Ok(())
    }

    pub fn count_active_deployments(&self, exclude_id: Option<&str>) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM deployments
                 WHERE status IN ('PENDING', 'PROVISIONING_INFRA', 'CONFIGURING_VPN', 'READY', 'TERMINATING')
                   AND (?1 IS NULL OR deployment_id != ?1)",
                params![exclude_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count active deployments: {}", e))?;

        Ok(count as usize)
    }

    pub fn list_active_deployments(&self) -> Result<Vec<RecoverableDeployment>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT deployment_id, status, region, admin_cidr, ttl_minutes, created_at, expires_at, is_dry_run
                 FROM deployments
                 WHERE status IN ('PENDING', 'PROVISIONING_INFRA', 'CONFIGURING_VPN', 'READY', 'TERMINATING')",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(1)?;
                let status = str_to_status(&status_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;

                let is_dry_run_int: i64 = row.get(7)?;

                Ok(RecoverableDeployment {
                    deployment_id: row.get(0)?,
                    status,
                    region: row.get(2)?,
                    admin_cidr: row.get(3)?,
                    ttl_minutes: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    is_dry_run: is_dry_run_int == 1,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| e.to_string())?);
        }

        Ok(result)
    }

    pub fn list_all_deployments(&self) -> Result<Vec<DeploymentResponse>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT deployment_id, status, server_ip, wireguard_port, server_public_key,
                        client_ip, dns_server, error_message, region, admin_cidr
                 FROM deployments",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(1)?;
                let status = str_to_status(&status_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;

                Ok(DeploymentResponse {
                    deployment_id: row.get(0)?,
                    status,
                    server_ip: row.get(2)?,
                    wireguard_port: row.get(3)?,
                    server_public_key: row.get(4)?,
                    client_ip: row.get(5)?,
                    dns_server: row.get(6)?,
                    error_message: row.get(7)?,
                    region: row.get(8)?,
                    admin_cidr: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| e.to_string())?);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_insert_and_get() {
        let db = DbStore::open_in_memory().unwrap();
        let resp = DeploymentResponse {
            deployment_id: "warp-abc123456789".to_string(),
            status: DeploymentStatus::ProvisioningInfra,
            server_ip: None,
            wireguard_port: Some(51820),
            server_public_key: None,
            client_ip: Some("10.200.0.2/32".to_string()),
            dns_server: Some("1.1.1.1".to_string()),
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "0.0.0.0/0".to_string(),
        };

        db.insert_deployment(&resp, 30, false).unwrap();

        let loaded = db.get_deployment("warp-abc123456789").unwrap().unwrap();
        assert_eq!(loaded.deployment_id, "warp-abc123456789");
        assert_eq!(loaded.status, DeploymentStatus::ProvisioningInfra);
        assert_eq!(loaded.wireguard_port, Some(51820));

        // Update status to Ready
        db.update_ready("warp-abc123456789", Some("54.210.1.2"), "test_pub_key", None)
            .unwrap();
        let updated = db.get_deployment("warp-abc123456789").unwrap().unwrap();
        assert_eq!(updated.status, DeploymentStatus::Ready);
        assert_eq!(updated.server_ip, Some("54.210.1.2".to_string()));
        assert_eq!(updated.server_public_key, Some("test_pub_key".to_string()));
    }

    #[test]
    fn test_count_active_deployments() {
        let db = DbStore::open_in_memory().unwrap();
        assert_eq!(db.count_active_deployments(None).unwrap(), 0);

        let resp1 = DeploymentResponse {
            deployment_id: "warp-000000000001".to_string(),
            status: DeploymentStatus::Ready,
            server_ip: Some("1.1.1.1".to_string()),
            wireguard_port: Some(51820),
            server_public_key: None,
            client_ip: None,
            dns_server: None,
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "0.0.0.0/0".to_string(),
        };
        db.insert_deployment(&resp1, 30, false).unwrap();
        assert_eq!(db.count_active_deployments(None).unwrap(), 1);

        // Exclude self
        assert_eq!(
            db.count_active_deployments(Some("warp-000000000001"))
                .unwrap(),
            0
        );

        // Mark Destroyed
        db.update_status("warp-000000000001", &DeploymentStatus::Destroyed, None)
            .unwrap();
        assert_eq!(db.count_active_deployments(None).unwrap(), 0);
    }

    #[test]
    fn test_list_active_deployments() {
        let db = DbStore::open_in_memory().unwrap();
        let resp = DeploymentResponse {
            deployment_id: "warp-000000000002".to_string(),
            status: DeploymentStatus::Ready,
            server_ip: Some("1.1.1.1".to_string()),
            wireguard_port: Some(51820),
            server_public_key: None,
            client_ip: None,
            dns_server: None,
            error_message: None,
            region: "eu-central-1".to_string(),
            admin_cidr: "0.0.0.0/0".to_string(),
        };
        db.insert_deployment(&resp, 60, true).unwrap();

        let list = db.list_active_deployments().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].deployment_id, "warp-000000000002");
        assert!(list[0].is_dry_run);
        assert_eq!(list[0].ttl_minutes, 60);
    }
}

