//! Sync V2: Batch sync protocol
//!
//! This protocol adds batch operations and signed URL uploads.
//! Used in firmware 2.x-3.x before the full hash-tree approach of V3.
//!
//! ## Endpoints
//! - `GET /sync/v2/root` - Get root generation
//! - `GET /sync/v2/sync-complete` - Sync completion status
//! - `GET /sync/v2/signed-urls/{path}` - Get signed URLs for blobs
//! - `POST /sync/v2/batch` - Execute batch operations

use std::collections::HashMap;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};

use crate::error::SyncError;
use crate::protocol::{
    SyncVersion, SyncProtocol, SyncConfig, RootInfo, DocumentInfo,
    Document, SyncResult, UploadResult, BoxFuture,
};

/// V2 root response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2Root {
    pub generation: u64,
    #[serde(default)]
    pub sync_complete: bool,
}

/// V2 document entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2DocEntry {
    pub id: String,
    pub hash: Option<String>,
    pub version: i32,
    #[serde(rename = "type")]
    pub doc_type: String,
    #[serde(rename = "visibleName")]
    pub visible_name: Option<String>,
    pub parent: Option<String>,
    #[serde(rename = "modifiedClient")]
    pub modified_client: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}

impl From<V2DocEntry> for DocumentInfo {
    fn from(entry: V2DocEntry) -> Self {
        Self {
            id: entry.id,
            doc_type: entry.doc_type,
            name: entry.visible_name,
            parent: entry.parent,
            hash: entry.hash,
            version: entry.version as u32,
            modified_at: entry.modified_client,
            deleted: entry.deleted,
            metadata: HashMap::new(),
        }
    }
}

/// Signed URL response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUrl {
    pub url: String,
    pub method: String,
    pub expires: String,
}

/// Batch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOp {
    pub op: String,  // "upload", "delete", "update"
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<String>,
    #[serde(rename = "visibleName", skip_serializing_if = "Option::is_none")]
    pub visible_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Batch request
#[derive(Debug, Clone, Serialize)]
pub struct BatchRequest {
    pub generation: u64,
    pub operations: Vec<BatchOp>,
}

/// Batch response
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResponse {
    pub generation: u64,
    pub results: Vec<BatchResult>,
}

/// Individual batch result
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResult {
    pub id: String,
    pub success: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub version: Option<i32>,
}

/// V2 Sync Protocol Client
pub struct SyncV2Client {
    client: Client,
    config: SyncConfig,
    generation: u64,
}

impl SyncV2Client {
    /// Create a new V2 client
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
        
        Ok(Self {
            client,
            config,
            generation: 0,
        })
    }
    
    /// Get authorization header
    fn auth_header(&self) -> Result<String, SyncError> {
        self.config.user_token.as_ref()
            .or(self.config.device_token.as_ref())
            .map(|t| format!("Bearer {}", t))
            .ok_or(SyncError::AuthRequired)
    }
    
    /// Get the sync API base URL
    fn api_url(&self) -> String {
        format!("{}/sync/v2", self.config.base_url)
    }
    
    /// Get root generation
    pub async fn get_root_info(&mut self) -> Result<V2Root, SyncError> {
        let url = format!("{}/root", self.api_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let root: V2Root = resp.json().await?;
                self.generation = root.generation;
                Ok(root)
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Check sync completion
    pub async fn sync_complete(&self) -> Result<bool, SyncError> {
        let url = format!("{}/sync-complete", self.api_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let result: V2Root = resp.json().await?;
                Ok(result.sync_complete)
            }
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Get signed URL for a blob
    pub async fn get_signed_url(&self, path: &str) -> Result<SignedUrl, SyncError> {
        let url = format!("{}/signed-urls/{}", self.api_url(), path);
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            404 => Err(SyncError::NotFound(path.to_string())),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Execute batch operations
    pub async fn batch(&mut self, operations: Vec<BatchOp>) -> Result<BatchResponse, SyncError> {
        let url = format!("{}/batch", self.api_url());
        
        let request = BatchRequest {
            generation: self.generation,
            operations,
        };
        
        let resp = self.client
            .post(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .json(&request)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => {
                let result: BatchResponse = resp.json().await?;
                self.generation = result.generation;
                Ok(result)
            }
            401 => Err(SyncError::TokenExpired),
            409 => Err(SyncError::Conflict {
                local_gen: self.generation,
                remote_gen: 0, // Would need to fetch from response
            }),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// List all documents
    pub async fn list_docs(&self) -> Result<Vec<V2DocEntry>, SyncError> {
        let url = format!("{}/docs", self.api_url());
        
        let resp = self.client
            .get(&url)
            .header(header::AUTHORIZATION, self.auth_header()?)
            .send()
            .await?;
        
        match resp.status().as_u16() {
            200 => Ok(resp.json().await?),
            401 => Err(SyncError::TokenExpired),
            status => Err(SyncError::Server {
                status,
                message: resp.text().await.unwrap_or_default(),
            }),
        }
    }
    
    /// Download blob using signed URL
    async fn download_blob(&self, path: &str) -> Result<Vec<u8>, SyncError> {
        let signed = self.get_signed_url(path).await?;
        
        let resp = self.client
            .get(&signed.url)
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
    
    /// Upload blob using signed URL
    async fn upload_blob(&self, path: &str, data: Vec<u8>) -> Result<(), SyncError> {
        let signed = self.get_signed_url(path).await?;
        
        let resp = self.client
            .put(&signed.url)
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

impl SyncProtocol for SyncV2Client {
    fn version(&self) -> SyncVersion {
        SyncVersion::V2
    }
    
    fn base_url(&self) -> &str {
        &self.config.base_url
    }
    
    fn is_authenticated(&self) -> bool {
        self.config.user_token.is_some() || self.config.device_token.is_some()
    }
    
    fn get_root(&self) -> BoxFuture<'_, Result<RootInfo, SyncError>> {
        Box::pin(async move {
            // V2 root doesn't have a hash, just generation
            Ok(RootInfo::new("v2-root".to_string())
                .with_generation(self.generation))
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
            // Get document metadata
            let docs = self.list_docs().await?;
            let entry = docs.iter()
                .find(|d| d.id == id)
                .ok_or_else(|| SyncError::NotFound(id.clone()))?;
            
            // Download blob
            let blob_path = format!("{}/document.zip", id);
            let blob_data = self.download_blob(&blob_path).await?;
            
            let mut doc = Document::new(id.clone());
            doc.info = DocumentInfo::from(entry.clone());
            doc.files.insert("document.zip".to_string(), blob_data);
            
            Ok(doc)
        })
    }
    
    fn upload_document(&self, doc: &Document) -> BoxFuture<'_, Result<UploadResult, SyncError>> {
        let doc = doc.clone();
        Box::pin(async move {
            // Get document data
            let data = doc.files.get("document.zip")
                .ok_or_else(|| SyncError::NotFound("No document.zip".into()))?;
            
            // Upload blob
            let blob_path = format!("{}/document.zip", doc.id);
            self.upload_blob(&blob_path, data.clone()).await?;
            
            Ok(UploadResult {
                document_id: doc.id.clone(),
                hash: None,
                version: doc.info.version + 1,
                size: data.len() as u64,
            })
        })
    }
    
    fn delete_document(&self, id: &str) -> BoxFuture<'_, Result<(), SyncError>> {
        let _id = id.to_string();
        Box::pin(async move {
            // V2 delete requires batch operation
            Err(SyncError::NotFound("V2 delete requires mutable batch state".into()))
        })
    }
    
    fn sync(&self) -> BoxFuture<'_, Result<SyncResult, SyncError>> {
        Box::pin(async move {
            let _docs = self.list_docs().await?;
            
            Ok(SyncResult {
                downloaded: Vec::new(),
                uploaded: Vec::new(),
                deleted: Vec::new(),
                generation: self.generation,
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
    fn test_v2_doc_entry_conversion() {
        let entry = V2DocEntry {
            id: "test-id".to_string(),
            hash: Some("abc123".to_string()),
            version: 2,
            doc_type: "DocumentType".to_string(),
            visible_name: Some("Test".to_string()),
            parent: None,
            modified_client: None,
            deleted: false,
        };
        
        let info: DocumentInfo = entry.into();
        assert_eq!(info.id, "test-id");
        assert_eq!(info.hash, Some("abc123".to_string()));
    }
}
