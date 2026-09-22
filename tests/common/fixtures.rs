//! Test fixtures and data generators
//!
//! Provides sample documents, tokens, and helpers for integration tests.

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use uuid::Uuid;

/// Device address for USB-connected reMarkable
pub const DEVICE_USB_ADDR: &str = "10.11.99.1";

/// Token file paths for real device testing
pub const TOKEN_DIR: &str = concat!(env!("HOME"), "/SiteResearch/remarkable/captured_tokens");

/// Test document metadata
#[derive(Clone, Serialize, Deserialize)]
pub struct TestDocMetadata {
    #[serde(rename = "createdTime")]
    pub created_time: String,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    #[serde(rename = "lastOpened")]
    pub last_opened: Option<String>,
    #[serde(rename = "lastOpenedPage")]
    pub last_opened_page: Option<u32>,
    pub parent: Option<String>,
    pub pinned: bool,
    #[serde(rename = "type")]
    pub doc_type: String,
    #[serde(rename = "visibleName")]
    pub visible_name: String,
}

impl Default for TestDocMetadata {
    fn default() -> Self {
        Self {
            created_time: "2024-01-01T00:00:00.000Z".to_string(),
            last_modified: "2024-01-01T00:00:00.000Z".to_string(),
            last_opened: None,
            last_opened_page: Some(0),
            parent: None,
            pinned: false,
            doc_type: "DocumentType".to_string(),
            visible_name: "Test Document".to_string(),
        }
    }
}

/// Test document content
#[derive(Clone, Serialize, Deserialize)]
pub struct TestDocContent {
    #[serde(rename = "coverPageNumber")]
    pub cover_page_number: i32,
    #[serde(rename = "fileType")]
    pub file_type: String,
    #[serde(rename = "pageCount")]
    pub page_count: u32,
    #[serde(rename = "cPages")]
    pub c_pages: TestCPages,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TestCPages {
    pub pages: Vec<TestPageInfo>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TestPageInfo {
    pub id: String,
}

impl Default for TestDocContent {
    fn default() -> Self {
        let page_id = Uuid::new_v4().to_string();
        Self {
            cover_page_number: 0,
            file_type: "notebook".to_string(),
            page_count: 1,
            c_pages: TestCPages {
                pages: vec![TestPageInfo { id: page_id }],
            },
        }
    }
}

/// Generate a minimal valid v6 .rm file (empty page)
pub fn generate_empty_rm_v6() -> Vec<u8> {
    let mut data = Vec::new();
    
    // Header: "reMarkable .lines file, version=6" + padding to 43 bytes
    let header = b"reMarkable .lines file, version=6          ";
    data.extend_from_slice(header);
    
    // Version 6 minimal content structure:
    // - u32: number of layers (0)
    data.extend_from_slice(&0u32.to_le_bytes());
    
    data
}

/// Generate a v6 .rm file with sample strokes
pub fn generate_sample_rm_v6() -> Vec<u8> {
    let mut data = Vec::new();
    
    // Header (43 bytes)
    let header = b"reMarkable .lines file, version=6          ";
    data.extend_from_slice(header);
    
    // Number of layers: 1
    data.extend_from_slice(&1u32.to_le_bytes());
    
    // Layer 0: single stroke
    data.extend_from_slice(&1u32.to_le_bytes()); // 1 stroke
    
    // Stroke header
    data.extend_from_slice(&0u32.to_le_bytes()); // pen type: ballpoint
    data.extend_from_slice(&0u32.to_le_bytes()); // color: black
    data.push(0); // unused
    data.extend_from_slice(&2.0f32.to_le_bytes()); // base width
    data.extend_from_slice(&3u32.to_le_bytes()); // 3 points
    
    // Points: simple diagonal line
    for i in 0..3 {
        let x = 100.0 + (i as f32 * 50.0);
        let y = 100.0 + (i as f32 * 50.0);
        data.extend_from_slice(&x.to_le_bytes());
        data.extend_from_slice(&y.to_le_bytes());
        data.extend_from_slice(&0.5f32.to_le_bytes()); // speed
        data.extend_from_slice(&0.0f32.to_le_bytes()); // direction
        data.extend_from_slice(&1.0f32.to_le_bytes()); // width
        data.extend_from_slice(&0.8f32.to_le_bytes()); // pressure
    }
    
    data
}

/// Generate a test document with all files
pub struct TestDocument {
    pub id: String,
    pub metadata: Vec<u8>,
    pub content: Vec<u8>,
    pub pages: Vec<(String, Vec<u8>)>,
}

impl TestDocument {
    /// Create a new test document
    pub fn new(name: &str) -> Self {
        let id = Uuid::new_v4().to_string();
        let page_id = Uuid::new_v4().to_string();
        
        let metadata = TestDocMetadata {
            visible_name: name.to_string(),
            ..Default::default()
        };
        
        let content = TestDocContent {
            c_pages: TestCPages {
                pages: vec![TestPageInfo { id: page_id.clone() }],
            },
            ..Default::default()
        };
        
        Self {
            id,
            metadata: serde_json::to_vec(&metadata).unwrap(),
            content: serde_json::to_vec(&content).unwrap(),
            pages: vec![(page_id, generate_sample_rm_v6())],
        }
    }
    
    /// Create an empty notebook
    pub fn empty(name: &str) -> Self {
        let id = Uuid::new_v4().to_string();
        let page_id = Uuid::new_v4().to_string();
        
        let metadata = TestDocMetadata {
            visible_name: name.to_string(),
            ..Default::default()
        };
        
        let content = TestDocContent {
            c_pages: TestCPages {
                pages: vec![TestPageInfo { id: page_id.clone() }],
            },
            ..Default::default()
        };
        
        Self {
            id,
            metadata: serde_json::to_vec(&metadata).unwrap(),
            content: serde_json::to_vec(&content).unwrap(),
            pages: vec![(page_id, generate_empty_rm_v6())],
        }
    }
}

/// Check if device is reachable via USB
pub async fn device_available() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    
    client
        .get(format!("http://{}", DEVICE_USB_ADDR))
        .send()
        .await
        .is_ok()
}

/// Load tokens from captured files (for real device tests)
pub fn load_captured_tokens() -> Option<(String, String)> {
    let device_path = format!("{}/device_token_actual.txt", TOKEN_DIR);
    let user_path = format!("{}/user_token_actual.txt", TOKEN_DIR);
    
    let device_token = std::fs::read_to_string(&device_path).ok()?;
    let user_token = std::fs::read_to_string(&user_path).ok()?;
    
    Some((device_token.trim().to_string(), user_token.trim().to_string()))
}

/// Generate a mock JWT token
pub fn mock_jwt(claims: &serde_json::Value) -> String {
    use base64::Engine;
    
    let header = r#"{"alg":"HS256","typ":"JWT"}"#;
    let header_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(header.as_bytes());
    
    let claims_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(claims.to_string().as_bytes());
    
    format!("{}.{}.mock-signature", header_b64, claims_b64)
}

/// Generate a mock device token
pub fn mock_device_token(device_id: &str) -> String {
    mock_jwt(&serde_json::json!({
        "device-id": device_id,
        "device-type": "remarkable",
        "iat": chrono::Utc::now().timestamp(),
        "exp": chrono::Utc::now().timestamp() + 86400 * 365,
    }))
}

/// Generate a mock user token
pub fn mock_user_token(region: &str) -> String {
    mock_jwt(&serde_json::json!({
        "sub": "test-user|123456",
        "https://auth.remarkable.com/tectonic": region,
        "scopes": "sync:fox intgr hwc screenshare",
        "iat": chrono::Utc::now().timestamp(),
        "exp": chrono::Utc::now().timestamp() + 3600 * 3,
    }))
}

/// Compute SHA256 hash of data
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_rm_v6_valid_header() {
        let rm = generate_empty_rm_v6();
        let header = std::str::from_utf8(&rm[..33]).unwrap();
        assert!(header.starts_with("reMarkable .lines file, version=6"));
    }
    
    #[test]
    fn test_sample_rm_v6_has_strokes() {
        let rm = generate_sample_rm_v6();
        // Should have header (43 bytes) + layer count + stroke data
        assert!(rm.len() > 50);
    }
    
    #[test]
    fn test_mock_jwt_format() {
        let token = mock_device_token("test-device");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }
    
    #[test]
    fn test_test_document_creates_valid_json() {
        let doc = TestDocument::new("Test Doc");
        
        // Metadata should be valid JSON
        let _: TestDocMetadata = serde_json::from_slice(&doc.metadata).unwrap();
        
        // Content should be valid JSON
        let _: TestDocContent = serde_json::from_slice(&doc.content).unwrap();
        
        // Should have one page
        assert_eq!(doc.pages.len(), 1);
    }
}
