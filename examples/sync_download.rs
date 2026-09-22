//! Example: Download documents from reMarkable cloud
//!
//! Usage:
//!   1. Get fresh tokens from your device or via pairing
//!   2. Save device token to device_token.txt
//!   3. Save user token to user_token.txt
//!   4. Run: cargo run --example sync_download

use remarkable_sync::{SyncClient, DeviceToken, UserToken};
use std::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    // Default token paths
    let device_token_path = args.get(1).map(|s| s.as_str()).unwrap_or("device_token.txt");
    let user_token_path = args.get(2).map(|s| s.as_str()).unwrap_or("user_token.txt");
    
    // Load tokens from files
    let device_token = fs::read_to_string(device_token_path)?;
    let user_token = fs::read_to_string(user_token_path)?;
    
    // Parse region from user token
    let region = remarkable_sync::auth::parse_jwt_claim(&user_token, "tectonic")
        .unwrap_or_else(|| "eu".to_string());
    
    println!("Using region: {}", region);
    
    // Create client
    let client = SyncClient::new()
        .with_device_token(DeviceToken { token: device_token.trim().to_string() })
        .with_user_token(UserToken { 
            token: user_token.trim().to_string(),
            region: region.clone(),
            scopes: vec![],
        })
        .with_region(region);
    
    // Get sync root
    println!("Fetching sync root...");
    let root = client.get_root().await?;
    println!("Root hash: {}", root.hash);
    println!("Generation: {}", root.generation);
    
    // List documents
    println!("\nListing documents...");
    let docs = client.list_documents().await?;
    
    println!("Found {} documents:", docs.len());
    for doc in &docs {
        println!("  {} (v{}, {} bytes)", doc.uuid, doc.version, doc.size);
    }
    
    // Download first document as example
    if let Some(first) = docs.first() {
        println!("\nDownloading document {}...", first.uuid);
        
        // Get document schema to show structure
        let schema = client.get_document_schema(&first.hash, &first.uuid).await?;
        println!("Document contains {} files:", schema.files.len());
        for file in &schema.files {
            println!("  {} ({} bytes)", file.filename, file.size);
        }
        
        // Full download
        let doc = client.download_document(&first.uuid).await?;
        println!("\nDownloaded:");
        println!("  Metadata: {}", doc.metadata.as_ref().map(|m| m.visible_name.as_deref().unwrap_or("unnamed")).unwrap_or("none"));
        println!("  Content: {}", doc.content.is_some());
        println!("  Pages: {}", doc.pages.len());
        println!("  PDF: {}", doc.pdf.is_some());
    }
    
    // Option to download all
    if args.contains(&"--all".to_string()) {
        let output_dir = args.get(3).map(|s| s.as_str()).unwrap_or("./backup");
        println!("\nDownloading all documents to {}...", output_dir);
        let downloaded = client.download_all(output_dir).await?;
        println!("Downloaded {} documents", downloaded.len());
    }
    
    Ok(())
}
