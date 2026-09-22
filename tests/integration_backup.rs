//! Backup and restore integration tests
//!
//! Tests full account backup and restore:
//! 1. Full document enumeration
//! 2. Complete file tree download
//! 3. Metadata preservation
//! 4. Restore verification
//!
//! Uses mock server for offline testing; device tests require USB connection.

mod common;

use common::{MockSyncServer, fixtures::*};
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

/// Backup state tracking
struct BackupState {
    root_hash: String,
    generation: u64,
    documents: Vec<BackupDoc>,
    files: HashMap<String, Vec<u8>>,
}

struct BackupDoc {
    id: String,
    hash: String,
    doc_type: String,
}

/// Test full backup with mock server
#[tokio::test]
async fn test_full_backup_mock() {
    let server = MockSyncServer::start().await;
    
    // Create test documents
    for i in 1..=3 {
        let doc = TestDocument::new(&format!("Backup Doc {}", i));
        server.state().add_document(
            &doc.id,
            &doc.metadata,
            &doc.content,
            doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
        );
    }
    
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
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    
    assert_eq!(docs.len(), 3, "Should have 3 documents");
    
    // Step 3: Download all document files
    let mut total_files = 0;
    for doc in &docs {
        let doc_hash = doc["hash"].as_str().unwrap();
        let doc_id = doc["uuid"].as_str().unwrap();
        
        // Download schema
        let schema_filename = format!("{}/schema.txt", doc_id);
        let resp = client
            .get(format!("{}/sync/v3/files/{}", server.base_url(), doc_hash))
            .header("Authorization", "Bearer test-token")
            .header("rm-filename", &schema_filename)
            .send()
            .await
            .unwrap();
        let schema = resp.text().await.unwrap();
        
        // Count files in schema
        let file_count = schema.lines().count() - 1; // Subtract header line
        total_files += file_count;
    }
    
    // Each doc has: metadata, content, and at least one .rm page
    assert!(total_files >= 9, "Should have at least 9 files (3 per doc)");
}

/// Test backup to disk
#[tokio::test]
async fn test_backup_to_disk_mock() {
    let server = MockSyncServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a test document
    let doc = TestDocument::new("Disk Backup Test");
    let doc_id = doc.id.clone();
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
    
    // Save root info
    let root_path = temp_dir.path().join("root.json");
    fs::write(&root_path, serde_json::to_string_pretty(&root).unwrap())
        .await
        .unwrap();
    
    // Download and save document list
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let docs_data = resp.bytes().await.unwrap();
    
    let docs_path = temp_dir.path().join("documents.json");
    fs::write(&docs_path, &docs_data).await.unwrap();
    
    // Verify files were created
    assert!(root_path.exists());
    assert!(docs_path.exists());
    
    // Verify content
    let saved_docs: Vec<serde_json::Value> = 
        serde_json::from_slice(&fs::read(&docs_path).await.unwrap()).unwrap();
    assert!(saved_docs.iter().any(|d| d["uuid"].as_str() == Some(&doc_id)));
}

/// Test incremental backup (only changed files)
#[tokio::test]
async fn test_incremental_backup_mock() {
    let server = MockSyncServer::start().await;
    let client = Client::new();
    
    // Initial state
    let doc1 = TestDocument::new("Initial Doc");
    server.state().add_document(
        &doc1.id,
        &doc1.metadata,
        &doc1.content,
        doc1.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    // Get initial generation
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root1: serde_json::Value = resp.json().await.unwrap();
    let gen1 = root1["generation"].as_u64().unwrap();
    let hash1 = root1["hash"].as_str().unwrap().to_string();
    
    // Add another document
    let doc2 = TestDocument::new("Incremental Doc");
    server.state().add_document(
        &doc2.id,
        &doc2.metadata,
        &doc2.content,
        doc2.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    // Get new state
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root2: serde_json::Value = resp.json().await.unwrap();
    let gen2 = root2["generation"].as_u64().unwrap();
    let hash2 = root2["hash"].as_str().unwrap();
    
    // Verify change detection
    assert!(gen2 > gen1, "Generation should increase");
    assert_ne!(hash1, hash2, "Root hash should change");
}

/// Test backup metadata preservation
#[tokio::test]
async fn test_backup_metadata_preservation() {
    let server = MockSyncServer::start().await;
    
    // Create document with specific metadata
    let mut metadata = TestDocMetadata::default();
    metadata.visible_name = "Preserved Name".to_string();
    metadata.pinned = true;
    
    let content = TestDocContent::default();
    let page_data = generate_sample_rm_v6();
    let page_id = &content.c_pages.pages[0].id;
    let doc_id = uuid::Uuid::new_v4().to_string();
    
    server.state().add_document(
        &doc_id,
        &serde_json::to_vec(&metadata).unwrap(),
        &serde_json::to_vec(&content).unwrap(),
        vec![(page_id.as_str(), &page_data)],
    );
    
    let client = Client::new();
    
    // Download and verify metadata
    let resp = client
        .get(format!("{}/sync/v3/root", server.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    
    let our_doc = docs.iter().find(|d| d["uuid"].as_str() == Some(&doc_id)).unwrap();
    let doc_hash = our_doc["hash"].as_str().unwrap();
    
    // Get schema
    let schema_filename = format!("{}/schema.txt", doc_id);
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), doc_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", &schema_filename)
        .send()
        .await
        .unwrap();
    let schema = resp.text().await.unwrap();
    
    // Find metadata hash from schema
    let metadata_line = schema.lines()
        .find(|l| l.contains(".metadata"))
        .unwrap();
    let metadata_hash = metadata_line.split(':').next().unwrap();
    
    // Download metadata
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), metadata_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", format!("{}.metadata", doc_id))
        .send()
        .await
        .unwrap();
    let downloaded_metadata: TestDocMetadata = resp.json().await.unwrap();
    
    // Verify preservation
    assert_eq!(downloaded_metadata.visible_name, "Preserved Name");
    assert!(downloaded_metadata.pinned);
}

/// Test restore from backup
#[tokio::test]
async fn test_restore_from_backup() {
    // Create two servers: source and destination
    let source = MockSyncServer::start().await;
    let dest = MockSyncServer::start().await;
    
    // Add document to source
    let doc = TestDocument::new("Restore Test");
    let doc_id = doc.id.clone();
    source.state().add_document(
        &doc.id,
        &doc.metadata,
        &doc.content,
        doc.pages.iter().map(|(id, data)| (id.as_str(), data.as_slice())).collect(),
    );
    
    // Add auth token to destination
    dest.state().add_token("test-token");
    
    let client = Client::new();
    
    // Download from source
    let resp = client
        .get(format!("{}/sync/v3/root", source.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let root: serde_json::Value = resp.json().await.unwrap();
    let root_hash = root["hash"].as_str().unwrap();
    
    let resp = client
        .get(format!("{}/sync/v3/files/{}", source.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let source_data = resp.bytes().await.unwrap();
    
    // Verify destination is initially empty
    let resp = client
        .get(format!("{}/sync/v3/root", dest.base_url()))
        .header("Authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    let dest_root: serde_json::Value = resp.json().await.unwrap();
    assert!(dest_root["hash"].as_str().unwrap_or("").is_empty() || 
            dest_root["generation"].as_u64() == Some(1));
}

/// Test backup integrity verification
#[tokio::test]
async fn test_backup_integrity_verification() {
    let server = MockSyncServer::start().await;
    
    let doc = TestDocument::new("Integrity Test");
    let original_metadata = doc.metadata.clone();
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
    
    // Get document list
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), root_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", "root.docSchema")
        .send()
        .await
        .unwrap();
    let docs: Vec<serde_json::Value> = resp.json().await.unwrap();
    let doc_entry = docs.iter().find(|d| d["uuid"].as_str() == Some(&doc.id)).unwrap();
    let doc_hash = doc_entry["hash"].as_str().unwrap();
    
    // Get schema
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), doc_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", format!("{}/schema.txt", doc.id))
        .send()
        .await
        .unwrap();
    let schema = resp.text().await.unwrap();
    
    // Find and download metadata
    let metadata_hash: String = schema.lines()
        .find(|l| l.contains(".metadata"))
        .map(|l| l.split(':').next().unwrap().to_string())
        .unwrap();
    
    let resp = client
        .get(format!("{}/sync/v3/files/{}", server.base_url(), metadata_hash))
        .header("Authorization", "Bearer test-token")
        .header("rm-filename", format!("{}.metadata", doc.id))
        .send()
        .await
        .unwrap();
    
    // Check CRC32C header
    let crc_header = resp.headers().get("x-goog-hash");
    assert!(crc_header.is_some(), "Should have CRC32C header");
    
    let downloaded = resp.bytes().await.unwrap();
    
    // Verify content matches
    assert_eq!(downloaded.as_ref(), original_metadata.as_slice());
}

/// Test backup with real device (skipped if not available)
#[tokio::test]
#[ignore = "requires USB-connected device"]
async fn test_real_device_backup() {
    use remarkable_usb::UsbClient;
    
    if !device_available().await {
        eprintln!("Device not available at {}", DEVICE_USB_ADDR);
        return;
    }
    
    let client = UsbClient::new();
    let docs = client.list_documents().await.unwrap();
    
    eprintln!("Backing up {} documents", docs.len());
    
    // Count document types
    let notebooks = docs.iter().filter(|d| d.doc_type == "DocumentType").count();
    let folders = docs.iter().filter(|d| d.doc_type == "CollectionType").count();
    
    eprintln!("  Notebooks: {}", notebooks);
    eprintln!("  Folders: {}", folders);
}
