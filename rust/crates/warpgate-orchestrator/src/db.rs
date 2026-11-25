use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use warpgate_common::{DeploymentResponse, DeploymentStatus, PeerInfo};

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
             CREATE INDEX IF NOT EXISTS idx_deployments_status ON deployments(status);
             CREATE TABLE IF NOT EXISTS peers (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 deployment_id TEXT NOT NULL,
                 device_name TEXT NOT NULL,
                 public_key TEXT NOT NULL,
                 assigned_ip TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 UNIQUE(deployment_id, public_key),
                 UNIQUE(deployment_id, assigned_ip)
             );
             CREATE INDEX IF NOT EXISTS idx_peers_deployment ON peers(deployment_id);",
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

    pub fn insert_primary_peer(
        &self,
        deployment_id: &str,
        public_key: &str,
    ) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO peers (deployment_id, device_name, public_key, assigned_ip, created_at)
             VALUES (?1, 'primary', ?2, '10.200.0.2/32', ?3)",
            params![deployment_id, public_key, now],
        )
        .map_err(|e| format!("Failed to insert primary peer for {}: {}", deployment_id, e))?;
        Ok(())
    }

    pub fn allocate_and_insert_peer(
        &self,
        deployment_id: &str,
        device_name: &str,
        public_key: &str,
    ) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        let existing_key: Option<String> = conn
            .query_row(
                "SELECT assigned_ip FROM peers WHERE deployment_id = ?1 AND public_key = ?2",
                params![deployment_id, public_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if existing_key.is_some() {
            return Err("Peer with this public key is already registered on this deployment".to_string());
        }

        let mut stmt = conn
            .prepare("SELECT assigned_ip FROM peers WHERE deployment_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![deployment_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let mut used_ips = HashSet::new();
        used_ips.insert("10.200.0.1/32".to_string());
        used_ips.insert("10.200.0.2/32".to_string());
        for row in rows {
            used_ips.insert(row.map_err(|e| e.to_string())?);
        }
        drop(stmt);

        let mut chosen_ip = None;
        for host_octet in 3..=254u8 {
            let candidate = format!("10.200.0.{}/32", host_octet);
            if !used_ips.contains(&candidate) {
                chosen_ip = Some(candidate);
                break;
            }
        }

        let assigned_ip = chosen_ip.ok_or_else(|| {
            "WireGuard subnet IP pool (10.200.0.0/24) is exhausted for this deployment".to_string()
        })?;

        conn.execute(
            "INSERT INTO peers (deployment_id, device_name, public_key, assigned_ip, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![deployment_id, device_name, public_key, assigned_ip, now],
        )
        .map_err(|e| format!("Failed to register peer: {}", e))?;

        Ok(assigned_ip)
    }

    pub fn list_peers(&self, deployment_id: &str) -> Result<Vec<PeerInfo>, String> {
        let dep = self
            .get_deployment(deployment_id)?
            .ok_or_else(|| format!("Deployment '{}' not found", deployment_id))?;

        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT deployment_id, device_name, public_key, assigned_ip
                 FROM peers WHERE deployment_id = ?1 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![deployment_id], |row| {
                Ok(PeerInfo {
                    deployment_id: row.get(0)?,
                    device_name: row.get(1)?,
                    public_key: row.get(2)?,
                    assigned_ip: row.get(3)?,
                    server_ip: dep.server_ip.clone(),
                    wireguard_port: dep.wireguard_port,
                    server_public_key: dep.server_public_key.clone(),
                    dns_server: dep.dns_server.clone(),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut peers = Vec::new();
        for r in rows {
            peers.push(r.map_err(|e| e.to_string())?);
        }
        Ok(peers)
    }

    pub fn remove_peer(&self, deployment_id: &str, public_key: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let affected = conn
            .execute(
                "DELETE FROM peers WHERE deployment_id = ?1 AND public_key = ?2",
                params![deployment_id, public_key],
            )
            .map_err(|e| format!("Failed to remove peer: {}", e))?;
        Ok(affected > 0)
    }

    pub fn is_deployment_dry_run(&self, deployment_id: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let flag: Option<i64> = conn
            .query_row(
                "SELECT is_dry_run FROM deployments WHERE deployment_id = ?1",
                params![deployment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(flag == Some(1))
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

    #[test]
    fn test_peer_pool_allocation_and_removal() {
        let db = DbStore::open_in_memory().unwrap();
        let resp = DeploymentResponse {
            deployment_id: "warp-000000000003".to_string(),
            status: DeploymentStatus::Ready,
            server_ip: Some("54.10.20.30".to_string()),
            wireguard_port: Some(51820),
            server_public_key: Some("M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=".to_string()),
            client_ip: Some("10.200.0.2/32".to_string()),
            dns_server: Some("1.1.1.1, 10.200.0.1".to_string()),
            error_message: None,
            region: "us-east-1".to_string(),
            admin_cidr: "0.0.0.0/0".to_string(),
        };
        db.insert_deployment(&resp, 30, true).unwrap();
        db.insert_primary_peer("warp-000000000003", "primaryPubKey0000000000000000000000000000000=")
            .unwrap();

        let ip1 = db
            .allocate_and_insert_peer(
                "warp-000000000003",
                "phone",
                "phonePubKey00000000000000000000000000000000=",
            )
            .unwrap();
        assert_eq!(ip1, "10.200.0.3/32");

        let ip2 = db
            .allocate_and_insert_peer(
                "warp-000000000003",
                "tablet",
                "tabletPubKey0000000000000000000000000000000=",
            )
            .unwrap();
        assert_eq!(ip2, "10.200.0.4/32");

        // Duplicate public key rejected
        assert!(db
            .allocate_and_insert_peer(
                "warp-000000000003",
                "phone-dup",
                "phonePubKey00000000000000000000000000000000=",
            )
            .is_err());

        let peers = db.list_peers("warp-000000000003").unwrap();
        assert_eq!(peers.len(), 3);
        assert_eq!(peers[0].assigned_ip, "10.200.0.2/32");
        assert_eq!(peers[1].device_name, "phone");
        assert_eq!(peers[2].device_name, "tablet");

        // Removing .3 frees it for reuse
        assert!(db
            .remove_peer(
                "warp-000000000003",
                "phonePubKey00000000000000000000000000000000="
            )
            .unwrap());
        let ip_reused = db
            .allocate_and_insert_peer(
                "warp-000000000003",
                "laptop-2",
                "laptop2PubKey000000000000000000000000000000=",
            )
            .unwrap();
        assert_eq!(ip_reused, "10.200.0.3/32");
    }
}

