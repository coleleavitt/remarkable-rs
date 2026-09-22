//! Document download integration tests
//!
//! Tests the complete document download flow:
//! 1. Get root hash
//! 2. Download root index (document list)
//! 3. Get document schema
//! 4. Download individual files (.metadata, .content, .rm)
//!
//! # Hash Tree Traversal
//!
//! ```text
//! /sync/v3/root          -> { hash, generation }
//! /sync/v3/files/{hash}  -> root.docSchema (JSON array of DocEntry)
//!   -> for each DocEntry.hash:
//!      /sync/v3/files/{hash} -> schema.txt (file list)
//!        -> for each file in schema:
//!           /sync/v3/files/{hash} -> file content
//! ```

mod common;

use common::{MockSyncServer, fixtures::*};
use remarkable_sync::{SyncClient, SyncError, DocEntry, DocumentSchema};
use reqwest::Client;

/// Test getting sync root
#[tokio::test]
async fn test_get_root_mock() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("hash").is_some());
    assert!(body.get("generation").is_some());
}

/// Test downloading file with rm-filename header
#[tokio::test]
async fn test_download_file_requires_rm_filename() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Request without rm-filename header should fail
    let resp = client
        .get(format!("{}/sync/v3/files/somehash", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 400);
}

/// Test downloading file with rm-filename header succeeds
#[tokio::test]
async fn test_download_file_with_header() {
    let server = MockSyncServer::start().await;
    
    // Add a test document
    let doc = TestDocument::new("Test Download");
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    let client = Client::new();
    
    // Get the root to find our document
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    // Download root index
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    
    // Verify it contains our document
    let docs: Vec<DocEntry> = resp.json().await.unwrap();
    assert!(!docs.is_empty());
    assert!(docs.iter().any(|d| d.uuid == doc.id));
}

/// Test full document download flow
#[tokio::test]
async fn test_full_document_download_mock() {
    let server = MockSyncServer::start().await;
    
    // Add a test document
    let doc = TestDocument::new("Full Download Test");
    let doc_id = doc.id.clone();
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    let client = Client::new();
    
    // Step 1: Get root
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    // Step 2: Download root index
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let docs: Vec<DocEntry> = resp.json().await.unwrap();
    
    // Step 3: Find our document
    let our_doc = docs.iter().find(|d| d.uuid == doc_id).unwrap();
    
    // Step 4: Download schema
    let schema_filename = format!("{}/schema.txt", doc_id);
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), our_doc.hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", &schema_filename)
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 200);
    let schema_text = resp.text().await.unwrap();
    
    // Schema should list files
    assert!(schema_text.contains(".metadata"));
    assert!(schema_text.contains(".content"));
    assert!(schema_text.contains(".rm"));
}

/// Test response includes CRC32C hash header
#[tokio::test]
async fn test_download_includes_crc_header() {
    let server = MockSyncServer::start().await;
    
    let doc = TestDocument::new("CRC Test");
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    let client = Client::new();
    
    // Get root
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    // Download file
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    // Check x-goog-hash header
    let headers = resp.headers();
    let hash_header = headers.get("x-goog-hash");
    assert!(hash_header.is_some(), "x-goog-hash header missing");
    
    let hash_value = hash_header.unwrap().to_str().unwrap();
    assert!(hash_value.starts_with("crc32c="), "Expected crc32c format");
}

/// Test 404 for missing file
#[tokio::test]
async fn test_download_missing_file() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    let resp = client
        .get(format!("{}/sync/v3/files/nonexistenthash", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "test.txt")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 404);
}

/// Test with USB device (skipped if device not available)
#[tokio::test]
#[ignore = "requires USB-connected device"]
async fn test_device_list_documents() {
    use remarkable_usb::UsbClient;
    
    if !device_available().await {
        eprintln!("Device not available at {}", DEVICE_USB_ADDR);
        return;
    }
    
    let client = UsbClient::new();
    
    assert!(client.is_connected().await, "Device should be connected");
    
    let docs = client.list_documents().await.unwrap();
    
    // Device should have at least some documents (even if empty)
    // Just verify the call succeeds
    eprintln!("Found {} documents on device", docs.len());
}

/// Test SyncClient with mock server  
#[tokio::test]
async fn test_sync_client_list_documents_mock() {
    // Note: This test would require modifying SyncClient to accept a custom base URL
    // For now, we test the HTTP layer directly
    
    let server = MockSyncServer::start().await;
    
    // Add documents
    let doc1 = TestDocument::new("Document 1");
    let doc2 = TestDocument::new("Document 2");
    
    server.state().add_document(
        &doc1.id,
        &doc1.metadata,
        &doc1.content,
        doc1.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    server.state().add_document(
        &doc2.id,
        &doc2.metadata,
        &doc2.content,
        doc2.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    let client = Client::new();
    
    // Get root
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    // Get document list
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    let docs: Vec<DocEntry> = resp.json().await.unwrap();
    assert_eq!(docs.len(), 2);
}

/// Test downloading .rm file structure
#[tokio::test]
async fn test_download_rm_file_structure() {
    // Test that generated .rm files have correct header
    let rm_data = generate_sample_rm_v6();
    
    // v6 header starts with "reMarkable .lines file, version=6"
    let header = std::str::from_utf8(&rm_data[..33]).unwrap();
    assert!(header.starts_with("reMarkable .lines file, version=6"),
        "Generated .rm should have v6 header");
    
    // Should have some data after header
    assert!(rm_data.len() > 43, "Generated .rm should have content after header");
}
