//! Authentication and token handling

use serde::{Deserialize, Serialize};
use base64::Engine;

/// User authentication token (from Auth0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

impl AuthToken {
    /// Extract claims from JWT (without verification)
    pub fn claims(&self) -> Option<serde_json::Value> {
        let parts: Vec<&str> = self.access_token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()?;
        
        serde_json::from_slice(&payload).ok()
    }
    
    /// Get user ID from token
    pub fn user_id(&self) -> Option<String> {
        self.claims()?
            .get("sub")?
            .as_str()
            .map(|s| s.to_string())
    }
    
    /// Get tectonic region from token
    pub fn tectonic_region(&self) -> Option<String> {
        self.claims()?
            .get("https://auth.remarkable.com/tectonic")?
            .as_str()
            .map(|s| s.to_string())
    }
}

/// Device token (for sync operations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    pub token: String,
    pub device_id: String,
}

impl DeviceToken {
    /// Create a new device token
    pub fn new(token: String, device_id: String) -> Self {
        Self { token, device_id }
    }
}
