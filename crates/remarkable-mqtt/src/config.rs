//! MQTT client configuration

use crate::MqttError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use uuid::Uuid;

/// Default MQTT broker hostname
pub const DEFAULT_BROKER: &str = "vernemq-prod.cloud.remarkable.engineering";

/// Default MQTT TLS port (443 for reMarkable, not standard 8883)
pub const DEFAULT_PORT: u16 = 443;

/// MQTT client configuration
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// VerneMQ broker hostname
    pub broker: String,
    /// MQTT port (8883 for TLS)
    pub port: u16,
    /// Device token (JWT) - used as MQTT username
    pub device_token: String,
    /// User token (JWT) - used as MQTT password
    pub user_token: String,
    /// User ID extracted from token claims
    pub user_id: String,
    /// Unique client ID for this connection
    pub client_id: String,
    /// Keep-alive interval in seconds
    pub keep_alive_secs: u64,
}

impl MqttConfig {
    /// Create config from device and user tokens
    ///
    /// Extracts user_id from user token claims.
    /// Generates a unique client_id.
    pub fn from_tokens(device_token: &str, user_token: &str) -> Result<Self, MqttError> {
        let user_id = extract_user_id(user_token)
            .ok_or_else(|| MqttError::TokenParse("Failed to extract user_id from token".into()))?;

        let client_id = format!("remarkable-rs-{}", Uuid::new_v4().as_simple());

        Ok(Self {
            broker: DEFAULT_BROKER.to_string(),
            port: DEFAULT_PORT,
            device_token: device_token.to_string(),
            user_token: user_token.to_string(),
            user_id,
            client_id,
            keep_alive_secs: 60,
        })
    }

    /// Set custom broker
    pub fn with_broker(mut self, broker: impl Into<String>) -> Self {
        self.broker = broker.into();
        self
    }

    /// Set custom port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set keep-alive interval
    pub fn with_keep_alive(mut self, secs: u64) -> Self {
        self.keep_alive_secs = secs;
        self
    }

    /// Set custom client ID
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }
}

/// Extract user ID from JWT token claims
///
/// Looks for `auth0-userid`, `sub`, or nested `auth0-profile.UserID`
fn extract_user_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // JWT uses URL-safe base64 without padding
    let payload = parts[1];
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

    // Try different claim paths
    json.get("auth0-userid")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("auth0-profile")
                .and_then(|p| p.get("UserID"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| json.get("sub").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_user_id_from_auth0_userid() {
        // JWT with auth0-userid claim
        let payload = r#"{"auth0-userid":"auth0|12345","sub":"other"}"#;
        let encoded = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let token = format!("header.{}.sig", encoded);

        let user_id = extract_user_id(&token);
        assert_eq!(user_id, Some("auth0|12345".to_string()));
    }

    #[test]
    fn test_extract_user_id_from_sub() {
        // JWT with only sub claim
        let payload = r#"{"sub":"auth0|67890"}"#;
        let encoded = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let token = format!("header.{}.sig", encoded);

        let user_id = extract_user_id(&token);
        assert_eq!(user_id, Some("auth0|67890".to_string()));
    }

    #[test]
    fn test_config_from_tokens() {
        let device = "header.e30.sig"; // empty payload {}
        let user_payload = r#"{"auth0-userid":"user-123"}"#;
        let user_encoded = URL_SAFE_NO_PAD.encode(user_payload.as_bytes());
        let user = format!("header.{}.sig", user_encoded);

        let config = MqttConfig::from_tokens(device, &user).unwrap();
        assert_eq!(config.user_id, "user-123");
        assert_eq!(config.broker, DEFAULT_BROKER);
        assert_eq!(config.port, DEFAULT_PORT);
        assert!(config.client_id.starts_with("remarkable-rs-"));
    }
}
