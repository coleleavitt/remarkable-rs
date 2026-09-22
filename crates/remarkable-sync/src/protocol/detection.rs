//! Protocol version detection
//!
//! Automatically detects which sync protocol version a server supports.
//! Probes endpoints in order from newest to oldest and returns the
//! highest supported version.

use reqwest::{Client, header};

use crate::error::SyncError;
use crate::protocol::{SyncVersion, SyncConfig};

/// Protocol detection result
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Detected protocol version
    pub version: SyncVersion,
    /// Whether detection was successful
    pub success: bool,
    /// Detection message
    pub message: String,
    /// All versions that were probed
    pub probed: Vec<(SyncVersion, bool)>,
}

impl DetectionResult {
    #[allow(dead_code)]
    fn success(version: SyncVersion) -> Self {
        Self {
            version,
            success: true,
            message: format!("Detected protocol version {}", version),
            probed: Vec::new(),
        }
    }
    
    #[allow(dead_code)]
    fn failed(default: SyncVersion, message: impl Into<String>) -> Self {
        Self {
            version: default,
            success: false,
            message: message.into(),
            probed: Vec::new(),
        }
    }
}

/// Detect the sync protocol version supported by a server
pub struct ProtocolDetector {
    client: Client,
    config: SyncConfig,
}

impl ProtocolDetector {
    /// Create a new protocol detector
    pub fn new(config: SyncConfig) -> Result<Self, SyncError> {
        let client = if config.skip_tls_verify {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(10))
                .build()?
        } else {
            Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?
        };
        
        Ok(Self { client, config })
    }
    
    /// Get authorization header if available
    fn auth_header(&self) -> Option<String> {
        self.config.user_token.as_ref()
            .or(self.config.device_token.as_ref())
            .map(|t| format!("Bearer {}", t))
    }
    
    /// Probe a specific endpoint
    async fn probe_endpoint(&self, url: &str) -> Result<bool, SyncError> {
        let mut req = self.client.get(url);
        
        if let Some(auth) = self.auth_header() {
            req = req.header(header::AUTHORIZATION, auth);
        }
        
        let resp = req.send().await?;
        
        match resp.status().as_u16() {
            200 | 401 => Ok(true),  // 401 means endpoint exists but needs auth
            404 => Ok(false),
            _ => Ok(false),
        }
    }
    
    /// Detect the highest supported protocol version
    pub async fn detect(&self) -> DetectionResult {
        let sync_url = self.config.sync_url();
        let cloud_url = &self.config.base_url;
        
        let mut probed = Vec::new();
        
        // Try V4 first (newest)
        let v4_url = format!("{}/sync/v4/root", sync_url);
        match self.probe_endpoint(&v4_url).await {
            Ok(true) => {
                probed.push((SyncVersion::V4, true));
                return DetectionResult {
                    version: SyncVersion::V4,
                    success: true,
                    message: "V4 endpoint available".to_string(),
                    probed,
                };
            }
            Ok(false) => probed.push((SyncVersion::V4, false)),
            Err(_) => probed.push((SyncVersion::V4, false)),
        }
        
        // Try V3 (current production)
        let v3_url = format!("{}/sync/v3/root", sync_url);
        match self.probe_endpoint(&v3_url).await {
            Ok(true) => {
                probed.push((SyncVersion::V3, true));
                return DetectionResult {
                    version: SyncVersion::V3,
                    success: true,
                    message: "V3 endpoint available".to_string(),
                    probed,
                };
            }
            Ok(false) => probed.push((SyncVersion::V3, false)),
            Err(_) => probed.push((SyncVersion::V3, false)),
        }
        
        // Try V2
        let v2_url = format!("{}/sync/v2/root", cloud_url);
        match self.probe_endpoint(&v2_url).await {
            Ok(true) => {
                probed.push((SyncVersion::V2, true));
                return DetectionResult {
                    version: SyncVersion::V2,
                    success: true,
                    message: "V2 endpoint available".to_string(),
                    probed,
                };
            }
            Ok(false) => probed.push((SyncVersion::V2, false)),
            Err(_) => probed.push((SyncVersion::V2, false)),
        }
        
        // Try V1/V1.5 (document-storage)
        let v1_url = format!("{}/document-storage/json/2/docs", cloud_url);
        match self.probe_endpoint(&v1_url).await {
            Ok(true) => {
                probed.push((SyncVersion::V1, true));
                return DetectionResult {
                    version: SyncVersion::V1,
                    success: true,
                    message: "V1 endpoint available".to_string(),
                    probed,
                };
            }
            Ok(false) => probed.push((SyncVersion::V1, false)),
            Err(_) => probed.push((SyncVersion::V1, false)),
        }
        
        // No protocol detected
        DetectionResult {
            version: SyncVersion::V3,  // Default to V3
            success: false,
            message: "No sync endpoints detected, defaulting to V3".to_string(),
            probed,
        }
    }
    
    /// Detect with fallback to a specific version
    pub async fn detect_or(&self, fallback: SyncVersion) -> SyncVersion {
        let result = self.detect().await;
        if result.success {
            result.version
        } else {
            fallback
        }
    }
}

/// Quick detection helper
pub async fn detect_protocol(config: &SyncConfig) -> Result<SyncVersion, SyncError> {
    let detector = ProtocolDetector::new(config.clone())?;
    let result = detector.detect().await;
    Ok(result.version)
}

/// Detect from firmware version string
pub fn version_from_firmware(firmware: &str) -> SyncVersion {
    // Parse version components
    let parts: Vec<u32> = firmware
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    
    if parts.len() < 2 {
        return SyncVersion::V3;  // Default
    }
    
    let major = parts[0];
    let minor = parts[1];
    
    match (major, minor) {
        (3, m) if m >= 28 => SyncVersion::V4,  // 3.28+ uses V4
        (3, _) => SyncVersion::V3,              // 3.x uses V3
        (2, m) if m >= 5 => SyncVersion::V2,    // 2.5+ uses V2
        (2, _) => SyncVersion::V1_5,            // 2.x uses V1.5
        (1, _) => SyncVersion::V1,              // 1.x uses V1
        _ => SyncVersion::V3,                   // Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_version_from_firmware() {
        assert_eq!(version_from_firmware("1.0.0"), SyncVersion::V1);
        assert_eq!(version_from_firmware("2.0.0"), SyncVersion::V1_5);
        assert_eq!(version_from_firmware("2.5.0"), SyncVersion::V2);
        assert_eq!(version_from_firmware("3.0.0"), SyncVersion::V3);
        assert_eq!(version_from_firmware("3.27.0"), SyncVersion::V3);
        assert_eq!(version_from_firmware("3.28.0"), SyncVersion::V4);
        assert_eq!(version_from_firmware("3.29.0.1234"), SyncVersion::V4);
    }
    
    #[test]
    fn test_detection_result() {
        let result = DetectionResult::success(SyncVersion::V3);
        assert!(result.success);
        assert_eq!(result.version, SyncVersion::V3);
    }
}
