//! Document upload integration tests
//!
//! Tests document upload to local mock server:
//! 1. Hash content to get expected hash
//! 2. PUT file with rm-filename header
//! 3. Verify hash matches
//! 4. Update root index
//!
//! Note: Upload to production cloud requires Connect subscription.
//! These tests use mock server for full functionality.

mod common;

use common::{MockSyncServer, fixtures::*};
use reqwest::Client;
use sha2::{Sha256, Digest};

/// Hash content for upload
fn hash_content(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Test uploading a file
#[tokio::test]
async fn test_upload_file_mock() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let test_content = b"test file content";
    let content_hash = hash_content(test_content);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test.txt")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
}

/// Test upload fails without auth
#[tokio::test]
async fn test_upload_requires_auth() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let test_content = b"test";
    let content_hash = hash_content(test_content);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("rm-filename", "test.txt")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 401);
}

/// Test upload fails without rm-filename header
#[tokio::test]
async fn test_upload_requires_rm_filename() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let test_content = b"test";
    let content_hash = hash_content(test_content);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 400);
}

/// Test upload with hash mismatch fails
#[tokio::test]
async fn test_upload_hash_mismatch() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let test_content = b"test content";
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), wrong_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test.txt")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 400);
}

/// Test upload then download round-trip
#[tokio::test]
async fn test_upload_download_roundtrip() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let test_content = b"roundtrip test content";
    let content_hash = hash_content(test_content);
    
    // Upload
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "roundtrip.txt")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    // Download
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "roundtrip.txt")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(downloaded.as_ref(), test_content);
}

/// Test uploading .rm file
#[tokio::test]
async fn test_upload_rm_file() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let rm_data = generate_sample_rm_v6();
    let rm_hash = hash_content(&rm_data);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), rm_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test-page.rm")
        .body(rm_data.clone())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    // Download and verify it parses
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), rm_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test-page.rm")
        .send()
        .await
        .unwrap();
    
    let downloaded = resp.bytes().await.unwrap();
    
    // Verify round-trip
    assert_eq!(downloaded.as_ref(), rm_data.as_slice());
    
    // Verify it still parses
    use remarkable_lines::parse_rm_file;
    assert!(parse_rm_file(&downloaded).is_ok());
}

/// Test uploading metadata JSON
#[tokio::test]
async fn test_upload_metadata_json() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let metadata = TestDocMetadata {
        visible_name: "Uploaded Document".to_string(),
        ..Default::default()
    };
    let metadata_json = serde_json::to_vec(&metadata).unwrap();
    let metadata_hash = hash_content(&metadata_json);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), metadata_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test-doc.metadata")
        .body(metadata_json.clone())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    // Download and verify JSON
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), metadata_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test-doc.metadata")
        .send()
        .await
        .unwrap();
    
    let downloaded: TestDocMetadata = resp.json().await.unwrap();
    assert_eq!(downloaded.visible_name, "Uploaded Document");
}

/// Test creating a complete document
#[tokio::test]
async fn test_upload_complete_document() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let doc = TestDocument::new("Complete Upload Test");
    
    // Upload metadata
    let metadata_hash = hash_content(&doc.metadata);
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), metadata_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", format!("{}.metadata", doc.id))
        .body(doc.metadata.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    // Upload content
    let content_hash = hash_content(&doc.content);
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", format!("{}.content", doc.id))
        .body(doc.content.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    // Upload each page
    for (page_id, page_data) in &doc.pages {
        let page_hash = hash_content(page_data);
        let resp = client
            .put(format!("{}/sync/v3/files/{}", server.base_url(), page_hash))
            .header("Authorization", "Bearer test-token")
            .header("rm-filename", format!("{}/{}.rm", doc.id, page_id))
            .body(page_data.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }
}

/// Test generation increments on upload
#[tokio::test]
async fn test_upload_increments_generation() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Get initial generation
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root1: serde_json::Value = resp.json().await.unwrap();
    let gen1 = root1["generation"].as_u64().unwrap();
    
    // Upload a file
    let test_content = b"generation test";
    let content_hash = hash_content(test_content);
    
    client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "gen-test.txt")
        .body(test_content.to_vec())
        .send()
        .await
        .unwrap();
    
    // Check generation increased
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root2: serde_json::Value = resp.json().await.unwrap();
    let gen2 = root2["generation"].as_u64().unwrap();
    
    assert!(gen2 > gen1, "Generation should increase after upload");
}

/// Test concurrent uploads
#[tokio::test]
async fn test_concurrent_uploads() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let url = base_url.clone();
            tokio::spawn(async move {
                let client = Client::new();
                let content = format!("concurrent content {}", i);
                let content_hash = hash_content(content.as_bytes());
                
                client
                    .put(format!("{}/sync/v3/files/{}", url, content_hash))
                    .header("Authorization", "Bearer test-token")
                    .header("rm-filename", format!("concurrent-{}.txt", i))
                    .body(content)
                    .send()
                    .await
                    .unwrap()
                    .status()
            })
        })
        .collect();
    
    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, 200);
    }
}

/// Test uploading large file
#[tokio::test]
async fn test_upload_large_file() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Create 1MB of data
    let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    let content_hash = hash_content(&large_content);
    
    let resp = client
        .put(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "large-file.bin")
        .body(large_content.clone())
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    // Download and verify
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), content_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "large-file.bin")
        .send()
        .await
        .unwrap();
    
    let downloaded = resp.bytes().await.unwrap();
    assert_eq!(downloaded.len(), large_content.len());
}
