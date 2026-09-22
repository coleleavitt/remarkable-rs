//! Document operations for creating, updating, and deleting documents

use crate::error::SyncError;
use crate::client::{SyncClient, DocMetadata, ContentFile, CPages, CPageInfo};
use uuid::Uuid;
use chrono::Utc;

/// Create a new blank notebook document
pub async fn create_notebook(
    client: &SyncClient,
    name: &str,
    parent: Option<&str>,
) -> Result<String, SyncError> {
    let doc_id = Uuid::new_v4().to_string();
    let page_id = Uuid::new_v4().to_string();
    
    // Create metadata
    let metadata = DocMetadata {
        created_time: Some(Utc::now().to_rfc3339()),
        last_modified: Some(Utc::now().to_rfc3339()),
        last_opened: None,
        last_opened_page: Some(0),
        parent: parent.map(String::from),
        pinned: Some(false),
        doc_type: Some("DocumentType".to_string()),
        visible_name: Some(name.to_string()),
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
    
    // Serialize and upload metadata
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    let metadata_filename = format!("{}.metadata", doc_id);
    let _metadata_hash = client.upload_file(
        metadata_json.as_bytes(),
        &metadata_filename
    ).await?;
    
    // Serialize and upload content
    let content_json = serde_json::to_string(&content)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    let content_filename = format!("{}.content", doc_id);
    let _content_hash = client.upload_file(
        content_json.as_bytes(),
        &content_filename
    ).await?;
    
    // Create empty .rm file for the page
    let rm_data = create_empty_rm_v6();
    let rm_filename = format!("{}/{}.rm", doc_id, page_id);
    let _rm_hash = client.upload_file(
        &rm_data,
        &rm_filename
    ).await?;
    
    Ok(doc_id)
}

/// Create an empty v6 .rm file
fn create_empty_rm_v6() -> Vec<u8> {
    use remarkable_lines::V6Writer;
    
    let mut writer = V6Writer::new();
    writer.write_strokes(&[]).unwrap_or_else(|_| {
        // Fallback: just the header
        b"reMarkable .lines file, version=6          ".to_vec()
    })
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
    
    metadata.visible_name = Some(new_name.to_string());
    metadata.last_modified = Some(Utc::now().to_rfc3339());
    
    // Re-upload
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file(new_metadata.as_bytes(), &metadata_entry.filename).await?;
    
    Ok(())
}

/// Move a document to a different folder
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
    
    metadata.parent = new_parent.map(String::from);
    metadata.last_modified = Some(Utc::now().to_rfc3339());
    
    let new_metadata = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    client.upload_file(new_metadata.as_bytes(), &metadata_entry.filename).await?;
    
    Ok(())
}

/// Create a folder
pub async fn create_folder(
    client: &SyncClient,
    name: &str,
    parent: Option<&str>,
) -> Result<String, SyncError> {
    let folder_id = Uuid::new_v4().to_string();
    
    // Create metadata with CollectionType
    let metadata = DocMetadata {
        created_time: Some(Utc::now().to_rfc3339()),
        last_modified: Some(Utc::now().to_rfc3339()),
        last_opened: None,
        last_opened_page: None,
        parent: parent.map(String::from),
        pinned: Some(false),
        doc_type: Some("CollectionType".to_string()),
        visible_name: Some(name.to_string()),
    };
    
    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| SyncError::Parse(e.to_string()))?;
    
    client.upload_file(
        metadata_json.as_bytes(),
        &format!("{}.metadata", folder_id)
    ).await?;
    
    Ok(folder_id)
}
