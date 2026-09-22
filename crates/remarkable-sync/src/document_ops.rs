//! Document operations for creating, updating, and deleting documents
//!
//! Provides high-level operations for document management:
//! - Create notebooks and folders
//! - Rename and move documents
//! - Delete documents (soft and hard delete)
//! - Folder operations

use crate::error::SyncError;
use crate::client::{SyncClient, DocMetadata, ContentFile, CPages, CPageInfo, DocEntry, UploadContext};
use uuid::Uuid;
use chrono::Utc;
use sha2::{Sha256, Digest};

/// Create a new blank notebook document
/// 
/// Creates:
/// - Metadata file with visible name
/// - Content file with single page
/// - Empty .rm file for the page
pub async fn create_notebook(
    client: &SyncClient,
    name: &str,
    parent: Option<&str>,
) -> Result<String, SyncError> {
    let doc_id = Uuid::new_v4().to_string();
    let page_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp_millis().to_string();
    
    // Create metadata
    let metadata = DocMetadata {
        created_time: Some(now.clone()),
        last_modified: Some(now.clone()),
        last_opened: None,
        last_opened_page: Some(0),
        parent: parent.map(String::from),
        pinned: Some(false),
        doc_type: Some("DocumentType".to_string()),
        visible_name: Some(name.to_string()),
        deleted: Some(false),
    };
    
    // Create content file
    let content = ContentFile {
        cover_page_number: Some(0),
        file_type: Some("notebook".to_string()),
        page_count: Some(1),
        c_pages: Some(CPages {
            pages: Some(vec![CPageInfo {
                id: page_id.clone(),
            }]),
        }),
    };
    
    // Get current root for upload context
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash.clone());
    ctx.expect_generation = Some(root.generation);
    
    // Serialize and upload metadata
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    let metadata_filename = format!("{}.metadata", doc_id);
    client.upload_file_with_context(
        metadata_json.as_bytes(),
        &metadata_filename,
        Some(&mut ctx)
    ).await?;
    
    // Serialize and upload content
    let content_json = serde_json::to_string(&content)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    let content_filename = format!("{}.content", doc_id);
    client.upload_file_with_context(
        content_json.as_bytes(),
        &content_filename,
        Some(&mut ctx)
    ).await?;
    
    // Create empty .rm file for the page
    let rm_data = create_empty_rm_v6();
    let rm_filename = format!("{}/{}.rm", doc_id, page_id);
    client.upload_file_with_context(
        &rm_data,
        &rm_filename,
        Some(&mut ctx)
    ).await?;
    
    Ok(doc_id)
}

/// Create an empty v6 .rm file
/// 
/// The v6 format header is 43 bytes plus a null terminator.
fn create_empty_rm_v6() -> Vec<u8> {
    // Try using remarkable-lines if available
    #[cfg(feature = "lines")]
    {
        use remarkable_lines::V6Writer;
        if let Ok(data) = V6Writer::new().write_strokes(&[]) {
            return data;
        }
    }
    
    // Fallback: minimal v6 header
    let mut data = Vec::new();
    // Header: "reMarkable .lines file, version=6" padded to 43 bytes + null
    let header = b"reMarkable .lines file, version=6          \0";
    data.extend_from_slice(header);
    data
}

/// Rename a document
pub async fn rename_document(
    client: &SyncClient,
    doc_id: &str,
    new_name: &str,
) -> Result<(), SyncError> {
    // Get document list to find the document
    let docs = client.list_documents().await?;
    let doc = docs.iter()
        .find(|d| d.uuid == doc_id)
        .ok_or_else(|| SyncError::NotFound(doc_id.to_string()))?;
    
    // Get document schema
    let schema = client.get_document_schema(&doc.hash, doc_id).await?;
    
    // Find metadata entry
    let metadata_entry = schema.files.iter()
        .find(|f| f.filename.ends_with(".metadata"))
        .ok_or_else(|| SyncError::NotFound("metadata file".to_string()))?;
    
    // Download metadata
    let metadata_bytes = client.download_file(
        &metadata_entry.hash,
        &metadata_entry.filename
    ).await?;
    
    // Parse and update
    let mut metadata: DocMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    let now = Utc::now().timestamp_millis().to_string();
    metadata.visible_name = Some(new_name.to_string());
    metadata.last_modified = Some(now);
    
    // Re-upload with context
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash);
    
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file_with_context(
        new_metadata.as_bytes(), 
        &metadata_entry.filename,
        Some(&mut ctx)
    ).await?;
    
    Ok(())
}

/// Move a document to a different folder
/// 
/// Set `new_parent` to `None` or empty string for root level.
pub async fn move_document(
    client: &SyncClient,
    doc_id: &str,
    new_parent: Option<&str>,
) -> Result<(), SyncError> {
    let docs = client.list_documents().await?;
    let doc = docs.iter()
        .find(|d| d.uuid == doc_id)
        .ok_or_else(|| SyncError::NotFound(doc_id.to_string()))?;
    
    let schema = client.get_document_schema(&doc.hash, doc_id).await?;
    let metadata_entry = schema.files.iter()
        .find(|f| f.filename.ends_with(".metadata"))
        .ok_or_else(|| SyncError::NotFound("metadata file".to_string()))?;
    
    let metadata_bytes = client.download_file(
        &metadata_entry.hash,
        &metadata_entry.filename
    ).await?;
    
    let mut metadata: DocMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    let now = Utc::now().timestamp_millis().to_string();
    metadata.parent = new_parent.filter(|p| !p.is_empty()).map(String::from);
    metadata.last_modified = Some(now);
    
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash);
    
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file_with_context(
        new_metadata.as_bytes(), 
        &metadata_entry.filename,
        Some(&mut ctx)
    ).await?;
    
    Ok(())
}

/// Soft delete a document (mark as deleted)
/// 
/// This sets the `deleted` flag in metadata to true.
/// The document can be recovered by setting `deleted` to false.
pub async fn soft_delete_document(
    client: &SyncClient,
    doc_id: &str,
) -> Result<(), SyncError> {
    let docs = client.list_documents().await?;
    let doc = docs.iter()
        .find(|d| d.uuid == doc_id)
        .ok_or_else(|| SyncError::NotFound(doc_id.to_string()))?;
    
    let schema = client.get_document_schema(&doc.hash, doc_id).await?;
    let metadata_entry = schema.files.iter()
        .find(|f| f.filename.ends_with(".metadata"))
        .ok_or_else(|| SyncError::NotFound("metadata file".to_string()))?;
    
    let metadata_bytes = client.download_file(
        &metadata_entry.hash,
        &metadata_entry.filename
    ).await?;
    
    let mut metadata: DocMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    let now = Utc::now().timestamp_millis().to_string();
    metadata.deleted = Some(true);
    metadata.last_modified = Some(now);
    
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash);
    
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file_with_context(
        new_metadata.as_bytes(),
        &metadata_entry.filename,
        Some(&mut ctx)
    ).await?;
    
    Ok(())
}

/// Hard delete a document (remove from root index)
/// 
/// This removes the document entry from the root index,
/// then updates the root hash.
/// 
/// **Warning**: This is permanent and cannot be undone.
pub async fn hard_delete_document(
    client: &SyncClient,
    doc_id: &str,
) -> Result<(), SyncError> {
    // Get current root
    let root = client.get_root().await?;
    
    // Get all documents
    let root_data = client.download_file(&root.hash, "root.docSchema").await?;
    let docs: Vec<DocEntry> = serde_json::from_slice(&root_data)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    // Filter out the document to delete
    let new_docs: Vec<&DocEntry> = docs.iter()
        .filter(|d| d.uuid != doc_id)
        .collect();
    
    if new_docs.len() == docs.len() {
        return Err(SyncError::NotFound(doc_id.to_string()));
    }
    
    // Upload new root index
    let new_root_data = serde_json::to_vec(&new_docs)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    let mut ctx = UploadContext::new(root.hash.clone());
    ctx.expect_generation = Some(root.generation);
    
    let upload_result = client.upload_file_with_context(
        &new_root_data,
        "root.docSchema",
        Some(&mut ctx)
    ).await?;
    
    // Update root hash
    client.update_root(&upload_result.hash, root.generation + 1).await?;
    
    Ok(())
}

/// Restore a soft-deleted document
pub async fn restore_document(
    client: &SyncClient,
    doc_id: &str,
) -> Result<(), SyncError> {
    let docs = client.list_documents().await?;
    let doc = docs.iter()
        .find(|d| d.uuid == doc_id)
        .ok_or_else(|| SyncError::NotFound(doc_id.to_string()))?;
    
    let schema = client.get_document_schema(&doc.hash, doc_id).await?;
    let metadata_entry = schema.files.iter()
        .find(|f| f.filename.ends_with(".metadata"))
        .ok_or_else(|| SyncError::NotFound("metadata file".to_string()))?;
    
    let metadata_bytes = client.download_file(
        &metadata_entry.hash,
        &metadata_entry.filename
    ).await?;
    
    let mut metadata: DocMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    let now = Utc::now().timestamp_millis().to_string();
    metadata.deleted = Some(false);
    metadata.last_modified = Some(now);
    
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash);
    
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file_with_context(
        new_metadata.as_bytes(),
        &metadata_entry.filename,
        Some(&mut ctx)
    ).await?;
    
    Ok(())
}

/// Create a folder (collection)
pub async fn create_folder(
    client: &SyncClient,
    name: &str,
    parent: Option<&str>,
) -> Result<String, SyncError> {
    let folder_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp_millis().to_string();
    
    // Create metadata with CollectionType
    let metadata = DocMetadata {
        created_time: Some(now.clone()),
        last_modified: Some(now),
        last_opened: None,
        last_opened_page: None,
        parent: parent.filter(|p| !p.is_empty()).map(String::from),
        pinned: Some(false),
        doc_type: Some("CollectionType".to_string()),
        visible_name: Some(name.to_string()),
        deleted: Some(false),
    };
    
    let root = client.get_root().await?;
    let mut ctx = UploadContext::new(root.hash);
    
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    client.upload_file_with_context(
        metadata_json.as_bytes(),
        &format!("{}.metadata", folder_id),
        Some(&mut ctx)
    ).await?;
    
    Ok(folder_id)
}

/// Rename a folder
pub async fn rename_folder(
    client: &SyncClient,
    folder_id: &str,
    new_name: &str,
) -> Result<(), SyncError> {
    // Same as rename_document - folders use same metadata structure
    rename_document(client, folder_id, new_name).await
}

/// Move a folder
pub async fn move_folder(
    client: &SyncClient,
    folder_id: &str,
    new_parent: Option<&str>,
) -> Result<(), SyncError> {
    // Same as move_document - folders use same metadata structure
    move_document(client, folder_id, new_parent).await
}

/// Delete a folder (soft delete)
/// 
/// Note: This does not automatically delete contents.
/// Use `delete_folder_recursive` to delete a folder and all its contents.
pub async fn delete_folder(
    client: &SyncClient,
    folder_id: &str,
) -> Result<(), SyncError> {
    soft_delete_document(client, folder_id).await
}

/// Recursively delete a folder and all its contents
/// Delete a folder and all its contents recursively
/// 
/// Uses Box::pin for the recursive async call.
pub fn delete_folder_recursive<'a>(
    client: &'a SyncClient,
    folder_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, SyncError>> + Send + 'a>> {
    Box::pin(async move {
        let docs = client.list_documents().await?;
        let mut deleted = Vec::new();
        
        // Find all documents in this folder
        for doc in &docs {
            let schema = match client.get_document_schema(&doc.hash, &doc.uuid).await {
                Ok(s) => s,
                Err(_) => continue,
            };
            
            let metadata_entry = match schema.files.iter().find(|f| f.filename.ends_with(".metadata")) {
                Some(e) => e,
                None => continue,
            };
            
            let metadata_bytes = match client.download_file(&metadata_entry.hash, &metadata_entry.filename).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            
            let metadata: DocMetadata = match serde_json::from_slice(&metadata_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };
            
            // Check if this item is in the folder
            if metadata.parent.as_deref() == Some(folder_id) {
                // Recursively delete folders
                if metadata.is_folder() {
                    let sub_deleted = delete_folder_recursive(client, &doc.uuid).await?;
                    deleted.extend(sub_deleted);
                }
                
                // Soft delete this item
                soft_delete_document(client, &doc.uuid).await?;
                deleted.push(doc.uuid.clone());
            }
        }
        
        // Delete the folder itself
        soft_delete_document(client, folder_id).await?;
        deleted.push(folder_id.to_string());
        
        Ok(deleted)
    })
}

/// List folder contents
pub async fn list_folder_contents(
    client: &SyncClient,
    folder_id: Option<&str>,
) -> Result<Vec<(DocEntry, DocMetadata)>, SyncError> {
    let docs = client.list_documents().await?;
    let mut contents = Vec::new();
    
    for doc in docs {
        let schema = match client.get_document_schema(&doc.hash, &doc.uuid).await {
            Ok(s) => s,
            Err(_) => continue,
        };
        
        let metadata_entry = match schema.files.iter().find(|f| f.filename.ends_with(".metadata")) {
            Some(e) => e,
            None => continue,
        };
        
        let metadata_bytes = match client.download_file(&metadata_entry.hash, &metadata_entry.filename).await {
            Ok(b) => b,
            Err(_) => continue,
        };
        
        let metadata: DocMetadata = match serde_json::from_slice(&metadata_bytes) {
            Ok(m) => m,
            Err(_) => continue,
        };
        
        // Filter by parent
        let parent = metadata.parent.as_deref().filter(|p| !p.is_empty());
        if parent == folder_id {
            // Skip deleted items
            if metadata.deleted.unwrap_or(false) {
                continue;
            }
            contents.push((doc, metadata));
        }
    }
    
    Ok(contents)
}

/// Compute hash of serialized data
pub fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_rm_v6() {
        let data = create_empty_rm_v6();
        assert!(data.len() >= 43);
        assert!(data.starts_with(b"reMarkable"));
    }
    
    #[test]
    fn test_compute_hash() {
        let hash = compute_hash(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
