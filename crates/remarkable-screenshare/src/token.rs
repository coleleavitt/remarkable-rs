//! JWT token parsing and user ID extraction

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Token pair containing device and user tokens
#[derive(Debug, Clone)]
pub struct TokenPair {
    pub device_token: String,
    pub user_token: String,
}

/// JWT claims structure (partial)
#[derive(Debug, Deserialize, Serialize)]
pub struct TokenClaims {
    #[serde(rename = "auth0-userid")]
    pub user_id: Option<String>,
    #[serde(rename = "device-id")]
    pub device_id: Option<String>,
    #[serde(rename = "tectonic")]
    pub tectonic: Option<String>,
    #[serde(rename = "exp")]
    pub expiration: Option<u64>,
    #[serde(rename = "iat")]
    pub issued_at: Option<u64>,
}

impl TokenPair {
    /// Create new token pair
    pub fn new(device_token: impl Into<String>, user_token: impl Into<String>) -> Self {
        Self {
            device_token: device_token.into(),
            user_token: user_token.into(),
        }
    }
    
    /// Load tokens from files
    pub fn from_files(device_path: &str, user_path: &str) -> Result<Self> {
        let device_token = std::fs::read_to_string(device_path)
            .map_err(|e| Error::Token(format!("Failed to read device token: {}", e)))?
            .trim()
            .to_string();
        let user_token = std::fs::read_to_string(user_path)
            .map_err(|e| Error::Token(format!("Failed to read user token: {}", e)))?
            .trim()
            .to_string();
        Ok(Self { device_token, user_token })
    }
    
    /// Load tokens from xochitl.conf content
    pub fn from_xochitl_conf(content: &str) -> Result<Self> {
        let mut device_token = None;
        let mut user_token = None;
        
        for line in content.lines() {
            if let Some(value) = line.strip_prefix("devicetoken=") {
                device_token = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("usertoken=") {
                user_token = Some(value.trim().to_string());
            }
        }
        
        Ok(Self {
            device_token: device_token.ok_or_else(|| Error::Token("Missing devicetoken".into()))?,
            user_token: user_token.ok_or_else(|| Error::Token("Missing usertoken".into()))?,
        })
    }
    
    /// Parse the device token and extract claims
    pub fn parse_device_claims(&self) -> Result<TokenClaims> {
        parse_jwt_claims(&self.device_token)
    }
    
    /// Get user ID from device token
    pub fn user_id(&self) -> Result<String> {
        self.parse_device_claims()?
            .user_id
            .ok_or_else(|| Error::Token("No user ID in token".into()))
    }
    
    /// Get device ID from device token
    pub fn device_id(&self) -> Result<String> {
        self.parse_device_claims()?
            .device_id
            .ok_or_else(|| Error::Token("No device ID in token".into()))
    }
    
    /// Get tectonic region from device token
    pub fn tectonic(&self) -> Result<Option<String>> {
        Ok(self.parse_device_claims()?.tectonic)
    }
}

/// Parse JWT and extract claims without validation
fn parse_jwt_claims(token: &str) -> Result<TokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::Token(format!(
            "Invalid JWT format: expected 3 parts, got {}",
            parts.len()
        )));
    }
    
    let payload = parts[1];
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| Error::Token(format!("Base64 decode error: {}", e)))?;
    
    serde_json::from_slice(&decoded)
        .map_err(|e| Error::Token(format!("JSON parse error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_xochitl_conf() {
        let conf = r#"
devicetoken=eyJhbGciOiJIUzI1NiJ9.eyJhdXRoMC11c2VyaWQiOiJhdXRoMHwxMjM0NTYiLCJkZXZpY2UtaWQiOiJSTTEwMC0xMjM0NTYifQ.sig
usertoken=eyJhbGciOiJIUzI1NiJ9.eyJhdXRoMC11c2VyaWQiOiJhdXRoMHwxMjM0NTYifQ.sig
"#;
        let tokens = TokenPair::from_xochitl_conf(conf).unwrap();
        assert!(tokens.device_token.starts_with("eyJhbGc"));
        assert!(tokens.user_token.starts_with("eyJhbGc"));
    }
}
