//! Local server configuration and pairing
//!
//! Supports pairing with local remarkable-server instances as an alternative
//! to reMarkable cloud. Auto-detects server type and uses appropriate auth flow.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::SyncError;
// DeviceToken and UserToken types are in client module

/// Configuration for connecting to a local sync server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalServerConfig {
    /// Server base URL (e.g., "http://localhost:8080" or "https://sync.local:443")
    pub server_url: String,
    
    /// Skip TLS certificate verification (for self-signed certs)
    #[serde(default)]
    pub skip_tls_verify: bool,
    
    /// Path to custom CA certificate (PEM format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert_path: Option<PathBuf>,
    
    /// Device name for pairing
    #[serde(default = "default_device_name")]
    pub device_name: String,
}

fn default_device_name() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "remarkable-cli".to_string())
}

impl LocalServerConfig {
    /// Create a new local server config with default settings
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            skip_tls_verify: false,
            ca_cert_path: None,
            device_name: default_device_name(),
        }
    }
    
    /// Enable skipping TLS verification (for self-signed certs)
    pub fn with_skip_tls_verify(mut self, skip: bool) -> Self {
        self.skip_tls_verify = skip;
        self
    }
    
    /// Set custom CA certificate path
    pub fn with_ca_cert(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }
    
    /// Set device name for pairing
    pub fn with_device_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = name.into();
        self
    }
    
    /// Build an HTTP client with the appropriate TLS configuration
    pub fn build_client(&self) -> Result<Client, SyncError> {
        let mut builder = Client::builder();
        
        if self.skip_tls_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        
        if let Some(ca_path) = &self.ca_cert_path {
            let cert_pem = std::fs::read(ca_path)?;
            let cert = reqwest::Certificate::from_pem(&cert_pem)
                .map_err(|e| SyncError::InvalidToken(format!("Invalid CA cert: {}", e)))?;
            builder = builder.add_root_certificate(cert);
        }
        
        builder.build()
            .map_err(|e| SyncError::Request(e))
    }
    
    /// Normalize the server URL (ensure no trailing slash)
    pub fn base_url(&self) -> &str {
        self.server_url.trim_end_matches('/')
    }
}

impl Default for LocalServerConfig {
    fn default() -> Self {
        Self::new("http://localhost:8080")
    }
}

/// Server type detection result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerType {
    /// reMarkable cloud
    Cloud,
    /// Local remarkable-server instance
    Local(String), // version
    /// Unknown server
    Unknown,
}

/// Pairing code response from local server
#[derive(Debug, Clone, Deserialize)]
pub struct PairingCodeResponse {
    /// 8-character pairing code
    pub code: String,
    /// Expiry time in seconds
    #[serde(default)]
    pub expires_in: u64,
}

/// Token pair from local server pairing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokens {
    /// Device token (long-lived)
    pub device_token: String,
    /// User token (short-lived, ~3 hours)
    pub user_token: String,
    /// Server URL these tokens are valid for
    pub server_url: String,
}

impl LocalTokens {
    /// Save tokens to files
    pub fn save(&self, device_path: &std::path::Path, user_path: &std::path::Path) -> Result<(), SyncError> {
        std::fs::write(device_path, &self.device_token)?;
        std::fs::write(user_path, &self.user_token)?;
        Ok(())
    }
    
    /// Load tokens from files
    pub fn load(device_path: &std::path::Path, user_path: &std::path::Path, server_url: impl Into<String>) -> Result<Self, SyncError> {
        Ok(Self {
            device_token: std::fs::read_to_string(device_path)?.trim().to_string(),
            user_token: std::fs::read_to_string(user_path)?.trim().to_string(),
            server_url: server_url.into(),
        })
    }
}

/// Local server client for pairing and discovery
pub struct LocalServerClient {
    config: LocalServerConfig,
    client: Client,
}

impl LocalServerClient {
    /// Create a new local server client
    pub fn new(config: LocalServerConfig) -> Result<Self, SyncError> {
        let client = config.build_client()?;
        Ok(Self { config, client })
    }
    
    /// Detect server type from discovery endpoint
    /// 
    /// Tries the server's /discovery/v1/info endpoint to determine type.
    pub async fn detect_server_type(&self) -> Result<ServerType, SyncError> {
        let url = format!("{}/discovery/v1/info", self.config.base_url());
        
        let resp = self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        
        match resp {
            Ok(r) if r.status().is_success() => {
                // Try to parse as local server info
                #[derive(Deserialize)]
                struct ServerInfo {
                    #[serde(default)]
                    server: Option<String>,
                    #[serde(default)]
                    version: Option<String>,
                }
                
                if let Ok(info) = r.json::<ServerInfo>().await {
                    if info.server.as_ref().map(|s| s.contains("remarkable-server")).unwrap_or(false) {
                        return Ok(ServerType::Local(info.version.unwrap_or_else(|| "unknown".into())));
                    }
                }
                Ok(ServerType::Unknown)
            }
            Ok(r) => {
                // Check if it looks like reMarkable cloud (will 404 on this endpoint)
                if r.status().as_u16() == 404 {
                    // Try cloud discovery endpoint
                    let cloud_url = format!("{}/service/json/1/document-storage", self.config.base_url());
                    if let Ok(cr) = self.client.get(&cloud_url)
                        .query(&[("environment", "production"), ("group", "auth0|user"), ("apiVer", "2")])
                        .send().await
                    {
                        if cr.status().is_success() {
                            return Ok(ServerType::Cloud);
                        }
                    }
                }
                Ok(ServerType::Unknown)
            }
            Err(_) => Ok(ServerType::Unknown),
        }
    }
    
    /// Request a new pairing code from the local server
    /// 
    /// The code is displayed to the user who enters it on their device
    /// or in the CLI to complete pairing.
    pub async fn request_pairing_code(&self) -> Result<PairingCodeResponse, SyncError> {
        let url = format!("{}/api/v1/pair/code", self.config.base_url());
        
        #[derive(Serialize)]
        struct CodeRequest {
            device_name: String,
        }
        
        let resp = self.client
            .post(&url)
            .json(&CodeRequest {
                device_name: self.config.device_name.clone(),
            })
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(resp.json().await?),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Exchange a pairing code for device and user tokens
    /// 
    /// This is the same flow as cloud pairing but against the local server.
    pub async fn exchange_code(&self, code: &str, device_id: &str) -> Result<LocalTokens, SyncError> {
        let url = format!("{}/token/json/2/device/new", self.config.base_url());
        
        #[derive(Serialize)]
        struct PairRequest {
            code: String,
            #[serde(rename = "deviceDesc")]
            device_desc: String,
            #[serde(rename = "deviceID")]
            device_id: String,
        }
        
        let resp = self.client
            .post(&url)
            .json(&PairRequest {
                code: code.to_string(),
                device_desc: "remarkable".to_string(),
                device_id: device_id.to_string(),
            })
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                // Response is the device token directly or as JSON
                let text = resp.text().await?;
                
                // Try to parse as JSON first
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    let device_token = json.get("token")
                        .and_then(|t| t.as_str())
                        .unwrap_or(&text)
                        .to_string();
                    
                    // Now get user token
                    let user_token = self.refresh_user_token(&device_token).await?;
                    
                    Ok(LocalTokens {
                        device_token,
                        user_token,
                        server_url: self.config.server_url.clone(),
                    })
                } else {
                    // Plain text token
                    let device_token = text.trim().to_string();
                    let user_token = self.refresh_user_token(&device_token).await?;
                    
                    Ok(LocalTokens {
                        device_token,
                        user_token,
                        server_url: self.config.server_url.clone(),
                    })
                }
            }
            401 => Err(SyncError::Auth("Invalid or expired pairing code".into())),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Refresh user token using device token
    async fn refresh_user_token(&self, device_token: &str) -> Result<String, SyncError> {
        let url = format!("{}/token/json/2/user/new", self.config.base_url());
        
        let resp = self.client
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", device_token))
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.text().await?.trim().to_string()),
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Full pairing flow: request code, wait for user input, exchange
    /// 
    /// Returns a callback that should be called with the code once the user
    /// enters it. This allows for async user interaction.
    pub fn start_pairing(&self) -> PairingSession {
        PairingSession {
            client: self.client.clone(),
            config: self.config.clone(),
            device_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// An in-progress pairing session
pub struct PairingSession {
    client: Client,
    config: LocalServerConfig,
    device_id: String,
}

impl PairingSession {
    /// Get the device ID for this session
    pub fn device_id(&self) -> &str {
        &self.device_id
    }
    
    /// Complete pairing by exchanging the code
    pub async fn complete(self, code: &str) -> Result<LocalTokens, SyncError> {
        let local_client = LocalServerClient {
            config: self.config,
            client: self.client,
        };
        local_client.exchange_code(code, &self.device_id).await
    }
}

/// Unified server configuration (local or cloud)
#[derive(Debug, Clone)]
pub enum ServerConfig {
    /// Use reMarkable cloud
    Cloud,
    /// Use local server
    Local(LocalServerConfig),
}

impl ServerConfig {
    /// Create cloud config
    pub fn cloud() -> Self {
        Self::Cloud
    }
    
    /// Create local server config
    pub fn local(url: impl Into<String>) -> Self {
        Self::Local(LocalServerConfig::new(url))
    }
    
    /// Auto-detect server type from URL
    /// 
    /// If URL points to a local server, returns Local config.
    /// If URL is empty or points to remarkable.com, returns Cloud.
    pub async fn auto_detect(url: Option<&str>) -> Result<Self, SyncError> {
        match url {
            None => Ok(Self::Cloud),
            Some(u) if u.is_empty() => Ok(Self::Cloud),
            Some(u) if u.contains("remarkable.com") || u.contains("remarkable.engineering") => {
                Ok(Self::Cloud)
            }
            Some(u) => {
                let config = LocalServerConfig::new(u);
                let client = LocalServerClient::new(config.clone())?;
                
                match client.detect_server_type().await? {
                    ServerType::Local(_) => Ok(Self::Local(config)),
                    ServerType::Cloud => Ok(Self::Cloud),
                    ServerType::Unknown => {
                        // Assume local if not cloud
                        Ok(Self::Local(config))
                    }
                }
            }
        }
    }
    
    /// Get the base URL
    pub fn base_url(&self) -> &str {
        match self {
            Self::Cloud => "https://webapp.cloud.remarkable.engineering",
            Self::Local(c) => c.base_url(),
        }
    }
    
    /// Check if this is a local server
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_local_server_config_builder() {
        let config = LocalServerConfig::new("https://sync.local:8443")
            .with_skip_tls_verify(true)
            .with_device_name("test-device");
        
        assert_eq!(config.server_url, "https://sync.local:8443");
        assert!(config.skip_tls_verify);
        assert_eq!(config.device_name, "test-device");
    }
    
    #[test]
    fn test_base_url_normalization() {
        let config = LocalServerConfig::new("http://localhost:8080/");
        assert_eq!(config.base_url(), "http://localhost:8080");
    }
    
    #[test]
    fn test_server_config_detection() {
        assert!(matches!(ServerConfig::cloud(), ServerConfig::Cloud));
        assert!(matches!(ServerConfig::local("http://localhost"), ServerConfig::Local(_)));
    }
}
