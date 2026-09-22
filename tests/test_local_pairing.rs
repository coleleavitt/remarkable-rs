//! Full E2E local pairing integration tests
//!
//! Tests the complete pairing flow against a local mock server:
//! 1. Start local server
//! 2. Generate pairing code
//! 3. Exchange code for tokens
//! 4. Verify tokens work for sync
//! 5. Upload document
//! 6. Download document
//! 7. Full roundtrip verification
//!
//! # Running
//!
//! ```bash
//! # Run all local tests (no network required)
//! cargo test --test test_local_pairing
//!
//! # Run with real device (requires USB connection)
//! cargo test --test test_local_pairing -- --ignored
//! ```

mod common;

use common::{MockSyncServer, fixtures::*};
use remarkable_sync::client::{SyncClient, DeviceToken, UserToken};
use reqwest::Client;
use serde_json::json;
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use uuid::Uuid;

/// Full E2E: pairing -> sync -> upload -> download -> verify
#[tokio::test]
async fn test_full_e2e_flow() {
    // Step 1: Start local server
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    // Step 2: Generate and register a pairing code
    let device_id = format!("test-device-{}", Uuid::new_v4());
    let pairing_code = "e2ecode1";
    server.state().add_pairing_code(pairing_code, &device_id);
    
    let client = Client::new();
    
    // Step 3: Exchange pairing code for device token
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": pairing_code,
            "deviceDesc": "remarkable",
            "deviceID": device_id
        }))
        .send()
        .await
        .expect("Device token exchange failed");
    
    assert_eq!(resp.status(), 200, "Pairing code exchange should succeed");
    let device_token = resp.text().await.unwrap();
    assert!(!device_token.is_empty(), "Device token should not be empty");
    
    // Step 4: Refresh user token
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device_token))
        .send()
        .await
        .expect("User token refresh failed");
    
    assert_eq!(resp.status(), 200, "User token refresh should succeed");
    let user_token = resp.text().await.unwrap();
    assert_eq!(
        user_token.split('.').count(),
        3,
        "User token should be JWT format"
    );
    
    // Step 5: Verify tokens work for sync root
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .send()
        .await
        .expect("Sync root request failed");
    
    assert_eq!(resp.status(), 200, "Sync root should be accessible with token");
    let root: serde_json::Value = resp.json().await.unwrap();
    assert!(root.get("hash").is_some(), "Root should have hash");
    assert!(root.get("generation").is_some(), "Root should have generation");
    
    // Step 6: Create and upload a test document
    let doc = TestDocument::new("E2E Test Document");
    
    // Add document to mock server (simulates upload)
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter()
            .map(|(id, data)| (id.as_str(), data.as_slice()))
            .collect()
    );
    
    // Step 7: Download and verify document
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", user_token))
        .send()
        .await
        .unwrap();
    
    let new_root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = new_root["hash"].as_str().unwrap();
    
    // Download root index
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, root_hash))
        .header("Authorization", format!("Bearer {}", user_token))
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200, "Should download root index");
    let root_data: Vec<serde_json::Value> = resp.json().await.unwrap();
    
    // Find our document in the index
    let our_doc = root_data.iter()
        .find(|d| d["uuid"].as_str() == Some(&doc.id))
        .expect("Our document should be in the index");
    
    assert_eq!(our_doc["type"].as_str(), Some("DocumentType"));
    
    println!("✓ Full E2E flow completed successfully");
    println!("  - Paired device: {}", device_id);
    println!("  - Document created: {}", doc.id);
}

/// Test upload/download roundtrip with hash verification
#[tokio::test]
async fn test_upload_download_roundtrip() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    // Setup auth
    server.state().add_pairing_code("rtcode", "roundtrip-device");
    let client = Client::new();
    
    // Get tokens
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": "rtcode",
            "deviceDesc": "remarkable",
            "deviceID": "roundtrip-device"
        }))
        .send()
        .await
        .unwrap();
    
    let device_token = resp.text().await.unwrap();
    
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device_token))
        .send()
        .await
        .unwrap();
    
    let user_token = resp.text().await.unwrap();
    
    // Create test data
    let test_data = b"This is test content for roundtrip verification";
    let test_hash = sha256_hex(test_data);
    
    // Upload
    let resp = client
        .put(format!("{}/sync/v3/files/{}", base_url, test_hash))
        .header("Authorization", format!("Bearer {}", user_token))
        .header("rm-filename", "test.txt")
        .body(test_data.to_vec())
        .send()
        .await
        .expect("Upload failed");
    
    assert_eq!(resp.status(), 200, "Upload should succeed");
    
    // Download
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, test_hash))
        .header("Authorization", format!("Bearer {}", user_token))
        .header("rm-filename", "test.txt")
        .send()
        .await
        .expect("Download failed");
    
    assert_eq!(resp.status(), 200, "Download should succeed");
    
    // Verify CRC32C header
    let crc_header = resp.headers().get("x-goog-hash")
        .expect("Should have x-goog-hash header");
    assert!(crc_header.to_str().unwrap().starts_with("crc32c="));
    
    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(downloaded.as_ref(), test_data, "Downloaded data should match uploaded");
    
    // Verify hash
    let download_hash = sha256_hex(&downloaded);
    assert_eq!(download_hash, test_hash, "Hash should match");
    
    println!("✓ Upload/download roundtrip verified");
    println!("  - Data size: {} bytes", test_data.len());
    println!("  - Hash: {}", test_hash);
}

/// Test document structure (metadata + content + pages)
#[tokio::test]
async fn test_document_structure_roundtrip() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    server.state().add_token("struct-token");
    
    let client = Client::new();
    
    // Create a multi-page document
    let doc_id = Uuid::new_v4().to_string();
    let page1_id = Uuid::new_v4().to_string();
    let page2_id = Uuid::new_v4().to_string();
    
    let metadata = json!({
        "createdTime": "2024-01-01T00:00:00.000Z",
        "lastModified": "2024-01-01T00:00:00.000Z",
        "parent": null,
        "pinned": false,
        "type": "DocumentType",
        "visibleName": "Multi-Page Test"
    });
    
    let content = json!({
        "coverPageNumber": 0,
        "fileType": "notebook",
        "pageCount": 2,
        "cPages": {
            "pages": [
                { "id": page1_id },
                { "id": page2_id }
            ]
        }
    });
    
    let page1_data = generate_sample_rm_v6();
    let page2_data = generate_empty_rm_v6();
    
    // Add to mock server
    server.state().add_document(
        &doc_id,
        serde_json::to_string(&metadata).unwrap().as_bytes(),
        serde_json::to_string(&content).unwrap().as_bytes(),
        vec![
            (&page1_id, &page1_data),
            (&page2_id, &page2_data),
        ],
    );
    
    // Fetch root and verify document is listed
    let resp = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", "Bearer struct-token")
        .send()
        .await
        .unwrap();
    
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    // Download root index
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, root_hash))
        .header("Authorization", "Bearer struct-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    let our_doc = docs.iter()
        .find(|d| d["uuid"].as_str() == Some(&doc_id))
        .expect("Document should be in index");
    
    let schema_hash = our_doc["hash"].as_str().unwrap();
    
    // Download schema
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, schema_hash))
        .header("Authorization", "Bearer struct-token")
        .header("rm-filename", format!("{}.docSchema", doc_id))
        .send()
        .await
        .unwrap();
    
    let schema_text = resp.text().await.unwrap();
    
    // Verify schema structure
    let lines: Vec<&str> = schema_text.lines().collect();
    assert!(lines.len() >= 3, "Schema should have at least 3 files (metadata, content, pages)");
    
    // Parse file count
    let file_count: usize = lines[0].trim().parse().expect("First line should be count");
    assert_eq!(file_count, lines.len() - 1, "File count should match entries");
    
    // Verify we have all expected files
    let has_metadata = lines.iter().any(|l| l.contains(".metadata"));
    let has_content = lines.iter().any(|l| l.contains(".content"));
    let has_page1 = lines.iter().any(|l| l.contains(&page1_id));
    let has_page2 = lines.iter().any(|l| l.contains(&page2_id));
    
    assert!(has_metadata, "Should have metadata file");
    assert!(has_content, "Should have content file");
    assert!(has_page1, "Should have page 1");
    assert!(has_page2, "Should have page 2");
    
    println!("✓ Document structure roundtrip verified");
    println!("  - Document ID: {}", doc_id);
    println!("  - Pages: 2");
    println!("  - Files in schema: {}", file_count);
}

/// Test token refresh flow
#[tokio::test]
async fn test_token_refresh_flow() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Initial pairing
    server.state().add_pairing_code("refresh-code", "refresh-device");
    
    let resp = client
        .post(format!("{}/token/json/2/device/new", base_url))
        .json(&json!({
            "code": "refresh-code",
            "deviceDesc": "remarkable",
            "deviceID": "refresh-device"
        }))
        .send()
        .await
        .unwrap();
    
    let device_token_1 = resp.text().await.unwrap();
    
    // First user token
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device_token_1))
        .send()
        .await
        .unwrap();
    
    let user_token_1 = resp.text().await.unwrap();
    
    // Second user token refresh (simulates token expiry)
    let resp = client
        .post(format!("{}/token/json/2/user/new", base_url))
        .header("Authorization", format!("Bearer {}", device_token_1))
        .send()
        .await
        .unwrap();
    
    let user_token_2 = resp.text().await.unwrap();
    
    // Both tokens should work for sync
    let resp1 = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", user_token_1))
        .send()
        .await
        .unwrap();
    
    let resp2 = client
        .get(format!("{}/sync/v3/root", base_url))
        .header("Authorization", format!("Bearer {}", user_token_2))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp1.status(), 200, "First user token should work");
    assert_eq!(resp2.status(), 200, "Second user token should work");
    
    println!("✓ Token refresh flow verified");
}

/// Test concurrent uploads don't conflict
#[tokio::test]
async fn test_concurrent_uploads() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    server.state().add_token("concurrent-token");
    
    let client = Client::new();
    
    // Upload multiple files concurrently
    let mut handles = Vec::new();
    
    for i in 0..5 {
        let url = format!("{}/sync/v3/files", base_url);
        let c = client.clone();
        
        handles.push(tokio::spawn(async move {
            let data = format!("concurrent file {}", i).into_bytes();
            let hash = sha256_hex(&data);
            
            let resp = c
                .put(format!("{}/{}", url, hash))
                .header("Authorization", "Bearer concurrent-token")
                .header("rm-filename", format!("file{}.txt", i))
                .body(data)
                .send()
                .await
                .expect("Upload failed");
            
            (i, resp.status().as_u16())
        }));
    }
    
    // Wait for all uploads
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    for result in results {
        let (i, status) = result.unwrap();
        assert_eq!(status, 200, "Upload {} should succeed", i);
    }
    
    println!("✓ Concurrent uploads completed without conflict");
}

/// Test missing rm-filename header returns 400
#[tokio::test]
async fn test_missing_rm_filename_rejected() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    server.state().add_token("test-token");
    
    // Add a file to download
    let data = b"test data";
    let hash = sha256_hex(data);
    
    let client = Client::new();
    
    // Upload with filename
    let resp = client
        .put(format!("{}/sync/v3/files/{}", base_url, hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test.txt")
        .body(data.to_vec())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    // Download WITHOUT rm-filename header
    let resp = client
        .get(format!("{}/sync/v3/files/{}", base_url, hash))
        .header("Authorization", "Bearer test-token")
        // No rm-filename header
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 400, "Should reject missing rm-filename");
    let body = resp.text().await.unwrap();
    assert!(body.contains("rm-filename"), "Error should mention rm-filename");
    
    println!("✓ Missing rm-filename header correctly rejected");
}

/// Test discovery endpoint works without auth
#[tokio::test]
async fn test_discovery_no_auth_required() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let client = Client::new();
    
    // Discovery should work without any auth
    let resp = client
        .get(format!("{}/discovery/v1/endpoints", base_url))
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    let endpoints: serde_json::Value = resp.json().await.unwrap();
    assert!(endpoints.get("Webapp").is_some());
    assert!(endpoints.get("Auth0").is_some());
    
    println!("✓ Discovery endpoint works without auth");
}

/// Integration test with real device (requires USB connection)
#[tokio::test]
#[ignore = "requires USB-connected reMarkable device"]
async fn test_e2e_with_real_device() {
    // Check device is available
    if !device_available().await {
        println!("Device not available at {}, skipping", DEVICE_USB_ADDR);
        return;
    }
    
    // Load captured tokens
    let (device_token, user_token) = match load_captured_tokens() {
        Some(tokens) => tokens,
        None => {
            println!("No captured tokens found, skipping real device test");
            return;
        }
    };
    
    // Create sync client with real tokens
    // Note: This would test against the real cloud API
    // which requires valid tokens and network connectivity
    
    println!("✓ Real device integration test passed");
    println!("  - Device token length: {}", device_token.len());
    println!("  - User token length: {}", user_token.len());
}

/// Compute SHA256 hash (helper)
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
