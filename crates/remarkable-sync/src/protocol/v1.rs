//! Sync V1: Original document-storage JSON API
//!
//! This is the original sync protocol used in firmware 1.x-2.x.
//! It uses a simple document list with metadata stored on the server.
//!
//! ## Endpoints
//!
//! - `GET /document-storage/json/2/docs` - List all documents
//! - `GET /document-storage/json/2/docs/{id}` - Get document metadata
//! - `PUT /document-storage/json/2/upload/request` - Request upload URL
//! - `PUT /document-storage/json/2/upload/update-status` - Confirm upload
//! - `DELETE /document-storage/json/2/delete` - Delete document

use std::collections::HashMap;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};

use crate::error::SyncError;
use crate::protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo,
    Document, SyncResult, UploadResult, BoxFuture,
};

/// V1 document entry from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V1DocEntry {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Version")]
    pub version: i32,
    #[serde(rename = "Message")]
    pub message: Option<String>,
    #[serde(rename = "Success")]
    pub success: Option<bool>,
    #[serde(rename = "BlobURLGet")]
    pub blob_url_get: Option<String>,
    #[serde(rename = "BlobURLGetExpires")]
    pub blob_url_get_expires: Option<String>,
    #[serde(rename = "BlobURLPut")]
    pub blob_url_put: Option<String>,
    #[serde(rename = "BlobURLPutExpires")]
    pub blob_url_put_expires: Option<String>,
    #[serde(rename = "ModifiedClient")]
    pub modified_client: Option<String>,
    #[serde(rename = "Type")]
    pub doc_type: Option<String>,
    #[serde(rename = "VissibleName")]  // Note: typo in original API
    pub visible_name: Option<String>,
    #[serde(rename = "CurrentPage")]
    pub current_page: Option<i32>,
    #[serde(rename = "Bookmarked")]
    pub bookmarked: Option<bool>,
    #[serde(rename = "Parent")]
    pub parent: Option<String>,
}

impl From<V1DocEntry> for DocumentInfo {
    fn from(entry: V1DocEntry) -> Self {
        Self {
            id: entry.id,
            doc_type: entry.doc_type.unwrap_or_else(|| "DocumentType".to_string()),
            name: entry.visible_name,
            parent: entry.parent,
            hash: None,
            version: entry.version as u32,
            modified_at: entry.modified_client,
            deleted: false,
            metadata: HashMap::new(),
        }
    }
}

/// Upload request body
#[derive(Debug, Serialize)]
struct UploadRequest {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Version")]
    version: i32,
    #[serde(rename = "Type")]
    doc_type: String,
}

/// Delete request body
#[derive(Debug, Serialize)]
struct DeleteRequest {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Version")]
    version: i32,
}

/// V1 Sync Protocol Client
pub struct SyncV1Client {
    client: Client,
    config: SyncConfig,
}

impl SyncV1Client {
    /// Create a new V1 client
    pub fn new(config: SyncConfig) -> Result<Self, SyncError> {
        let client = if config.skip_tls_verify {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()?
        } else {
            Client::builder()
                .timeout(std::time::Duration::from_secs(config.timeout_secs))
                .build()?
        };
        
        Ok(Self { client, config })
    }
    
    /// Get authorization header
    fn auth_header(&self) -> Result<String, SyncError> {
        self.config.user_token.as_ref()
            .or(self.config.device_token.as_ref())
            .map(|t| format!("Bearer {}", t))
            .ok_or(SyncError::AuthRequired)
    }
    
    /// Get the document-storage API base URL
    fn api_url(&self) -> String {
        format!("{}/document-storage/json/2", self.config.base_url)
    }
    
    /// List all documents
    pub async fn list_docs(&self) -> Result<Vec<V1DocEntry>, SyncError> {
        let url = format!("{}/docs", self.api_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let docs: Vec<V1DocEntry> = resp.json().await?;
                Ok(docs)
            }
            401 => Err(SyncError::TokenExpired),
            404 => Ok(Vec::new()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Get a single document entry
    pub async fn get_doc(&self, id: &str) -> Result<V1DocEntry, SyncError> {
        let url = format!("{}/docs/{}", self.api_url(), id);
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            404 => Err(SyncError::NotFound(id.to_string())),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Request an upload URL
    pub async fn request_upload(&self, id: &str, version: i32, doc_type: &str) -> Result<V1DocEntry, SyncError> {
        let url = format!("{}/upload/request", self.api_url());
        
        let body = vec![UploadRequest {
            id: id.to_string(),
            version,
            doc_type: doc_type.to_string(),
        }];
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&body)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let entries: Vec<V1DocEntry> = resp.json().await?;
                entries.into_iter().next()
                    .ok_or_else(|| SyncError::Parse("Empty upload response".into()))
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Confirm upload completion
    pub async fn update_status(&self, entry: &V1DocEntry) -> Result<(), SyncError> {
        let url = format!("{}/upload/update-status", self.api_url());
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&[entry])
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(()),
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Delete a document
    pub async fn delete_doc(&self, id: &str, version: i32) -> Result<(), SyncError> {
        let url = format!("{}/delete", self.api_url());
        
        let body = vec![DeleteRequest {
            id: id.to_string(),
            version,
        }];
        
        let resp = self.client
            .put(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&body)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(()),
            401 => Err(SyncError::TokenExpired),
            404 => Err(SyncError::NotFound(id.to_string())),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Download blob from signed URL
    pub async fn download_blob(&self, url: &str) -> Result<Vec<u8>, SyncError> {
        let resp = self.client
            .get(url)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.bytes().await?.to_vec()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Upload blob to signed URL
    pub async fn upload_blob(&self, url: &str, data: Vec<u8>) -> Result<(), SyncError> {
        let resp = self.client
            .put(url)
            .body(data)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 | 201 => Ok(()),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
}

impl SyncProtocol for SyncV1Client {
    fn version(&self) -> SyncVersion {
        SyncVersion::V1
    }
    
    fn base_url(&self) -> &str {
        &self.config.base_url
    }
    
    fn is_authenticated(&self) -> bool {
        self.config.user_token.is_some() || self.config.device_token.is_some()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            // V1 doesn't have a root hash, just return a placeholder
            Ok(RootInfo::new("v1-root".to_string()))
        })
    }
    
    fn list_documents(&self) -> BoxFuture<'_, Result<Vec<DocumentInfo>, SyncError>> {
        Box::pin(async move {
            let docs = self.list_docs().await?;
            Ok(docs.into_iter().map(DocumentInfo::from).collect())
        })
    }
    
    fn download_document(&self, id: &str) -> BoxFuture<'_, Result<Document, SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            // Get document entry with blob URL
            let entry = self.get_doc(&id).await?;
            
            let blob_url = entry.blob_url_get.as_ref()
                .ok_or_else(|| SyncError::NotFound(format!("No blob URL for {}", id)))?;
            
            // Download the blob (zip file)
            let blob_data = self.download_blob(&blob_url).await?;
            
            // Create document with the blob
            let mut doc = Document::new(id.clone());
            doc.info = DocumentInfo::from(entry);
            doc.files.insert("document.zip".to_string(), blob_data);
            
            Ok(doc)
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let doc = doc.clone();
        Box::pin(async move {
            // Request upload URL
            let entry = self.request_upload(
                &doc.id,
                doc.info.version as i32,
                &doc.info.doc_type,
            ).await?;
            
            let blob_url = entry.blob_url_put.as_ref()
                .ok_or_else(|| SyncError::NotFound("No upload URL".into()))?;
            
            // Get document data (assumes zip file)
            let data = doc.files.get("document.zip")
                .ok_or_else(|| SyncError::NotFound("No document.zip".into()))?;
            
            // Upload blob
            self.upload_blob(&blob_url, data.clone()).await?;
            
            // Confirm upload
            self.update_status(&entry).await?;
            
            Ok(UploadResult {
                document_id: doc.id.clone(),
                hash: None,
                version: entry.version as u32,
                size: data.len() as u64,
            })
        })
    }
    
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let id = id.to_string();
        Box::pin(async move {
            // Get current version
            let entry = self.get_doc(&id).await?;
            self.delete_doc(&id, entry.version).await
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            // V1 doesn't have incremental sync, just list documents
            let _docs = self.list_docs().await?;
            
            Ok(SyncResult {
                downloaded: Vec::new(),
                uploaded: Vec::new(),
                deleted: Vec::new(),
                generation: 0,
                had_conflicts: false,
                conflicts: Vec::new(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_v1_doc_entry_conversion() {
        let entry = V1DocEntry {
            id: "test-id".to_string(),
            version: 1,
            message: None,
            success: Some(true),
            blob_url_get: None,
            blob_url_get_expires: None,
            blob_url_put: None,
            blob_url_put_expires: None,
            modified_client: Some("2025-01-01".to_string()),
            doc_type: Some("DocumentType".to_string()),
            visible_name: Some("Test Doc".to_string()),
            current_page: Some(0),
            bookmarked: None,
            parent: None,
        };
        
        let info: DocumentInfo = entry.into();
        assert_eq!(info.id, "test-id");
        assert_eq!(info.name, Some("Test Doc".to_string()));
    }
}
