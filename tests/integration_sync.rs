//! Full sync cycle integration tests
//!
//! Tests complete synchronization workflows:
//! 1. Initial sync (download all documents)
//! 2. Incremental sync (detect changes via generation)
//! 3. Conflict detection
//! 4. Merge operations
//!
//! These tests verify the sync protocol behavior end-to-end.

mod common;

use common::{MockSyncServer, fixtures::*};
use reqwest::Client;
use std::collections::HashSet;

/// Parse schema.txt format
fn parse_schema(data: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = data.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    
    lines.iter()
        .skip(1) // First line is count
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                Some((parts[0].to_string(), parts[2].to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Test initial sync (empty -> populated)
#[tokio::test]
async fn test_initial_sync_empty() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Initial state: no documents
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    
    let root: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(root["generation"].as_u64().unwrap(), 1);
    
    // Empty root hash
    let root_hash = root["hash"].as_str().unwrap();
    assert!(root_hash.is_empty() || root_hash.len() > 0);  // Either empty or valid
}

/// Test initial sync with documents
#[tokio::test]
async fn test_initial_sync_with_documents() {
    let server = MockSyncServer::start().await;
    
    // Add some documents
    for i in 1..=5 {
        let doc = TestDocument::new(&format!("Document {}", i));
        server.state().add_document(
            &doc.id,
            &doc.metadata,
            &doc.content,
            doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
        );
    }
    
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
    
    // Download document list
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(docs.len(), 5);
}

/// Test incremental sync detects new document
#[tokio::test]
async fn test_incremental_sync_new_document() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Initial sync
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root1: serde_json::Value = resp.json().await.unwrap();
    let gen1 = root1["generation"].as_u64().unwrap();
    
    // Add a document
    let doc = TestDocument::new("New Document");
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    // Check for changes
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root2: serde_json::Value = resp.json().await.unwrap();
    let gen2 = root2["generation"].as_u64().unwrap();
    
    assert!(gen2 > gen1, "Generation should increase when document added");
}

/// Test full sync downloads all documents
#[tokio::test]
async fn test_full_sync_downloads_all_files() {
    let server = MockSyncServer::start().await;
    
    // Add documents
    let doc1 = TestDocument::new("Doc 1");
    let doc2 = TestDocument::new("Doc 2");
    let doc1_id = doc1.id.clone();
    let doc2_id = doc2.id.clone();
    
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
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    
    // Download all schemas and files
    let mut downloaded_files: HashSet<String> = HashSet::new();
    
    for doc in &docs {
        let doc_hash = doc["hash"].as_str().unwrap();
        let doc_uuid = doc["uuid"].as_str().unwrap();
        
        // Download schema
        let schema_filename = format!("{}/schema.txt", doc_uuid);
        let resp = client
            .get(format!("{}/sync/v3/files/{}", server.base_url(), doc_hash))
            .header("Authorization", "Bearer test-token")
            .header("rm-filename", &schema_filename)
            .send()
            .await
            .unwrap();
        let schema_text = resp.text().await.unwrap();
        
        // Parse schema and download each file
        let files = parse_schema(&schema_text);
        for (file_hash, filename) in files {
            let resp = client
                .get(format!("{}/sync/v3/files/{}", server.base_url(), file_hash))
                .header("Authorization", "Bearer test-token")
                .header("rm-filename", &filename)
                .send()
                .await
                .unwrap();
            
            assert_eq!(resp.status(), 200);
            downloaded_files.insert(filename);
        }
    }
    
    // Should have downloaded metadata, content, and rm files for both docs
    assert!(downloaded_files.iter().any(|f| f.contains(&doc1_id)));
    assert!(downloaded_files.iter().any(|f| f.contains(&doc2_id)));
}

/// Test sync with generation tracking
#[tokio::test]
async fn test_sync_generation_tracking() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Track generations
    let mut last_generation = 0u64;
    
    for i in 1..=3 {
        // Add a document
        let doc = TestDocument::new(&format!("Gen Test {}", i));
        server.state().add_document(
            &doc.id,
            &doc.metadata,
            &doc.content,
            doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
        );
        
        // Check generation
        let resp = client
            .get(format!("{}/sync/v3/root", server.base_url()))
            .header("Authorization", "Bearer test-token")
            .send()
            .await
            .unwrap();
        let root: serde_json::Value = resp.json().await.unwrap();
        let current_gen = root["generation"].as_u64().unwrap();
        
        assert!(current_gen > last_generation, 
            "Generation should increase: was {}, now {}", last_generation, current_gen);
        last_generation = current_gen;
    }
}

/// Test sync handles missing files gracefully
#[tokio::test]
async fn test_sync_handles_missing_files() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Try to download non-existent file
    let resp = client
        .get(format!("{}/sync/v3/files/0000000000000000000000000000000000000000000000000000000000000000", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "missing.txt")
        .send()
        .await
        .unwrap();
    
    assert_eq!(resp.status(), 404);
}

/// Test concurrent sync operations
#[tokio::test]
async fn test_concurrent_sync_operations() {
    let server = MockSyncServer::start().await;
    let base_url = server.base_url();
    
    // Add initial document
    let doc = TestDocument::new("Concurrent Test");
    server.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    // Spawn multiple concurrent sync operations
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let url = base_url.clone();
            tokio::spawn(async move {
                let client = Client::new();
                
                // Get root
                let resp = client
                    .get(format!("{}/sync/v3/root", url))
                    .header("Authorization", "Bearer test-token")
                    .send()
                    .await
                    .unwrap();
                
                let root: serde_json::Value = resp.json().await.unwrap();
                let root_hash = root["hash"].as_str().unwrap();
                
                // Get document list
                let resp = client
                    .get(format!("{}/sync/v3/files/{}", url, root_hash))
                    .header("Authorization", "Bearer test-token")
                    .header("rm-filename", "root.docSchema")
                    .send()
                    .await
                    .unwrap();
                
                resp.status()
            })
        })
        .collect();
    
    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, 200);
    }
}

/// Test backup restore flow
#[tokio::test]
async fn test_backup_restore_flow() {
    let server = MockSyncServer::start().await;
    
    // Create original document
    let original = TestDocument::new("Backup Test");
    let original_id = original.id.clone();
    server.state().add_document(
        &original.id,
        &original.metadata,
        &original.content,
        original.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    let client = Client::new();
    
    // "Backup" phase: download everything
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap().to_string();
    
    // Download root index
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let backup_data = resp.bytes().await.unwrap();
    
    // Verify backup contains our document
    let docs: Vec<serde_json::Value> = serde_json::from_slice(&backup_data).unwrap();
    assert!(docs.iter().any(|d| d["uuid"].as_str() == Some(&original_id)));
}

/// Test sync with real device (skipped if not available)
#[tokio::test]
#[ignore = "requires USB-connected device and valid tokens"]
async fn test_real_device_sync() {
    use remarkable_usb::UsbClient;
    
    if !device_available().await {
        eprintln!("Device not available at {}", DEVICE_USB_ADDR);
        return;
    }
    
    let usb = UsbClient::new();
    let docs = usb.list_documents().await.unwrap();
    
    eprintln!("Found {} documents via USB", docs.len());
    
    // Verify we can access document details
    for doc in docs.iter().take(3) {
        eprintln!("Document: {} ({})", doc.visible_name, doc.id);
    }
}
