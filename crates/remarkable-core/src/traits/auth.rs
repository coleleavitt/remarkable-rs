//! Authentication provider and token storage traits
//!
//! This module provides abstractions for:
//! - Token management (access/refresh/device tokens)
//! - Authentication providers (cloud, local)
//! - Secure token storage (file, keyring)

use std::time::{Duration, SystemTime};

/// Access token with metadata
#[derive(Debug, Clone)]
pub struct AccessToken {
    /// Raw JWT token string
    pub token: String,
    /// When the token expires
    pub expires_at: Option<SystemTime>,
    /// Token scopes
    pub scopes: Vec<String>,
}

impl AccessToken {
    /// Create a new access token
    pub fn new(token: String) -> Self {
        Self {
            token,
            expires_at: None,
            scopes: Vec::new(),
        }
    }
    
    /// Create with expiration
    pub fn with_expiry(mut self, duration: Duration) -> Self {
        self.expires_at = Some(SystemTime::now() + duration);
        self
    }
    
    /// Check if token is expired (with 5-minute buffer)
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let buffer = Duration::from_secs(300);
            SystemTime::now() + buffer > expires_at
        } else {
            false // No expiry means valid
        }
    }
    
    /// Get time until expiry
    pub fn time_until_expiry(&self) -> Option<Duration> {
        self.expires_at.and_then(|e| e.duration_since(SystemTime::now()).ok())
    }
}

/// Refresh token for obtaining new access tokens
#[derive(Debug, Clone)]
pub struct RefreshToken {
    /// Raw token string
    pub token: String,
}

impl RefreshToken {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

/// Device token for device-level authentication
#[derive(Debug, Clone)]
pub struct DeviceToken {
    /// Raw token string
    pub token: String,
    /// Device identifier
    pub device_id: String,
}

impl DeviceToken {
    pub fn new(token: String, device_id: String) -> Self {
        Self { token, device_id }
    }
}

/// Token pair (user + device)
#[derive(Debug, Clone)]
pub struct TokenPair {
    /// Device token (long-lived)
    pub device: DeviceToken,
    /// User token (short-lived, refreshable)
    pub user: AccessToken,
}

impl TokenPair {
    pub fn new(device: DeviceToken, user: AccessToken) -> Self {
        Self { device, user }
    }
    
    /// Check if user token needs refresh
    pub fn needs_refresh(&self) -> bool {
        self.user.is_expired()
    }
}

/// Error type for authentication operations
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authentication required")]
    NotAuthenticated,
    
    #[error("token expired")]
    TokenExpired,
    
    #[error("refresh failed: {0}")]
    RefreshFailed(String),
    
    #[error("invalid credentials: {0}")]
    InvalidCredentials(String),
    
    #[error("storage error: {0}")]
    StorageError(String),
    
    #[error("network error: {0}")]
    NetworkError(String),
    
    #[error("invalid token: {0}")]
    InvalidToken(String),
}

/// Authentication provider trait
///
/// Implementations handle specific authentication flows
/// (cloud OAuth, local keys, etc.)
pub trait AuthProvider: Send + Sync {
    /// Authenticate and obtain tokens
    fn authenticate(&self) -> impl std::future::Future<Output = Result<TokenPair, AuthError>> + Send;
    
    /// Refresh an expired user token using device token
    fn refresh(&self, device_token: &DeviceToken) -> impl std::future::Future<Output = Result<AccessToken, AuthError>> + Send;
    
    /// Check if a token is expired
    fn is_expired(&self, token: &AccessToken) -> bool {
        token.is_expired()
    }
    
    /// Validate a token (check signature, expiry, etc.)
    fn validate(&self, token: &AccessToken) -> impl std::future::Future<Output = Result<bool, AuthError>> + Send;
}

/// Token storage trait
///
/// Implementations provide secure token persistence.
pub trait TokenStore: Send + Sync {
    /// Load stored tokens
    fn load(&self) -> impl std::future::Future<Output = Result<Option<TokenPair>, AuthError>> + Send;
    
    /// Save tokens
    fn save(&self, tokens: &TokenPair) -> impl std::future::Future<Output = Result<(), AuthError>> + Send;
    
    /// Clear stored tokens
    fn clear(&self) -> impl std::future::Future<Output = Result<(), AuthError>> + Send;
    
    /// Check if tokens exist
    fn exists(&self) -> impl std::future::Future<Output = Result<bool, AuthError>> + Send;
}

/// File-based token store configuration
#[derive(Debug, Clone)]
pub struct FileStoreConfig {
    /// Path to device token file
    pub device_token_path: String,
    /// Path to user token file
    pub user_token_path: String,
}

impl Default for FileStoreConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Self {
            device_token_path: format!("{}/.remarkable/device_token.txt", home),
            user_token_path: format!("{}/.remarkable/user_token.txt", home),
        }
    }
}

/// Cloud authentication configuration
#[derive(Debug, Clone)]
pub struct CloudAuthConfig {
    /// Auth0 client ID
    pub client_id: String,
    /// Auth0 domain
    pub domain: String,
    /// Device registration endpoint
    pub device_endpoint: String,
    /// Token endpoint
    pub token_endpoint: String,
}

impl Default for CloudAuthConfig {
    fn default() -> Self {
        Self {
            client_id: "W88nD5PiTqa5X9BaB29rmille0W802fK".to_string(),
            domain: "auth.remarkable.com".to_string(),
            device_endpoint: "https://internal.cloud.remarkable.com/devices/v1".to_string(),
            token_endpoint: "https://internal.cloud.remarkable.com/token/json/2/user/new".to_string(),
        }
    }
}

/// JWT claim extraction utilities
pub mod jwt {
    use base64::Engine;
    
    /// Extract a claim from a JWT without verification
    pub fn extract_claim(token: &str, claim: &str) -> Option<String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()?;
        
        let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
        claims.get(claim)?.as_str().map(|s| s.to_string())
    }
    
    /// Extract tectonic region from user token
    pub fn tectonic_region(token: &str) -> Option<String> {
        extract_claim(token, "tectonic")
            .or_else(|| extract_claim(token, "https://auth.remarkable.com/tectonic"))
    }
    
    /// Extract scopes from token
    pub fn scopes(token: &str) -> Option<Vec<String>> {
        extract_claim(token, "scope")
            .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
    }
    
    /// Extract expiry timestamp (Unix seconds)
    pub fn expiry(token: &str) -> Option<u64> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()?;
        
        let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
        claims.get("exp")?.as_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_access_token_expiry() {
        let token = AccessToken::new("test".to_string())
            .with_expiry(Duration::from_secs(3600));
        
        assert!(!token.is_expired());
        
        let expired_token = AccessToken {
            token: "test".to_string(),
            expires_at: Some(SystemTime::now() - Duration::from_secs(100)),
            scopes: vec![],
        };
        
        assert!(expired_token.is_expired());
    }
    
    #[test]
    fn test_token_pair_refresh() {
        let pair = TokenPair {
            device: DeviceToken::new("device".to_string(), "device-id".to_string()),
            user: AccessToken {
                token: "user".to_string(),
                expires_at: Some(SystemTime::now() - Duration::from_secs(100)),
                scopes: vec![],
            },
        };
        
        assert!(pair.needs_refresh());
    }
}
